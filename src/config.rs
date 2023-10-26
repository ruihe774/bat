use serde::{Deserialize, Serialize};

use crate::controller::VisibleLines;
use crate::line_range::HighlightedLineRanges;
#[cfg(feature = "paging")]
use crate::pager::PagingMode;
use crate::preprocessor::NonprintableNotation;
use crate::printer::WrappingMode;
use crate::style::StyleComponents;
use crate::syntax_mapping::SyntaxMapping;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config<'a> {
    /// The explicitly configured language, if any
    pub language: Option<&'a str>,

    /// The configured notation for non-printable characters
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
    pub paging_mode: PagingMode,

    /// Specifies which lines should be printed
    pub visible_lines: VisibleLines,

    /// The syntax highlighting theme
    pub theme: Option<String>,

    /// File extension/name mappings
    #[serde(skip)]
    pub syntax_mapping: SyntaxMapping<'a>,

    /// Command to start the pager
    pub pager: Option<&'a str>,

    /// Whether or not to use ANSI italics
    pub use_italic_text: bool,

    /// Ranges of lines which should be highlighted with a special background color
    pub highlighted_lines: HighlightedLineRanges,

    // Whether or not to use $LESSOPEN if set
    #[cfg(feature = "lessopen")]
    pub use_lessopen: bool,
}

#[cfg(all(feature = "minimal-application", feature = "paging"))]
pub fn get_pager_executable(config_pager: Option<&str>) -> Option<String> {
    crate::pager::get_pager(config_pager)
        .ok()
        .flatten()
        .map(|pager| pager.bin)
}

#[test]
fn default_config_should_include_all_lines() {
    use crate::line_range::RangeCheckResult;

    assert_eq!(LineRanges::default().check(17), RangeCheckResult::InRange);
}

#[test]
fn default_config_should_highlight_no_lines() {
    use crate::line_range::RangeCheckResult;

    assert_ne!(
        Config::default().highlighted_lines.0.check(17),
        RangeCheckResult::InRange
    );
}
