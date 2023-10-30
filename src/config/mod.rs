use std::env::{self, VarError};
use std::io::{self, IsTerminal};
use std::num::NonZeroUsize;

use serde::{Deserialize, Serialize};

use crate::assets::syntax_mapping::{ConsolidatedSyntaxMapping, SyntaxMapping};
use crate::controller::line_range::{HighlightedLineRanges, VisibleLines};
use crate::error::{Context, Result};
use crate::input::{Input, InputKind};
#[cfg(feature = "paging")]
use crate::output::pager::PagingMode;
use crate::printer::preprocessor::NonprintableNotation;
use crate::printer::style::{ConsolidatedStyleComponents, StyleComponents};
use crate::printer::{TabWidth, WrappingMode};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The explicitly configured language, if any
    pub language: Option<String>,

    /// The configured notation for non-printable characters
    pub nonprintable_notation: Option<NonprintableNotation>,

    /// The character width of the terminal
    pub term_width: Option<NonZeroUsize>,

    /// The width of tab characters
    /// None will cause tabs to be passed through without expanding them.
    pub tab_width: TabWidth,

    /// Whether or not to simply loop through all input (`cat` mode)
    pub loop_through: Option<bool>,

    /// Whether or not the output should be colorized
    pub colored_output: Option<bool>,

    /// Whether or not the output terminal supports true color
    pub true_color: Option<bool>,

    /// Style elements (grid, line numbers, ...)
    pub style_components: StyleComponents,

    /// If and how text should be wrapped
    pub wrapping_mode: Option<WrappingMode>,

    /// Pager or STDOUT
    #[cfg(feature = "paging")]
    pub paging_mode: Option<PagingMode>,

    /// Specifies which lines should be printed
    pub visible_lines: VisibleLines,

    /// The syntax highlighting theme
    pub theme: Option<String>,

    /// File extension/name mappings
    pub syntax_mapping: SyntaxMapping,

    /// Command to start the pager
    pub pager: Option<String>,

    /// Whether or not to use ANSI italics
    pub use_italic_text: bool,

    /// Ranges of lines which should be highlighted with a special background color
    pub highlighted_lines: HighlightedLineRanges,

    /// Always show decorations
    pub always_show_decorations: bool,

    /// Whether or not to use $LESSOPEN if set
    #[cfg(feature = "lessopen")]
    pub no_lessopen: bool,
}

impl Config {
    pub fn consolidate(self, inputs: &'_ [Input]) -> Result<ConsolidatedConfig> {
        let stdout = io::stdout();
        let is_terminal = stdout.is_terminal();
        let interactive = is_terminal || self.always_show_decorations;
        let style = self.style_components.consolidate(interactive)?;
        let plain = style.plain();
        Ok(ConsolidatedConfig {
            language: self.language,
            nonprintable_notation: self.nonprintable_notation,
            term_width: self.term_width.unwrap_or_else(|| {
                is_terminal
                    .then(|| console::Term::stdout().size().1)
                    .and_then(|width| NonZeroUsize::try_from(width as usize).ok())
                    .unwrap_or(NonZeroUsize::new(100).unwrap())
            }),
            tab_width: self.tab_width,
            loop_through: self.loop_through.unwrap_or_else(|| {
                !interactive && !self.colored_output.unwrap_or_default() && style.plain()
            }),
            colored_output: self
                .colored_output
                .unwrap_or_else(|| is_terminal && env::var_os("NO_COLOR").is_none()),
            true_color: self.true_color.unwrap_or_else(|| {
                env::var("COLORTERM")
                    .ok()
                    .is_some_and(|colorterm| colorterm == "truecolor" || colorterm == "24bit")
            }),
            style_components: style,
            wrapping_mode: self.wrapping_mode.unwrap_or(if plain {
                WrappingMode::NoWrapping
            } else {
                WrappingMode::Character
            }),
            #[cfg(feature = "paging")]
            paging_mode: self.paging_mode.unwrap_or_else(|| {
                if is_terminal
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
            syntax_mapping: self.syntax_mapping.consolidate()?,
            pager: self.pager,
            use_italic_text: self.use_italic_text,
            highlighted_lines: self.highlighted_lines,
            always_show_decorations: self.always_show_decorations,
            #[cfg(feature = "lessopen")]
            no_lessopen: self.no_lessopen,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ConsolidatedConfig {
    pub language: Option<String>,
    pub nonprintable_notation: Option<NonprintableNotation>,
    pub term_width: NonZeroUsize,
    pub tab_width: TabWidth,
    pub loop_through: bool,
    pub colored_output: bool,
    pub true_color: bool,
    pub style_components: ConsolidatedStyleComponents,
    pub wrapping_mode: WrappingMode,
    #[cfg(feature = "paging")]
    pub paging_mode: PagingMode,
    pub visible_lines: VisibleLines,
    pub theme: Option<String>,
    pub syntax_mapping: ConsolidatedSyntaxMapping,
    pub pager: Option<String>,
    pub use_italic_text: bool,
    pub highlighted_lines: HighlightedLineRanges,
    pub always_show_decorations: bool,
    #[cfg(feature = "lessopen")]
    pub no_lessopen: bool,
}

pub(crate) fn get_env_var(key: &str) -> Result<Option<String>> {
    match env::var(key) {
        Ok(value) => Ok((!value.is_empty()).then_some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(e @ VarError::NotUnicode(_)) => Err(e)
            .with_context(|| format!("the value of environment variable '{key}' is not unicode")),
    }
}

#[cfg(all(
    feature = "minimal-application",
    feature = "paging",
    feature = "bugreport"
))]
#[doc(hidden)]
pub fn get_pager_executable(config_pager: Option<&str>) -> Option<String> {
    crate::output::pager::get_pager(config_pager)
        .ok()
        .flatten()
        .map(|pager| pager.bin)
}
