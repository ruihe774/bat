use std::borrow::Cow;
use std::convert::{TryFrom, TryInto};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use clircle::{Clircle, Identifier};

use crate::error::*;

/// A description of an Input source.
/// This tells bat how to refer to the input.
#[derive(Clone)]
pub struct InputDescription {
    pub name: Option<OsString>,
    pub kind: String,
}

impl InputDescription {
    /// Creates a description for an input.
    fn new(name: Option<OsString>, kind: String) -> Self {
        InputDescription { name, kind }
    }
}

pub enum InputKind {
    OrdinaryFile(PathBuf),
    StdIn,
    CustomReader(Box<dyn Read>),
}

impl InputKind {
    pub fn description(&self) -> InputDescription {
        match self {
            InputKind::OrdinaryFile(ref path) => {
                InputDescription::new(Some(path.as_os_str().to_os_string()), "File".to_owned())
            }
            InputKind::StdIn => InputDescription::new(None, "STDIN".to_owned()),
            InputKind::CustomReader(_) => InputDescription::new(None, "READER".to_owned()),
        }
    }
}

pub struct Input {
    pub kind: InputKind,
    pub description: InputDescription,
}

#[cfg(feature = "git")]
pub(crate) enum OpenedInputKind {
    OrdinaryFile(PathBuf),
    StdIn,
    CustomReader,
}

pub(crate) struct OpenedInput {
    #[cfg(feature = "git")]
    pub(crate) kind: OpenedInputKind,
    pub(crate) reader: InputReader,
    pub(crate) description: InputDescription,
}

impl OpenedInput {
    pub(crate) fn path(&self) -> Option<&Path> {
        self.description.name.as_ref().map(|name| Path::new(name))
    }
}

impl Input {
    pub fn from_file(path: impl Into<PathBuf>) -> Self {
        let kind = InputKind::OrdinaryFile(path.into());
        Input {
            description: kind.description(),
            kind,
        }
    }

    pub fn from_stdin() -> Self {
        let kind = InputKind::StdIn;
        Input {
            description: kind.description(),
            kind,
        }
    }

    pub fn from_reader(reader: impl Read + 'static) -> Self {
        let kind = InputKind::CustomReader(Box::new(reader));
        Input {
            description: kind.description(),
            kind,
        }
    }

    pub(crate) fn open(self, stdout_identifier: Option<&Identifier>) -> Result<OpenedInput> {
        let description = self.description.clone();
        match self.kind {
            InputKind::StdIn => {
                if let Some(stdout) = stdout_identifier {
                    let input_identifier = Identifier::try_from(clircle::Stdio::Stdin)
                        .map_err(|e| format!("Stdin: Error identifying file: {}", e))?;
                    if stdout.surely_conflicts_with(&input_identifier) {
                        return Err("IO circle detected. The input from stdin is also an output. Aborting to avoid infinite loop.".into());
                    }
                }

                Ok(OpenedInput {
                    #[cfg(feature = "git")]
                    kind: OpenedInputKind::StdIn,
                    description,
                    reader: InputReader::new(io::stdin().lock()),
                })
            }

            InputKind::OrdinaryFile(path) => Ok(OpenedInput {
                #[cfg(feature = "git")]
                kind: OpenedInputKind::OrdinaryFile(path.clone()),
                description,
                reader: {
                    let mut file =
                        File::open(&path).map_err(|e| format!("'{}': {}", path.display(), e))?;
                    if file.metadata()?.is_dir() {
                        return Err(format!("'{}' is a directory.", path.display()).into());
                    }

                    if let Some(stdout) = stdout_identifier {
                        let input_identifier = Identifier::try_from(file).map_err(|e| {
                            format!("{}: Error identifying file: {}", path.display(), e)
                        })?;
                        if stdout.surely_conflicts_with(&input_identifier) {
                            return Err(format!(
                                "IO circle detected. The input from '{}' is also an output. Aborting to avoid infinite loop.",
                                path.display()
                            )
                            .into());
                        }
                        file = input_identifier.into_inner().expect("The file was lost in the clircle::Identifier, this should not have happened...");
                    }

                    InputReader::new(BufReader::new(file))
                },
            }),
            InputKind::CustomReader(reader) => Ok(OpenedInput {
                #[cfg(feature = "git")]
                kind: OpenedInputKind::CustomReader,
                description,
                reader: InputReader::new(BufReader::new(reader)),
            }),
        }
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ContentType {
    /// "binary" data
    BINARY(Option<String>),

    /// UTF-8 encoded "text" data
    UTF_8,

    /// UTF-16 encoded "text" data (little endian)
    UTF_16LE,

    /// UTF-16 encoded "text" data (big endian)
    UTF_16BE,

    /// UTF-32 encoded "text" data (little endian)
    UTF_32LE,

    /// UTF-32 encoded "text" data (big endian)
    UTF_32BE,
}

pub(crate) struct InputReader {
    inner: Box<dyn BufRead>,
    pub(crate) first_read: Option<String>,
    pub(crate) content_type: Option<ContentType>,
}

impl InputReader {
    pub(crate) fn new<R: BufRead + 'static>(mut reader: R) -> InputReader {
        let first_read = reader.fill_buf().ok().filter(|buf| !buf.is_empty());

        let (first_read, content_type) = if let Some(first_read) = first_read {
            let content_type = inspect(first_read);
            let first_read = decode(first_read, &content_type, true);
            let first_read = if let Some(first_read) = first_read {
                let truncated = first_read.trim_end_matches(char::REPLACEMENT_CHARACTER);
                let len = truncated.len();
                if len == 0 {
                    None
                } else {
                    Some(match first_read {
                        Cow::Borrowed(_) => truncated.to_owned(),
                        Cow::Owned(mut s) => {
                            s.truncate(len);
                            s
                        }
                    })
                }
            } else {
                None
            };
            (first_read, Some(content_type))
        } else {
            (None, None)
        };

        InputReader {
            inner: Box::new(reader),
            first_read,
            content_type,
        }
    }

    pub(crate) fn read_line(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
        use ContentType::*;
        let delimiter: &[u8] = match self.content_type {
            Some(UTF_16LE) => b"\n\0",
            Some(UTF_16BE) => b"\0\n",
            Some(UTF_32LE) => b"\n\0\0\0",
            Some(UTF_32BE) => b"\0\0\0\n",
            _ => b"\n",
        };

        let mut inner_buf = [0, 0, 0, 0];
        let read_buf = &mut inner_buf[..delimiter.len()];
        let mut r = Ok(false);
        'outer: loop {
            let mut read_bytes = 0;
            while read_bytes < read_buf.len() {
                let bytes = self.inner.read(&mut read_buf[read_bytes..])?;
                if bytes == 0 {
                    if read_bytes == 0 {
                        break 'outer r;
                    } else {
                        break 'outer Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                    }
                }
                read_bytes += bytes;
            }
            buf.extend_from_slice(read_buf);
            r = Ok(true);
            if read_buf == delimiter {
                break r;
            }
        }
    }
}

impl ContentType {
    pub(crate) fn is_binary(&self) -> bool {
        matches!(self, ContentType::BINARY(_))
    }

    pub(crate) fn is_text(&self) -> bool {
        !self.is_binary()
    }
}

pub(crate) fn decode<'a>(
    input: &'a [u8],
    content_type: &ContentType,
    remove_bom: bool,
) -> Option<Cow<'a, str>> {
    use ContentType::*;
    let remove_bom = remove_bom.then_some(());
    Some(match content_type {
        UTF_8 => {
            let input = remove_bom
                .and_then(|_| input.strip_prefix(&[0xEF, 0xBB, 0xBF]))
                .unwrap_or(input);
            String::from_utf8_lossy(input)
        }
        UTF_16LE => {
            let input = remove_bom
                .and_then(|_| input.strip_prefix(&[0xFF, 0xFE]))
                .unwrap_or(input);
            let mut s: String = char::decode_utf16(
                input
                    .chunks_exact(2)
                    .map(|chunk| u16::from_le_bytes(chunk.try_into().unwrap())),
            )
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
            if input.len() & 1 != 0 {
                s.push(char::REPLACEMENT_CHARACTER);
            }
            s.into()
        }
        UTF_16BE => {
            let input = remove_bom
                .and_then(|_| input.strip_prefix(&[0xFE, 0xFF]))
                .unwrap_or(input);
            let mut s: String = char::decode_utf16(
                input
                    .chunks_exact(2)
                    .map(|chunk| u16::from_be_bytes(chunk.try_into().unwrap())),
            )
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
            if input.len() & 1 != 0 {
                s.push(char::REPLACEMENT_CHARACTER);
            }
            s.into()
        }
        UTF_32LE => {
            let input = remove_bom
                .and_then(|_| input.strip_prefix(&[0xFF, 0xFE, 0x00, 0x00]))
                .unwrap_or(input);
            let mut s: String = input
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                .map(|ch| char::from_u32(ch).unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();
            if input.len() & 3 != 0 {
                s.push(char::REPLACEMENT_CHARACTER);
            }
            s.into()
        }
        UTF_32BE => {
            let input = remove_bom
                .and_then(|_| input.strip_prefix(&[0x00, 0x00, 0xFE, 0xFF]))
                .unwrap_or(input);
            let mut s: String = input
                .chunks_exact(4)
                .map(|chunk| u32::from_be_bytes(chunk.try_into().unwrap()))
                .map(|ch| char::from_u32(ch).unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();
            if input.len() & 3 != 0 {
                s.push(char::REPLACEMENT_CHARACTER);
            }
            s.into()
        }
        BINARY(_) => return None,
    })
}

#[cfg(not(unix))]
fn inspect(buffer: &[u8]) -> ContentType {
    use content_inspector::ContentType::*;
    match content_inspector::inspect(buffer) {
        UTF_8 | UTF_8_BOM => ContentType::UTF_8,
        UTF_16LE => ContentType::UTF_16LE,
        UTF_16BE => ContentType::UTF_16BE,
        UTF_32LE => ContentType::UTF_32LE,
        UTF_32BE => ContentType::UTF_32BE,
        BINARY => ContentType::BINARY(None),
    }
}

#[cfg(unix)]
fn execuate_file(args: impl IntoIterator<Item = impl AsRef<OsStr>>, buffer: &[u8]) -> String {
    use std::process::{Command, Stdio};
    let mut child = Command::new("/usr/bin/file")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to execuate /usr/bin/file");
    child.stdin.take().unwrap().write_all(buffer).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "/usr/bin/file exited with failure");
    let mut s = String::from_utf8(output.stdout).unwrap();
    s.truncate(s.trim_end().len());
    s
}

#[cfg(unix)]
fn inspect(buffer: &[u8]) -> ContentType {
    let encoding = execuate_file(["--brief", "--mime-encoding", "-"], buffer);
    match encoding.as_str() {
        "us-ascii" | "utf-8" | "unknown-8bit" => ContentType::UTF_8,
        "utf-16le" => ContentType::UTF_16LE,
        "utf-16be" => ContentType::UTF_16BE,
        "utf-32le" => ContentType::UTF_32LE,
        "utf-32be" => ContentType::UTF_32BE,
        _ => ContentType::BINARY({
            let format = execuate_file(["--brief", "-"], buffer);
            (&format != "data" && &format != "very short file (no magic)").then_some(format)
        }),
    }
}

#[test]
fn basic() {
    let content = b"#!/bin/bash\necho hello";
    let mut reader = InputReader::new(&content[..]);

    assert_eq!("#!/bin/bash\n", &reader.first_read.as_ref().unwrap()[..12]);

    let mut buffer = vec![];

    let res = reader.read_line(&mut buffer);
    assert!(res.is_ok());
    assert!(res.unwrap());
    assert_eq!(b"#!/bin/bash\n", &buffer[..]);

    buffer.clear();

    let res = reader.read_line(&mut buffer);
    assert!(res.is_ok());
    assert!(res.unwrap());
    assert_eq!(b"echo hello", &buffer[..]);

    buffer.clear();

    let res = reader.read_line(&mut buffer);
    assert!(res.is_ok());
    assert!(!res.unwrap());
    assert!(buffer.is_empty());
}

#[test]
fn utf16le() {
    let content = b"\xFF\xFE\x73\x00\x0A\x00\x64\x00";
    let mut reader = InputReader::new(&content[..]);

    let mut buffer = vec![];

    let res = reader.read_line(&mut buffer);
    assert!(res.is_ok());
    assert!(res.unwrap());
    assert_eq!(b"\xFF\xFE\x73\x00\x0A\x00", &buffer[..]);

    buffer.clear();

    let res = reader.read_line(&mut buffer);
    assert!(res.is_ok());
    assert!(res.unwrap());
    assert_eq!(b"\x64\x00", &buffer[..]);

    buffer.clear();

    let res = reader.read_line(&mut buffer);
    assert!(res.is_ok());
    assert!(!res.unwrap());
    assert!(buffer.is_empty());
}
