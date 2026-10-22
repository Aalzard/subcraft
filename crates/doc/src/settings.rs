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
}
