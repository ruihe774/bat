#![allow(missing_docs)]

use compact_str::CompactString;
use zwrite::write;

use crate::style::{Color, Style};

impl Style {
    /// Write any bytes that go *before* a piece of text to the given writer.
    fn write_prefix(&self, f: &mut CompactString) {
        // If there are actually no styles here, then don’t write *any* codes
        // as the prefix. An empty ANSI code may not affect the terminal
        // output at all, but a user may just want a code-free string.
        if self.is_plain() {
            return;
        }

        // Write the codes’ prefix, then write numbers, separated by
        // semicolons, for each text style we want to apply.
        write!(f, "\x1B[").unwrap();
        let mut written_anything = false;

        {
            let mut write_char = |c: char| {
                if written_anything {
                    write!(f, ";").unwrap();
                }
                written_anything = true;
                #[cfg(feature = "gnu_legacy")]
                write!(f, "0").unwrap();
                write!(f, "{}", c).unwrap();
            };

            if self.is_bold {
                write_char('1')
            }
            if self.is_dimmed {
                write_char('2')
            }
            if self.is_italic {
                write_char('3')
            }
            if self.is_underline {
                write_char('4')
            }
            if self.is_blink {
                write_char('5')
            }
            if self.is_reverse {
                write_char('7')
            }
            if self.is_hidden {
                write_char('8')
            }
            if self.is_strikethrough {
                write_char('9')
            }
        }

        // The foreground and background colors, if specified, need to be
        // handled specially because the number codes are more complicated.
        // (see `write_background_code` and `write_foreground_code`)
        if let Some(bg) = self.background {
            if written_anything {
                write!(f, ";").unwrap();
            }
            written_anything = true;
            bg.write_background_code(f);
        }

        if let Some(fg) = self.foreground {
            if written_anything {
                write!(f, ";").unwrap();
            }
            fg.write_foreground_code(f);
        }

        // All the codes end with an `m`, because reasons.
        write!(f, "m").unwrap();
    }

    /// Write any bytes that go *after* a piece of text to the given writer.
    fn write_suffix(&self, f: &mut CompactString) {
        if !self.is_plain() {
            write!(f, "{:s}", RESET).unwrap()
        }
    }
}

/// The code to send to reset all styles and return to `Style::default()`.
pub static RESET: &str = "\x1B[0m";

impl Color {
    fn write_foreground_code(&self, f: &mut CompactString) {
        match self {
            Color::Black => write!(f, "30").unwrap(),
            Color::Red => write!(f, "31").unwrap(),
            Color::Green => write!(f, "32").unwrap(),
            Color::Yellow => write!(f, "33").unwrap(),
            Color::Blue => write!(f, "34").unwrap(),
            Color::Purple => write!(f, "35").unwrap(),
            Color::Magenta => write!(f, "35").unwrap(),
            Color::Cyan => write!(f, "36").unwrap(),
            Color::White => write!(f, "37").unwrap(),
            Color::Fixed(num) => write!(f, "38;5;{}", num).unwrap(),
            Color::Rgb(r, g, b) => write!(f, "38;2;{};{};{}", r, g, b).unwrap(),
            Color::Default => write!(f, "39").unwrap(),
            Color::DarkGray => write!(f, "90").unwrap(),
            Color::LightRed => write!(f, "91").unwrap(),
            Color::LightGreen => write!(f, "92").unwrap(),
            Color::LightYellow => write!(f, "93").unwrap(),
            Color::LightBlue => write!(f, "94").unwrap(),
            Color::LightPurple => write!(f, "95").unwrap(),
            Color::LightMagenta => write!(f, "95").unwrap(),
            Color::LightCyan => write!(f, "96").unwrap(),
            Color::LightGray => write!(f, "97").unwrap(),
        }
    }

    fn write_background_code(&self, f: &mut CompactString) {
        match self {
            Color::Black => write!(f, "40").unwrap(),
            Color::Red => write!(f, "41").unwrap(),
            Color::Green => write!(f, "42").unwrap(),
            Color::Yellow => write!(f, "43").unwrap(),
            Color::Blue => write!(f, "44").unwrap(),
            Color::Purple => write!(f, "45").unwrap(),
            Color::Magenta => write!(f, "45").unwrap(),
            Color::Cyan => write!(f, "46").unwrap(),
            Color::White => write!(f, "47").unwrap(),
            Color::Fixed(num) => write!(f, "48;5;{}", num).unwrap(),
            Color::Rgb(r, g, b) => write!(f, "48;2;{};{};{}", r, g, b).unwrap(),
            Color::Default => write!(f, "49").unwrap(),
            Color::DarkGray => write!(f, "100").unwrap(),
            Color::LightRed => write!(f, "101").unwrap(),
            Color::LightGreen => write!(f, "102").unwrap(),
            Color::LightYellow => write!(f, "103").unwrap(),
            Color::LightBlue => write!(f, "104").unwrap(),
            Color::LightPurple => write!(f, "105").unwrap(),
            Color::LightMagenta => write!(f, "105").unwrap(),
            Color::LightCyan => write!(f, "106").unwrap(),
            Color::LightGray => write!(f, "107").unwrap(),
        }
    }
}

impl Style {
    pub fn prefix(self) -> CompactString {
        let mut f = CompactString::default();
        self.write_prefix(&mut f);
        f
    }

    pub fn suffix(self) -> CompactString {
        let mut f = CompactString::default();
        self.write_suffix(&mut f);
        f
    }
}

impl Color {
    pub fn prefix(self) -> CompactString {
        self.normal().prefix()
    }

    pub fn suffix(self) -> CompactString {
        self.normal().suffix()
    }
}

#[cfg(test)]
macro_rules! test {
    ($name: ident: $style: expr; $input: expr => $result: expr) => {
        #[test]
        fn $name() {
            assert_eq!(
                format_compact!("{}{}{}", $style.prefix(), $input, $style.suffix()),
                $result
            );
        }
    };
}

#[cfg(test)]
#[cfg(not(feature = "gnu_legacy"))]
mod test {
    use crate::style::Color::*;
    use crate::style::Style;
    use compact_str::format_compact;

    test!(plain:                 Style::default();                  "text/plain" => "text/plain");
    test!(red:                   Red;                               "hi" => "\x1B[31mhi\x1B[0m");
    test!(black:                 Black.normal();                    "hi" => "\x1B[30mhi\x1B[0m");
    test!(yellow_bold:           Yellow.bold();                     "hi" => "\x1B[1;33mhi\x1B[0m");
    test!(yellow_bold_2:         Yellow.normal().bold();            "hi" => "\x1B[1;33mhi\x1B[0m");
    test!(blue_underline:        Blue.underline();                  "hi" => "\x1B[4;34mhi\x1B[0m");
    test!(green_bold_ul:         Green.bold().underline();          "hi" => "\x1B[1;4;32mhi\x1B[0m");
    test!(green_bold_ul_2:       Green.underline().bold();          "hi" => "\x1B[1;4;32mhi\x1B[0m");
    test!(purple_on_white:       Purple.on(White);                  "hi" => "\x1B[47;35mhi\x1B[0m");
    test!(purple_on_white_2:     Purple.normal().on(White);         "hi" => "\x1B[47;35mhi\x1B[0m");
    test!(yellow_on_blue:        Style::new().on(Blue).fg(Yellow);  "hi" => "\x1B[44;33mhi\x1B[0m");
    test!(magenta_on_white:      Magenta.on(White);                  "hi" => "\x1B[47;35mhi\x1B[0m");
    test!(magenta_on_white_2:    Magenta.normal().on(White);         "hi" => "\x1B[47;35mhi\x1B[0m");
    test!(yellow_on_blue_2:      Cyan.on(Blue).fg(Yellow);          "hi" => "\x1B[44;33mhi\x1B[0m");
    test!(cyan_bold_on_white:    Cyan.bold().on(White);             "hi" => "\x1B[1;47;36mhi\x1B[0m");
    test!(cyan_ul_on_white:      Cyan.underline().on(White);        "hi" => "\x1B[4;47;36mhi\x1B[0m");
    test!(cyan_bold_ul_on_white: Cyan.bold().underline().on(White); "hi" => "\x1B[1;4;47;36mhi\x1B[0m");
    test!(cyan_ul_bold_on_white: Cyan.underline().bold().on(White); "hi" => "\x1B[1;4;47;36mhi\x1B[0m");
    test!(fixed:                 Fixed(100);                        "hi" => "\x1B[38;5;100mhi\x1B[0m");
    test!(fixed_on_purple:       Fixed(100).on(Purple);             "hi" => "\x1B[45;38;5;100mhi\x1B[0m");
    test!(fixed_on_fixed:        Fixed(100).on(Fixed(200));         "hi" => "\x1B[48;5;200;38;5;100mhi\x1B[0m");
    test!(rgb:                   Rgb(70,130,180);                   "hi" => "\x1B[38;2;70;130;180mhi\x1B[0m");
    test!(rgb_on_blue:           Rgb(70,130,180).on(Blue);          "hi" => "\x1B[44;38;2;70;130;180mhi\x1B[0m");
    test!(blue_on_rgb:           Blue.on(Rgb(70,130,180));          "hi" => "\x1B[48;2;70;130;180;34mhi\x1B[0m");
    test!(rgb_on_rgb:            Rgb(70,130,180).on(Rgb(5,10,15));  "hi" => "\x1B[48;2;5;10;15;38;2;70;130;180mhi\x1B[0m");
    test!(bold:                  Style::new().bold();               "hi" => "\x1B[1mhi\x1B[0m");
    test!(underline:             Style::new().underline();          "hi" => "\x1B[4mhi\x1B[0m");
    test!(bunderline:            Style::new().bold().underline();   "hi" => "\x1B[1;4mhi\x1B[0m");
    test!(dimmed:                Style::new().dimmed();             "hi" => "\x1B[2mhi\x1B[0m");
    test!(italic:                Style::new().italic();             "hi" => "\x1B[3mhi\x1B[0m");
    test!(blink:                 Style::new().blink();              "hi" => "\x1B[5mhi\x1B[0m");
    test!(reverse:               Style::new().reverse();            "hi" => "\x1B[7mhi\x1B[0m");
    test!(hidden:                Style::new().hidden();             "hi" => "\x1B[8mhi\x1B[0m");
    test!(stricken:              Style::new().strikethrough();      "hi" => "\x1B[9mhi\x1B[0m");
    test!(lr_on_lr:              LightRed.on(LightRed);             "hi" => "\x1B[101;91mhi\x1B[0m");
}

#[cfg(test)]
#[cfg(feature = "gnu_legacy")]
mod gnu_legacy_test {
    use crate::style::Color::*;
    use crate::style::Style;

    test!(plain:                 Style::default();                  "text/plain" => "text/plain");
    test!(red:                   Red;                               "hi" => "\x1B[31mhi\x1B[0m");
    test!(black:                 Black.normal();                    "hi" => "\x1B[30mhi\x1B[0m");
    test!(yellow_bold:           Yellow.bold();                     "hi" => "\x1B[01;33mhi\x1B[0m");
    test!(yellow_bold_2:         Yellow.normal().bold();            "hi" => "\x1B[01;33mhi\x1B[0m");
    test!(blue_underline:        Blue.underline();                  "hi" => "\x1B[04;34mhi\x1B[0m");
    test!(green_bold_ul:         Green.bold().underline();          "hi" => "\x1B[01;04;32mhi\x1B[0m");
    test!(green_bold_ul_2:       Green.underline().bold();          "hi" => "\x1B[01;04;32mhi\x1B[0m");
    test!(purple_on_white:       Purple.on(White);                  "hi" => "\x1B[47;35mhi\x1B[0m");
    test!(purple_on_white_2:     Purple.normal().on(White);         "hi" => "\x1B[47;35mhi\x1B[0m");
    test!(yellow_on_blue:        Style::new().on(Blue).fg(Yellow);  "hi" => "\x1B[44;33mhi\x1B[0m");
    test!(yellow_on_blue_reset:  Cyan.on(Blue).reset_before_style().fg(Yellow); "hi" => "\x1B[0m\x1B[44;33mhi\x1B[0m");
    test!(yellow_on_blue_reset_2: Cyan.on(Blue).fg(Yellow).reset_before_style(); "hi" => "\x1B[0m\x1B[44;33mhi\x1B[0m");
    test!(magenta_on_white:      Magenta.on(White);                  "hi" => "\x1B[47;35mhi\x1B[0m");
    test!(magenta_on_white_2:    Magenta.normal().on(White);         "hi" => "\x1B[47;35mhi\x1B[0m");
    test!(yellow_on_blue_2:      Cyan.on(Blue).fg(Yellow);          "hi" => "\x1B[44;33mhi\x1B[0m");
    test!(cyan_bold_on_white:    Cyan.bold().on(White);             "hi" => "\x1B[01;47;36mhi\x1B[0m");
    test!(cyan_ul_on_white:      Cyan.underline().on(White);        "hi" => "\x1B[04;47;36mhi\x1B[0m");
    test!(cyan_bold_ul_on_white: Cyan.bold().underline().on(White); "hi" => "\x1B[01;04;47;36mhi\x1B[0m");
    test!(cyan_ul_bold_on_white: Cyan.underline().bold().on(White); "hi" => "\x1B[01;04;47;36mhi\x1B[0m");
    test!(fixed:                 Fixed(100);                        "hi" => "\x1B[38;5;100mhi\x1B[0m");
    test!(fixed_on_purple:       Fixed(100).on(Purple);             "hi" => "\x1B[45;38;5;100mhi\x1B[0m");
    test!(fixed_on_fixed:        Fixed(100).on(Fixed(200));         "hi" => "\x1B[48;5;200;38;5;100mhi\x1B[0m");
    test!(rgb:                   Rgb(70,130,180);                   "hi" => "\x1B[38;2;70;130;180mhi\x1B[0m");
    test!(rgb_on_blue:           Rgb(70,130,180).on(Blue);          "hi" => "\x1B[44;38;2;70;130;180mhi\x1B[0m");
    test!(blue_on_rgb:           Blue.on(Rgb(70,130,180));          "hi" => "\x1B[48;2;70;130;180;34mhi\x1B[0m");
    test!(rgb_on_rgb:            Rgb(70,130,180).on(Rgb(5,10,15));  "hi" => "\x1B[48;2;5;10;15;38;2;70;130;180mhi\x1B[0m");
    test!(bold:                  Style::new().bold();               "hi" => "\x1B[01mhi\x1B[0m");
    test!(bold_with_reset:       Style::new().reset_before_style().bold(); "hi" => "\x1B[0m\x1B[01mhi\x1B[0m");
    test!(bold_with_reset_2:     Style::new().bold().reset_before_style(); "hi" => "\x1B[0m\x1B[01mhi\x1B[0m");
    test!(underline:             Style::new().underline();          "hi" => "\x1B[04mhi\x1B[0m");
    test!(bunderline:            Style::new().bold().underline();   "hi" => "\x1B[01;04mhi\x1B[0m");
    test!(dimmed:                Style::new().dimmed();             "hi" => "\x1B[02mhi\x1B[0m");
    test!(italic:                Style::new().italic();             "hi" => "\x1B[03mhi\x1B[0m");
    test!(blink:                 Style::new().blink();              "hi" => "\x1B[05mhi\x1B[0m");
    test!(reverse:               Style::new().reverse();            "hi" => "\x1B[07mhi\x1B[0m");
    test!(hidden:                Style::new().hidden();             "hi" => "\x1B[08mhi\x1B[0m");
    test!(stricken:              Style::new().strikethrough();      "hi" => "\x1B[09mhi\x1B[0m");
    test!(lr_on_lr:              LightRed.on(LightRed);             "hi" => "\x1B[101;91mhi\x1B[0m");
}
