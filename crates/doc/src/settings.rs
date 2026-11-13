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
    /// Shader instances (Shader Settings) and which one each texture set uses. Optional in the
    /// file: empty = every set uses the implicit "Main shader", so no schema bump is needed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shader_instances: Vec<crate::shader::ShaderInstance>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shader_assignments: Vec<crate::shader::ShaderAssignment>,
    /// Cameras imported from the mesh file (New project > Import cameras). Optional in the file:
    /// absent = none, so no schema bump is needed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cameras: Vec<crate::camera::SceneCamera>,
    /// Export window settings stored by its Save settings button. Optional in the file: absent =
    /// the window's defaults, so no schema bump is needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export: Option<ExportConfig>,
}

/// One texture set's export settings replacing the Export window's global ones (Painter's
/// texture set entries). `None` = the global setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOverrides {
    /// File type and bit depth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<subcraft_image::FileFormat>,
    /// 8-bit dithering, with `format`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dither: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<subcraft_image::dilate::Padding>,
}

impl ExportOverrides {
    pub fn is_empty(&self) -> bool {
        self.format.is_none() && self.size.is_none() && self.padding.is_none()
    }
}

/// The Export window's settings saved with the project (Painter's Save settings).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportConfig {
    /// Output folder; `None` = the window's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// File type and bit depth for every map; `None` = from the output template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<subcraft_image::FileFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dither: Option<bool>,
    /// Output size; `None` = each texture set's size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<subcraft_image::dilate::Padding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_to_normal: Option<f32>,
    /// Per texture set overrides, by texture set id.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub sets: std::collections::BTreeMap<subcraft_core::TextureSetId, ExportOverrides>,
}

fn is_default_match(m: &subcraft_core::BakeMatch) -> bool {
    *m == subcraft_core::BakeMatch::default()
}

fn is_default_placement(p: &subcraft_core::BakePlacement) -> bool {
    p.is_identity()
}

#[cfg(test)]
mod export_config_tests {
    use super::*;

    #[test]
    fn export_config_round_trips_and_stays_out_of_files_that_have_none() {
        let id = subcraft_core::TextureSetId::new();
        let mut config = ExportConfig { folder: Some("out".into()), format: Some(subcraft_image::FileFormat::Exr16), size: Some(1024), padding: Some(subcraft_image::dilate::Padding::Diffusion(8)), ..Default::default() };
        config.sets.insert(id, ExportOverrides { size: Some(256), ..Default::default() });
        let settings = ProjectSettings { export: Some(config.clone()), ..Default::default() };
        let json = serde_json::to_string(&settings).unwrap();
        let back: ProjectSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.export, Some(config));
        // Projects without saved export settings don't write the key, and older files read as None.
        let plain = serde_json::to_string(&ProjectSettings::default()).unwrap();
        assert!(!plain.contains("\"export\""), "{plain}");
        let old: ProjectSettings = serde_json::from_str(r#"{"export_template":"Godot 4 (ORM)"}"#).unwrap();
        assert_eq!(old.export, None);
        assert!(ExportOverrides::default().is_empty() && !ExportOverrides { size: Some(2), ..Default::default() }.is_empty());
    }
}
