use std::{ffi::OsString, path::Path};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, Anchored, Input, MatchKind, StartKind};
use globset::{Candidate, Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use os_str_bytes::RawOsString;
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingTarget {
    /// For mapping a path to a specific syntax.
    MapTo(&'static str),

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
        ignored_suffixes: impl IntoIterator<Item = String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxMapping {
    use_builtins: bool,
    #[serde(deserialize_with = "deserialize_leaky_mapped_syntaxes")]
    mapped_syntaxes: Vec<(String, MappingTarget)>,
    ignored_suffixes: Vec<String>,
}

impl SyntaxMapping {
    pub fn consolidate(self) -> Result<ConsolidatedSyntaxMapping> {
        use MappingTarget::*;
        ConsolidatedSyntaxMapping::new(
            {
                let iter = if self.use_builtins {
                    include!("../../assets/syntax_mapping.ron").as_slice()
                } else {
                    &[]
                }
                .iter()
                .copied()
                .chain(self.mapped_syntaxes.iter().map(|(s, t)| (s.as_str(), *t)))
                .map(|(s, t)| {
                    GlobBuilder::new(s)
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
            .map(ToOwned::to_owned)
            .chain(self.ignored_suffixes),
        )
    }

    pub fn map_syntax(&mut self, glob: impl Into<String>, target: MappingTarget) {
        self.mapped_syntaxes.push((glob.into(), target));
    }

    pub fn ignore_suffix(&mut self, suffix: impl Into<String>) {
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

fn deserialize_leaky_mapped_syntaxes<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<(String, MappingTarget)>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use self::MappingTarget as RealMappingTarget;
    #[derive(Deserialize)]
    enum MappingTarget {
        MapTo(String),
        MapToUnknown,
        MapExtensionToUnknown,
    }
    let v = Vec::<(String, MappingTarget)>::deserialize(deserializer)?;
    Ok(v.into_iter()
        .map(|(s, t)| {
            (
                s,
                match t {
                    MappingTarget::MapTo(s) => RealMappingTarget::MapTo(s.leak()),
                    MappingTarget::MapToUnknown => RealMappingTarget::MapToUnknown,
                    MappingTarget::MapExtensionToUnknown => {
                        RealMappingTarget::MapExtensionToUnknown
                    }
                },
            )
        })
        .collect())
}
