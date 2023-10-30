use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use clap::ArgMatches;

use bat::assets::syntax_mapping::MappingTarget;
use bat::config::{leak_config_string, Config};
use bat::controller::line_range::{HighlightedLineRanges, LineRange, LineRanges, VisibleLines};
use bat::error::*;
use bat::input::Input;
use bat::output::PagingMode;
use bat::printer::{
    style::{StyleComponent, StyleComponents},
    NonprintableNotation, WrappingMode,
};

use crate::clap_app;
use crate::config::parse_config_file;
use crate::input::{new_file_input, new_stdin_input};

pub fn get_matches() -> ArgMatches {
    clap_app::build_app().get_matches_from(wild::args_os())
}

pub fn get_inputs(matches: &ArgMatches) -> Result<Vec<Input>> {
    let filenames: Option<Vec<_>> = matches
        .get_many::<OsString>("file-name")
        .map(|vs| vs.map(|p| p.as_os_str()).collect());

    let files: Option<Vec<_>> = matches
        .get_many::<PathBuf>("FILE")
        .map(|vs| vs.map(|p| p.as_path()).collect());

    // verify equal length of file-names and input FILEs
    match (filenames.as_ref(), files.as_ref()) {
        (Some(filenames), Some(files)) if filenames.len() != files.len() => {
            return Err(Error::msg(
                "the number of filenames and the number of input files mismatch",
            ))
        }
        _ => (),
    }

    let mut filenames = (0..).map(|i| {
        filenames
            .as_ref()
            .and_then(|filenames| filenames.get(i))
            .copied()
    });

    if let Some(files) = files {
        Ok(files
            .into_iter()
            .zip(filenames)
            .map(|(file, name)| {
                if file.to_str().is_some_and(|s| s == "-") {
                    new_stdin_input(name)
                } else {
                    new_file_input(file, name)
                }
            })
            .collect())
    } else {
        Ok(vec![new_stdin_input(filenames.next().unwrap_or_default())])
    }
}

pub fn get_config(matches: &ArgMatches, config_path: &Path) -> Result<Config> {
    let mut config = if matches.get_flag("no-config") {
        Config::default()
    } else {
        parse_config_file(config_path)?
    };

    if let language @ Some(_) = matches.get_one::<String>("language").cloned().or_else(|| {
        (matches.get_flag("show-all")
            || matches.get_one::<String>("nonprintable-notation").is_some())
        .then(|| "show-nonprintable".to_owned())
    }) {
        config.language = language
    }

    if let nonprintable_notation @ Some(_) = match (
        matches.get_flag("show-all"),
        matches
            .get_one::<String>("nonprintable-notation")
            .map(|s| s.as_str()),
    ) {
        (true, None) => Some(NonprintableNotation::Unicode),
        (_, Some("unicode")) => Some(NonprintableNotation::Unicode),
        (_, Some("caret")) => Some(NonprintableNotation::Caret),
        _ => None,
    } {
        config.nonprintable_notation = nonprintable_notation;
    }

    if let wrapping_mode @ Some(_) = match matches.get_one::<String>("wrap").unwrap().as_str() {
        "character" => Some(WrappingMode::Character),
        "never" => Some(WrappingMode::NoWrapping),
        _ => None,
    } {
        config.wrapping_mode = wrapping_mode;
    }

    if let Some(colored_output) = match (
        matches.get_flag("force-colorization"),
        matches.get_one::<String>("color").unwrap().as_str(),
    ) {
        (_, "always") => Some(true),
        (_, "never") => Some(false),
        (true, _) => Some(true),
        _ => None,
    } {
        config.colored_output = Some(colored_output);
        if colored_output {
            config.always_show_decorations = true;
        }
    }

    if let paging_mode @ Some(_) = match (
        matches.get_flag("no-paging"),
        matches.get_one::<String>("paging").unwrap().as_str(),
    ) {
        (true, _) => Some(PagingMode::Never),
        (_, "always") => Some(PagingMode::Always),
        (_, "never") => Some(PagingMode::Never),
        _ => (matches.get_count("plain") > 1).then_some(PagingMode::Never),
    } {
        config.paging_mode = paging_mode;
    }

    if let term_width @ Some(_) = matches.get_one::<NonZeroUsize>("terminal-width").copied() {
        config.term_width = term_width;
    }

    if let Some(tab_width) = matches.get_one::<usize>("tabs").copied() {
        config.tab_width = NonZeroUsize::new(tab_width).into()
    }

    if let theme @ Some(_) = matches
        .get_one::<String>("theme")
        .and_then(|s| (s != "default").then(|| s.to_owned()))
    {
        config.theme = theme;
    }

    if let Some(visible_lines) = matches
        .get_many::<LineRange>("line-range")
        .map(|ranges| LineRanges::from(ranges.copied().collect()))
        .map(VisibleLines)
    {
        config.visible_lines = visible_lines;
    }

    if let Some(style_components) = match matches.get_one::<String>("decorations").unwrap().as_str()
    {
        "never" => Some(StyleComponents::plain()),
        _ => {
            if matches.get_count("plain") != 0 {
                Some(StyleComponents::plain())
            } else if matches.get_flag("number") {
                Some(StyleComponents::new(vec![StyleComponent::LineNumbers]))
            } else {
                matches
                    .get_many::<StyleComponent>("style")
                    .map(|components| StyleComponents::new(components.copied().collect()))
            }
        }
    } {
        config.style_components = style_components;
    }

    let syntax_mapping = &mut config.syntax_mapping;
    if let Some(values) = matches.get_many::<String>("ignored-suffix") {
        for suffix in values {
            syntax_mapping.ignore_suffix(
                if !suffix.contains(|ch: char| !ch.is_ascii_alphanumeric()) {
                    format!(".{}", suffix)
                } else {
                    suffix.to_owned()
                },
            );
        }
    }
    if let Some(values) = matches.get_many::<String>("map-syntax") {
        for from_to in values {
            let mut parts = from_to.split(':');
            syntax_mapping.map_syntax(
                parts.next().unwrap(),
                MappingTarget::MapTo(leak_config_string(parts.next().unwrap().to_owned())),
            );
        }
    }

    if let pager @ Some(_) = matches.get_one::<String>("pager").map(|s| s.to_owned()) {
        config.pager = pager;
    }

    if let Some(use_italic_text) = matches
        .get_one::<String>("italic-text")
        .map(|s| s == "always")
    {
        config.use_italic_text = use_italic_text;
    }

    if let Some(hightlighted_lines) = matches
        .get_many::<LineRange>("highlight-line")
        .map(|ranges| LineRanges::from(ranges.copied().collect()))
        .map(HighlightedLineRanges)
    {
        config.highlighted_lines = hightlighted_lines;
    }

    if matches.get_one::<String>("decorations").unwrap() == "always" {
        config.always_show_decorations = true;
    }

    #[cfg(feature = "lessopen")]
    if let Some(no_lessopen) = (matches.get_count("no-lessopen") != 0).then_some(true) {
        config.no_lessopen = no_lessopen;
    }

    Ok(config)
}
