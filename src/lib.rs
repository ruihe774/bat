//! `bat` is a library to print syntax highlighted content.
//!
//! The main struct of this crate is `PrettyPrinter` which can be used to
//! configure and run the syntax highlighting.
//!
//! If you need more control, you can also use the structs in the submodules
//! (start with `controller::Controller`), but note that the API of these
//! internal modules is much more likely to change. Some or all of these
//! modules might be removed in the future.

#[macro_export]
macro_rules! bat_warning {
    ($($arg:tt)*) => ({
        use nu_ansi_term::Color::Yellow;
        eprintln!("{}: {}", Yellow.paint("[bat warning]"), format!($($arg)*));
    })
}

pub mod assets;
pub mod config;
pub mod controller;
pub mod error;
pub mod input;
pub mod output;
pub mod printer;
