#![deny(unsafe_code)]

use std::fmt::Write as _;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::{env, process};

use clap::ArgMatches;
use etcetera::BaseStrategy;
use nu_ansi_term::{Color, Style};

use bat::assets::{get_acknowledgements, HighlightingAssets};
use bat::config::ConsolidatedConfig as Config;
use bat::controller::{default_error_handler, Controller, ErrorHandling};
use bat::error::*;
use bat::input::Input;
use bat::printer::style::StyleComponents;

use crate::config::{config_file_path, generate_config_file};
// #[cfg(feature = "bugreport")]
// use crate::config::system_config_file;

mod clap_app;
mod cli;
mod config;
mod input;

#[cfg(feature = "build-assets")]
fn build_assets(matches: &clap::ArgMatches, config_dir: &Path, cache_dir: &Path) -> Result<()> {
    let source_dir = matches
        .get_one::<String>("source")
        .map_or(config_dir, Path::new);

    bat::assets::build(source_dir, cache_dir)
}
#[cfg(feature = "build-assets")]
fn run_cache_subcommand(
    matches: &clap::ArgMatches,
    config_dir: &Path,
    default_cache_dir: &Path,
) -> Result<()> {
    let cache_dir = matches
        .get_one::<String>("target")
        .map_or(default_cache_dir, Path::new);

    build_assets(matches, config_dir, cache_dir)?;

    Ok(())
}

fn get_languages(config: &Config, cache_dir: &Path) -> Result<String> {
    let mut result: String = String::new();

    let assets = HighlightingAssets::new(cache_dir)?;
    let mut languages = assets
        .syntaxes()
        .map(|name| assets.get_syntax_by_name(name).unwrap())
        .map(|syntax_ref| syntax_ref.syntax)
        .filter(|syntax| !syntax.hidden && !syntax.file_extensions.is_empty())
        .cloned()
        .collect::<Vec<_>>();

    // Handling of file-extension conflicts, see issue #1076
    for lang in &mut languages {
        let lang_name = lang.name.clone();
        lang.file_extensions.retain(|extension| {
            // The 'extension' variable is not certainly a real extension.
            //
            // Skip if 'extension' starts with '.', likely a hidden file like '.vimrc'
            // Also skip if the 'extension' contains another real extension, likely
            // that is a full match file name like 'CMakeLists.txt' and 'Cargo.lock'
            if extension.starts_with('.') || Path::new(extension).extension().is_some() {
                return true;
            }

            let test_file = Path::new("test").with_extension(extension);
            let syntax_in_set = assets.get_syntax_for_path(test_file, &config.syntax_mapping);
            matches!(syntax_in_set, Ok(syntax_in_set) if syntax_in_set.syntax.name == lang_name)
        });
    }

    languages.sort_by_key(|lang| lang.name.to_ascii_uppercase());

    if config.loop_through {
        for lang in languages {
            writeln!(result, "{}:{}", lang.name, lang.file_extensions.join(",")).unwrap();
        }
    } else {
        let longest = languages
            .iter()
            .map(|syntax| syntax.name.len())
            .max()
            .unwrap_or(32); // Fallback width if they have no language definitions.

        let comma_separator = ", ";
        let separator = " ";
        // Line-wrapping for the possible file extension overflow.
        let desired_width = usize::from(config.term_width) - longest - separator.len();

        let style = config
            .colored_output
            .then_some(Color::Green.normal())
            .unwrap_or_default();

        for lang in languages {
            write!(result, "{:width$}{}", lang.name, separator, width = longest).unwrap();

            // Number of characters on this line so far, wrap before `desired_width`
            let mut num_chars = 0;

            let mut extension = lang.file_extensions.iter().peekable();
            while let Some(word) = extension.next() {
                // If we can't fit this word in, then create a line break and align it in.
                let new_chars = word.len() + comma_separator.len();
                if num_chars + new_chars >= desired_width {
                    num_chars = 0;
                    write!(result, "\n{:width$}{}", "", separator, width = longest).unwrap();
                }

                num_chars += new_chars;
                write!(result, "{}", style.paint(&word[..])).unwrap();
                if extension.peek().is_some() {
                    result.push_str(comma_separator);
                }
            }
            result.push('\n');
        }
    }

    Ok(result)
}

fn list_languages(
    mut config: Config,
    _config_dir: &Path,
    cache_dir: &Path,
) -> Result<ErrorHandling> {
    let languages: String = get_languages(&config, cache_dir)?;
    let inputs: Vec<Input> = vec![Input::from_reader(io::Cursor::<Vec<u8>>::new(
        languages.into(),
    ))];
    config.loop_through = true;
    run_controller(inputs, &config, cache_dir)
}

fn list_themes(mut config: Config, _config_dir: &Path, cache_dir: &Path) -> Result<ErrorHandling> {
    let assets = HighlightingAssets::new(cache_dir)?;
    config.language = Some("Rust".to_owned());
    config.style_components = StyleComponents::plain().expand(false).unwrap();

    if config.colored_output && !config.loop_through {
        for theme in assets.themes() {
            println!("Theme: {}\n", Style::new().bold().paint(theme));
            config.theme = Some(theme.to_owned());
            assert!(matches!(
                Controller::new(&config, &assets).run(vec![Input::from_reader(
                    include_bytes!("../../../assets/theme_preview.rs").as_slice()
                )]),
                Ok(ErrorHandling::NoError)
            ));
            println!();
        }
    } else {
        for theme in assets.themes() {
            println!("{}", theme);
        }
    }

    Ok(ErrorHandling::NoError)
}

fn run_controller(inputs: Vec<Input>, config: &Config, cache_dir: &Path) -> Result<ErrorHandling> {
    let assets = HighlightingAssets::new(cache_dir)?;
    let controller = Controller::new(config, &assets);
    controller.run(inputs)
}

#[cfg(feature = "bugreport")]
fn invoke_bugreport(matches: &ArgMatches, config_dir: &Path, cache_dir: &Path) {
    use bugreport::{bugreport, collector::*, format::Plaintext};
    let pager =
        bat::config::get_pager_executable(matches.get_one::<String>("pager").map(|s| s.as_str()))
            .unwrap_or_else(|| "less".to_owned()); // FIXME: Avoid non-canonical path to "less".

    let mut report = bugreport!()
        .info(SoftwareVersion::default())
        .info(OperatingSystem::default())
        .info(CommandLine::default())
        .info(EnvironmentVariables::list(&[
            "SHELL",
            "PAGER",
            "LESS",
            "LANG",
            "LC_ALL",
            "BAT_PAGER",
            "BAT_PAGING",
            "BAT_CACHE_PATH",
            "BAT_CONFIG_PATH",
            "BAT_OPTS",
            "BAT_STYLE",
            "BAT_TABS",
            "BAT_THEME",
            "XDG_CONFIG_HOME",
            "XDG_CACHE_HOME",
            "COLORTERM",
            "NO_COLOR",
            "MANPAGER",
        ]))
        // .info(FileContent::new("System Config file", system_config_file()))
        .info(FileContent::new(
            "Config file",
            config_file_path(config_dir),
        ))
        .info(DirectoryEntries::new("Cached assets", cache_dir))
        .info(CompileTimeInformation::default());

    #[cfg(feature = "paging")]
    if let Ok(resolved_path) = grep_cli::resolve_binary(pager) {
        report = report.info(CommandOutput::new(
            "Less version",
            resolved_path,
            &["--version"],
        ))
    };

    report.print::<Plaintext>();
}

/// Returns `Err(..)` upon fatal errors. Otherwise, returns `Ok(true)` on full success and
/// `Ok(false)` if any intermediate errors occurred (were printed).
fn run() -> Result<ErrorHandling> {
    #[cfg(windows)]
    let _ = nu_ansi_term::enable_ansi_support();

    let matches = cli::get_matches();
    let cache_dir = get_cache_dir()?;
    let config_dir = get_config_dir()?;
    let config_file = config_file_path(&config_dir);

    #[cfg(feature = "bugreport")]
    if matches.get_flag("diagnostic") {
        invoke_bugreport(&matches, &config_dir, &cache_dir);
        return Ok(ErrorHandling::NoError);
    }

    match matches.subcommand() {
        #[cfg(feature = "build-assets")]
        Some(("cache", cache_matches)) => {
            // If there is a file named 'cache' in the current working directory,
            // arguments for subcommand 'cache' are not mandatory.
            // If there are non-zero arguments, execute the subcommand cache, else, open the file cache.
            if cache_matches.args_present() {
                run_cache_subcommand(cache_matches, config_dir, cache_dir)?;
                Ok(true)
            } else {
                let inputs = vec![Input::from_file("cache")];
                let config = app.config(&inputs)?;

                run_controller(inputs, &config, cache_dir)
            }
        }
        _ => {
            let inputs = cli::get_inputs(&matches)?;
            let config = cli::get_config(&matches, &config_file)?;

            if matches.get_flag("list-languages") {
                list_languages(config.consolidate(&inputs), &config_dir, &cache_dir)
            } else if matches.get_flag("list-themes") {
                list_themes(config.consolidate(&inputs), &config_dir, &cache_dir)
            } else if matches.get_flag("config-file") {
                println!("{}", config_file.display());
                Ok(ErrorHandling::NoError)
            } else if matches.get_flag("generate-config-file") {
                generate_config_file(&config, &config_file)?;
                Ok(ErrorHandling::NoError)
            } else if matches.get_flag("config-dir") {
                println!("{}", config_dir.display());
                Ok(ErrorHandling::NoError)
            } else if matches.get_flag("cache-dir") {
                println!("{}", cache_dir.display());
                Ok(ErrorHandling::NoError)
            } else if matches.get_flag("acknowledgements") {
                println!("{}", get_acknowledgements());
                Ok(ErrorHandling::NoError)
            } else {
                let config = config.consolidate(&inputs);
                run_controller(inputs, &config, &cache_dir)
            }
        }
    }
}

fn get_cache_dir() -> Result<PathBuf> {
    Ok(if let Some(cache_dir) = env::var_os("BAT_CACHE_PATH") {
        cache_dir.into()
    } else {
        etcetera::choose_base_strategy()?.cache_dir().join("bat")
    })
}

fn get_config_dir() -> Result<PathBuf> {
    Ok(if let Some(config_dir) = env::var_os("BAT_CONFIG_DIR") {
        config_dir.into()
    } else {
        etcetera::choose_base_strategy()?.config_dir().join("bat")
    })
}

fn handle_result(result: Result<ErrorHandling>) -> ! {
    process::exit(match result {
        Ok(ErrorHandling::NoError) | Ok(ErrorHandling::SilentFail) => 0,
        Ok(ErrorHandling::Handled) => 1,
        Err(error) => {
            let mut stderr = io::stderr();
            let is_terminal = stderr.is_terminal();
            let new_result = default_error_handler(&error, &mut stderr, is_terminal);
            handle_result(Ok(new_result));
        }
    })
}

fn main() {
    handle_result(run());
}
