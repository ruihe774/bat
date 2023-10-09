use std::borrow::Cow;
use std::convert::TryFrom;
use std::fs;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use clircle::{Clircle, Identifier};
use content_inspector::{self, ContentType};

use crate::error::*;

/// A description of an Input source.
/// This tells bat how to refer to the input.
#[derive(Clone)]
pub struct InputDescription {
    pub(crate) name: String,

    /// The input title.
    /// This replaces the name if provided.
    title: Option<String>,

    /// The input kind.
    kind: Option<String>,

    /// A summary description of the input.
    /// Defaults to "{kind} '{name}'"
    summary: Option<String>,
}

impl InputDescription {
    /// Creates a description for an input.
    pub fn new(name: impl Into<String>) -> Self {
        InputDescription {
            name: name.into(),
            title: None,
            kind: None,
            summary: None,
        }
    }

    pub fn set_kind(&mut self, kind: Option<String>) {
        self.kind = kind;
    }

    pub fn set_summary(&mut self, summary: Option<String>) {
        self.summary = summary;
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    pub fn title(&self) -> &String {
        match &self.title {
            Some(title) => title,
            None => &self.name,
        }
    }

    pub fn kind(&self) -> Option<&String> {
        self.kind.as_ref()
    }

    pub fn summary(&self) -> String {
        self.summary.clone().unwrap_or_else(|| match &self.kind {
            None => self.name.clone(),
            Some(kind) => format!("{} '{}'", kind.to_lowercase(), self.name),
        })
    }
}

pub(crate) enum InputKind<'a> {
    OrdinaryFile(PathBuf),
    StdIn,
    CustomReader(Box<dyn Read + 'a>),
}

impl<'a> InputKind<'a> {
    pub fn description(&self) -> InputDescription {
        match self {
            InputKind::OrdinaryFile(ref path) => InputDescription::new(path.to_string_lossy()),
            InputKind::StdIn => InputDescription::new("STDIN"),
            InputKind::CustomReader(_) => InputDescription::new("READER"),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct InputMetadata {
    pub(crate) user_provided_name: Option<PathBuf>,
    pub(crate) size: Option<u64>,
}

pub struct Input<'a> {
    pub(crate) kind: InputKind<'a>,
    pub(crate) metadata: InputMetadata,
    pub(crate) description: InputDescription,
}

pub(crate) enum OpenedInputKind {
    OrdinaryFile(PathBuf),
    StdIn,
    CustomReader,
}

pub(crate) struct OpenedInput<'a> {
    pub(crate) kind: OpenedInputKind,
    pub(crate) metadata: InputMetadata,
    pub(crate) reader: InputReader<'a>,
    pub(crate) description: InputDescription,
}

impl OpenedInput<'_> {
    /// Get the path of the file:
    /// If this was set by the metadata, that will take priority.
    /// If it wasn't, it will use the real file path (if available).
    pub(crate) fn path(&self) -> Option<&PathBuf> {
        self.metadata
            .user_provided_name
            .as_ref()
            .or(match self.kind {
                OpenedInputKind::OrdinaryFile(ref path) => Some(path),
                _ => None,
            })
    }
}

impl<'a> Input<'a> {
    pub fn ordinary_file(path: impl AsRef<Path>) -> Self {
        Self::_ordinary_file(path.as_ref())
    }

    fn _ordinary_file(path: &Path) -> Self {
        let kind = InputKind::OrdinaryFile(path.to_path_buf());
        let metadata = InputMetadata {
            size: fs::metadata(path).map(|m| m.len()).ok(),
            ..InputMetadata::default()
        };

        Input {
            description: kind.description(),
            metadata,
            kind,
        }
    }

    pub fn stdin() -> Self {
        let kind = InputKind::StdIn;
        Input {
            description: kind.description(),
            metadata: InputMetadata::default(),
            kind,
        }
    }

    pub fn from_reader(reader: Box<dyn Read + 'a>) -> Self {
        let kind = InputKind::CustomReader(reader);
        Input {
            description: kind.description(),
            metadata: InputMetadata::default(),
            kind,
        }
    }

    pub fn is_stdin(&self) -> bool {
        matches!(self.kind, InputKind::StdIn)
    }

    pub fn with_name(self, provided_name: Option<impl AsRef<Path>>) -> Self {
        self._with_name(provided_name.as_ref().map(|it| it.as_ref()))
    }

    fn _with_name(mut self, provided_name: Option<&Path>) -> Self {
        if let Some(name) = provided_name {
            self.description.name = name.to_string_lossy().to_string()
        }

        self.metadata.user_provided_name = provided_name.map(|n| n.to_owned());
        self
    }

    pub fn description(&self) -> &InputDescription {
        &self.description
    }

    pub fn description_mut(&mut self) -> &mut InputDescription {
        &mut self.description
    }

    pub(crate) fn open<R: BufRead + 'a>(
        self,
        stdin: R,
        stdout_identifier: Option<&Identifier>,
    ) -> Result<OpenedInput<'a>> {
        let description = self.description().clone();
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
                    kind: OpenedInputKind::StdIn,
                    description,
                    metadata: self.metadata,
                    reader: InputReader::new(stdin),
                })
            }

            InputKind::OrdinaryFile(path) => Ok(OpenedInput {
                kind: OpenedInputKind::OrdinaryFile(path.clone()),
                description,
                metadata: self.metadata,
                reader: {
                    let mut file = File::open(&path)
                        .map_err(|e| format!("'{}': {}", path.to_string_lossy(), e))?;
                    if file.metadata()?.is_dir() {
                        return Err(format!("'{}' is a directory.", path.to_string_lossy()).into());
                    }

                    if let Some(stdout) = stdout_identifier {
                        let input_identifier = Identifier::try_from(file).map_err(|e| {
                            format!("{}: Error identifying file: {}", path.to_string_lossy(), e)
                        })?;
                        if stdout.surely_conflicts_with(&input_identifier) {
                            return Err(format!(
                                "IO circle detected. The input from '{}' is also an output. Aborting to avoid infinite loop.",
                                path.to_string_lossy()
                            )
                            .into());
                        }
                        file = input_identifier.into_inner().expect("The file was lost in the clircle::Identifier, this should not have happened...");
                    }

                    InputReader::new(BufReader::new(file))
                },
            }),
            InputKind::CustomReader(reader) => Ok(OpenedInput {
                description,
                kind: OpenedInputKind::CustomReader,
                metadata: self.metadata,
                reader: InputReader::new(BufReader::new(reader)),
            }),
        }
    }
}

pub(crate) struct InputReader<'a> {
    inner: Box<dyn BufRead + 'a>,
    pub(crate) first_read: Option<String>,
    pub(crate) content_type: Option<ContentType>,
}

impl<'a> InputReader<'a> {
    pub(crate) fn new<R: BufRead + 'a>(mut reader: R) -> InputReader<'a> {
        let first_read = reader.fill_buf().ok().filter(|buf| !buf.is_empty());

        let (first_read, content_type) = if let Some(first_read) = first_read {
            let content_type = content_inspector::inspect(first_read);
            let first_read = decode(first_read, content_type, true);
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

        let mut r = Ok(false);
        if self.inner.fill_buf()?.len() % delimiter.len() == 0 {
            loop {
                let filled_buf = self.inner.fill_buf()?;
                let filled_len = filled_buf.len();
                if filled_len == 0 {
                    break r;
                }
                let orig_size = buf.len();
                buf.extend(
                    filled_buf
                        .chunks_exact(delimiter.len())
                        .take_while(|chunk| *chunk != delimiter)
                        .flatten(),
                );
                let new_size = buf.len();
                let read_bytes = new_size - orig_size;
                self.inner.consume(read_bytes);
                r = Ok(true);
                if read_bytes < filled_len {
                    if filled_len - read_bytes < delimiter.len() {
                        break Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                    }
                    buf.extend_from_slice(delimiter);
                    self.inner.consume(delimiter.len());
                    break r;
                }
            }
        } else {
            // slow path
            let mut inner_buf = [0, 0, 0, 0];
            let read_buf = &mut inner_buf[..delimiter.len()];
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
}

pub(crate) fn decode(
    input: &[u8],
    content_type: ContentType,
    remove_bom: bool,
) -> Option<Cow<'_, str>> {
    use ContentType::*;
    let remove_bom = remove_bom.then_some(());
    Some(match content_type {
        UTF_8 | UTF_8_BOM => {
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
                    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]])),
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
                    .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]])),
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
                .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
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
                .map(|chunk| u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .map(|ch| char::from_u32(ch).unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();
            if input.len() & 3 != 0 {
                s.push(char::REPLACEMENT_CHARACTER);
            }
            s.into()
        }
        BINARY => return None,
    })
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
