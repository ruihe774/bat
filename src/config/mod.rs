use std::env::{self, VarError};

use serde::{Deserialize, Serialize};

use crate::assets::syntax_mapping::SyntaxMapping;
use crate::controller::line_range::{HighlightedLineRanges, VisibleLines};
use crate::error::*;
#[cfg(feature = "paging")]
use crate::output::pager::PagingMode;
use crate::printer::preprocessor::NonprintableNotation;
use crate::printer::style::StyleComponents;
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
    pub term_width: usize,

    /// The width of tab characters.
    /// Currently, a value of 0 will cause tabs to be passed through without expanding them.
    pub tab_width: usize,

    /// Whether or not to simply loop through all input (`cat` mode)
    pub loop_through: bool,

    /// Whether or not the output should be colorized
    pub colored_output: bool,

    /// Whether or not the output terminal supports true color
    pub true_color: bool,

    /// Style elements (grid, line numbers, ...)
    pub style_components: StyleComponents,

    /// If and how text should be wrapped
    pub wrapping_mode: WrappingMode,

    /// Pager or STDOUT
    #[cfg(feature = "paging")]
    #[serde(default)]
    pub paging_mode: Option<PagingMode>,

    /// Specifies which lines should be printed
    #[serde(default)]
    pub visible_lines: VisibleLines,

    /// The syntax highlighting theme
    #[serde(default)]
    pub theme: Option<String>,

    /// File extension/name mappings
    #[serde(skip)]
    pub syntax_mapping: SyntaxMapping<'a>,

    /// Command to start the pager
    #[serde(default)]
    pub pager: Option<&'a str>,

    /// Whether or not to use ANSI italics
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

pub(crate) fn get_env_var(key: &str) -> Result<Option<String>> {
    match env::var(key) {
        Ok(value) => Ok((!value.is_empty()).then_some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(e @ VarError::NotUnicode(_)) => Err(e)
            .with_context(|| format!("the value of environment variable '{}' is not unicode", key)),
    }
}

#[cfg(all(feature = "minimal-application", feature = "paging"))]
pub fn get_pager_executable(config_pager: Option<&str>) -> Option<String> {
    crate::output::pager::get_pager(config_pager)
        .ok()
        .flatten()
        .map(|pager| pager.bin)
}

#[test]
fn default_config_should_include_all_lines() {
    use crate::controller::line_range::{LineRanges, RangeCheckResult};

    assert_eq!(LineRanges::all().check(17), RangeCheckResult::InRange);
}

#[test]
fn default_config_should_highlight_no_lines() {
    use crate::controller::line_range::RangeCheckResult;

    assert_ne!(
        Config::default().highlighted_lines.0.check(17),
        RangeCheckResult::InRange
    );
}
