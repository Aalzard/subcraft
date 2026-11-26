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
    /// Signed distance from the low-poly surface to the high poly along the cage normal
    /// (0.5 = on the surface, above 0.5 = high poly stands proud).
    Height,
    /// Average unoccluded direction, in the low poly's tangent space (like the Normal map).
    BentNormal,
    /// Additional ID maps (issue #733), each baked with its own source mesh and colour source.
    /// They are not in [`MeshMapKind::ALL`]: a project lists the ones it set up.
    Id2,
    Id3,
    Id4,
    Id5,
    Id6,
    Id7,
    Id8,
    Id9,
}

impl MeshMapKind {
    pub const ALL: [MeshMapKind; 9] = [
        MeshMapKind::Normal,
        MeshMapKind::WorldSpaceNormal,
        MeshMapKind::Id,
        MeshMapKind::AmbientOcclusion,
        MeshMapKind::Curvature,
        MeshMapKind::Position,
        MeshMapKind::Thickness,
        MeshMapKind::Height,
        MeshMapKind::BentNormal,
    ];

    /// The additional ID map slots, in order (ID 2 first).
    pub const EXTRA_IDS: [MeshMapKind; 8] = [
        MeshMapKind::Id2,
        MeshMapKind::Id3,
        MeshMapKind::Id4,
        MeshMapKind::Id5,
        MeshMapKind::Id6,
        MeshMapKind::Id7,
        MeshMapKind::Id8,
        MeshMapKind::Id9,
    ];

    /// The main ID map or one of the additional ones.
    pub fn is_id(self) -> bool {
        self == MeshMapKind::Id || self.extra_id_index().is_some()
    }

    /// Position of an additional ID map in [`MeshMapKind::EXTRA_IDS`].
    pub fn extra_id_index(self) -> Option<usize> {
        Self::EXTRA_IDS.iter().position(|k| *k == self)
    }

    /// Every kind: [`MeshMapKind::ALL`] then the additional ID maps.
    pub fn all_with_extra_ids() -> impl Iterator<Item = MeshMapKind> {
        Self::ALL.into_iter().chain(Self::EXTRA_IDS)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            MeshMapKind::Id2 => "ID 2",
            MeshMapKind::Id3 => "ID 3",
            MeshMapKind::Id4 => "ID 4",
            MeshMapKind::Id5 => "ID 5",
            MeshMapKind::Id6 => "ID 6",
            MeshMapKind::Id7 => "ID 7",
            MeshMapKind::Id8 => "ID 8",
            MeshMapKind::Id9 => "ID 9",
            MeshMapKind::Normal => "Normal",
            MeshMapKind::WorldSpaceNormal => "World Space Normal",
            MeshMapKind::Id => "ID",
            MeshMapKind::AmbientOcclusion => "Ambient Occlusion",
            MeshMapKind::Curvature => "Curvature",
            MeshMapKind::Position => "Position",
            MeshMapKind::Thickness => "Thickness",
            MeshMapKind::Height => "Height",
            MeshMapKind::BentNormal => "Bent Normal",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            MeshMapKind::Id2 => "id_2",
            MeshMapKind::Id3 => "id_3",
            MeshMapKind::Id4 => "id_4",
            MeshMapKind::Id5 => "id_5",
            MeshMapKind::Id6 => "id_6",
            MeshMapKind::Id7 => "id_7",
            MeshMapKind::Id8 => "id_8",
            MeshMapKind::Id9 => "id_9",
            MeshMapKind::Normal => "normal_base",
            MeshMapKind::WorldSpaceNormal => "world_normal",
            MeshMapKind::Id => "id",
            MeshMapKind::AmbientOcclusion => "ambient_occlusion",
            MeshMapKind::Curvature => "curvature",
            MeshMapKind::Position => "position",
            MeshMapKind::Thickness => "thickness",
            MeshMapKind::Height => "height",
            MeshMapKind::BentNormal => "bent_normal",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::all_with_extra_ids().find(|m| m.key() == key)
    }

    /// Number of stored components.
    pub fn components(self) -> usize {
        match self {
            MeshMapKind::Normal
            | MeshMapKind::WorldSpaceNormal
            | MeshMapKind::Id
            | MeshMapKind::Id2
            | MeshMapKind::Id3
            | MeshMapKind::Id4
            | MeshMapKind::Id5
            | MeshMapKind::Id6
            | MeshMapKind::Id7
            | MeshMapKind::Id8
            | MeshMapKind::Id9
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
            MeshMapKind::Id
            | MeshMapKind::Id2
            | MeshMapKind::Id3
            | MeshMapKind::Id4
            | MeshMapKind::Id5
            | MeshMapKind::Id6
            | MeshMapKind::Id7
            | MeshMapKind::Id8
            | MeshMapKind::Id9 => [0.0, 0.0, 0.0],
            MeshMapKind::AmbientOcclusion => [1.0; 3],
            MeshMapKind::Curvature => [0.5; 3],
            MeshMapKind::Position => [0.5; 3],
            MeshMapKind::Thickness => [0.0; 3],
            MeshMapKind::Height => [0.5; 3],
            MeshMapKind::BentNormal => [0.5, 0.5, 1.0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_id_maps_have_their_own_keys_and_read_like_the_id_map() {
        for (i, k) in MeshMapKind::EXTRA_IDS.into_iter().enumerate() {
            assert!(!MeshMapKind::ALL.contains(&k));
            assert_eq!(MeshMapKind::from_key(k.key()), Some(k));
            assert_eq!(k.extra_id_index(), Some(i));
            assert_eq!(k.display_name(), format!("ID {}", i + 2));
            assert!(k.is_id());
            assert_eq!((k.components(), k.neutral_value()), (MeshMapKind::Id.components(), MeshMapKind::Id.neutral_value()));
        }
        assert!(MeshMapKind::Id.is_id() && !MeshMapKind::Normal.is_id());
        assert_eq!(MeshMapKind::all_with_extra_ids().count(), 17);
    }
}
