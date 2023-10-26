use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WrappingMode {
    Character,
    // The bool specifies whether wrapping has been explicitly disabled by the user via --wrap=never
    NoWrapping(bool),
}

impl Default for WrappingMode {
    fn default() -> Self {
        WrappingMode::NoWrapping(false)
    }
}
