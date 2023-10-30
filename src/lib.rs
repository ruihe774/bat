//! `bat` is a library to print syntax highlighted content.
//!
//! The main struct of this crate is `PrettyPrinter` which can be used to
//! configure and run the syntax highlighting.
//!
//! If you need more control, you can also use the structs in the submodules
//! (start with `controller::Controller`), but note that the API of these
//! internal modules is much more likely to change. Some or all of these
//! modules might be removed in the future.

#![warn(clippy::pedantic)]
#![allow(clippy::enum_glob_use)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::redundant_else)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::too_many_lines)]

pub mod assets;
pub mod config;
pub mod controller;
pub mod error;
pub mod input;
pub mod output;
pub mod printer;
