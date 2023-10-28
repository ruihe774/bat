use crate::printer::{Colors, InteractivePrinter};
use nu_ansi_term::Style;

#[derive(Debug, Clone)]
pub(crate) struct DecorationText {
    pub width: usize,
    pub text: String,
}

pub(crate) trait Decoration {
    fn generate(
        &self,
        line_number: usize,
        continuation: bool,
        printer: &InteractivePrinter,
    ) -> DecorationText;
    fn width(&self) -> usize;
}

pub(crate) struct LineNumberDecoration {
    color: Style,
    cached_wrap: DecorationText,
    cached_wrap_invalid_at: usize,
}

impl LineNumberDecoration {
    pub(crate) fn new(colors: &Colors) -> Self {
        LineNumberDecoration {
            color: colors.line_number,
            cached_wrap_invalid_at: 10000,
            cached_wrap: DecorationText {
                text: colors.line_number.paint(" ".repeat(4)).to_string(),
                width: 4,
            },
        }
    }
}

impl Decoration for LineNumberDecoration {
    fn generate(
        &self,
        line_number: usize,
        continuation: bool,
        _printer: &InteractivePrinter,
    ) -> DecorationText {
        if continuation {
            if line_number > self.cached_wrap_invalid_at {
                let new_width = self.cached_wrap.width + 1;
                return DecorationText {
                    text: self.color.paint(" ".repeat(new_width)).to_string(),
                    width: new_width,
                };
            }

            self.cached_wrap.clone()
        } else {
            let plain: String = format!("{:4}", line_number);
            DecorationText {
                width: plain.len(),
                text: self.color.paint(plain).to_string(),
            }
        }
    }

    fn width(&self) -> usize {
        4
    }
}

pub(crate) struct GridBorderDecoration {
    cached: DecorationText,
}

impl GridBorderDecoration {
    pub(crate) fn new(colors: &Colors) -> Self {
        GridBorderDecoration {
            cached: DecorationText {
                text: colors.grid.paint("│").to_string(),
                width: 1,
            },
        }
    }
}

impl Decoration for GridBorderDecoration {
    fn generate(
        &self,
        _line_number: usize,
        _continuation: bool,
        _printer: &InteractivePrinter,
    ) -> DecorationText {
        self.cached.clone()
    }

    fn width(&self) -> usize {
        self.cached.width
    }
}
