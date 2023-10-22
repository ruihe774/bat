//! `bat` is a library to print syntax highlighted content.
//!
//! The main struct of this crate is `PrettyPrinter` which can be used to
//! configure and run the syntax highlighting.
//!
//! If you need more control, you can also use the structs in the submodules
//! (start with `controller::Controller`), but note that the API of these
//! internal modules is much more likely to change. Some or all of these
//! modules might be removed in the future.

mod macros;

pub mod assets;
pub mod config;
pub mod controller;
mod decorations;
#[cfg(feature = "git")]
mod diff;
pub mod error;
#[cfg(feature = "guesslang")]
mod guesslang;
pub mod input;
mod less;
#[cfg(feature = "lessopen")]
mod lessopen;
pub mod line_range;
pub(crate) mod nonprintable_notation;
mod output;
#[cfg(feature = "paging")]
mod pager;
#[cfg(feature = "paging")]
pub(crate) mod paging;
mod preprocessor;
mod pretty_printer;
pub(crate) mod printer;
pub mod style;
pub(crate) mod syntax_mapping;
mod terminal;
mod vscreen;
pub(crate) mod wrapping;
#[cfg(feature = "zero-copy")]
mod zero_copy;

pub use input::Input;
pub use nonprintable_notation::NonprintableNotation;
pub use pretty_printer::{PrettyPrinter, Syntax};
pub use syntax_mapping::{MappingTarget, SyntaxMapping};
pub use wrapping::WrappingMode;

#[cfg(feature = "paging")]
pub use paging::PagingMode;
