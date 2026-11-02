//! How high-poly bake rays choose which high-poly parts they may hit.

use serde::{Deserialize, Serialize};

/// Which high-poly parts a ray cast from a low-poly part may hit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BakeMatchMode {
    /// Any high-poly surface (the closest hit wins).
    #[default]
    Always,
    /// Only high-poly parts whose object name matches the low-poly part's name once the low /
    /// high suffixes are removed.
    ByName,
}

impl BakeMatchMode {
    pub const ALL: [Self; 2] = [Self::Always, Self::ByName];
    pub fn label(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::ByName => "By mesh name",
        }
    }
}

/// Bake part matching: mode plus the name suffixes that mark low and high parts.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct BakeMatch {
    pub mode: BakeMatchMode,
    /// Suffix of low-poly part names (case-insensitive). `_lo` is always accepted too.
    pub low_suffix: String,
    /// Suffix of high-poly part names (case-insensitive). `_hi` is always accepted too.
    pub high_suffix: String,
}

impl BakeMatch {
    pub const DEFAULT_LOW: &'static str = "_low";
    pub const DEFAULT_HIGH: &'static str = "_high";

    /// Match by name with the default suffixes.
    pub fn by_name() -> Self {
        Self { mode: BakeMatchMode::ByName, ..Self::default() }
    }
}

impl Default for BakeMatch {
    fn default() -> Self {
        Self { mode: BakeMatchMode::Always, low_suffix: Self::DEFAULT_LOW.into(), high_suffix: Self::DEFAULT_HIGH.into() }
    }
}
