use std::borrow::Cow;
use std::fmt::Write;

use bstr::ByteSlice;
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NonprintableNotation {
    /// Use caret notation (^G, ^J, ^@, ..)
    Caret,

    /// Use unicode notation (␇, ␊, ␀, ..)
    Unicode,
}

/// Expand tabs
pub(crate) fn expand_tabs<'a>(mut text: &'a str, width: usize, cursor: &mut usize) -> Cow<'a, str> {
    let mut buffer = String::new();

    while let Some(index) = text.find('\t') {
        // Add previous text.
        if index != 0 {
            *cursor += index;
            buffer.push_str(&text[..index]);
        }

        // Add tab.
        let spaces = width - (*cursor % width);
        *cursor += spaces;
        buffer.extend([' '].into_iter().cycle().take(spaces));

        // Next.
        text = &text[index + 1..];
    }

    *cursor += text.len();
    if buffer.is_empty() {
        text.into()
    } else {
        buffer.push_str(text);
        buffer.into()
    }
}

pub(crate) fn replace_nonprintable(
    input: &[u8],
    tab_width: usize,
    nonprintable_notation: NonprintableNotation,
) -> String {
    let mut output = Vec::with_capacity(input.len());
    let tab_width = if tab_width == 0 { 4 } else { tab_width };
    let mut line_idx = 0;
    let mut buf = String::with_capacity(6);
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
                    output.extend_from_slice(match nonprintable_notation {
                        NonprintableNotation::Caret => &['^', 'J', '\x0A'],
                        NonprintableNotation::Unicode => &['␊', '\x0A'],
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
                    }
                }
                // delete
                '\x7F' => match nonprintable_notation {
                    NonprintableNotation::Caret => output.extend_from_slice(&['^', '?']),
                    NonprintableNotation::Unicode => output.push('\u{2421}'),
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
            write!(buf, "\\x{:02X}", byte).unwrap();
            output.extend(buf.chars());
            buf.clear();
            line_idx += 6;
        }
    }

    output.into_iter().collect()
}