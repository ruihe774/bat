use bat::input::Input;
use std::{ffi::OsString, path::Path};

pub fn new_file_input(file: &'_ Path, name: Option<&'_ Path>) -> Input {
    named(Input::from_file(file), name.or(Some(file)))
}

pub fn new_stdin_input(name: Option<&Path>) -> Input {
    named(Input::from_stdin(), name)
}

fn named(mut input: Input, name: Option<&Path>) -> Input {
    if let Some(provided_name) = name {
        input.description.name = Some(OsString::from(provided_name));
        input.description.kind = "File".to_owned();
        input
    } else {
        input
    }
}
