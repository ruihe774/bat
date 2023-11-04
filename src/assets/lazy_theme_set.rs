#[allow(unused_imports)]
use zwrite::{write, writeln};

use std::cell::RefCell;
use std::collections::BTreeMap;

use once_cell::unsync::OnceCell;
use serde::Deserialize;
use serde::Serialize;
use serde_bytes::{ByteBuf, Bytes};
use syntect::highlighting::Theme;

use super::asset_from_bytes;
use crate::error::Result;

/// Same structure as a [`syntect::highlighting::ThemeSet`] but with themes
/// stored in raw serialized form, and deserialized on demand.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LazyThemeSet {
    /// This is a [`BTreeMap`] because that's what [`syntect::highlighting::ThemeSet`] uses
    themes: BTreeMap<String, LazyTheme>,
}

/// Stores raw serialized data for a theme with methods to lazily deserialize
/// (load) the theme.
#[derive(Debug, Serialize, Deserialize)]
struct LazyTheme {
    #[serde(
        serialize_with = "serialize_refcell_bytes",
        deserialize_with = "deserialize_refcell_bytes"
    )]
    serialized: RefCell<Vec<u8>>,

    #[serde(skip, default = "OnceCell::new")]
    deserialized: OnceCell<syntect::highlighting::Theme>,
}

impl LazyThemeSet {
    /// Lazily load the given theme
    pub fn get(&self, name: &str) -> Option<&Theme> {
        self.themes
            .get(name)
            .map(|lazy_theme| lazy_theme.deserialize().unwrap())
    }

    /// Returns the name of all themes.
    pub fn themes(&self) -> impl Iterator<Item = &str> {
        self.themes.keys().map(String::as_str)
    }
}

impl LazyTheme {
    fn deserialize(&self) -> Result<&Theme> {
        self.deserialized
            .get_or_try_init(|| asset_from_bytes(self.serialized.take()))
            .map_err(|e| e.context("failed to load theme"))
    }
}

fn serialize_refcell_bytes<S>(
    bytes: &RefCell<Vec<u8>>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    Bytes::new(bytes.borrow().as_slice()).serialize(serializer)
}

fn deserialize_refcell_bytes<'de, D>(
    deserializer: D,
) -> std::result::Result<RefCell<Vec<u8>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let bytebuf = ByteBuf::deserialize(deserializer)?;
    Ok(RefCell::new(bytebuf.into_vec()))
}

#[cfg(feature = "build-assets")]
impl TryFrom<syntect::highlighting::ThemeSet> for LazyThemeSet {
    type Error = crate::error::Error;

    /// To collect themes, a [`syntect::highlighting::ThemeSet`] is needed. Once all desired themes
    /// have been added, we need a way to convert that into [`LazyThemeSet`] so
    /// that themes can be lazy-loaded later. This function does that
    /// conversion.
    fn try_from(theme_set: syntect::highlighting::ThemeSet) -> Result<Self> {
        let mut lazy_theme_set = LazyThemeSet::default();

        for (name, theme) in theme_set.themes {
            // All we have to do is to serialize the theme
            let lazy_theme = LazyTheme {
                serialized: RefCell::new(crate::assets::build_assets::asset_to_contents(
                    &theme,
                    &format!("theme {}", name),
                    false,
                )?),
                deserialized: OnceCell::new(),
            };

            // Ok done, now we can add it
            lazy_theme_set.themes.insert(name, lazy_theme);
        }

        Ok(lazy_theme_set)
    }
}
