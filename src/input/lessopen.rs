use std::env::{self, VarError};
use std::error::Error as StdError;
use std::ffi::{CStr, CString, OsStr};
use std::fmt::{self, Display};
use std::io::{self, IoSliceMut, Read};
use std::mem;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use bstr::ByteSlice;
use libc::snprintf;

use super::{Input, InputKind};
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
    close: Option<(String, String, String)>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum LessOpenKind {
    Piped,
    PipedIgnoreExitCode,
    TempFile,
}

fn get_env_var(key: &str) -> Result<Option<String>> {
    match env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(e @ VarError::NotUnicode(_)) => Err(e)
            .with_context(|| format!("the value of environment variable '{}' is not unicode", key)),
    }
}

fn run_script(
    script: impl AsRef<OsStr>,
    args: &[impl AsRef<OsStr>],
    stdin: Stdio,
    stdout: Stdio,
) -> io::Result<Child> {
    Command::new(script.as_ref())
        .args(args)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::inherit())
        .spawn()
}

impl LessOpen {
    pub fn new(input: &mut Input) -> Result<Option<LessOpen>> {
        if let Some(lessopen_s) = get_env_var("LESSOPEN")? {
            let (kind, lessopen) = if lessopen_s.starts_with("||") {
                // "||" means pipe directly to bat without making a temporary file
                // Also, if preprocessor output is empty and exit code is zero, use the empty output
                // Otherwise, if output is empty and exit code is nonzero, use original file contents
                (LessOpenKind::Piped, &lessopen_s[2..])
            } else if lessopen_s.starts_with('|') {
                // "|" means pipe, but ignore exit code, always using preprocessor output if not empty
                (LessOpenKind::PipedIgnoreExitCode, &lessopen_s[1..])
            } else {
                // If neither appear, write output to a temporary file and read from that
                (LessOpenKind::TempFile, lessopen_s.as_str())
            };

            // "-" means that stdin is preprocessed along with files and may appear alongside "|" and "||"
            let (process_stdin, lessopen) = if lessopen.starts_with('-') {
                (true, &lessopen[1..])
            } else {
                (false, lessopen)
            };

            let lessclose = get_env_var("LESSCLOSE")?;

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
            let file_name_c = CString::new(file_name)?;

            const BUFSIZE: usize = 1024;
            let mut buf = [0; BUFSIZE];
            let lessopen_c = CString::new(lessopen)?;
            unsafe {
                snprintf(
                    mem::transmute(buf.as_mut_ptr()),
                    BUFSIZE,
                    lessopen_c.as_ptr(),
                    file_name_c.as_ptr(),
                )
            };
            let script = CStr::from_bytes_until_nul(buf.as_slice())?.to_str()?;
            let file_name = file_name_c.into_string()?;

            let all_args = shell_words::split(script)?;
            let (script, args) = all_args.split_first().unwrap();

            let mut child = run_script(
                script,
                args,
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
                            input.kind = InputKind::OrdinaryFile(replacement.clone().into());
                            Some(LessOpen {
                                child: None,
                                close: lessclose
                                    .map(|lessclose| (lessclose, file_name, replacement)),
                            })
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
                        input.kind = InputKind::CustomReader(Box::new(reader));
                        Some(LessOpen {
                            child: Some(child),
                            close: lessclose
                                .map(|lessclose| (lessclose, file_name, "-".to_owned())),
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
                        input.kind = InputKind::CustomReader(Box::new(reader));
                        Some(LessOpen {
                            child: Some(child),
                            close: lessclose
                                .map(|lessclose| (lessclose, file_name, "-".to_owned())),
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
        if let Some((lessclose, file_name, replacement)) = self.close.take() {
            const BUFSIZE: usize = 1024;
            let mut buf = [0; BUFSIZE];
            let lessclose_c = CString::new(lessclose).unwrap();
            let file_name_c = CString::new(file_name).unwrap();
            let replacement_c = CString::new(replacement).unwrap();
            unsafe {
                snprintf(
                    mem::transmute(buf.as_mut_ptr()),
                    BUFSIZE,
                    lessclose_c.as_ptr(),
                    file_name_c.as_ptr(),
                    replacement_c.as_ptr(),
                )
            };
            let script = CStr::from_bytes_until_nul(buf.as_slice())
                .unwrap()
                .to_str()
                .unwrap();

            let all_args = shell_words::split(script).unwrap();
            let (script, args) = all_args.split_first().unwrap();

            _ = run_script(script, args, Stdio::null(), Stdio::null())
                .and_then(|mut child| child.wait())
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
