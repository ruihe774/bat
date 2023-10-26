use std::{ffi::OsString, path::Path};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, Anchored, Input, MatchKind, StartKind};
use globset::{Candidate, Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use os_str_bytes::RawOsString;

use crate::error::Result;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MappingTarget<'a> {
    /// For mapping a path to a specific syntax.
    MapTo(&'a str),

    /// For mapping a path (typically an extension-less file name) to an unknown
    /// syntax. This typically means later using the contents of the first line
    /// of the file to determine what syntax to use.
    MapToUnknown,

    /// For mapping a file extension (e.g. `*.conf`) to an unknown syntax. This
    /// typically means later using the contents of the first line of the file
    /// to determine what syntax to use. However, if a syntax handles a file
    /// name that happens to have the given file extension (e.g. `resolv.conf`),
    /// then that association will have higher precedence, and the mapping will
    /// be ignored.
    MapExtensionToUnknown,
}

#[derive(Debug, Clone)]
pub struct SyntaxMapping<'a> {
    targets: Vec<MappingTarget<'a>>,
    globset: GlobSet,
    ignored_suffixes: AhoCorasick,
}

impl<'a> SyntaxMapping<'a> {
    pub fn new(
        mapping: impl IntoIterator<Item = (Glob, MappingTarget<'a>)>,
        ignored_suffixes: impl IntoIterator<Item = String>,
    ) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        Ok(SyntaxMapping {
            targets: mapping
                .into_iter()
                .map(|(glob, target)| {
                    builder.add(glob);
                    target
                })
                .collect(),
            globset: builder.build()?,
            ignored_suffixes: AhoCorasickBuilder::new()
                .ascii_case_insensitive(true)
                .match_kind(MatchKind::LeftmostLongest)
                .start_kind(StartKind::Anchored)
                .build(ignored_suffixes.into_iter().map(|s| {
                    let mut v: Vec<u8> = s.into();
                    v.reverse();
                    v
                }))?,
        })
    }

    pub(crate) fn get_syntax_for(&self, path: impl AsRef<Path>) -> Option<MappingTarget> {
        let candidate_path = Candidate::new(path.as_ref());
        let candidate_filename = Path::new(path.as_ref()).file_name().map(Candidate::new);
        let path_matches = self.globset.matches_candidate(&candidate_path);
        let name_matches = candidate_filename
            .as_ref()
            .map(|candidate_filename| self.globset.matches_candidate(candidate_filename))
            .unwrap_or_default();
        path_matches
            .into_iter()
            .chain(name_matches)
            .max()
            .map(|i| self.targets[i])
    }

    pub(crate) fn strip_ignored_suffixes(&self, file_name: OsString) -> OsString {
        let file_name = RawOsString::new(file_name);
        let mut bytes = file_name.into_raw_vec();
        bytes.reverse();
        let ignored_len: usize = self
            .ignored_suffixes
            .find_iter(Input::new(&bytes).anchored(Anchored::Yes))
            .map(|m| m.len())
            .sum();
        bytes.reverse();
        bytes.truncate(bytes.len() - ignored_len);
        RawOsString::assert_from_raw_vec(bytes).into_os_string()
    }
}

impl<'a> Default for SyntaxMapping<'a> {
    fn default() -> Self {
        let patterns: [&[u8]; 0] = [];
        SyntaxMapping {
            targets: Default::default(),
            globset: Default::default(),
            ignored_suffixes: AhoCorasick::new(patterns).unwrap(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SyntaxMappingBuilder<'a> {
    pub mapping: Vec<(Glob, MappingTarget<'a>)>,
    pub ignored_suffixes: Vec<String>,
}

impl<'a> SyntaxMappingBuilder<'a> {
    pub fn new() -> Self {
        SyntaxMappingBuilder {
            mapping: Vec::new(),
            ignored_suffixes: Vec::new(),
        }
    }

    pub fn with_builtin(mut self) -> Self {
        use MappingTarget::*;
        self.mapping.extend(
            include!("../assets/syntax_mapping.ron")
                .into_iter()
                .map(|(s, t)| {
                    (
                        GlobBuilder::new(s)
                            .case_insensitive(true)
                            .literal_separator(true)
                            .build()
                            .expect("invalid builtin syntax mapping"),
                        t,
                    )
                }),
        );
        self.ignored_suffixes.extend(
            include!("../assets/ignored_suffixes.ron")
                .into_iter()
                .map(|s| s.to_owned()),
        );
        self
    }

    pub fn build(self) -> Result<SyntaxMapping<'a>> {
        SyntaxMapping::new(self.mapping, self.ignored_suffixes)
    }

    pub fn map_syntax(mut self, glob: &'_ str, target: MappingTarget<'a>) -> Result<Self> {
        self.mapping.push((
            GlobBuilder::new(glob)
                .case_insensitive(true)
                .literal_separator(true)
                .build()?,
            target,
        ));
        Ok(self)
    }

    pub fn ignored_suffix(mut self, suffix: String) -> Self {
        self.ignored_suffixes.push(suffix);
        self
    }
}

impl<'a> Default for SyntaxMappingBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}
