use std::borrow::Cow;
use std::io;
use std::io::Write;

use console::AnsiCodeIterator;
use nu_ansi_term::{Color as TermColor, Style};
use serde::{Deserialize, Serialize};
use syntect::easy::HighlightLines;
use syntect::highlighting::Color;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;
use unicode_width::UnicodeWidthChar;

use crate::assets::{HighlightingAssets, SyntaxReferenceInSet, SyntaxUndetected};
use crate::config::Config;
use crate::controller::line_range::RangeCheckResult;
use crate::error::*;
use crate::input::{decode, ContentType, OpenedInput};
use preprocessor::{expand_tabs, replace_nonprintable};
use terminal::{to_ansi_color, to_ansi_style};
use vscreen::AnsiStyle;

pub mod preprocessor;
pub mod style;
mod terminal;
mod vscreen;

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WrappingMode {
    Character,
    // The bool specifies whether wrapping has been explicitly disabled by the user via --wrap=never
    #[default]
    NoWrapping,
}

#[allow(type_alias_bounds)]
pub(crate) type OutputHandle<'a, W: Write> = &'a mut W;

pub(crate) trait Printer<W: Write> {
    fn print_header(
        &mut self,
        handle: OutputHandle<W>,
        input: &OpenedInput,
        add_header_padding: bool,
    ) -> Result<()>;
    fn print_footer(&mut self, handle: OutputHandle<W>, input: &OpenedInput) -> Result<()>;

    fn print_snip(&mut self, handle: OutputHandle<W>) -> Result<()>;

    fn print_line(
        &mut self,
        out_of_range: bool,
        handle: OutputHandle<W>,
        line_number: usize,
        line_buffer: &[u8],
    ) -> Result<()>;
}

pub(crate) struct SimplePrinter<'a> {
    config: &'a Config<'a>,
}

impl<'a> SimplePrinter<'a> {
    pub(crate) fn new(config: &'a Config) -> Self {
        SimplePrinter { config }
    }
}

impl<'a, W: Write> Printer<W> for SimplePrinter<'a> {
    fn print_header(
        &mut self,
        _handle: OutputHandle<W>,
        _input: &OpenedInput,
        _add_header_padding: bool,
    ) -> Result<()> {
        Ok(())
    }

    fn print_footer(&mut self, _handle: OutputHandle<W>, _input: &OpenedInput) -> Result<()> {
        Ok(())
    }

    fn print_snip(&mut self, _handle: OutputHandle<W>) -> Result<()> {
        Ok(())
    }

    fn print_line(
        &mut self,
        out_of_range: bool,
        handle: OutputHandle<W>,
        _line_number: usize,
        line_buffer: &[u8],
    ) -> Result<()> {
        if !out_of_range {
            if let Some(nonprintable_notation) = self.config.nonprintable_notation {
                let line =
                    replace_nonprintable(line_buffer, self.config.tab_width, nonprintable_notation);
                write!(handle, "{}", line)?;
            } else {
                handle.write_all(line_buffer)?;
            };
        }
        Ok(())
    }
}

struct HighlighterFromSet<'a> {
    highlighter: HighlightLines<'a>,
    syntax_set: &'a SyntaxSet,
}

impl<'a> HighlighterFromSet<'a> {
    fn new(syntax_in_set: SyntaxReferenceInSet<'a>, theme: &'a Theme) -> Self {
        Self {
            highlighter: HighlightLines::new(syntax_in_set.syntax, theme),
            syntax_set: syntax_in_set.syntax_set,
        }
    }
}

pub(crate) struct InteractivePrinter<'a> {
    colors: Colors,
    config: &'a Config<'a>,
    panel_width: usize,
    ansi_style: AnsiStyle,
    content_type: Option<ContentType>,
    highlighter_from_set: Option<HighlighterFromSet<'a>>,
    background_color_highlight: Option<Color>,
    line_number_width: usize,
    line_number_width_invalid_at: usize,
}

impl<'a> InteractivePrinter<'a> {
    pub(crate) fn new(
        config: &'a Config,
        assets: &'a HighlightingAssets,
        input: &mut OpenedInput,
    ) -> Result<Self> {
        let theme = config.theme.as_ref().map_or_else(
            || Ok(assets.get_default_theme()),
            |name| assets.get_theme(name),
        )?;

        let background_color_highlight = theme.settings.line_highlight;

        let colors = if config.colored_output {
            Colors::colored(theme, config.true_color)
        } else {
            Colors::plain()
        };

        // Create decorations.
        let mut panel_width: usize = Self::get_panel_width(config);

        // Disable the panel if the terminal is too small
        if config.term_width < panel_width + 7 {
            panel_width = 0;
        }

        let highlighter_from_set = if input.reader.content_type.as_ref().map_or(false, |c| {
            c.is_binary() && config.nonprintable_notation.is_none()
        }) {
            None
        } else {
            // Determine the type of syntax for highlighting
            let syntax_in_set =
                match assets.get_syntax(config.language, input, &config.syntax_mapping) {
                    Ok(syntax_in_set) => syntax_in_set,
                    Err(e) if e.downcast_ref::<SyntaxUndetected>().is_some() => {
                        assets.get_fallback_syntax()
                    }
                    Err(e) => return Err(e),
                };

            Some(HighlighterFromSet::new(syntax_in_set, theme))
        };

        Ok(InteractivePrinter {
            panel_width,
            colors,
            config,
            content_type: input.reader.content_type.clone(),
            ansi_style: AnsiStyle::new(),
            highlighter_from_set,
            background_color_highlight,
            line_number_width: 4,
            line_number_width_invalid_at: 10000,
        })
    }

    pub(crate) fn get_panel_width(config: &'a Config) -> usize {
        if config.style_components.numbers() {
            5
        } else {
            0
        }
    }

    fn print_horizontal_line_term<W: Write>(
        &self,
        handle: OutputHandle<W>,
        style: Style,
    ) -> io::Result<()> {
        write!(handle, "{}", style.prefix())?;
        for _ in 0..self.config.term_width {
            write!(handle, "─")?;
        }
        writeln!(handle, "{}", style.suffix())?;
        Ok(())
    }

    fn print_horizontal_line<W: Write>(
        &self,
        handle: OutputHandle<W>,
        grid_char: char,
    ) -> io::Result<()> {
        if self.panel_width == 0 {
            self.print_horizontal_line_term(handle, self.colors.grid)?;
        } else {
            write!(handle, "{}", self.colors.grid.prefix())?;
            for _ in 0..self.panel_width {
                write!(handle, "─")?;
            }
            write!(handle, "{}", grid_char)?;
            for _ in 0..(self.config.term_width - (self.panel_width + 1)) {
                write!(handle, "─")?;
            }
            writeln!(handle, "{}", self.colors.grid.suffix())?;
        }

        Ok(())
    }

    fn print_header_component_indent<W: Write>(&self, handle: OutputHandle<W>) -> io::Result<()> {
        for _ in 0..self.panel_width {
            write!(handle, " ")?;
        }
        if self.config.style_components.grid() {
            write!(
                handle,
                "{}{}{}",
                self.colors.grid.prefix(),
                if self.panel_width > 0 { "│ " } else { "" },
                self.colors.grid.suffix(),
            )?;
        }
        Ok(())
    }

    fn print_line_number<W: Write>(
        &mut self,
        line_number: usize,
        continuation: bool,
        handle: OutputHandle<W>,
    ) -> io::Result<usize> {
        if line_number >= self.line_number_width_invalid_at {
            self.line_number_width += 1;
            self.line_number_width_invalid_at *= 10;
        }
        write!(handle, "{}", self.colors.line_number.prefix())?;
        if continuation {
            for _ in 0..self.line_number_width {
                write!(handle, " ")?;
            }
        } else {
            write!(handle, "{:4}", line_number)?;
        }
        write!(handle, "{}", self.colors.line_number.suffix())?;
        Ok(self.line_number_width)
    }

    fn print_grid<W: Write>(&mut self, handle: OutputHandle<W>) -> io::Result<usize> {
        write!(
            handle,
            "{}│{}",
            self.colors.grid.prefix(),
            self.colors.grid.suffix()
        )?;
        Ok(1)
    }

    fn print_decorations<W: Write>(
        &mut self,
        line_number: usize,
        continuation: bool,
        handle: OutputHandle<W>,
    ) -> io::Result<usize> {
        let mut len = 0;
        if self.panel_width != 0 {
            if self.config.style_components.numbers() {
                len += self.print_line_number(line_number, continuation, handle)?;
                write!(handle, " ")?;
                len += 1;
            }
            if self.config.style_components.grid() {
                len += self.print_grid(handle)?;
                write!(handle, " ")?;
                len += 1;
            }
        }
        Ok(len)
    }

    fn preprocess<'b>(&self, text: &'b str, cursor: &mut usize) -> Cow<'b, str> {
        if self.config.tab_width != 0 {
            expand_tabs(text, self.config.tab_width, cursor)
        } else {
            *cursor += text.len();
            text.into()
        }
    }
}

impl<'a, W: Write> Printer<W> for InteractivePrinter<'a> {
    fn print_header(
        &mut self,
        handle: OutputHandle<W>,
        input: &OpenedInput,
        add_header_padding: bool,
    ) -> Result<()> {
        if add_header_padding && self.config.style_components.rule() {
            self.print_horizontal_line_term(handle, self.colors.rule)?;
        }

        if !self.config.style_components.header() {
            if self
                .content_type
                .as_ref()
                .map_or(false, |content_type| content_type.is_binary())
                && self.config.nonprintable_notation.is_none()
            {
                writeln!(
                    handle,
                    "{}: Binary content from {} will not be printed to the terminal \
                     (but will be present if the output of 'bat' is piped). You can use 'bat -A' \
                     to show the binary file contents.",
                    TermColor::Yellow.paint("[bat warning]"),
                    if &input.description.kind == "File" {
                        format!(
                            "file '{}'",
                            input
                                .description
                                .name
                                .as_ref()
                                .expect("file must have a name")
                                .to_string_lossy()
                                .as_ref()
                        )
                    } else {
                        input.description.kind.to_owned()
                    },
                )?;
            } else if self.config.style_components.grid() {
                self.print_horizontal_line(handle, '┬')?;
            }
            return Ok(());
        }

        let description = &input.description;

        // Print the cornering grid before the first header component
        if self.config.style_components.grid() {
            self.print_horizontal_line(handle, '┬')?;
        } else {
            // Only pad space between files, if we haven't already drawn a horizontal rule
            if add_header_padding && !self.config.style_components.rule() {
                writeln!(handle)?;
            }
        }

        self.print_header_component_indent(handle)?;
        if self.config.style_components.header_filename() {
            if let Some(name) = description.name.as_ref() {
                write!(
                    handle,
                    "{}: {}{}{}",
                    description.kind.as_str(),
                    self.colors.header_value.prefix(),
                    name.to_string_lossy(),
                    self.colors.header_value.suffix()
                )?;
            } else {
                write!(
                    handle,
                    "{}{}{}",
                    self.colors.header_value.prefix(),
                    description.kind.as_str(),
                    self.colors.header_value.suffix()
                )?;
            }
            write!(
                handle,
                "{}",
                match self.content_type {
                    Some(ContentType::Binary(_)) => "   <BINARY>",
                    Some(ContentType::UTF_16LE) => "   <UTF-16LE>",
                    Some(ContentType::UTF_16BE) => "   <UTF-16BE>",
                    Some(ContentType::UTF_32LE) => "   <UTF-32LE>",
                    Some(ContentType::UTF_32BE) => "   <UTF-32BE>",
                    Some(ContentType::UTF_8) => "",
                    None => "   <EMPTY>",
                },
            )?;
            if let Some(ContentType::Binary(Some(ref binary_type))) = self.content_type {
                writeln!(handle, " {}", binary_type)?;
            } else {
                writeln!(handle)?;
            }
        };

        if self.config.style_components.grid() {
            if self.content_type.as_ref().map_or(false, |c| c.is_text())
                || self.config.nonprintable_notation.is_some()
            {
                self.print_horizontal_line(handle, '┼')?;
            } else {
                self.print_horizontal_line(handle, '┴')?;
            }
        }

        Ok(())
    }

    fn print_footer(&mut self, handle: OutputHandle<W>, _input: &OpenedInput) -> Result<()> {
        if self.config.style_components.grid()
            && (self.content_type.as_ref().map_or(false, |c| c.is_text())
                || self.config.nonprintable_notation.is_some())
        {
            Ok(self.print_horizontal_line(handle, '┴')?)
        } else {
            Ok(())
        }
    }

    fn print_snip(&mut self, handle: OutputHandle<W>) -> Result<()> {
        write!(handle, "{}", self.colors.grid.prefix())?;

        let panel_text = " ...";
        let panel_count = if self.panel_width != 0 {
            let text_truncated = &panel_text[..(self.panel_width - 1)];
            write!(handle, "{}", text_truncated)?;
            for _ in 0..(self.panel_width - 1 - text_truncated.len()) {
                write!(handle, " ")?;
            }
            if self.config.style_components.grid() {
                write!(handle, " │ ")?;
                self.panel_width + 2
            } else {
                self.panel_width - 1
            }
        } else {
            0
        };

        let title = "8<";
        let title_count = 2;

        let snip_left_count = (self.config.term_width - panel_count - (title_count / 2)) / 4;
        for _ in 0..snip_left_count {
            write!(handle, "─ ")?;
        }
        let snip_left_count = snip_left_count * 2;

        write!(handle, "{}", title)?;

        for _ in 0..((self.config.term_width - panel_count - snip_left_count - title_count) / 2) {
            write!(handle, " ─")?;
        }

        writeln!(handle, "{}", self.colors.grid.suffix())?;

        Ok(())
    }

    fn print_line(
        &mut self,
        out_of_range: bool,
        handle: OutputHandle<W>,
        line_number: usize,
        line_buffer: &[u8],
    ) -> Result<()> {
        let line = if let Some(nonprintable_notation) = self.config.nonprintable_notation {
            replace_nonprintable(line_buffer, self.config.tab_width, nonprintable_notation).into()
        } else {
            match self
                .content_type
                .as_ref()
                .and_then(|content_type| decode(line_buffer, content_type, line_number == 1))
            {
                Some(line) => line,
                None => return Ok(()),
            }
        };

        let regions = {
            let highlighter_from_set = match self.highlighter_from_set {
                Some(ref mut highlighter_from_set) => highlighter_from_set,
                _ => return Ok(()),
            };

            // skip syntax highlighting on long lines
            let too_long = line.len() > 8192;

            let for_highlighting: &str = if too_long { "\n" } else { &line };

            let mut highlighted_line = highlighter_from_set
                .highlighter
                .highlight_line(for_highlighting, highlighter_from_set.syntax_set)?;

            if too_long {
                highlighted_line[0].1 = &line;
            }

            highlighted_line
        };

        if out_of_range {
            return Ok(());
        }

        let mut cursor: usize = 0;
        let mut cursor_max: usize = self.config.term_width;
        let mut cursor_total: usize = 0;

        // Line highlighting
        let highlight_this_line =
            self.config.highlighted_lines.0.check(line_number) == RangeCheckResult::InRange;

        if highlight_this_line
            && self
                .config
                .theme
                .as_ref()
                .map(|name| name == "ansi")
                .unwrap_or(false)
        {
            self.ansi_style.update("^[4m");
        }

        let background_color = self
            .background_color_highlight
            .filter(|_| highlight_this_line);

        // Line decorations.
        cursor_max -= self.print_decorations(line_number, false, handle)?;

        // Line contents.
        let true_color = self.config.true_color;
        let colored_output = self.config.colored_output;
        let italics = self.config.use_italic_text;
        if self.config.wrapping_mode == WrappingMode::NoWrapping {
            for &(style, region) in &regions {
                let ansi_iterator = AnsiCodeIterator::new(region);
                for chunk in ansi_iterator {
                    match chunk {
                        // ANSI escape passthrough.
                        (ansi, true) => {
                            self.ansi_style.update(ansi);
                            write!(handle, "{}", ansi)?;
                        }

                        // Regular text.
                        (text, false) => {
                            let text_cow = self.preprocess(text, &mut cursor_total);
                            let text = text_cow.as_ref();
                            let text_trimmed = text.trim_end_matches(|c| c == '\r' || c == '\n');

                            if !text_trimmed.is_empty() {
                                let style = to_ansi_style(
                                    style,
                                    true_color,
                                    colored_output,
                                    italics,
                                    background_color,
                                );
                                write!(
                                    handle,
                                    "{}{}{}{}",
                                    style.prefix(),
                                    &self.ansi_style,
                                    text_trimmed,
                                    style.suffix()
                                )?;
                            }

                            if text.len() != text_trimmed.len() {
                                if let Some(background_color) = background_color {
                                    let ansi_style = Style {
                                        background: to_ansi_color(background_color, true_color),
                                        ..Default::default()
                                    };

                                    if let Some(width) =
                                        cursor_max.checked_sub(cursor_total).map(|width| width + 1)
                                    {
                                        write!(handle, "{}", ansi_style.prefix())?;
                                        for _ in 0..width {
                                            write!(handle, " ")?;
                                        }
                                        write!(handle, "{}", ansi_style.suffix())?;
                                    }
                                }
                                write!(handle, "{}", &text[text_trimmed.len()..])?;
                            }
                        }
                    }
                }
            }

            if !self.config.style_components.plain() && line.bytes().next_back() != Some(b'\n') {
                writeln!(handle)?;
            }
        } else {
            for &(style, region) in &regions {
                let ansi_iterator = AnsiCodeIterator::new(region);
                for chunk in ansi_iterator {
                    match chunk {
                        // ANSI escape passthrough.
                        (ansi, true) => {
                            self.ansi_style.update(ansi);
                            write!(handle, "{}", ansi)?;
                        }

                        // Regular text.
                        (text, false) => {
                            let text = self.preprocess(
                                text.trim_end_matches(|c| c == '\r' || c == '\n'),
                                &mut cursor_total,
                            );

                            let mut max_width = cursor_max - cursor;
                            let mut current_width = 0;

                            let style = to_ansi_style(
                                style,
                                true_color,
                                colored_output,
                                italics,
                                background_color,
                            );
                            write!(handle, "{}{}", style.prefix(), &self.ansi_style)?;

                            for c in text.chars() {
                                // calculate the displayed width for next character
                                let cw = c.width().unwrap_or(0);
                                current_width += cw;

                                // if next character cannot be printed on this line,
                                // flush the buffer.
                                if current_width > max_width {
                                    // It wraps.
                                    writeln!(handle, "{}", style.suffix())?;

                                    self.print_decorations(line_number, true, handle)?;

                                    write!(handle, "{}{}", style.prefix(), &self.ansi_style)?;

                                    cursor = 0;
                                    max_width = cursor_max;
                                    current_width = cw;
                                }

                                write!(handle, "{}", c)?;
                            }

                            // flush the buffer
                            cursor += current_width;
                            write!(handle, "{}", style.suffix())?;
                        }
                    }
                }
            }

            if let Some(background_color) = background_color {
                let ansi_style = Style {
                    background: to_ansi_color(background_color, true_color),
                    ..Default::default()
                };

                let width = cursor_max - cursor;
                if width != 0 {
                    write!(handle, "{}", ansi_style.prefix())?;
                    for _ in 0..width {
                        write!(handle, " ")?;
                    }
                    write!(handle, "{}", ansi_style.suffix())?;
                }
            }
            writeln!(handle)?;
        }

        if highlight_this_line
            && self
                .config
                .theme
                .as_ref()
                .map(|name| name == "ansi")
                .unwrap_or(false)
        {
            self.ansi_style.update("^[24m");
            write!(handle, "\x1B[24m")?;
        }

        Ok(())
    }
}

const DEFAULT_GUTTER_COLOR: u8 = 238;

#[derive(Debug, Default)]
pub(crate) struct Colors {
    pub grid: Style,
    pub rule: Style,
    pub header_value: Style,
    pub line_number: Style,
}

impl Colors {
    fn plain() -> Self {
        Colors::default()
    }

    fn colored(theme: &Theme, true_color: bool) -> Self {
        let gutter_style = Style {
            foreground: match theme.settings.gutter_foreground {
                // If the theme provides a gutter foreground color, use it.
                // Note: It might be the special value #00000001, in which case
                // to_ansi_color returns None and we use an empty Style
                // (resulting in the terminal's default foreground color).
                Some(c) => to_ansi_color(c, true_color),
                // Otherwise, use a specific fallback color.
                None => Some(TermColor::Fixed(DEFAULT_GUTTER_COLOR)),
            },
            ..Style::default()
        };

        Colors {
            grid: gutter_style,
            rule: gutter_style,
            header_value: Style::new().bold(),
            line_number: gutter_style,
        }
    }
}
