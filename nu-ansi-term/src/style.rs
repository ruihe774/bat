#[derive(Eq, PartialEq, Clone, Copy)]
#[cfg_attr(
    feature = "derive_serde_style",
    derive(serde::Deserialize, serde::Serialize)
)]
pub struct Style {
    /// The style's foreground color, if it has one.
    pub foreground: Option<Color>,

    /// The style's background color, if it has one.
    pub background: Option<Color>,

    /// Whether this style is bold.
    pub is_bold: bool,

    /// Whether this style is dimmed.
    pub is_dimmed: bool,

    /// Whether this style is italic.
    pub is_italic: bool,

    /// Whether this style is underlined.
    pub is_underline: bool,

    /// Whether this style is blinking.
    pub is_blink: bool,

    /// Whether this style has reverse colors.
    pub is_reverse: bool,

    /// Whether this style is hidden.
    pub is_hidden: bool,

    /// Whether this style is struckthrough.
    pub is_strikethrough: bool,
}

impl Style {
    pub fn new() -> Style {
        Style::default()
    }

    pub const fn bold(&self) -> Style {
        Style {
            is_bold: true,
            ..*self
        }
    }

    pub const fn dimmed(&self) -> Style {
        Style {
            is_dimmed: true,
            ..*self
        }
    }

    pub const fn italic(&self) -> Style {
        Style {
            is_italic: true,
            ..*self
        }
    }

    pub const fn underline(&self) -> Style {
        Style {
            is_underline: true,
            ..*self
        }
    }

    pub const fn blink(&self) -> Style {
        Style {
            is_blink: true,
            ..*self
        }
    }

    pub const fn reverse(&self) -> Style {
        Style {
            is_reverse: true,
            ..*self
        }
    }

    pub const fn hidden(&self) -> Style {
        Style {
            is_hidden: true,
            ..*self
        }
    }

    pub const fn strikethrough(&self) -> Style {
        Style {
            is_strikethrough: true,
            ..*self
        }
    }

    pub const fn fg(&self, foreground: Color) -> Style {
        Style {
            foreground: Some(foreground),
            ..*self
        }
    }

    pub const fn on(&self, background: Color) -> Style {
        Style {
            background: Some(background),
            ..*self
        }
    }

    pub fn is_plain(self) -> bool {
        self == Style::default()
    }
}

impl Default for Style {
    fn default() -> Style {
        Style {
            foreground: None,
            background: None,
            is_bold: false,
            is_dimmed: false,
            is_italic: false,
            is_underline: false,
            is_blink: false,
            is_reverse: false,
            is_hidden: false,
            is_strikethrough: false,
        }
    }
}

// ---- colors ----

/// A color is one specific type of ANSI escape code, and can refer
/// to either the foreground or background color.
///
/// These use the standard numeric sequences.
/// See <http://invisible-island.net/xterm/ctlseqs/ctlseqs.html>
#[derive(Eq, PartialEq, Clone, Copy, Debug, Default)]
#[cfg_attr(
    feature = "derive_serde_style",
    derive(serde::Deserialize, serde::Serialize)
)]
pub enum Color {
    /// Color #0 (foreground code `30`, background code `40`).
    ///
    /// This is not necessarily the background color, and using it as one may
    /// render the text hard to read on terminals with dark backgrounds.
    Black,

    /// Color #0 (foreground code `90`, background code `100`).
    DarkGray,

    /// Color #1 (foreground code `31`, background code `41`).
    Red,

    /// Color #1 (foreground code `91`, background code `101`).
    LightRed,

    /// Color #2 (foreground code `32`, background code `42`).
    Green,

    /// Color #2 (foreground code `92`, background code `102`).
    LightGreen,

    /// Color #3 (foreground code `33`, background code `43`).
    Yellow,

    /// Color #3 (foreground code `93`, background code `103`).
    LightYellow,

    /// Color #4 (foreground code `34`, background code `44`).
    Blue,

    /// Color #4 (foreground code `94`, background code `104`).
    LightBlue,

    /// Color #5 (foreground code `35`, background code `45`).
    Purple,

    /// Color #5 (foreground code `95`, background code `105`).
    LightPurple,

    /// Color #5 (foreground code `35`, background code `45`).
    Magenta,

    /// Color #5 (foreground code `95`, background code `105`).
    LightMagenta,

    /// Color #6 (foreground code `36`, background code `46`).
    Cyan,

    /// Color #6 (foreground code `96`, background code `106`).
    LightCyan,

    /// Color #7 (foreground code `37`, background code `47`).
    ///
    /// As above, this is not necessarily the foreground color, and may be
    /// hard to read on terminals with light backgrounds.
    White,

    /// Color #7 (foreground code `97`, background code `107`).
    LightGray,

    /// A color number from 0 to 255, for use in 256-color terminal
    /// environments.
    ///
    /// - colors 0 to 7 are the `Black` to `White` variants respectively.
    ///   These colors can usually be changed in the terminal emulator.
    /// - colors 8 to 15 are brighter versions of the eight colors above.
    ///   These can also usually be changed in the terminal emulator, or it
    ///   could be configured to use the original colors and show the text in
    ///   bold instead. It varies depending on the program.
    /// - colors 16 to 231 contain several palettes of bright colors,
    ///   arranged in six squares measuring six by six each.
    /// - colors 232 to 255 are shades of grey from black to white.
    ///
    /// It might make more sense to look at a [color chart][cc].
    ///
    /// [cc]: https://upload.wikimedia.org/wikipedia/commons/1/15/Xterm_256color_chart.svg
    Fixed(u8),

    /// A 24-bit Rgb color, as specified by ISO-8613-3.
    Rgb(u8, u8, u8),

    /// The default color (foreground code `39`, background codr `49`).
    #[default]
    Default,
}

impl Color {
    pub fn normal(self) -> Style {
        Style {
            foreground: Some(self),
            ..Style::default()
        }
    }

    pub fn bold(self) -> Style {
        Style {
            foreground: Some(self),
            is_bold: true,
            ..Style::default()
        }
    }

    pub fn dimmed(self) -> Style {
        Style {
            foreground: Some(self),
            is_dimmed: true,
            ..Style::default()
        }
    }

    pub fn italic(self) -> Style {
        Style {
            foreground: Some(self),
            is_italic: true,
            ..Style::default()
        }
    }

    pub fn underline(self) -> Style {
        Style {
            foreground: Some(self),
            is_underline: true,
            ..Style::default()
        }
    }

    pub fn blink(self) -> Style {
        Style {
            foreground: Some(self),
            is_blink: true,
            ..Style::default()
        }
    }

    pub fn reverse(self) -> Style {
        Style {
            foreground: Some(self),
            is_reverse: true,
            ..Style::default()
        }
    }

    pub fn hidden(self) -> Style {
        Style {
            foreground: Some(self),
            is_hidden: true,
            ..Style::default()
        }
    }

    pub fn strikethrough(self) -> Style {
        Style {
            foreground: Some(self),
            is_strikethrough: true,
            ..Style::default()
        }
    }

    pub fn on(self, background: Color) -> Style {
        Style {
            foreground: Some(self),
            background: Some(background),
            ..Style::default()
        }
    }
}

impl From<Color> for Style {
    /// You can turn a `Color` into a `Style` with the foreground color set
    /// with the `From` trait.
    ///
    /// ```
    /// use nu_ansi_term::{Style, Color};
    /// let green_foreground = Style::default().fg(Color::Green);
    /// assert_eq!(green_foreground, Color::Green.normal());
    /// assert_eq!(green_foreground, Color::Green.into());
    /// assert_eq!(green_foreground, Style::from(Color::Green));
    /// ```
    fn from(color: Color) -> Style {
        color.normal()
    }
}

#[cfg(test)]
#[cfg(feature = "derive_serde_style")]
mod serde_json_tests {
    use super::{Color, Style};

    #[test]
    fn color_serialization() {
        let colors = &[
            Color::Red,
            Color::Blue,
            Color::Rgb(123, 123, 123),
            Color::Fixed(255),
        ];

        assert_eq!(
            serde_json::to_string(&colors).unwrap(),
            "[\"Red\",\"Blue\",{\"Rgb\":[123,123,123]},{\"Fixed\":255}]"
        );
    }

    #[test]
    fn color_deserialization() {
        let colors = [
            Color::Red,
            Color::Blue,
            Color::Rgb(123, 123, 123),
            Color::Fixed(255),
        ];

        for color in colors {
            let serialized = serde_json::to_string(&color).unwrap();
            let deserialized: Color = serde_json::from_str(&serialized).unwrap();

            assert_eq!(color, deserialized);
        }
    }

    #[test]
    fn style_serialization() {
        let style = Style::default();

        assert_eq!(serde_json::to_string(&style).unwrap(), "{\"foreground\":null,\"background\":null,\"is_bold\":false,\"is_dimmed\":false,\"is_italic\":false,\"is_underline\":false,\"is_blink\":false,\"is_reverse\":false,\"is_hidden\":false,\"is_strikethrough\":false,\"prefix_with_reset\":false}".to_string());
    }
}
