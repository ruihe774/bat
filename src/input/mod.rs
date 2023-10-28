use std::borrow::Cow;
use std::error::Error as StdError;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Display};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bstr::{ByteSlice, ByteVec};
use clircle::{Clircle, Identifier};
#[cfg(feature = "zero-copy")]
use memmap2::MmapOptions;

use crate::error::*;
#[cfg(feature = "lessopen")]
use lessopen::LessOpen;
#[cfg(feature = "zero-copy")]
use zero_copy::{leak_mmap, LeakySliceReader};

#[cfg(feature = "lessopen")]
pub mod lessopen;
#[cfg(feature = "zero-copy")]
pub(crate) mod zero_copy;

#[derive(Debug)]
pub struct IoCircle {
    pub path: PathBuf,
}

impl Display for IoCircle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IO circle detected for '{}'", self.path.display())
    }
}

impl StdError for IoCircle {}

#[derive(Debug)]
pub struct IsDirectory {
    pub path: PathBuf,
}

impl Display for IsDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' is a directory", self.path.display())
    }
}

impl StdError for IsDirectory {}

/// A description of an Input source.
/// This tells bat how to refer to the input.
#[derive(Debug, Clone)]
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

#[allow(dead_code)]
pub(crate) struct OpenedInput {
    pub(crate) reader: InputReader,
    pub(crate) description: InputDescription,
    #[cfg(feature = "lessopen")]
    lessopen: Option<LessOpen>,
}

impl OpenedInput {
    pub(crate) fn path(&self) -> Option<&Path> {
        self.description.name.as_ref().map(Path::new)
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

    pub(crate) fn open(
        mut self,
        stdout_identifier: Option<&Identifier>,
        #[cfg(feature = "lessopen")] lessopen: bool,
    ) -> Result<OpenedInput> {
        #[cfg(feature = "lessopen")]
        let lessopen = if lessopen {
            LessOpen::new(&mut self)?
        } else {
            None
        };
        let description = self.description;
        match self.kind {
            InputKind::StdIn => {
                if let Some(stdout) = stdout_identifier {
                    let input_identifier = Identifier::try_from(clircle::Stdio::Stdin)?;
                    if stdout.surely_conflicts_with(&input_identifier) {
                        return Err(IoCircle {
                            path: "STDIN".into(),
                        }
                        .into());
                    }
                }

                Ok(OpenedInput {
                    description,
                    reader: InputReader::new(io::stdin().lock()),
                    #[cfg(feature = "lessopen")]
                    lessopen,
                })
            }

            InputKind::OrdinaryFile(path) => Ok(OpenedInput {
                description,
                reader: {
                    let mut file = File::open(&path)
                        .with_context(|| format!("failed to open '{}'", path.display()))?;
                    let metadata = file.metadata().with_context(|| {
                        format!("failed to get metadata of '{}'", path.display())
                    })?;
                    if metadata.is_dir() {
                        return Err(IsDirectory { path }.into());
                    }

                    if let Some(stdout) = stdout_identifier {
                        let input_identifier = Identifier::try_from(file)?;
                        if stdout.surely_conflicts_with(&input_identifier) {
                            return Err(IoCircle { path }.into());
                        }
                        file = input_identifier.into_inner().unwrap();
                    }

                    #[cfg(feature = "zero-copy")]
                    let r = metadata
                        .is_file()
                        .then_some(metadata.len())
                        .and_then(|len| {
                            unsafe {
                                MmapOptions::new()
                                    .len(isize::try_from(len).ok()?.try_into().unwrap())
                                    .map_copy(&file)
                            }
                            .ok()
                        })
                        .map_or_else(
                            || InputReader::new(BufReader::new(file)),
                            |mmap| InputReader::new(LeakySliceReader::new(leak_mmap(mmap))),
                        );
                    #[cfg(not(feature = "zero-copy"))]
                    let r = InputReader::new(BufReader::new(file));
                    r
                },
                #[cfg(feature = "lessopen")]
                lessopen,
            }),
            InputKind::CustomReader(reader) => Ok(OpenedInput {
                description,
                reader: InputReader::new(BufReader::new(reader)),
                #[cfg(feature = "lessopen")]
                lessopen,
            }),
        }
    }
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentType {
    /// "binary" data
    Binary(Option<String>),

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
        let first_read = reader.fill_buf().ok().and_then(|buf| {
            let limit = 8192;
            let len = buf.len();
            (len != 0).then_some(&buf[..limit.min(len)])
        });

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

    fn read_char<const WIDTH: usize>(&mut self) -> io::Result<Option<[u8; WIDTH]>> {
        let mut buffer = [0; WIDTH];
        let mut read_bytes = 0;
        while read_bytes < WIDTH {
            let bytes = self.inner.read(&mut buffer[read_bytes..])?;
            if bytes == 0 {
                if read_bytes == 0 {
                    return Ok(None);
                } else {
                    return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                }
            }
            read_bytes += bytes;
        }
        Ok(Some(buffer))
    }

    fn scan_line<const WIDTH: usize>(
        &mut self,
        buf: &mut Vec<u8>,
        delimiter: [u8; WIDTH],
    ) -> io::Result<bool> {
        let mut r = Ok(false);
        loop {
            let chunks = self.inner.fill_buf()?.chunks_exact(WIDTH);
            let len = chunks.len() * WIDTH;
            for (i, chunk) in chunks
                .map(|slice| -> [u8; WIDTH] { slice.try_into().unwrap() })
                .enumerate()
            {
                buf.extend_from_slice(chunk.as_slice());
                if chunk == delimiter {
                    self.inner.consume((i + 1) * WIDTH);
                    return Ok(true);
                }
            }
            if len != 0 {
                self.inner.consume(len);
                r = Ok(true);
            }
            match self.read_char()? {
                Some(chunk) => {
                    buf.extend_from_slice(chunk.as_slice());
                    r = Ok(true);
                    if chunk == delimiter {
                        return Ok(true);
                    }
                }
                None => return r,
            }
        }
    }

    pub(crate) fn read_line(&mut self, buf: &mut Vec<u8>) -> io::Result<bool> {
        use ContentType::*;
        match self.content_type {
            Some(UTF_16LE) => self.scan_line(buf, [b'\n', b'\0']),
            Some(UTF_16BE) => self.scan_line(buf, [b'\0', b'\n']),
            Some(UTF_32LE) => self.scan_line(buf, [b'\n', b'\0', b'\0', b'\0']),
            Some(UTF_32BE) => self.scan_line(buf, [b'\0', b'\0', b'\0', b'\n']),
            _ => self.scan_line(buf, [b'\n']),
        }
    }
}

impl ContentType {
    pub(crate) fn is_binary(&self) -> bool {
        matches!(self, ContentType::Binary(_))
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
        Binary(_) => return None,
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
        BINARY => ContentType::Binary(None),
    }
}

#[cfg(unix)]
fn execuate_file(args: impl IntoIterator<Item = impl AsRef<OsStr>>, buffer: &[u8]) -> Vec<u8> {
    let failure_msg = "failed to execuate /usr/bin/file";
    let mut child = Command::new("/usr/bin/file")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect(failure_msg);
    _ = child
        .stdin
        .take()
        .expect(failure_msg)
        .write(buffer)
        .expect(failure_msg);
    let output = child.wait_with_output().expect(failure_msg);
    assert!(output.status.success(), "{}", failure_msg);
    let mut s = output.stdout;
    s.truncate(s.trim_end().len());
    s
}

#[cfg(unix)]
fn inspect(buffer: &[u8]) -> ContentType {
    let encoding = execuate_file(["--brief", "--mime-encoding", "-"], buffer);
    match encoding.as_slice() {
        b"us-ascii" | b"utf-8" | b"unknown-8bit" => ContentType::UTF_8,
        b"utf-16le" => ContentType::UTF_16LE,
        b"utf-16be" => ContentType::UTF_16BE,
        b"utf-32le" => ContentType::UTF_32LE,
        b"utf-32be" => ContentType::UTF_32BE,
        _ => ContentType::Binary({
            let format = execuate_file(["--brief", "-"], buffer);
            (&format != b"data" && &format != b"very short file (no magic)")
                .then(|| format.into_string_lossy())
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
