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
    /// Signed high-poly displacement along the low-poly normal (0.5 = on the low surface).
    Height,
    /// World-space average unoccluded direction (from the AO rays).
    BentNormal,
    /// High-poly coverage: 1 where the projection found the high poly, 0 where it missed.
    Opacity,
}

impl MeshMapKind {
    pub const ALL: [MeshMapKind; 10] = [
        MeshMapKind::Normal,
        MeshMapKind::WorldSpaceNormal,
        MeshMapKind::Id,
        MeshMapKind::AmbientOcclusion,
        MeshMapKind::Curvature,
        MeshMapKind::Position,
        MeshMapKind::Thickness,
        MeshMapKind::Height,
        MeshMapKind::BentNormal,
        MeshMapKind::Opacity,
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
            MeshMapKind::Height => "Height",
            MeshMapKind::BentNormal => "Bent Normals",
            MeshMapKind::Opacity => "Opacity",
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
            MeshMapKind::Height => "height",
            MeshMapKind::BentNormal => "bent_normals",
            MeshMapKind::Opacity => "opacity",
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
            | MeshMapKind::Position
            | MeshMapKind::BentNormal => 3,
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
            MeshMapKind::Height => [0.5; 3],
            MeshMapKind::BentNormal => [0.5, 1.0, 0.5],
            MeshMapKind::Opacity => [1.0; 3],
        }
    }
}
