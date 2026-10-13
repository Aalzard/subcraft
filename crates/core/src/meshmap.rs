//! Mesh map vocabulary: UV-space maps derived from geometry (baked).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshMapKind {
    /// Tangent-space normal (from high poly, or flat when baking low on low).
    Normal,
    WorldSpaceNormal,
    Id,
    AmbientOcclusion,
    Curvature,
    Position,
    Thickness,
}

impl MeshMapKind {
    pub const ALL: [MeshMapKind; 7] = [
        MeshMapKind::Normal,
        MeshMapKind::WorldSpaceNormal,
        MeshMapKind::Id,
        MeshMapKind::AmbientOcclusion,
        MeshMapKind::Curvature,
        MeshMapKind::Position,
        MeshMapKind::Thickness,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            MeshMapKind::Normal => "Normal",
            MeshMapKind::WorldSpaceNormal => "World Space Normal",
            MeshMapKind::Id => "ID",
            MeshMapKind::AmbientOcclusion => "Ambient Occlusion",
            MeshMapKind::Curvature => "Curvature",
            MeshMapKind::Position => "Position",
            MeshMapKind::Thickness => "Thickness",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            MeshMapKind::Normal => "normal_base",
            MeshMapKind::WorldSpaceNormal => "world_normal",
            MeshMapKind::Id => "id",
            MeshMapKind::AmbientOcclusion => "ambient_occlusion",
            MeshMapKind::Curvature => "curvature",
            MeshMapKind::Position => "position",
            MeshMapKind::Thickness => "thickness",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    /// Number of stored components.
    pub fn components(self) -> usize {
        match self {
            MeshMapKind::Normal
            | MeshMapKind::WorldSpaceNormal
            | MeshMapKind::Id
            | MeshMapKind::Position => 3,
            _ => 1,
        }
    }

    /// Value used when the map has not been baked.
    pub fn neutral_value(self) -> [f32; 3] {
        match self {
            MeshMapKind::Normal => [0.5, 0.5, 1.0],
            MeshMapKind::WorldSpaceNormal => [0.5, 1.0, 0.5],
            MeshMapKind::Id => [0.0, 0.0, 0.0],
            MeshMapKind::AmbientOcclusion => [1.0; 3],
            MeshMapKind::Curvature => [0.5; 3],
            MeshMapKind::Position => [0.5; 3],
            MeshMapKind::Thickness => [0.0; 3],
        }
    }
}
