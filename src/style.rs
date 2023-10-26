use std::cmp;
use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::*;

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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StyleComponent {
    Auto,
    #[cfg(feature = "git")]
    Changes,
    Grid,
    Rule,
    Header,
    HeaderFilename,
    LineNumbers,
    Snip,
    Full,
    Plain,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct StyleComponentWrapper(StyleComponent);

impl PartialOrd for StyleComponentWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        (self.0 as u8).partial_cmp(&(other.0 as u8))
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

impl Into<StyleComponent> for StyleComponentWrapper {
    fn into(self) -> StyleComponent {
        self.0
    }
}

impl StyleComponent {
    pub fn components(self, interactive_terminal: bool) -> &'static [StyleComponent] {
        match self {
            StyleComponent::Auto => {
                if interactive_terminal {
                    StyleComponent::Full.components(interactive_terminal)
                } else {
                    StyleComponent::Plain.components(interactive_terminal)
                }
            }
            #[cfg(feature = "git")]
            StyleComponent::Changes => &[StyleComponent::Changes],
            StyleComponent::Grid => &[StyleComponent::Grid],
            StyleComponent::Rule => &[StyleComponent::Rule],
            StyleComponent::Header => &[StyleComponent::HeaderFilename],
            StyleComponent::HeaderFilename => &[StyleComponent::HeaderFilename],
            StyleComponent::LineNumbers => &[StyleComponent::LineNumbers],
            StyleComponent::Snip => &[StyleComponent::Snip],
            StyleComponent::Full => &[
                #[cfg(feature = "git")]
                StyleComponent::Changes,
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
            #[cfg(feature = "git")]
            "changes" => Ok(StyleComponent::Changes),
            "grid" => Ok(StyleComponent::Grid),
            "rule" => Ok(StyleComponent::Rule),
            "header" => Ok(StyleComponent::Header),
            "header-filename" => Ok(StyleComponent::HeaderFilename),
            "numbers" => Ok(StyleComponent::LineNumbers),
            "snip" => Ok(StyleComponent::Snip),
            "full" => Ok(StyleComponent::Full),
            "plain" => Ok(StyleComponent::Plain),
            // for backward compatibility, default is to full
            "default" => Ok(StyleComponent::Full),
            _ => Err(UnknownStyle { name: s.to_owned() }.into()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StyleComponents(BTreeSet<StyleComponentWrapper>);

impl StyleComponents {
    pub fn new(components: &[StyleComponent]) -> StyleComponents {
        let set: BTreeSet<_> = components
            .iter()
            .copied()
            .map(|component| component.into())
            .collect();
        assert!(
            !set.contains(&StyleComponent::Grid.into())
                || !set.contains(&StyleComponent::Rule.into()),
            "cannot specify style components 'grid' and 'rule' together"
        );
        StyleComponents(set)
    }

    #[cfg(feature = "git")]
    pub fn changes(&self) -> bool {
        self.0.contains(&StyleComponent::Changes.into())
    }

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
        self.0
            .iter()
            .copied()
            .all(|c| c == StyleComponent::Plain.into())
    }

    pub fn insert(&mut self, component: StyleComponent) {
        self.0.insert(component.into());
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}
