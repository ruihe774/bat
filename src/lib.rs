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
mod output;
#[cfg(feature = "paging")]
mod pager;
#[cfg(feature = "paging")]
mod paging;
mod preprocessor;
mod printer;
pub mod style;
mod syntax_mapping;
mod terminal;
mod vscreen;
mod wrapping;
#[cfg(feature = "zero-copy")]
mod zero_copy;

pub use input::Input;
pub use preprocessor::NonprintableNotation;
pub use syntax_mapping::{MappingTarget, SyntaxMapping, SyntaxMappingBuilder};
pub use wrapping::WrappingMode;

#[cfg(feature = "paging")]
pub use paging::PagingMode;
