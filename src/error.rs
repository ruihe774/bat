use std::io::{self, Write};

pub type Error = anyhow::Error;
pub type Result<T> = anyhow::Result<T>;

pub fn default_error_handler(error: &Error, output: &mut dyn Write) {
    use nu_ansi_term::Color::Red;

    if let Some(io_error) = error.downcast_ref::<io::Error>() {
        if io_error.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
    }

    writeln!(output, "{}: {:?}", Red.paint("[bat error]"), error).unwrap();
}
