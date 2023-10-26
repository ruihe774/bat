use std::borrow::Cow;
use std::fmt::Write;

use bstr::ByteSlice;
use console::AnsiCodeIterator;

use crate::input::{decode, ContentType};

#[derive(Debug, Copy, Clone, Default)]
pub enum NonprintableNotation {
    /// Use caret notation (^G, ^J, ^@, ..)
    Caret,

    /// Use unicode notation (␇, ␊, ␀, ..)
    Unicode,

    /// No nonprintable replacement
    #[default]
    None,
}

impl NonprintableNotation {
    pub(crate) fn show_nonprintable(&self) -> bool {
        match self {
            NonprintableNotation::Caret | NonprintableNotation::Unicode => true,
            NonprintableNotation::None => false,
        }
    }
}

/// Expand tabs like an ANSI-enabled expand(1).
pub(crate) fn expand_tabs(line: &str, width: usize, cursor: &mut usize) -> String {
    let mut buffer = String::with_capacity(line.len() * 2);

    for chunk in AnsiCodeIterator::new(line) {
        match chunk {
            (text, true) => buffer.push_str(text),
            (mut text, false) => {
                while let Some(index) = text.find('\t') {
                    // Add previous text.
                    if index > 0 {
                        *cursor += index;
                        buffer.push_str(&text[0..index]);
                    }

                    // Add tab.
                    let spaces = width - (*cursor % width);
                    *cursor += spaces;
                    buffer.push_str(&" ".repeat(spaces));

                    // Next.
                    text = &text[index + 1..text.len()];
                }

                *cursor += text.len();
                buffer.push_str(text);
            }
        }
    }

    buffer
}

pub(crate) fn preprocess<'a>(
    input: &'a [u8],
    content_type: Option<&ContentType>,
    is_first_line: bool,
    tab_width: usize,
    nonprintable_notation: NonprintableNotation,
) -> Cow<'a, str> {
    if !nonprintable_notation.show_nonprintable() {
        return content_type
            .map(|content_type| {
                decode(input, content_type, is_first_line).expect("cannot decode binary")
            })
            .unwrap_or_else(|| {
                assert!(
                    input.is_empty(),
                    "cannot decode input with unknown content type"
                );
                "".into()
            });
    }

    let mut output = String::new();
    let tab_width = if tab_width == 0 { 4 } else { tab_width };
    let mut line_idx = 0;
    for chunk in input.utf8_chunks() {
        for chr in chunk.valid().chars() {
            let mut before_size = output.len();
            match chr {
                // space
                ' ' => output.push('·'),
                // tab
                '\t' => {
                    let tab_stop = tab_width - line_idx % tab_width;
                    if tab_stop == 1 {
                        output.push('↹');
                    } else {
                        output.push('├');
                        output.extend(['─'].into_iter().cycle().take(tab_stop - 2));
                        output.push('┤');
                    }
                }
                // line feed
                '\x0A' => {
                    output.push_str(match nonprintable_notation {
                        NonprintableNotation::Caret => "^J\x0A",
                        NonprintableNotation::Unicode => "␊\x0A",
                        NonprintableNotation::None => unreachable!(),
                    });
                    before_size = output.len();
                }
                // ASCII control characters
                '\x00'..='\x1F' => {
                    let c = u32::from(chr);

                    match nonprintable_notation {
                        NonprintableNotation::Caret => {
                            let caret_character = char::from_u32(0x40 + c).unwrap();
                            output.push('^');
                            output.push(caret_character);
                        }

                        NonprintableNotation::Unicode => {
                            let replacement_symbol = char::from_u32(0x2400 + c).unwrap();
                            output.push(replacement_symbol)
                        }

                        NonprintableNotation::None => unreachable!(),
                    }
                }
                // delete
                '\x7F' => match nonprintable_notation {
                    NonprintableNotation::Caret => output.push_str("^?"),
                    NonprintableNotation::Unicode => output.push('\u{2421}'),
                    NonprintableNotation::None => unreachable!(),
                },
                // printable ASCII
                c if c.is_ascii_alphanumeric()
                    || c.is_ascii_punctuation()
                    || c.is_ascii_graphic() =>
                {
                    output.push(c)
                }
                // everything else
                c => output.extend(c.escape_unicode()),
            }
            line_idx += output.len() - before_size;
        }
        for byte in chunk.invalid() {
            write!(output, "\\x{:02X}", byte).unwrap();
            line_idx += 6;
        }
    }

    output.into()
}

#[test]
fn test_try_parse_utf8_char() {
    assert_eq!(try_parse_utf8_char(&[0x20]), Some((' ', 1)));
    assert_eq!(try_parse_utf8_char(&[0x20, 0x20]), Some((' ', 1)));
    assert_eq!(try_parse_utf8_char(&[0x20, 0xef]), Some((' ', 1)));

    assert_eq!(try_parse_utf8_char(&[0x00]), Some(('\x00', 1)));
    assert_eq!(try_parse_utf8_char(&[0x1b]), Some(('\x1b', 1)));

    assert_eq!(try_parse_utf8_char(&[0xc3, 0xa4]), Some(('ä', 2)));
    assert_eq!(try_parse_utf8_char(&[0xc3, 0xa4, 0xef]), Some(('ä', 2)));
    assert_eq!(try_parse_utf8_char(&[0xc3, 0xa4, 0x20]), Some(('ä', 2)));

    assert_eq!(try_parse_utf8_char(&[0xe2, 0x82, 0xac]), Some(('€', 3)));
    assert_eq!(
        try_parse_utf8_char(&[0xe2, 0x82, 0xac, 0xef]),
        Some(('€', 3))
    );
    assert_eq!(
        try_parse_utf8_char(&[0xe2, 0x82, 0xac, 0x20]),
        Some(('€', 3))
    );

    assert_eq!(try_parse_utf8_char(&[0xe2, 0x88, 0xb0]), Some(('∰', 3)));

    assert_eq!(
        try_parse_utf8_char(&[0xf0, 0x9f, 0x8c, 0x82]),
        Some(('🌂', 4))
    );
    assert_eq!(
        try_parse_utf8_char(&[0xf0, 0x9f, 0x8c, 0x82, 0xef]),
        Some(('🌂', 4))
    );
    assert_eq!(
        try_parse_utf8_char(&[0xf0, 0x9f, 0x8c, 0x82, 0x20]),
        Some(('🌂', 4))
    );

    assert_eq!(try_parse_utf8_char(&[]), None);
    assert_eq!(try_parse_utf8_char(&[0xef]), None);
    assert_eq!(try_parse_utf8_char(&[0xef, 0x20]), None);
    assert_eq!(try_parse_utf8_char(&[0xf0, 0xf0]), None);
}
