#[allow(unused_imports)]
use zwrite::{write, writeln};

use std::cmp;
use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug)]
pub struct UnknownStyle {
    pub name: String,
}

impl Display for UnknownStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown style component '{}'", self.name)
    }
}

impl StdError for UnknownStyle {}

#[derive(Debug)]
pub struct ConflictStyle(pub String, pub String);

impl Display for ConflictStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot specify style components '{}' and '{}' together",
            self.0, self.1
        )
    }
}

impl StdError for ConflictStyle {}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StyleComponent {
    Auto,
    Grid,
    Rule,
    Header,
    HeaderFilename,
    LineNumbers,
    Snip,
    Full,
    Plain,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct StyleComponentWrapper(StyleComponent);

impl PartialOrd for StyleComponentWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StyleComponentWrapper {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        (self.0 as u8).cmp(&(other.0 as u8))
    }
}

impl From<StyleComponent> for StyleComponentWrapper {
    fn from(value: StyleComponent) -> Self {
        StyleComponentWrapper(value)
    }
}

impl From<StyleComponentWrapper> for StyleComponent {
    fn from(value: StyleComponentWrapper) -> Self {
        value.0
    }
}

impl StyleComponent {
    fn components(self, interactive: bool) -> &'static [StyleComponent] {
        match self {
            StyleComponent::Auto => {
                if interactive {
                    StyleComponent::Full.components(interactive)
                } else {
                    StyleComponent::Plain.components(interactive)
                }
            }
            StyleComponent::Grid => &[StyleComponent::Grid],
            StyleComponent::Rule => &[StyleComponent::Rule],
            StyleComponent::Header | StyleComponent::HeaderFilename => {
                &[StyleComponent::HeaderFilename]
            }
            StyleComponent::LineNumbers => &[StyleComponent::LineNumbers],
            StyleComponent::Snip => &[StyleComponent::Snip],
            StyleComponent::Full => &[
                StyleComponent::Grid,
                StyleComponent::HeaderFilename,
                StyleComponent::LineNumbers,
                StyleComponent::Snip,
            ],
            StyleComponent::Plain => &[],
        }
    }
}

impl FromStr for StyleComponent {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "auto" => Ok(StyleComponent::Auto),
            "grid" => Ok(StyleComponent::Grid),
            "rule" => Ok(StyleComponent::Rule),
            "header" => Ok(StyleComponent::Header),
            "header-filename" => Ok(StyleComponent::HeaderFilename),
            "numbers" => Ok(StyleComponent::LineNumbers),
            "snip" => Ok(StyleComponent::Snip),
            // for backward compatibility, default is to full
            "full" | "default" => Ok(StyleComponent::Full),
            "plain" => Ok(StyleComponent::Plain),
            _ => Err(UnknownStyle { name: s.to_owned() }.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StyleComponents(Vec<StyleComponent>);

impl StyleComponents {
    pub fn new(components: Vec<StyleComponent>) -> Self {
        StyleComponents(components)
    }

    pub fn plain() -> Self {
        Self::new(Vec::new())
    }

    pub fn full() -> Self {
        Self::new(vec![StyleComponent::Full])
    }

    pub fn consolidate(self, interactive: bool) -> Result<ConsolidatedStyleComponents> {
        let components: BTreeSet<_> = self
            .0
            .into_iter()
            .flat_map(|component| component.components(interactive))
            .copied()
            .map(Into::into)
            .collect();
        if components.contains(&StyleComponent::Grid.into())
            && components.contains(&StyleComponent::Rule.into())
        {
            Err(ConflictStyle("grid".to_owned(), "rule".to_owned()).into())
        } else {
            Ok(ConsolidatedStyleComponents(components))
        }
    }
}

impl Default for StyleComponents {
    fn default() -> Self {
        StyleComponents(vec![StyleComponent::Auto])
    }
}

#[derive(Debug, Clone)]
pub struct ConsolidatedStyleComponents(BTreeSet<StyleComponentWrapper>);

impl ConsolidatedStyleComponents {
    pub fn grid(&self) -> bool {
        self.0.contains(&StyleComponent::Grid.into())
    }

    pub fn rule(&self) -> bool {
        self.0.contains(&StyleComponent::Rule.into())
    }

    pub fn header(&self) -> bool {
        self.header_filename()
    }

    pub fn header_filename(&self) -> bool {
        self.0.contains(&StyleComponent::HeaderFilename.into())
    }

    pub fn numbers(&self) -> bool {
        self.0.contains(&StyleComponent::LineNumbers.into())
    }

    pub fn snip(&self) -> bool {
        self.0.contains(&StyleComponent::Snip.into())
    }

    pub fn plain(&self) -> bool {
        self.0.is_empty()
    }
}
