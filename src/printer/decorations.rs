use std::io;

use crate::printer::Colors;
use nu_ansi_term::Style;

use super::OutputHandle;

pub(crate) trait Decoration {
    fn print(
        &mut self,
        line_number: usize,
        continuation: bool,
        handle: OutputHandle,
    ) -> io::Result<usize>;
    fn width(&self) -> usize;
}

#[derive(Debug)]
pub(crate) struct LineNumberDecoration {
    color: Style,
    cached_width: usize,
    cached_width_invalid_at: usize,
}

impl LineNumberDecoration {
    pub(crate) fn new(colors: &Colors) -> Self {
        LineNumberDecoration {
            color: colors.line_number,
            cached_width: 4,
            cached_width_invalid_at: 10000,
        }
    }
}

impl Decoration for LineNumberDecoration {
    fn print(
        &mut self,
        line_number: usize,
        continuation: bool,
        handle: OutputHandle,
    ) -> io::Result<usize> {
        if line_number >= self.cached_width_invalid_at {
            self.cached_width += 1;
            self.cached_width_invalid_at *= 10;
        }
        write!(handle, "{}", self.color.prefix())?;
        if continuation {
            for _ in 0..self.cached_width {
                write!(handle, " ")?;
            }
        } else {
            write!(handle, "{:4}", line_number)?;
        }
        write!(handle, "{}", self.color.suffix())?;
        Ok(self.cached_width)
    }

    fn width(&self) -> usize {
        self.cached_width
    }
}

#[derive(Debug)]
pub(crate) struct GridBorderDecoration {
    color: Style,
}

impl GridBorderDecoration {
    pub(crate) fn new(colors: &Colors) -> Self {
        GridBorderDecoration { color: colors.grid }
    }
}

impl Decoration for GridBorderDecoration {
    fn print(
        &mut self,
        _line_number: usize,
        _continuation: bool,
        handle: OutputHandle,
    ) -> io::Result<usize> {
        write!(handle, "{}│{}", self.color.prefix(), self.color.suffix())?;
        Ok(1)
    }

    fn width(&self) -> usize {
        1
    }
}
