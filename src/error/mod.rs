use std::io::{self, Write};

pub(crate) use anyhow::Context;
pub use anyhow::{Error, Result};

pub fn default_error_handler(error: &Error, output: &mut dyn Write) {
    use nu_ansi_term::Color::Red;

    if let Some(io_error) = error.downcast_ref::<io::Error>() {
        if io_error.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
    }

    writeln!(output, "{}: {:?}", Red.paint("[bat error]"), error).unwrap();
}
