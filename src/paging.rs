use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PagingMode {
    Always,
    QuitIfOneScreen,
    #[default]
    Never,
}
