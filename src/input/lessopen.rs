use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::io::{self, IoSliceMut, Read};
use std::mem;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use bstr::ByteSlice;

use super::{Input, InputKind};
use crate::config::get_env_var;
use crate::error::*;

#[derive(Debug)]
pub struct PathNotUnicode {
    pub path: PathBuf,
}

impl Display for PathNotUnicode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "path '{}' is not unicode", self.path.display())
    }
}

impl StdError for PathNotUnicode {}

#[derive(Debug)]
pub(crate) struct LessOpen {
    child: Option<Child>,
    close: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum LessOpenKind {
    Piped,
    PipedIgnoreExitCode,
    TempFile,
}

#[cfg(unix)]
fn run_script(script: &str, stdin: Stdio, stdout: Stdio) -> Result<Child> {
    Ok(Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .env_remove("LESSOPEN")
        .env_remove("LESSCLOSE")
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::inherit())
        .spawn()?)
}

#[cfg(not(unix))]
fn run_script(script: &str, stdin: Stdio, stdout: Stdio) -> Result<Child> {
    let all_args = shell_words::split(script)?;
    let (script, args) = all_args.split_first().unwrap();
    let script = grep_cli::resolve_binary(script)?;
    Ok(Command::new(script)
        .args(args)
        .env_remove("LESSOPEN")
        .env_remove("LESSCLOSE")
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::inherit())
        .spawn()?)
}

fn make_lessclose(mut lessclose: String, file_name: &str, replacement: &str) -> String {
    let mut iter = lessclose.match_indices("%s").map(|(pos, _)| pos);
    let first = iter.next();
    let second = iter.next();
    mem::drop(iter);
    if let Some(pos) = first {
        lessclose.replace_range(pos..(pos + 2), file_name);
    }
    if let Some(pos) = second {
        debug_assert!(first.is_some());
        let offset = file_name.len().wrapping_sub(2);
        let pos = pos.wrapping_add(offset);
        lessclose.replace_range(pos..(pos + 2), replacement);
    }
    lessclose
}

impl LessOpen {
    pub fn new(input: &mut Input) -> Result<Option<LessOpen>> {
        if let Some(lessopen_s) = get_env_var("LESSOPEN")? {
            let (kind, lessopen) = if let Some(lessopen) = lessopen_s.strip_prefix("||") {
                // "||" means pipe directly to bat without making a temporary file
                // Also, if preprocessor output is empty and exit code is zero, use the empty output
                // Otherwise, if output is empty and exit code is nonzero, use original file contents
                (LessOpenKind::Piped, lessopen)
            } else if let Some(lessopen) = lessopen_s.strip_prefix('|') {
                // "|" means pipe, but ignore exit code, always using preprocessor output if not empty
                (LessOpenKind::PipedIgnoreExitCode, lessopen)
            } else {
                // If neither appear, write output to a temporary file and read from that
                (LessOpenKind::TempFile, lessopen_s.as_str())
            };

            // "-" means that stdin is preprocessed along with files and may appear alongside "|" and "||"
            let (process_stdin, lessopen) = if let Some(lessopen) = lessopen.strip_prefix('-') {
                (true, lessopen)
            } else {
                (false, lessopen)
            };

            let file_name = match input.kind {
                InputKind::StdIn => {
                    if process_stdin {
                        "-"
                    } else {
                        return Ok(None);
                    }
                }
                InputKind::OrdinaryFile(ref path) => {
                    path.to_str().ok_or_else(|| PathNotUnicode {
                        path: path.to_owned(),
                    })?
                }
                InputKind::CustomReader(_) => return Ok(None), // maybe it needs a warning?
            };

            let lessopen = lessopen.replacen("%s", file_name, 1);
            let lessclose = get_env_var("LESSCLOSE")?;
            mem::drop(lessopen_s);

            let mut child = run_script(
                lessopen.as_str(),
                if let InputKind::StdIn = input.kind {
                    Stdio::inherit()
                } else {
                    Stdio::null()
                },
                Stdio::piped(),
            )
            .context("failed to spawn lessopen preprocessor")?;

            Ok(match kind {
                LessOpenKind::TempFile => {
                    let output = child
                        .wait_with_output()
                        .context("failed to execuate lessopen preprocessor")?;
                    if output.status.success() {
                        let mut stdout = output.stdout;
                        stdout.truncate(stdout.trim_end().len());
                        if stdout.is_empty() {
                            None
                        } else {
                            let replacement = String::from_utf8(stdout)
                                .context("path returned by lessopen preprocessor is not utf8")?;
                            let close = lessclose.map(|lessclose| {
                                make_lessclose(lessclose, file_name, replacement.as_str())
                            });
                            input.kind = InputKind::OrdinaryFile(replacement.into());
                            Some(LessOpen { child: None, close })
                        }
                    } else {
                        None
                    }
                }
                LessOpenKind::PipedIgnoreExitCode => {
                    let stdout = child.stdout.take().unwrap();
                    let mut reader = PeekReader::new(stdout);
                    if reader.peek().map(|byte| byte.is_none()).unwrap_or(true) {
                        None
                    } else {
                        let close =
                            lessclose.map(|lessclose| make_lessclose(lessclose, file_name, "-"));
                        input.kind = InputKind::CustomReader(Box::new(reader));
                        Some(LessOpen {
                            child: Some(child),
                            close,
                        })
                    }
                }
                LessOpenKind::Piped => {
                    let stdout = child.stdout.take().unwrap();
                    let mut reader = PeekReader::new(stdout);
                    if reader.peek().is_err()
                        || {
                            sleep(Duration::from_millis(10));
                            false
                        }
                        || child
                            .try_wait()
                            .map(|status| status.map_or(false, |status| !status.success()))
                            .unwrap_or(true)
                    {
                        None
                    } else {
                        let close =
                            lessclose.map(|lessclose| make_lessclose(lessclose, file_name, "-"));
                        input.kind = InputKind::CustomReader(Box::new(reader));
                        Some(LessOpen {
                            child: Some(child),
                            close,
                        })
                    }
                }
            })
        } else {
            Ok(None)
        }
    }
}

impl Drop for LessOpen {
    fn drop(&mut self) {
        // wait child
        if let Some(ref mut child) = self.child {
            _ = child.wait();
        }

        // call lessclose
        if let Some(ref lessclose) = self.close {
            if let Ok(mut child) = run_script(
                lessclose,
                Stdio::null(),
                if cfg!(debug_assertions) {
                    // for testing
                    Stdio::inherit()
                } else {
                    Stdio::null()
                },
            ) {
                _ = child.wait();
            }
        }
    }
}

#[derive(Debug)]
struct PeekReader<R: Read> {
    inner: R,
    peek: Option<u8>,
}

impl<R: Read> PeekReader<R> {
    fn new(reader: R) -> Self {
        PeekReader {
            inner: reader,
            peek: None,
        }
    }

    fn peek(&mut self) -> io::Result<Option<u8>> {
        if let Some(byte) = self.peek {
            return Ok(Some(byte));
        }
        let mut buf = [0; 1];
        match self.inner.read(&mut buf)? {
            0 => Ok(None),
            _ => {
                let byte = buf[0];
                self.peek = Some(byte);
                Ok(Some(byte))
            }
        }
    }
}

impl<R: Read> Read for PeekReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let len = if let Some(byte) = self.peek.take() {
            buf[0] = byte;
            1
        } else {
            0
        };
        Ok(self.inner.read(&mut buf[len..])? + len)
    }

    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        let slice = match bufs.iter_mut().find(|slice| !slice.is_empty()) {
            Some(slice) => slice,
            None => return Ok(0),
        };
        let len = if let Some(byte) = self.peek.take() {
            slice[0] = byte;
            let new_slice = IoSliceMut::new(unsafe {
                std::slice::from_raw_parts_mut(slice.as_mut_ptr().add(1), slice.len() - 1)
            });
            _ = mem::replace(slice, new_slice);
            1
        } else {
            0
        };
        Ok(self.inner.read_vectored(bufs)? + len)
    }
}
