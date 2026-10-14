//! Brush presets: the brush's settings, stroke options, material and tip saved as a small JSON
//! file in the user library (`brush-presets/*.scbrush`).
//!
//! The tip is referenced by name (built-in tips, or tips in the library's `alphas/` folder) rather
//! than embedded: imported tips already live in the library.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use subcraft_assets::{BrushAlpha, UserAssetKind, UserLibrary};
use subcraft_paint::{BrushMaterial, BrushSettings, StrokeOptions};

use crate::session::Session;

pub const FORMAT: &str = "subcraft-brush";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushPreset {
    pub format: String,
    pub version: u32,
    pub name: String,
    /// Tip name (`None` = the round procedural tip).
    pub tip: Option<String>,
    pub settings: BrushSettings,
    #[serde(default)]
    pub options: StrokeOptions,
    #[serde(default)]
    pub material: BrushMaterial,
    /// Library file this preset was loaded from / saved to.
    #[serde(skip)]
    pub user_file: Option<PathBuf>,
}

impl Session {
    /// The current brush as a preset.
    pub fn brush_preset(&self, name: &str) -> BrushPreset {
        let b = &self.brush;
        BrushPreset {
            format: FORMAT.into(),
            version: 1,
            name: name.trim().to_string(),
            tip: b.alpha.is_some().then(|| b.alpha_name.clone()),
            settings: b.settings,
            options: b.options,
            material: b.material.clone(),
            user_file: None,
        }
    }

    /// Load a preset into the brush. The tip is looked up by name in `tips`; if it is missing the
    /// round tip is used and the returned message says so.
    pub fn apply_brush_preset(&mut self, p: &BrushPreset, tips: &[BrushAlpha]) -> Option<String> {
        self.brush.settings = p.settings;
        self.brush.options = p.options;
        if !p.material.channels.is_empty() {
            self.brush.material = p.material.clone();
        }
        let tip = match &p.tip {
            None => tips.iter().find(|a| a.bitmap.is_none()),
            Some(name) => tips.iter().find(|a| &a.name == name),
        };
        match tip {
            Some(a) => {
                self.set_brush_alpha(a);
                None
            }
            None => {
                if let Some(round) = tips.iter().find(|a| a.bitmap.is_none()) {
                    self.set_brush_alpha(round);
                }
                Some(format!("brush tip \"{}\" not found; using Round", p.tip.as_deref().unwrap_or("?")))
            }
        }
    }
}

/// Save to `brush-presets/<name>.scbrush` (numbered if taken). Returns the stored preset.
pub fn save_brush_preset(lib: &UserLibrary, preset: &BrushPreset) -> Result<BrushPreset, String> {
    let name = if preset.name.is_empty() { "Brush" } else { preset.name.as_str() };
    let path = lib.new_file_path(UserAssetKind::BrushPreset, name);
    let dir = lib.folder(UserAssetKind::BrushPreset);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let json = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut stored = preset.clone();
    stored.name = display_name(&path, name);
    stored.user_file = Some(path);
    Ok(stored)
}

pub fn load_brush_preset(path: &Path) -> Result<BrushPreset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut p: BrushPreset = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if p.format != FORMAT {
        return Err(format!("{}: not a SubCraft brush preset", path.display()));
    }
    p.name = display_name(path, &p.name);
    p.user_file = Some(path.to_path_buf());
    Ok(p)
}

/// All presets in the library; unreadable files are reported, not fatal.
pub fn load_brush_presets(lib: &UserLibrary) -> (Vec<BrushPreset>, Vec<String>) {
    let mut ok = Vec::new();
    let mut errors = Vec::new();
    for p in lib.list(UserAssetKind::BrushPreset) {
        match load_brush_preset(&p) {
            Ok(b) => ok.push(b),
            Err(e) => errors.push(e),
        }
    }
    (ok, errors)
}

fn display_name(path: &Path, fallback: &str) -> String {
    path.file_stem().and_then(|s| s.to_str()).map(str::to_string).unwrap_or_else(|| fallback.to_string())
}
