use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    List,
    #[serde(rename = "cards", alias = "grid")]
    Grid,
}

impl Mode {
    pub const ALL: [Self; 2] = [Self::List, Self::Grid];

    pub fn key(self) -> &'static str {
        match self {
            Self::List => "view-list",
            Self::Grid => "view-cards",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::List => "icons/list.svg",
            Self::Grid => "icons/layout-grid.svg",
        }
    }
}
