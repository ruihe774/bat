use std::ffi::OsStr;
use std::path::Path;

use bat::input::Input;

pub fn new_file_input(file: &Path, name: Option<&OsStr>) -> Input {
    named(Input::from_file(file), name.or(Some(file.as_os_str())))
}

pub fn new_stdin_input(name: Option<&OsStr>) -> Input {
    named(Input::from_stdin(), name)
}

fn named(mut input: Input, name: Option<&OsStr>) -> Input {
    if let Some(provided_name) = name {
        input.description.name = Some(provided_name.to_owned());
        input.description.kind = "File".to_owned();
    }
    input
}
