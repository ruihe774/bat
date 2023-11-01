#[allow(unused_imports)]
use zwrite::{write, writeln};

use std::{ffi::OsString, path::Path};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, Anchored, Input, MatchKind, StartKind};
use globset::{Candidate, Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use os_str_bytes::RawOsString;
use serde::{Deserialize, Serialize};

use crate::config::ConfigString;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingTarget {
    /// For mapping a path to a specific syntax.
    MapTo(ConfigString),

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
pub struct ConsolidatedSyntaxMapping {
    targets: Vec<MappingTarget>,
    globset: GlobSet,
    ignored_suffixes: AhoCorasick,
}

impl ConsolidatedSyntaxMapping {
    fn new(
        mapped_syntaxes: impl IntoIterator<Item = (Glob, MappingTarget)>,
        ignored_suffixes: impl IntoIterator<Item = impl AsRef<[u8]> + AsMut<[u8]>>,
    ) -> Result<Self> {
        let mut builder = GlobSetBuilder::new();
        Ok(ConsolidatedSyntaxMapping {
            targets: mapped_syntaxes
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
                .build(ignored_suffixes.into_iter().map(|mut v| {
                    v.as_mut().reverse();
                    v
                }))?,
        })
    }

    pub(crate) fn get_syntax_for(&self, path: impl AsRef<Path>) -> Option<&MappingTarget> {
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
            .map(|i| &self.targets[i])
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxMapping {
    use_builtins: bool,
    mapped_syntaxes: Vec<(ConfigString, MappingTarget)>,
    ignored_suffixes: Vec<ConfigString>,
}

impl SyntaxMapping {
    pub fn consolidate(self) -> Result<ConsolidatedSyntaxMapping> {
        ConsolidatedSyntaxMapping::new(
            {
                let iter = {
                    use self::MappingTarget as RealMappingTarget;
                    enum MappingTarget {
                        MapTo(&'static str),
                        MapToUnknown,
                        MapExtensionToUnknown,
                    }
                    use MappingTarget::*;
                    if self.use_builtins {
                        include!("../../assets/syntax_mapping.ron").as_slice()
                    } else {
                        &[]
                    }
                    .iter()
                    .map(|(s, t)| {
                        (
                            (*s).into(),
                            match t {
                                MapTo(s) => RealMappingTarget::MapTo((*s).into()),
                                MapToUnknown => RealMappingTarget::MapToUnknown,
                                MapExtensionToUnknown => RealMappingTarget::MapExtensionToUnknown,
                            },
                        )
                    })
                }
                .chain(self.mapped_syntaxes)
                .map(|(s, t)| {
                    GlobBuilder::new(s.as_str())
                        .case_insensitive(true)
                        .literal_separator(true)
                        .build()
                        .map(|g| (g, t))
                });
                let mut mapped_syntaxes = Vec::with_capacity(iter.size_hint().0);
                for mapping in iter {
                    mapped_syntaxes.push(mapping?);
                }
                mapped_syntaxes
            },
            if self.use_builtins {
                include!("../../assets/ignored_suffixes.ron").as_slice()
            } else {
                &[]
            }
            .iter()
            .copied()
            .map(ConfigString::from)
            .chain(self.ignored_suffixes)
            .map(ConfigString::into_bytes),
        )
    }

    pub fn map_syntax(&mut self, glob: impl Into<ConfigString>, target: MappingTarget) {
        self.mapped_syntaxes.push((glob.into(), target));
    }

    pub fn ignore_suffix(&mut self, suffix: impl Into<ConfigString>) {
        self.ignored_suffixes.push(suffix.into());
    }

    pub fn use_builtins(&mut self, yes: bool) {
        self.use_builtins = yes;
    }
}

impl Default for SyntaxMapping {
    fn default() -> Self {
        SyntaxMapping {
            use_builtins: true,
            mapped_syntaxes: Vec::new(),
            ignored_suffixes: Vec::new(),
        }
    }
}
