//! Project-wide settings that are saved with the project but are not part of the undo history:
//! how normal maps are exported and which export maps are switched off.

use serde::{Deserialize, Serialize};

/// Normal-map handedness for exported textures (the viewport is always OpenGL, +Y up).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalFormat {
    #[default]
    OpenGl,
    DirectX,
}

impl NormalFormat {
    pub fn label(self) -> &'static str {
        match self {
            NormalFormat::OpenGl => "OpenGL (Y+)",
            NormalFormat::DirectX => "DirectX (Y-)",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    /// Applied to exported normal maps whatever the template says (see `export::ExportSettings`).
    /// `None` keeps each template's own convention (Unreal: DirectX, most others: OpenGL).
    pub normal_format: Option<NormalFormat>,
    /// Name of the export template last used; empty = the default one.
    pub export_template: String,
    /// Output maps unchecked in the Export window, as `"texture set/map suffix"` (e.g. `Body/Height`).
    pub export_maps_off: Vec<String>,
    /// Selection to come back to when Alt+click solo is toggled off. Not saved.
    #[serde(skip)]
    pub export_solo_previous: Option<Vec<String>>,
    /// Put each UDIM tile's files in its own sub-folder (`1001/`, `1002/`...).
    pub udim_folders: bool,
    /// High-poly bake part matching (Always / By mesh name + suffixes). Optional in the file:
    /// absent = Always with `_low` / `_high`, so no schema bump is needed.
    #[serde(skip_serializing_if = "is_default_match")]
    pub bake_match: subcraft_core::BakeMatch,
    /// Transform applied to the high-poly bake source (offset / scale / rotation / centring).
    /// Optional in the file: absent = identity, so no schema bump is needed.
    #[serde(skip_serializing_if = "is_default_placement")]
    pub bake_placement: subcraft_core::BakePlacement,
    /// Model file the project's mesh was last imported or reloaded from (default for
    /// File > Reload mesh). A hint only: the project never depends on the file existing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_source: Option<String>,
    /// Texture sets whose baked mesh maps were made for an earlier version of the mesh (after a
    /// mesh reload) and should be baked again. Cleared per set by its next bake.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stale_mesh_maps: Vec<subcraft_core::TextureSetId>,
    /// Display settings of the 3D view (environment, camera, post effects, visualisation).
    /// Optional in the file: absent = the user's own defaults, so no schema bump is needed.
    #[serde(skip_serializing_if = "Option::is_none", default, deserialize_with = "crate::display::lenient_display")]
    pub display: Option<crate::display::DisplaySettings>,
}

fn is_default_match(m: &subcraft_core::BakeMatch) -> bool {
    *m == subcraft_core::BakeMatch::default()
}

fn is_default_placement(p: &subcraft_core::BakePlacement) -> bool {
    p.is_identity()
}
