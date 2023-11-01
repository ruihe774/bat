#![crate_name = "nu_ansi_term"]
#![crate_type = "rlib"]
#![warn(missing_copy_implementations)]
// #![warn(missing_docs)]
#![warn(trivial_casts, trivial_numeric_casts)]
// #![warn(unused_extern_crates, unused_qualifications)]

pub mod ansi;

mod style;
pub use style::{Color, Style};

mod windows;
pub use crate::windows::*;

mod debug;

pub mod gradient;
pub use gradient::*;

mod rgb;
pub use rgb::*;
