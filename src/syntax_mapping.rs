use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

use crate::error::Result;

use globset::{Candidate, Glob, GlobSet, GlobSetBuilder};
use os_str_bytes::RawOsStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Default)]
pub struct SyntaxMapping {
    targets: Vec<MappingTarget>,
    globset: GlobSet,
    ignored_suffixes: Vec<&'static str>,
}

impl SyntaxMapping {
    pub fn new(
        mapping: Vec<(Glob, MappingTarget)>,
        ignored_suffixes: &[&'static str],
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
            ignored_suffixes: Vec::from(ignored_suffixes),
        })
    }

    pub fn builtin() -> Self {
        use MappingTarget::*;
        Self::new(
            Vec::from_iter(
                include!("../assets/syntax_mapping.plist")
                    .into_iter()
                    .map(|(s, t)| (Glob::new(s).expect("invalid builtin syntax mapping"), t)),
            ),
            include!("../assets/ignored_suffixes.plist").as_slice(),
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

    pub(crate) fn strip_ignored_suffixes(&self, file_name: impl AsRef<OsStr>) -> OsString {
        let mut name = RawOsStr::new(file_name.as_ref()).into_owned();
        'outer: loop {
            for suffix in self.ignored_suffixes.iter().cloned() {
                if name.strip_suffix(suffix).is_some() {
                    name.truncate(name.raw_len() - suffix.len());
                    continue 'outer;
                }
            }
            break name.to_os_str().into_owned();
        }
    }
}
