//! Stable vocabulary for geometry selections. Indices refer to the saved, immutable mesh.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    #[default]
    Connected,
    UvIsland,
    Object,
    Material,
}

impl PartKind {
    pub const ALL: [Self; 4] = [Self::Connected, Self::UvIsland, Self::Object, Self::Material];
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Connected pieces",
            Self::UvIsland => "UV islands",
            Self::Object => "Objects",
            Self::Material => "Material IDs",
        }
    }
}
