#![warn(clippy::pedantic)]
#![allow(clippy::enum_glob_use)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::redundant_else)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::too_many_lines)]

pub mod assets;
pub mod config;
pub mod controller;
pub mod error;
pub mod input;
pub mod output;
pub mod printer;
