use std::collections::hash_map::DefaultHasher;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::hash::Hasher;
use std::io::Write;
use std::os::unix::prelude::OsStrExt;
use std::path::Path;

use flate2::write::GzDecoder;

use serde::de::DeserializeOwned;
use syntect::highlighting::Theme;
use syntect::parsing::{SyntaxReference, SyntaxSet};

use path_abs::PathAbs;

use crate::error::*;
#[cfg(feature = "guesslang")]
use crate::guesslang::GuessLang;
use crate::input::{InputReader, OpenedInput};
use crate::syntax_mapping::ignored_suffixes::IgnoredSuffixes;
use crate::syntax_mapping::MappingTarget;
use crate::{bat_warning, SyntaxMapping};

use lazy_theme_set::LazyThemeSet;

#[cfg(feature = "build-assets")]
pub use crate::assets::build_assets::*;

pub(crate) mod assets_metadata;
#[cfg(feature = "build-assets")]
mod build_assets;
mod lazy_theme_set;

macro_rules! include_asset_bytes {
    ($assert_path:literal, $cache_dir:expr) => {
        load_asset_bytes($assert_path, include_bytes!($assert_path), $cache_dir)
    };
}

macro_rules! include_asset {
    ($assert_path:literal, $cache_dir:expr) => {
        include_asset_bytes!($assert_path, $cache_dir).and_then(|bytes| asset_from_contents(&bytes, $assert_path))
    };
}

#[derive(Debug)]
pub struct HighlightingAssets {
    syntax_set: SyntaxSet,
    theme_set: LazyThemeSet,
    guesslang: GuessLang,
}

#[derive(Debug)]
pub struct SyntaxReferenceInSet<'a> {
    pub syntax: &'a SyntaxReference,
    pub syntax_set: &'a SyntaxSet,
}

impl HighlightingAssets {
    pub fn new(cache_path: impl AsRef<Path>) -> Result<Self> {
        let cache_path = cache_path.as_ref();
        Ok(HighlightingAssets {
            syntax_set: include_asset!("../assets/syntaxes.gz", Some(cache_path))?,
            theme_set: include_asset!("../assets/themes.gz", Some(cache_path))?,
            guesslang: GuessLang::new(include_asset_bytes!(
                "../assets/guesslang.ort.gz",
                Some(cache_path)
            )?),
        })
    }

    #[cfg(test)]
    pub fn with_no_cache() -> Self {
        HighlightingAssets {
            syntax_set: include_asset!("../assets/syntaxes.gz", Option::<&Path>::None).unwrap(),
            theme_set: include_asset!("../assets/themes.gz", Option::<&Path>::None).unwrap(),
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
    pub fn default_theme() -> &'static str {
        #[cfg(not(target_os = "macos"))]
        {
            Self::default_dark_theme()
        }
        #[cfg(target_os = "macos")]
        {
            if macos_dark_mode_active() {
                Self::default_dark_theme()
            } else {
                Self::default_light_theme()
            }
        }
    }

    /**
     * The default theme that looks good on a dark background.
     */
    fn default_dark_theme() -> &'static str {
        "Monokai Extended"
    }

    /**
     * The default theme that looks good on a light background.
     */
    #[cfg(target_os = "macos")]
    fn default_light_theme() -> &'static str {
        "Monokai Extended Light"
    }

    /// Return the collection of syntect syntax definitions.
    pub fn get_syntax_set(&self) -> &SyntaxSet {
        &self.syntax_set
    }

    pub fn get_syntaxes(&self) -> &[SyntaxReference] {
        self.get_syntax_set().syntaxes()
    }

    fn get_theme_set(&self) -> &LazyThemeSet {
        &self.theme_set
    }

    pub fn themes(&self) -> impl Iterator<Item = &str> {
        self.get_theme_set().themes()
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
    /// Returns [Error::UndetectedSyntax] if it was not possible detect syntax
    /// based on path/file name/extension (or if the path was mapped to
    /// [MappingTarget::MapToUnknown] or [MappingTarget::MapExtensionToUnknown]).
    /// In this case it is appropriate to fall back to other methods to detect
    /// syntax. Such as using the contents of the first line of the file.
    ///
    /// Returns [Error::UnknownSyntax] if a syntax mapping exist, but the mapped
    /// syntax does not exist.
    pub fn get_syntax_for_path(
        &self,
        path: impl AsRef<Path>,
        mapping: &SyntaxMapping,
    ) -> Result<SyntaxReferenceInSet> {
        let path = path.as_ref();

        let syntax_match = mapping.get_syntax_for(path);

        if let Some(MappingTarget::MapToUnknown) = syntax_match {
            return Err(Error::UndetectedSyntax(path.to_string_lossy().into()));
        }

        if let Some(MappingTarget::MapTo(syntax_name)) = syntax_match {
            return self
                .find_syntax_by_name(syntax_name)?
                .ok_or_else(|| Error::UnknownSyntax(syntax_name.to_owned()));
        }

        let file_name = path.file_name().unwrap_or_default();

        match (
            self.get_syntax_for_file_name(file_name, &mapping.ignored_suffixes)?,
            syntax_match,
        ) {
            (Some(syntax), _) => Ok(syntax),

            (_, Some(MappingTarget::MapExtensionToUnknown)) => {
                Err(Error::UndetectedSyntax(path.to_string_lossy().into()))
            }

            _ => self
                .get_syntax_for_file_extension(file_name, &mapping.ignored_suffixes)?
                .ok_or_else(|| Error::UndetectedSyntax(path.to_string_lossy().into())),
        }
    }

    /// Look up a syntect theme by name.
    pub fn get_theme(&self, theme: &str) -> &Theme {
        match self.get_theme_set().get(theme) {
            Some(theme) => theme,
            None => {
                if theme == "ansi-light" || theme == "ansi-dark" {
                    bat_warning!("Theme '{}' is deprecated, using 'ansi' instead.", theme);
                    return self.get_theme("ansi");
                }
                if !theme.is_empty() {
                    bat_warning!("Unknown theme '{}', using default.", theme)
                }
                self.get_theme_set()
                    .get(Self::default_theme())
                    .expect("something is very wrong if the default theme is missing")
            }
        }
    }

    pub(crate) fn get_syntax(
        &self,
        language: Option<&str>,
        input: &mut OpenedInput,
        mapping: &SyntaxMapping,
    ) -> Result<SyntaxReferenceInSet> {
        if let Some(language) = language {
            let syntax_set = self.get_syntax_set();
            return syntax_set
                .find_syntax_by_token(language)
                .map(|syntax| SyntaxReferenceInSet { syntax, syntax_set })
                .ok_or_else(|| Error::UnknownSyntax(language.to_owned()));
        }

        let path = input.path();
        let path_syntax = if let Some(path) = path {
            self.get_syntax_for_path(
                PathAbs::new(path).map_or_else(|_| path.to_owned(), |p| p.as_path().to_path_buf()),
                mapping,
            )
        } else {
            Err(Error::UndetectedSyntax("[unknown]".into()))
        };

        match path_syntax {
            // If a path wasn't provided, or if path based syntax detection
            // above failed, we fall back to first-line syntax detection.
            Err(Error::UndetectedSyntax(path)) => {
                if let Some(sr) = self.get_first_line_syntax(&mut input.reader)? {
                    Ok(sr)
                } else if let Some(sr) = self.get_syntax_by_guesslang(&mut input.reader)? {
                    Ok(sr)
                } else {
                    Err(Error::UndetectedSyntax(path))
                }
            }
            _ => path_syntax,
        }
    }

    pub(crate) fn find_syntax_by_name(
        &self,
        syntax_name: &str,
    ) -> Result<Option<SyntaxReferenceInSet>> {
        let syntax_set = self.get_syntax_set();
        Ok(syntax_set
            .find_syntax_by_name(syntax_name)
            .map(|syntax| SyntaxReferenceInSet { syntax, syntax_set }))
    }

    fn find_syntax_by_extension(&self, e: Option<&OsStr>) -> Result<Option<SyntaxReferenceInSet>> {
        let syntax_set = self.get_syntax_set();
        let extension = e.and_then(|x| x.to_str()).unwrap_or_default();
        Ok(syntax_set
            .find_syntax_by_extension(extension)
            .map(|syntax| SyntaxReferenceInSet { syntax, syntax_set }))
    }

    fn get_syntax_for_file_name(
        &self,
        file_name: &OsStr,
        ignored_suffixes: &IgnoredSuffixes,
    ) -> Result<Option<SyntaxReferenceInSet>> {
        let mut syntax = self.find_syntax_by_extension(Some(file_name))?;
        if syntax.is_none() {
            syntax =
                ignored_suffixes.try_with_stripped_suffix(file_name, |stripped_file_name| {
                    // Note: recursion
                    self.get_syntax_for_file_name(stripped_file_name, ignored_suffixes)
                })?;
        }
        Ok(syntax)
    }

    fn get_syntax_for_file_extension(
        &self,
        file_name: &OsStr,
        ignored_suffixes: &IgnoredSuffixes,
    ) -> Result<Option<SyntaxReferenceInSet>> {
        let mut syntax = self.find_syntax_by_extension(Path::new(file_name).extension())?;
        if syntax.is_none() {
            syntax =
                ignored_suffixes.try_with_stripped_suffix(file_name, |stripped_file_name| {
                    // Note: recursion
                    self.get_syntax_for_file_extension(stripped_file_name, ignored_suffixes)
                })?;
        }
        Ok(syntax)
    }

    fn get_first_line_syntax(
        &self,
        reader: &mut InputReader,
    ) -> Result<Option<SyntaxReferenceInSet>> {
        let syntax_set = self.get_syntax_set();
        Ok(reader
            .first_read
            .as_ref()
            .map(|s| s.split_inclusive('\n').next().unwrap_or(s))
            .and_then(|l| syntax_set.find_syntax_by_first_line(l))
            .map(|syntax| SyntaxReferenceInSet { syntax, syntax_set }))
    }

    #[cfg(not(feature = "guesslang"))]
    fn get_syntax_by_guesslang(
        &self,
        reader: &mut InputReader,
    ) -> Result<Option<SyntaxReferenceInSet>> {
        Ok(None)
    }

    #[cfg(feature = "guesslang")]
    fn get_syntax_by_guesslang(
        &self,
        reader: &mut InputReader,
    ) -> Result<Option<SyntaxReferenceInSet>> {
        let syntax_set = self.get_syntax_set();
        Ok(reader
            .first_read
            .as_ref()
            .and_then(|s| self.guesslang.guess(s.clone()))
            .and_then(|l| syntax_set.find_syntax_by_token(l))
            .map(|syntax| SyntaxReferenceInSet { syntax, syntax_set }))
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

fn hash(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(data);
    hasher.finish()
}

fn load_asset_bytes(
    assert_path: impl AsRef<Path>,
    data: &[u8],
    cache_dir: Option<impl AsRef<Path>>,
) -> Result<Vec<u8>> {
    let assert_path = assert_path.as_ref();
    let mut uncompressed_data: Option<Vec<u8>> = None;
    let mut cache_file = OsString::new();
    if let Some(cache_dir) = &cache_dir {
        let digest = hash(data);
        cache_file.push(
            assert_path
                .file_stem()
                .expect("assert_path has no file stem"),
        );
        debug_assert_eq!(
            assert_path.extension().unwrap().to_str().unwrap(),
            "gz",
            "assert_path must end with .gz"
        );
        cache_file.push(OsString::from(format!(".{:X}.", digest)));
        let cache_dir = cache_dir.as_ref();
        for entry in cache_dir.read_dir()? {
            if let Ok(entry) = entry {
                let file_name = entry.file_name();
                if file_name.as_bytes().starts_with(cache_file.as_bytes()) {
                    let cache_path = entry.path();
                    if let Ok(data) = fs::read(&cache_path) {
                        let digest = hash(&data);
                        let file_name = Path::new(&file_name);
                        if let Some(expected_digest) = file_name
                            .file_stem()
                            .map(|stem| Path::new(stem))
                            .and_then(|stem| stem.extension())
                        {
                            if expected_digest
                                .to_str()
                                .map(|expected| expected == format!("{:X}", digest))
                                .unwrap_or(false)
                            {
                                uncompressed_data = Some(data);
                                break;
                            } else {
                                bat_warning!("Cache corrupted: {}", cache_path.display());
                            }
                        }
                    }
                }
            }
        }
    }
    let uncompressed_data = if let Some(uncompressed_data) = uncompressed_data {
        uncompressed_data
    } else {
        let buffer = Vec::new();
        let mut decoder = GzDecoder::new(buffer);
        decoder.write_all(data)?;
        let buffer = decoder.finish()?;
        if let Some(cache_dir) = &cache_dir {
            let cache_dir = cache_dir.as_ref();
            let digest = hash(&buffer);
            cache_file.push(OsString::from(format!("{:X}.bin", digest)));
            let cache_path = cache_dir.join(cache_file);
            fs::write(&cache_path, &buffer)?;
        }
        buffer
    };
    Ok(uncompressed_data)
}

fn asset_from_contents<T: DeserializeOwned>(contents: &[u8], description: &str) -> Result<T> {
    bincode::deserialize_from(contents)
        .map_err(|_| format!("Could not parse {}", description).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsStr;

    use std::fs::File;
    use std::io::{BufReader, Write};
    use tempfile::TempDir;

    use crate::input::Input;

    struct SyntaxDetectionTest<'a> {
        assets: HighlightingAssets,
        pub syntax_mapping: SyntaxMapping<'a>,
        pub temp_dir: TempDir,
    }

    impl<'a> SyntaxDetectionTest<'a> {
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

            let input = Input::ordinary_file(&file_path);
            let dummy_stdin: &[u8] = &[];
            let mut opened_input = input.open(dummy_stdin, None).unwrap();

            self.get_syntax_name(None, &mut opened_input, &self.syntax_mapping)
        }

        fn syntax_for_file_with_content_os(&self, file_name: &OsStr, first_line: &str) -> String {
            let file_path = self.temp_dir.path().join(file_name);
            let input = Input::from_reader(Box::new(BufReader::new(first_line.as_bytes())))
                .with_name(Some(&file_path));
            let dummy_stdin: &[u8] = &[];
            let mut opened_input = input.open(dummy_stdin, None).unwrap();

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
            let input = Input::stdin().with_name(Some(file_name));
            let mut opened_input = input.open(content, None).unwrap();

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
        let mut test = SyntaxDetectionTest::new();

        test.syntax_mapping
            .insert("*.myext", MappingTarget::MapTo("C"))
            .ok();
        test.syntax_mapping
            .insert("MY_FILE", MappingTarget::MapTo("Markdown"))
            .ok();

        assert!(test.syntax_is_same_for_inputkinds("Test.md", ""));
        assert!(test.syntax_is_same_for_inputkinds("Test.txt", "#!/bin/bash"));
        assert!(test.syntax_is_same_for_inputkinds(".bashrc", ""));
        assert!(test.syntax_is_same_for_inputkinds("test.h", ""));
        assert!(test.syntax_is_same_for_inputkinds("test.js", "#!/bin/bash"));
        assert!(test.syntax_is_same_for_inputkinds("test.myext", ""));
        assert!(test.syntax_is_same_for_inputkinds("MY_FILE", ""));
        assert!(test.syntax_is_same_for_inputkinds("MY_FILE", "<?php"));
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

    #[test]
    fn syntax_detection_with_custom_mapping() {
        let mut test = SyntaxDetectionTest::new();

        assert_eq!(test.syntax_for_file("test.h"), "C++");
        test.syntax_mapping
            .insert("*.h", MappingTarget::MapTo("C"))
            .ok();
        assert_eq!(test.syntax_for_file("test.h"), "C");
    }

    #[test]
    fn syntax_detection_with_extension_mapping_to_unknown() {
        let mut test = SyntaxDetectionTest::new();

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

        // If we setup MapExtensionToUnknown on *.txt, the match on the full
        // file name of "CMakeLists.txt" shall have higher prio, and CMake shall
        // still be used for it
        test.syntax_mapping
            .insert("*.txt", MappingTarget::MapExtensionToUnknown)
            .ok();
        assert_eq!(
            test.syntax_for_file_with_content("CMakeLists.txt", "#!/bin/bash"),
            "CMake"
        );

        // However, for *other* files with a .txt extension, first-line fallback
        // shall now be used
        assert_eq!(
            test.syntax_for_file_with_content("some-other.txt", "#!/bin/bash"),
            "Bourne Again Shell (bash)"
        );
    }

    #[test]
    fn syntax_detection_is_case_insensitive() {
        let mut test = SyntaxDetectionTest::new();

        assert_eq!(test.syntax_for_file("README.md"), "Markdown");
        assert_eq!(test.syntax_for_file("README.mD"), "Markdown");
        assert_eq!(test.syntax_for_file("README.Md"), "Markdown");
        assert_eq!(test.syntax_for_file("README.MD"), "Markdown");

        // Adding a mapping for "MD" in addition to "md" should not break the mapping
        test.syntax_mapping
            .insert("*.MD", MappingTarget::MapTo("Markdown"))
            .ok();

        assert_eq!(test.syntax_for_file("README.md"), "Markdown");
        assert_eq!(test.syntax_for_file("README.mD"), "Markdown");
        assert_eq!(test.syntax_for_file("README.Md"), "Markdown");
        assert_eq!(test.syntax_for_file("README.MD"), "Markdown");
    }

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

        let input = Input::ordinary_file(&file_path_symlink);
        let dummy_stdin: &[u8] = &[];
        let mut opened_input = input.open(dummy_stdin, None).unwrap();

        assert_eq!(
            test.get_syntax_name(None, &mut opened_input, &test.syntax_mapping),
            "SSH Config"
        );
    }
}
