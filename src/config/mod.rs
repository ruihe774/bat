use std::env::{self, VarError};
use std::io::{self, IsTerminal};
use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

use crate::assets::syntax_mapping::SyntaxMapping;
use crate::controller::line_range::{HighlightedLineRanges, VisibleLines};
use crate::error::*;
use crate::input::{Input, InputKind};
#[cfg(feature = "paging")]
use crate::output::pager::PagingMode;
use crate::printer::preprocessor::NonprintableNotation;
use crate::printer::style::{ExpandedStyleComponents, StyleComponents};
use crate::printer::WrappingMode;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config<'a> {
    /// The explicitly configured language, if any
    #[serde(default)]
    pub language: Option<&'a str>,

    /// The configured notation for non-printable characters
    #[serde(default)]
    pub nonprintable_notation: Option<NonprintableNotation>,

    /// The character width of the terminal
    #[serde(default)]
    pub term_width: Option<NonZeroUsize>,

    /// The width of tab characters
    /// None will cause tabs to be passed through without expanding them.
    #[serde(default)]
    pub tab_width: Option<NonZeroUsize>,

    /// Whether or not to simply loop through all input (`cat` mode)
    #[serde(default)]
    pub loop_through: Option<bool>,

    /// Whether or not the output should be colorized
    #[serde(default)]
    pub colored_output: Option<bool>,

    /// Whether or not the output terminal supports true color
    #[serde(default)]
    pub true_color: Option<bool>,

    /// Style elements (grid, line numbers, ...)
    #[serde(default)]
    pub style_components: StyleComponents,

    /// If and how text should be wrapped
    #[serde(default)]
    pub wrapping_mode: Option<WrappingMode>,

    /// Pager or STDOUT
    #[cfg(feature = "paging")]
    #[serde(default)]
    pub paging_mode: Option<PagingMode>,

    /// Specifies which lines should be printed
    #[serde(default)]
    pub visible_lines: VisibleLines,

    /// The syntax highlighting theme
    #[serde(default)]
    pub theme: Option<&'a str>,

    /// File extension/name mappings
    #[serde(skip)]
    pub syntax_mapping: SyntaxMapping<'a>,

    /// Command to start the pager
    #[serde(default)]
    pub pager: Option<&'a str>,

    /// Whether or not to use ANSI italics
    #[serde(default)]
    pub use_italic_text: bool,

    /// Ranges of lines which should be highlighted with a special background color
    #[serde(default)]
    pub highlighted_lines: HighlightedLineRanges,

    /// Whether or not to use $LESSOPEN if set
    #[cfg(feature = "lessopen")]
    #[serde(default = "default_true")]
    pub use_lessopen: bool,
}

fn default_true() -> bool {
    true
}

impl<'a> Config<'a> {
    pub fn consolidate(self, inputs: &'_ [Input]) -> ConsolidatedConfig<'a> {
        let stdout = io::stdout();
        let interactive = stdout.is_terminal();
        let style = self.style_components.expand(interactive).unwrap();
        let plain = style.plain();
        ConsolidatedConfig {
            language: self.language,
            nonprintable_notation: self.nonprintable_notation,
            term_width: self.term_width.unwrap_or_else(|| {
                interactive
                    .then(|| console::Term::stdout().size().1)
                    .and_then(|width| NonZeroUsize::try_from(width as usize).ok())
                    .unwrap_or(NonZeroUsize::new(100).unwrap())
            }),
            tab_width: self.tab_width,
            loop_through: self.loop_through.unwrap_or_else(|| {
                !interactive && !self.colored_output.unwrap_or_default() && style.plain()
            }),
            colored_output: self.colored_output.unwrap_or(interactive),
            true_color: self.true_color.unwrap_or_else(|| {
                env::var("COLORTERM")
                    .map(|colorterm| colorterm == "truecolor" || colorterm == "24bit")
                    .unwrap_or_default()
            }),
            style_components: style,
            wrapping_mode: self.wrapping_mode.unwrap_or_else(|| {
                if plain {
                    WrappingMode::NoWrapping
                } else {
                    WrappingMode::Character
                }
            }),
            #[cfg(feature = "paging")]
            paging_mode: self.paging_mode.unwrap_or_else(|| {
                if interactive
                    && (!inputs
                        .iter()
                        .any(|input| matches!(input.kind, InputKind::StdIn))
                        || !io::stdin().is_terminal())
                {
                    PagingMode::QuitIfOneScreen
                } else {
                    PagingMode::Never
                }
            }),
            visible_lines: self.visible_lines,
            theme: self.theme,
            syntax_mapping: self.syntax_mapping,
            pager: self.pager,
            use_italic_text: self.use_italic_text,
            highlighted_lines: self.highlighted_lines,
            #[cfg(feature = "lessopen")]
            use_lessopen: self.use_lessopen,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConsolidatedConfig<'a> {
    pub language: Option<&'a str>,
    pub nonprintable_notation: Option<NonprintableNotation>,
    pub term_width: NonZeroUsize,
    pub tab_width: Option<NonZeroUsize>,
    pub loop_through: bool,
    pub colored_output: bool,
    pub true_color: bool,
    pub style_components: ExpandedStyleComponents,
    pub wrapping_mode: WrappingMode,
    #[cfg(feature = "paging")]
    pub paging_mode: PagingMode,
    pub visible_lines: VisibleLines,
    pub theme: Option<&'a str>,
    pub syntax_mapping: SyntaxMapping<'a>,
    pub pager: Option<&'a str>,
    pub use_italic_text: bool,
    pub highlighted_lines: HighlightedLineRanges,
    #[cfg(feature = "lessopen")]
    pub use_lessopen: bool,
}

pub(crate) fn get_env_var(key: &str) -> Result<Option<String>> {
    match env::var(key) {
        Ok(value) => Ok((!value.is_empty()).then_some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(e @ VarError::NotUnicode(_)) => Err(e)
            .with_context(|| format!("the value of environment variable '{}' is not unicode", key)),
    }
}

#[cfg(all(feature = "minimal-application", feature = "paging", feature = "bugreport"))]
pub fn get_pager_executable(config_pager: Option<&str>) -> Option<String> {
    crate::output::pager::get_pager(config_pager)
        .ok()
        .flatten()
        .map(|pager| pager.bin)
}
