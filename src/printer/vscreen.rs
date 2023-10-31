use std::fmt::{self, Display, Write};

use crate::config::ConfigString;

// Wrapper to avoid unnecessary branching when input doesn't have ANSI escape sequences.
#[derive(Debug, Clone, Default)]
pub struct AnsiStyle {
    attributes: Option<Attributes>,
}

impl AnsiStyle {
    pub fn new() -> Self {
        AnsiStyle::default()
    }

    pub fn update(&mut self, sequence: &str) -> bool {
        match &mut self.attributes {
            Some(a) => a.update(sequence),
            None => {
                self.attributes = Some(Attributes::new());
                self.attributes.as_mut().unwrap().update(sequence)
            }
        }
    }
}

impl Display for AnsiStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.attributes {
            Some(ref a) => a.fmt(f),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Attributes {
    foreground: ConfigString,
    background: ConfigString,
    underlined: ConfigString,

    /// The character set to use.
    /// REGEX: `\^[()][AB0-3]`
    charset: ConfigString,

    /// A buffer for unknown sequences.
    unknown_buffer: ConfigString,

    /// ON:  ^[1m
    /// OFF: ^[22m
    bold: ConfigString,

    /// ON:  ^[2m
    /// OFF: ^[22m
    dim: ConfigString,

    /// ON:  ^[4m
    /// OFF: ^[24m
    underline: ConfigString,

    /// ON:  ^[3m
    /// OFF: ^[23m
    italic: ConfigString,

    /// ON:  ^[9m
    /// OFF: ^[29m
    strike: ConfigString,
}

impl Attributes {
    pub fn new() -> Self {
        Attributes::default()
    }

    /// Update the attributes with an escape sequence.
    /// Returns `false` if the sequence is unsupported.
    pub fn update(&mut self, sequence: &str) -> bool {
        if let Some(t) = sequence.as_bytes().get(1) {
            match t {
                b'(' => self.update_with_charset('(', &sequence[2..]),
                b')' => self.update_with_charset(')', &sequence[2..]),
                b'[' => {
                    if let Some(last) = sequence[2..].bytes().last() {
                        // SAFETY: Always starts with ^[ and ends with m.
                        self.update_with_csi(last, &sequence[2..(sequence.len() - 1)])
                    } else {
                        false
                    }
                }
                _ => self.update_with_unsupported(sequence),
            }
        } else {
            false
        }
    }

    fn sgr_reset(&mut self) {
        self.foreground.clear();
        self.background.clear();
        self.underlined.clear();
        self.bold.clear();
        self.dim.clear();
        self.underline.clear();
        self.italic.clear();
        self.strike.clear();
    }

    fn update_with_sgr(&mut self, parameters: &str) -> bool {
        let mut iter = parameters
            .split(';')
            .map(str::parse)
            .map(Result::unwrap_or_default); // Treat errors as 0.

        while let Some(p) = iter.next() {
            match p {
                0 => self.sgr_reset(),
                1 => {
                    self.bold.clear();
                    write!(self.bold, "\x1B[{parameters}m").unwrap();
                }
                2 => {
                    self.dim.clear();
                    write!(self.dim, "\x1B[{parameters}m").unwrap();
                }
                3 => {
                    self.italic.clear();
                    write!(self.italic, "\x1B[{parameters}m").unwrap();
                }
                4 => {
                    self.underline.clear();
                    write!(self.underline, "\x1B[{parameters}m").unwrap();
                }
                23 => self.italic.clear(),
                24 => self.underline.clear(),
                22 => {
                    self.bold.clear();
                    self.dim.clear();
                }
                30..=39 | 90..=97 | 100..=107 => {
                    self.foreground.clear();
                    Self::parse_color(&mut self.foreground, p, &mut iter);
                }
                40..=49 => {
                    self.background.clear();
                    Self::parse_color(&mut self.background, p, &mut iter);
                }
                58..=59 => {
                    self.underlined.clear();
                    Self::parse_color(&mut self.underlined, p, &mut iter);
                }
                _ => {
                    // Unsupported SGR sequence.
                    // Be compatible and pretend one just wasn't was provided.
                }
            }
        }

        true
    }

    fn update_with_csi(&mut self, finalizer: u8, sequence: &str) -> bool {
        if finalizer == b'm' {
            self.update_with_sgr(sequence)
        } else {
            false
        }
    }

    fn update_with_unsupported(&mut self, sequence: &str) -> bool {
        self.unknown_buffer.push_str(sequence);
        false
    }

    fn update_with_charset(&mut self, kind: char, set: &str) -> bool {
        self.charset.clear();
        write!(self.charset, "\x1B{}{}", kind, &set[..set.len().min(1)]).unwrap();
        true
    }

    fn parse_color(
        mut out: impl fmt::Write,
        color: u16,
        parameters: &mut impl Iterator<Item = u16>,
    ) {
        match color % 10 {
            8 => match parameters.next() {
                Some(5) /* 256-color */ => {
                    write!(out, "\x1B[{color};5").unwrap();
                    if let Some(value) = parameters.next() {
                        write!(out, ";{value}").unwrap();
                    }
                    write!(out, "m").unwrap();
                },
                Some(2) /* 24-bit color */ => {
                    write!(out, "\x1B[{color};2").unwrap();
                    for value in parameters.take(3) {
                        write!(out, ";{value}").unwrap();
                    }
                    write!(out, "m").unwrap();
                },
                Some(c) => write!(out, "\x1B[{color};{c}m").unwrap(),
                _ => (),
            },
            9 => (),
            _ => write!(out, "\x1B[{color}m").unwrap(),
        }
    }
}

impl Display for Attributes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}{}{}{}{}{}",
            self.foreground,
            self.background,
            self.underlined,
            self.charset,
            self.bold,
            self.dim,
            self.underline,
            self.italic,
            self.strike,
        )
    }
}
