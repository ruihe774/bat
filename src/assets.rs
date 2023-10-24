use std::env;
use std::error::Error as StdError;
use std::ffi::OsStr;
use std::fmt::{self, Display, Write};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use flate2::bufread::GzDecoder;
use serde::de::DeserializeOwned;
use syntect::highlighting::Theme;
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::error::*;
#[cfg(feature = "guesslang")]
use crate::guesslang::GuessLang;
use crate::input::{InputReader, OpenedInput};
use crate::syntax_mapping::MappingTarget;
#[cfg(feature = "zero-copy")]
use crate::zero_copy::{create_file_mapped_leaky_slice, create_leaky_slice, LeakySliceReader};
use crate::SyntaxMapping;

#[cfg(feature = "build-assets")]
pub use build_assets::build;
use lazy_theme_set::LazyThemeSet;

#[cfg(feature = "build-assets")]
mod build_assets;
mod lazy_theme_set;

#[cfg(feature = "guesslang")]
macro_rules! include_asset_bytes {
    ($asset_path:literal, $cache_dir:expr) => {
        load_asset_bytes($asset_path, include_bytes!($asset_path), $cache_dir).with_context(|| {
            format!(
                "failed to load asset '{}'",
                Path::new($asset_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
            )
        })
    };
}

macro_rules! include_asset {
    ($asset_path:literal, $cache_dir:expr) => {
        load_asset_bytes($asset_path, include_bytes!($asset_path), $cache_dir)
            .and_then(|bytes| asset_from_bytes(bytes))
            .with_context(|| {
                format!(
                    "failed to load asset '{}'",
                    Path::new($asset_path)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                )
            })
    };
}

#[derive(Debug)]
pub struct UnknownSyntax {
    pub name: String,
}

impl Display for UnknownSyntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown syntax '{}'", self.name)
    }
}

impl StdError for UnknownSyntax {}

#[derive(Debug)]
pub struct SyntaxUndetected {
    pub path: PathBuf,
}

impl Display for SyntaxUndetected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unable to detect syntax for '{}'", self.path.display())
    }
}

impl StdError for SyntaxUndetected {}

#[derive(Debug)]
pub struct UnknownTheme {
    pub name: String,
}

impl Display for UnknownTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown theme '{}'", self.name)
    }
}

impl StdError for UnknownTheme {}

#[derive(Debug)]
pub struct HighlightingAssets {
    syntax_set: SyntaxSet,
    theme_set: LazyThemeSet,
    #[cfg(feature = "guesslang")]
    guesslang: GuessLang,
}

#[derive(Debug, Copy, Clone)]
pub struct SyntaxReferenceInSet<'a> {
    pub syntax: &'a SyntaxReference,
    pub(crate) syntax_set: &'a SyntaxSet,
}

impl HighlightingAssets {
    pub fn new(cache_path: impl AsRef<Path>) -> Result<Self> {
        let cache_path = cache_path.as_ref();
        Ok(HighlightingAssets {
            syntax_set: include_asset!("../assets/syntaxes.gz", Some(cache_path))?,
            theme_set: include_asset!("../assets/themes.gz", Some(cache_path))?,
            #[cfg(feature = "guesslang")]
            guesslang: GuessLang::new(include_asset_bytes!(
                "../assets/guesslang.ort.gz",
                Some(cache_path)
            )?),
        })
    }

    #[cfg(debug_assertions)]
    pub fn with_no_cache() -> Self {
        HighlightingAssets {
            syntax_set: include_asset!("../assets/syntaxes.gz", Option::<&Path>::None).unwrap(),
            theme_set: include_asset!("../assets/themes.gz", Option::<&Path>::None).unwrap(),
            #[cfg(feature = "guesslang")]
            guesslang: GuessLang::new(
                include_asset_bytes!("../assets/guesslang.ort.gz", Option::<&Path>::None).unwrap(),
            ),
        }
    }

    /// The default theme.
    ///
    /// ### Windows and Linux
    ///
    /// Windows and most Linux distributions has a dark terminal theme by
    /// default. On these platforms, this function always returns a theme that
    /// looks good on a dark background.
    ///
    /// ### macOS
    ///
    /// On macOS the default terminal background is light, but it is common that
    /// Dark Mode is active, which makes the terminal background dark. On this
    /// platform, the default theme depends on
    /// ```bash
    /// defaults read -globalDomain AppleInterfaceStyle
    /// ```
    /// To avoid the overhead of the check on macOS, simply specify a theme
    /// explicitly via `--theme`, `BAT_THEME`, or `~/.config/bat`.
    ///
    /// See <https://github.com/sharkdp/bat/issues/1746> and
    /// <https://github.com/sharkdp/bat/issues/1928> for more context.
    pub fn get_default_theme(&self) -> &Theme {
        let default_dark_theme = "Monokai Extended";
        let default_light_theme = "Monokai Extended Light";
        #[cfg(not(target_os = "macos"))]
        let name = default_dark_theme;
        #[cfg(target_os = "macos")]
        let name = if macos_dark_mode_active() {
            default_dark_theme
        } else {
            default_light_theme
        };
        self.get_theme(name).expect("no default theme")
    }

    /// The fallback syntax
    pub fn get_fallback_syntax(&self) -> SyntaxReferenceInSet {
        self.find_syntax_by_name("Plain Text")
            .expect("no fallback syntax")
    }

    pub fn syntaxes(&self) -> impl Iterator<Item = &str> {
        self.syntax_set
            .syntaxes()
            .iter()
            .map(|syntax| syntax.name.as_str())
    }

    pub fn themes(&self) -> impl Iterator<Item = &str> {
        self.theme_set.themes()
    }

    /// Detect the syntax based on, in order:
    ///  1. Syntax mappings with [MappingTarget::MapTo] and [MappingTarget::MapToUnknown]
    ///     (e.g. `/etc/profile` -> `Bourne Again Shell (bash)`)
    ///  2. The file name (e.g. `Dockerfile`)
    ///  3. Syntax mappings with [MappingTarget::MapExtensionToUnknown]
    ///     (e.g. `*.conf`)
    ///  4. The file name extension (e.g. `.rs`)
    ///
    /// When detecting syntax based on syntax mappings, the full path is taken
    /// into account. When detecting syntax based on file name, no regard is
    /// taken to the path of the file. Only the file name itself matters. When
    /// detecting syntax based on file name extension, only the file name
    /// extension itself matters.
    ///
    /// Returns [SyntaxUndetected] if it was not possible detect syntax
    /// based on path/file name/extension (or if the path was mapped to
    /// [MappingTarget::MapToUnknown] or [MappingTarget::MapExtensionToUnknown]).
    /// In this case it is appropriate to fall back to other methods to detect
    /// syntax. Such as using the contents of the first line of the file.
    ///
    /// Returns [UnknownSyntax] if a syntax mapping exist, but the mapped
    /// syntax does not exist.
    pub fn get_syntax_for_path(
        &self,
        path: impl AsRef<Path>,
        mapping: &SyntaxMapping,
    ) -> Result<SyntaxReferenceInSet> {
        let path = path.as_ref();
        let undetected = || {
            SyntaxUndetected {
                path: path.to_owned(),
            }
            .into()
        };
        let path: PathBuf = mapping
            .strip_ignored_suffixes(absolute_path(path)?.into())
            .into();
        let syntax_match = mapping.get_syntax_for(&path);
        match syntax_match {
            Some(MappingTarget::MapToUnknown) => Err(undetected()),
            Some(MappingTarget::MapTo(syntax_name)) => {
                self.find_syntax_by_name(syntax_name).ok_or_else(|| {
                    UnknownSyntax {
                        name: syntax_name.to_owned(),
                    }
                    .into()
                })
            }
            _ => {
                if let Some(sr) = path
                    .file_name()
                    .and_then(|name| self.find_syntax_by_extension(name))
                {
                    Ok(sr)
                } else if let Some(MappingTarget::MapExtensionToUnknown) = syntax_match {
                    Err(undetected())
                } else {
                    path.extension()
                        .and_then(|name| self.find_syntax_by_extension(name))
                        .ok_or(undetected())
                }
            }
        }
    }

    pub(crate) fn get_theme(&self, theme: &str) -> Result<&Theme> {
        self.theme_set.get(theme).ok_or_else(|| {
            UnknownTheme {
                name: theme.to_owned(),
            }
            .into()
        })
    }

    pub(crate) fn get_syntax(
        &self,
        language: Option<&str>,
        input: &mut OpenedInput,
        mapping: &SyntaxMapping,
    ) -> Result<SyntaxReferenceInSet> {
        if let Some(language) = language {
            return self
                .syntax_set
                .find_syntax_by_token(language)
                .map(|syntax| SyntaxReferenceInSet {
                    syntax,
                    syntax_set: &self.syntax_set,
                })
                .ok_or_else(|| {
                    UnknownSyntax {
                        name: language.to_owned(),
                    }
                    .into()
                });
        }

        let path = input.path();
        let path_syntax = if let Some(path) = path {
            self.get_syntax_for_path(path, mapping)
        } else {
            Err(SyntaxUndetected {
                path: "UNKNOWN".into(),
            }
            .into())
        };

        if path_syntax
            .as_ref()
            .err()
            .and_then(|err| err.downcast_ref::<SyntaxUndetected>())
            .is_some()
        {
            if let Some(sr) = self.get_first_line_syntax(&mut input.reader)? {
                return Ok(sr);
            }
            #[cfg(feature = "guesslang")]
            if let Some(sr) = self.get_syntax_by_guesslang(&mut input.reader)? {
                return Ok(sr);
            }
        }

        path_syntax
    }

    pub fn get_syntax_by_name(&self, name: &str) -> Result<SyntaxReferenceInSet> {
        self.find_syntax_by_name(name).ok_or_else(|| {
            UnknownSyntax {
                name: name.to_owned(),
            }
            .into()
        })
    }

    fn find_syntax_by_name(&self, syntax_name: &str) -> Option<SyntaxReferenceInSet> {
        self.syntax_set
            .find_syntax_by_name(syntax_name)
            .map(|syntax| SyntaxReferenceInSet {
                syntax,
                syntax_set: &self.syntax_set,
            })
    }

    fn find_syntax_by_extension(&self, e: &OsStr) -> Option<SyntaxReferenceInSet> {
        self.syntax_set
            .find_syntax_by_extension(e.to_str()?)
            .map(|syntax| SyntaxReferenceInSet {
                syntax,
                syntax_set: &self.syntax_set,
            })
    }

    fn get_first_line_syntax(
        &self,
        reader: &mut InputReader,
    ) -> Result<Option<SyntaxReferenceInSet>> {
        Ok(reader
            .first_read
            .as_ref()
            .map(|s| s.split_inclusive('\n').next().unwrap_or(s))
            .and_then(|l| self.syntax_set.find_syntax_by_first_line(l))
            .map(|syntax| SyntaxReferenceInSet {
                syntax,
                syntax_set: &self.syntax_set,
            }))
    }

    #[cfg(feature = "guesslang")]
    fn get_syntax_by_guesslang(
        &self,
        reader: &mut InputReader,
    ) -> Result<Option<SyntaxReferenceInSet>> {
        Ok(reader
            .first_read
            .as_ref()
            .and_then(|s| self.guesslang.guess(s.clone()))
            .and_then(|l| self.syntax_set.find_syntax_by_token(l))
            .map(|syntax| SyntaxReferenceInSet {
                syntax,
                syntax_set: &self.syntax_set,
            }))
    }
}

pub fn get_acknowledgements() -> String {
    include_asset!("../assets/acknowledgements.gz", Option::<&Path>::None).unwrap()
}

#[cfg(target_os = "macos")]
fn macos_dark_mode_active() -> bool {
    use std::process::Command;

    const STYLE_KEY: &str = "AppleInterfaceStyle";
    let output = Command::new("/usr/bin/defaults")
        .args(["read", "-g", STYLE_KEY])
        .output()
        .map(|output| output.stdout)
        .ok();
    let is_dark = output
        .map(|output| output.starts_with(b"Dark".as_slice()))
        .unwrap_or(false);
    is_dark
}

fn load_asset_bytes(
    asset_path: impl AsRef<Path>,
    data: &[u8],
    cache_dir: Option<impl AsRef<Path>>,
) -> Result<Vec<u8>> {
    let mut iter = data
        .rchunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()));
    let length = iter.next().expect("invalid gzip file") as usize;
    let checksum = iter.next().expect("invalid gzip file");
    let cache_file = if let Some(cache_dir) = cache_dir {
        let mut cache_file = asset_path
            .as_ref()
            .file_stem()
            .expect("asset_path has no file stem")
            .to_owned();
        debug_assert_eq!(
            asset_path.as_ref().extension().unwrap().to_str().unwrap(),
            "gz",
            "asset_path must end with .gz"
        );
        write!(&mut cache_file, ".{:x}.bin", checksum)?;
        let cache_file = cache_dir.as_ref().join(cache_file.as_os_str());
        Some(cache_file)
    } else {
        None
    };
    Ok(
        if let Some(buffer) = cache_file.as_ref().and_then(|cache_file| {
            #[cfg(feature = "zero-copy")]
            return File::open(cache_file)
                .and_then(|f| unsafe { create_file_mapped_leaky_slice(&f) })
                .map(|slice| unsafe {
                    Vec::from_raw_parts(slice.as_mut_ptr(), slice.len(), slice.len())
                })
                .ok();
            #[cfg(not(feature = "zero-copy"))]
            return fs::read(cache_file).ok();
        }) {
            buffer
        } else {
            let mut decoder = GzDecoder::new(data);
            #[cfg(feature = "zero-copy")]
            let buffer = {
                let mut buffer = unsafe {
                    let slice = create_leaky_slice(length)?;
                    Vec::from_raw_parts(slice.as_mut_ptr(), slice.len(), slice.len())
                };
                decoder.read_exact(buffer.as_mut_slice())?;
                buffer
            };
            #[cfg(not(feature = "zero-copy"))]
            let buffer = {
                let mut buffer = Vec::with_capacity(length);
                decoder.read_to_end(&mut buffer)?;
                buffer
            };
            if let Some(cache_file) = cache_file {
                fs::create_dir_all(cache_file.parent().unwrap())?;
                fs::write(cache_file, &buffer)?;
            }
            buffer
        },
    )
}

fn asset_from_bytes<T: DeserializeOwned>(bytes: Vec<u8>) -> Result<T> {
    #[cfg(feature = "zero-copy")]
    return Ok(bincode::deserialize_from_custom(
        LeakySliceReader::from_leaky_vec(bytes),
    )?);
    #[cfg(not(feature = "zero-copy"))]
    return Ok(bincode::deserialize(bytes.as_slice())?);
}

fn absolute_path(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let mut pathbuf = None;
    for comp in path.as_ref().components() {
        match comp {
            Component::Prefix(_) | Component::RootDir => {
                pathbuf.get_or_insert_with(PathBuf::new).push(comp);
            }
            Component::Normal(_) => match &mut pathbuf {
                Some(pathbuf) => pathbuf.push(comp),
                None => pathbuf = Some(env::current_dir()?.join(comp)),
            },
            Component::ParentDir => match &mut pathbuf {
                Some(pathbuf) => {
                    _ = pathbuf.pop();
                }
                None => {
                    pathbuf = Some({
                        let mut pathbuf = env::current_dir()?;
                        _ = pathbuf.pop();
                        pathbuf
                    })
                }
            },
            Component::CurDir => (),
        };
    }
    pathbuf.map_or_else(env::current_dir, Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::{OsStr, OsString};

    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    use crate::input::Input;

    struct SyntaxDetectionTest {
        assets: HighlightingAssets,
        pub syntax_mapping: SyntaxMapping<'static>,
        pub temp_dir: TempDir,
    }

    impl SyntaxDetectionTest {
        fn new() -> Self {
            SyntaxDetectionTest {
                assets: HighlightingAssets::with_no_cache(),
                syntax_mapping: SyntaxMapping::builtin(),
                temp_dir: TempDir::new().expect("creation of temporary directory"),
            }
        }

        fn get_syntax_name(
            &self,
            language: Option<&str>,
            input: &mut OpenedInput,
            mapping: &SyntaxMapping,
        ) -> String {
            self.assets
                .get_syntax(language, input, mapping)
                .map(|syntax_in_set| syntax_in_set.syntax.name.clone())
                .unwrap_or_else(|_| "!no syntax!".to_owned())
        }

        fn syntax_for_real_file_with_content_os(
            &self,
            file_name: &OsStr,
            first_line: &str,
        ) -> String {
            let file_path = self.temp_dir.path().join(file_name);
            {
                let mut temp_file = File::create(&file_path).unwrap();
                writeln!(temp_file, "{}", first_line).unwrap();
            }

            let input = Input::from_file(&file_path);
            let mut opened_input = input.open(None).unwrap();

            self.get_syntax_name(None, &mut opened_input, &self.syntax_mapping)
        }

        fn syntax_for_file_with_content_os(&self, file_name: &OsStr, first_line: &str) -> String {
            let file_path = self.temp_dir.path().join(file_name);
            let mut input = Input::from_reader(io::Cursor::new(Vec::from(first_line.as_bytes())));
            input.description.name = Some(OsString::from(file_path));
            let mut opened_input = input.open(None).unwrap();

            self.get_syntax_name(None, &mut opened_input, &self.syntax_mapping)
        }

        #[cfg(unix)]
        fn syntax_for_file_os(&self, file_name: &OsStr) -> String {
            self.syntax_for_file_with_content_os(file_name, "")
        }

        fn syntax_for_file_with_content(&self, file_name: &str, first_line: &str) -> String {
            self.syntax_for_file_with_content_os(OsStr::new(file_name), first_line)
        }

        fn syntax_for_file(&self, file_name: &str) -> String {
            self.syntax_for_file_with_content(file_name, "")
        }

        fn syntax_for_stdin_with_content(&self, file_name: &str, content: &[u8]) -> String {
            let mut input = Input::from_stdin();
            input.description.name = Some(OsString::from(file_name));
            let mut opened_input = input.open(None).unwrap();
            opened_input.reader = InputReader::new(io::Cursor::new(Vec::from(content)));

            self.get_syntax_name(None, &mut opened_input, &self.syntax_mapping)
        }

        fn syntax_is_same_for_inputkinds(&self, file_name: &str, content: &str) -> bool {
            let as_file = self.syntax_for_real_file_with_content_os(file_name.as_ref(), content);
            let as_reader = self.syntax_for_file_with_content_os(file_name.as_ref(), content);
            let consistent = as_file == as_reader;
            // TODO: Compare StdIn somehow?

            if !consistent {
                eprintln!(
                    "Inconsistent syntax detection:\nFor File: {}\nFor Reader: {}",
                    as_file, as_reader
                )
            }

            consistent
        }
    }

    #[test]
    fn syntax_detection_basic() {
        let test = SyntaxDetectionTest::new();

        assert_eq!(test.syntax_for_file("test.rs"), "Rust");
        assert_eq!(test.syntax_for_file("test.cpp"), "C++");
        assert_eq!(test.syntax_for_file("test.build"), "NAnt Build File");
        assert_eq!(
            test.syntax_for_file("PKGBUILD"),
            "Bourne Again Shell (bash)"
        );
        assert_eq!(test.syntax_for_file(".bashrc"), "Bourne Again Shell (bash)");
        assert_eq!(test.syntax_for_file("Makefile"), "Makefile");
    }

    #[cfg(unix)]
    #[test]
    fn syntax_detection_invalid_utf8() {
        use std::os::unix::ffi::OsStrExt;

        let test = SyntaxDetectionTest::new();

        assert_eq!(
            test.syntax_for_file_os(OsStr::from_bytes(b"invalid_\xFEutf8_filename.rs")),
            "Rust"
        );
    }

    #[test]
    fn syntax_detection_same_for_inputkinds() {
        let test = SyntaxDetectionTest::new();

        // test.syntax_mapping
        //     .insert("*.myext", MappingTarget::MapTo("C"))
        //     .ok();
        // test.syntax_mapping
        //     .insert("MY_FILE", MappingTarget::MapTo("Markdown"))
        //     .ok();

        assert!(test.syntax_is_same_for_inputkinds("Test.md", ""));
        assert!(test.syntax_is_same_for_inputkinds("Test.txt", "#!/bin/bash"));
        assert!(test.syntax_is_same_for_inputkinds(".bashrc", ""));
        assert!(test.syntax_is_same_for_inputkinds("test.h", ""));
        assert!(test.syntax_is_same_for_inputkinds("test.js", "#!/bin/bash"));
        // assert!(test.syntax_is_same_for_inputkinds("test.myext", ""));
        // assert!(test.syntax_is_same_for_inputkinds("MY_FILE", ""));
        // assert!(test.syntax_is_same_for_inputkinds("MY_FILE", "<?php"));
    }

    #[test]
    fn syntax_detection_well_defined_mapping_for_duplicate_extensions() {
        let test = SyntaxDetectionTest::new();

        assert_eq!(test.syntax_for_file("test.h"), "C++");
        assert_eq!(test.syntax_for_file("test.sass"), "Sass");
        assert_eq!(test.syntax_for_file("test.js"), "JavaScript (Babel)");
        assert_eq!(test.syntax_for_file("test.fs"), "F#");
        assert_eq!(test.syntax_for_file("test.v"), "Verilog");
    }

    #[test]
    fn syntax_detection_first_line() {
        let test = SyntaxDetectionTest::new();

        assert_eq!(
            test.syntax_for_file_with_content("my_script", "#!/bin/bash"),
            "Bourne Again Shell (bash)"
        );
        assert_eq!(
            test.syntax_for_file_with_content("build", "#!/bin/bash"),
            "Bourne Again Shell (bash)"
        );
        assert_eq!(
            test.syntax_for_file_with_content("my_script", "<?php"),
            "PHP"
        );
    }

    #[ignore]
    #[test]
    fn syntax_detection_with_custom_mapping() {
        let test = SyntaxDetectionTest::new();

        assert_eq!(test.syntax_for_file("test.h"), "C++");
        // test.syntax_mapping
        //     .insert("*.h", MappingTarget::MapTo("C"))
        //     .ok();
        assert_eq!(test.syntax_for_file("test.h"), "C");
    }

    #[test]
    fn syntax_detection_with_extension_mapping_to_unknown() {
        let test = SyntaxDetectionTest::new();

        // Normally, a CMakeLists.txt file shall use the CMake syntax, even if it is
        // a bash script in disguise
        assert_eq!(
            test.syntax_for_file_with_content("CMakeLists.txt", "#!/bin/bash"),
            "CMake"
        );

        // Other .txt files shall use the Plain Text syntax
        assert_eq!(
            test.syntax_for_file_with_content("some-other.txt", "#!/bin/bash"),
            "Plain Text"
        );

        // // If we setup MapExtensionToUnknown on *.txt, the match on the full
        // // file name of "CMakeLists.txt" shall have higher prio, and CMake shall
        // // still be used for it
        // test.syntax_mapping
        //     .insert("*.txt", MappingTarget::MapExtensionToUnknown)
        //     .ok();
        // assert_eq!(
        //     test.syntax_for_file_with_content("CMakeLists.txt", "#!/bin/bash"),
        //     "CMake"
        // );

        // // However, for *other* files with a .txt extension, first-line fallback
        // // shall now be used
        // assert_eq!(
        //     test.syntax_for_file_with_content("some-other.txt", "#!/bin/bash"),
        //     "Bourne Again Shell (bash)"
        // );
    }

    #[test]
    fn syntax_detection_is_case_insensitive() {
        let test = SyntaxDetectionTest::new();

        assert_eq!(test.syntax_for_file("README.md"), "Markdown");
        assert_eq!(test.syntax_for_file("README.mD"), "Markdown");
        assert_eq!(test.syntax_for_file("README.Md"), "Markdown");
        assert_eq!(test.syntax_for_file("README.MD"), "Markdown");

        // // Adding a mapping for "MD" in addition to "md" should not break the mapping
        // test.syntax_mapping
        //     .insert("*.MD", MappingTarget::MapTo("Markdown"))
        //     .ok();

        assert_eq!(test.syntax_for_file("README.md"), "Markdown");
        assert_eq!(test.syntax_for_file("README.mD"), "Markdown");
        assert_eq!(test.syntax_for_file("README.Md"), "Markdown");
        assert_eq!(test.syntax_for_file("README.MD"), "Markdown");
    }

    #[ignore]
    #[test]
    fn syntax_detection_stdin_filename() {
        let test = SyntaxDetectionTest::new();

        // from file extension
        assert_eq!(test.syntax_for_stdin_with_content("test.cpp", b"a"), "C++");
        // from first line (fallback)
        assert_eq!(
            test.syntax_for_stdin_with_content("my_script", b"#!/bin/bash"),
            "Bourne Again Shell (bash)"
        );
    }

    #[cfg(unix)]
    #[test]
    fn syntax_detection_for_symlinked_file() {
        use std::os::unix::fs::symlink;

        let test = SyntaxDetectionTest::new();
        let file_path = test.temp_dir.path().join("my_ssh_config_filename");
        {
            File::create(&file_path).unwrap();
        }
        let file_path_symlink = test.temp_dir.path().join(".ssh").join("config");

        std::fs::create_dir(test.temp_dir.path().join(".ssh"))
            .expect("creation of directory succeeds");
        symlink(&file_path, &file_path_symlink).expect("creation of symbolic link succeeds");

        let input = Input::from_file(&file_path_symlink);
        let mut opened_input = input.open(None).unwrap();

        assert_eq!(
            test.get_syntax_name(None, &mut opened_input, &test.syntax_mapping),
            "SSH Config"
        );
    }
}
