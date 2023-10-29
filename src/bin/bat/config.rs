use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use bat::config::Config;
use bat::error::*;

/* // we do not support system config file at present
#[cfg(unix)]
const DEFAULT_SYSTEM_CONFIG_PREFIX: Option<&str> = Some("/etc");
#[cfg(windows)]
const DEFAULT_SYSTEM_CONFIG_PREFIX: Option<&str> = Some("C:\\ProgramData");
#[cfg(all(not(unix), not(windows)))]
const DEFAULT_SYSTEM_CONFIG_PREFIX: Option<&str> = None;

pub fn system_config_file() -> Option<PathBuf> {
    let folder = option_env!("BAT_SYSTEM_CONFIG_PREFIX").or(DEFAULT_SYSTEM_CONFIG_PREFIX)?;
    let mut path = PathBuf::from(folder);
    path.push("bat");
    path.push("config");
    Some(path)
}
*/

pub fn generate_config_file(config: &Config, config_file: &Path) -> Result<()> {
    if config_file.exists() {
        print!(
            "A config file already exists at: {}\nOverwrite? (y/N): ",
            config_file.to_string_lossy()
        );

        io::stdout().flush()?;
        let mut decision = String::new();
        io::stdin().read_line(&mut decision)?;

        if !decision.trim().eq_ignore_ascii_case("Y") {
            return Ok(());
        }
    } else {
        let config_dir = config_file.parent().expect("invalid config path");
        fs::create_dir_all(config_dir)?
    }

    ron::ser::to_writer_pretty(
        io::BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&config_file)?,
        ),
        config,
        ron::ser::PrettyConfig::new().extensions(ron::extensions::Extensions::IMPLICIT_SOME),
    )?;

    println!("Success! Config file written to {}", config_file.display());

    Ok(())
}

pub fn config_file_path(config_dir: &Path) -> PathBuf {
    env::var_os("BAT_CONFIG_PATH")
        .map(|path| fs::canonicalize(path).expect("invalid BAT_CONFIG_PATH"))
        .unwrap_or_else(|| config_dir.join("config.ron"))
}

pub fn parse_config_file(user_config_file: &Path) -> Result<Config> {
    // FIXME: system config file
    // let system_config: Option<Config> = match system_config_file.and_then(|path| File::open(path).ok()).map(io::BufReader::new) {
    //     Some(r) => Some(ron::de::from_reader(r)?),
    //     None => None,
    // };

    let user_config: Option<Config> =
        match File::open(user_config_file).ok().map(io::BufReader::new) {
            Some(r) => Some(ron::de::from_reader(r)?),
            None => None,
        };

    Ok(user_config.unwrap_or_default())
}
