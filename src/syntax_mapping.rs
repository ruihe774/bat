use std::{ffi::OsString, path::Path};

use crate::error::{Error, Result};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, Anchored, Input, MatchKind, StartKind};
use globset::{Candidate, Glob, GlobSet, GlobSetBuilder};
use os_str_bytes::RawOsString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    pub fn builtin() -> Self {
        use MappingTarget::*;
        Self::new(
            include!("../assets/syntax_mapping.ron")
                .into_iter()
                .map(|(s, t)| (Glob::new(s).expect("invalid builtin syntax mapping"), t)),
            include!("../assets/ignored_suffixes.ron")
                .into_iter()
                .map(|s| String::from(s)),
        )
        .expect("invalid builtin syntax mapping")
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
            .chain(name_matches.into_iter())
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
