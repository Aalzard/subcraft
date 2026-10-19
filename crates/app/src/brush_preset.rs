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
    /// Version 2 embeds image bytes, so presets work in another document or library.
    #[serde(default)]
    pub images: Vec<PresetImage>,
    /// Library file this preset was loaded from / saved to.
    #[serde(skip)]
    pub user_file: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresetImage {
    pub id: subcraft_core::ResourceId,
    pub name: String,
    pub encoded: Vec<u8>,
}

impl Session {
    /// Persist a used starter tip only when a preset needs it. The whole pack stays lazy.
    pub fn save_current_brush_preset(&mut self, lib: &UserLibrary, name: &str) -> Result<(BrushPreset,Option<BrushAlpha>),String> {
        let mut saved_tip=None;
        if self.brush.alpha_name.starts_with("Starter: ") && let Some(bitmap)=self.brush.alpha.clone() {
            let path=lib.new_file_path(UserAssetKind::Alpha,&self.brush.alpha_name);
            std::fs::create_dir_all(lib.folder(UserAssetKind::Alpha)).map_err(|e|e.to_string())?;
            let (bytes,_)=subcraft_image::encode_bitmap(&bitmap).map_err(|e|e.to_string())?;
            std::fs::write(&path,bytes).map_err(|e|e.to_string())?;
            let alpha=BrushAlpha {id:subcraft_core::AssetId::from_name(&format!("alpha/user/{}",path.file_name().and_then(|n|n.to_str()).unwrap_or("tip.png"))),name:display_name(&path,&self.brush.alpha_name),bitmap:Some(bitmap),user_file:Some(path)};
            self.set_brush_alpha(&alpha);saved_tip=Some(alpha);
        }
        let preset=save_brush_preset(lib,&self.brush_preset(name))?;Ok((preset,saved_tip))
    }
    /// The current brush as a preset.
    pub fn brush_preset(&self, name: &str) -> BrushPreset {
        let b = &self.brush;
        BrushPreset {
            format: FORMAT.into(),
            version: 2,
            name: name.trim().to_string(),
            tip: b.alpha.is_some().then(|| b.alpha_name.clone()),
            settings: b.settings,
            options: b.options,
            material: b.material.clone(),
            images: b.material.textures.values().copied().collect::<std::collections::BTreeSet<_>>().into_iter()
                .filter_map(|id| self.resources().get(id).map(|r| PresetImage {
                    id, name: r.name.clone(), encoded: r.encoded.as_deref().cloned().unwrap_or_default(),
                })).collect(),
            user_file: None,
        }
    }

    /// Load a preset into the brush. The tip is looked up by name in `tips`; if it is missing the
    /// round tip is used and the returned message says so.
    pub fn apply_brush_preset(&mut self, p: &BrushPreset, tips: &[BrushAlpha]) -> Option<String> {
        self.load_brush_preset(p, tips).unwrap_or_else(Some)
    }

    /// Make a Shelf brush (preset or bare tip) the active brush in one click. The paint
    /// destination (content, mask or Paint effect) is tool state and is left untouched.
    /// Returns what actually became active, or why the brush is unchanged.
    pub fn select_shelf_brush(&mut self, brush: ShelfBrush, tips: &[BrushAlpha]) -> Result<SelectedBrush, String> {
        match brush {
            ShelfBrush::Preset(p) => {
                let warning = self.load_brush_preset(p, tips)?;
                Ok(SelectedBrush { name: p.name.clone(), tip: self.brush.alpha_name.clone(), warning })
            }
            ShelfBrush::Tip(a) => {
                self.set_brush_alpha(a);
                Ok(SelectedBrush { name: a.name.clone(), tip: a.name.clone(), warning: None })
            }
        }
    }

    /// `Err` leaves the brush unchanged; `Ok(Some)` applied with a warning (missing tip).
    fn load_brush_preset(&mut self, p: &BrushPreset, tips: &[BrushAlpha]) -> Result<Option<String>, String> {
        let mut material = p.material.clone();
        let mut remap = std::collections::BTreeMap::new();
        for old in material.textures.values() {
            let Some(image) = p.images.iter().find(|image| image.id == *old) else {
                return Err("Brush preset is missing an embedded channel image; brush unchanged".into());
            };
            match self.import_image_bytes(&image.name, image.encoded.clone()) {
                Ok(id) => { remap.insert(*old, id); }
                Err(e) => return Err(format!("Brush preset image: {e}; brush unchanged")),
            }
        }
        for id in material.textures.values_mut() { *id = remap[id]; }
        self.brush.settings = p.settings;
        self.brush.options = p.options;
        if !p.material.channels.is_empty() {
            self.brush.material = material;
        }
        let tip = match &p.tip {
            None => tips.iter().find(|a| a.bitmap.is_none()),
            Some(name) => tips.iter().find(|a| &a.name == name),
        };
        match tip {
            Some(a) => {
                self.set_brush_alpha(a);
                Ok(None)
            }
            None => {
                if let Some(round) = tips.iter().find(|a| a.bitmap.is_none()) {
                    self.set_brush_alpha(round);
                }
                Ok(Some(format!("brush tip \"{}\" not found; using Round", p.tip.as_deref().unwrap_or("?"))))
            }
        }
    }
}

/// A Shelf > Brushes entry.
#[derive(Clone, Copy, Debug)]
pub enum ShelfBrush<'a> {
    Preset(&'a BrushPreset),
    Tip(&'a BrushAlpha),
}

/// The brush that became active after a Shelf click (for the viewport acknowledgement).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedBrush {
    pub name: String,
    /// Tip actually in use (a preset whose tip is missing falls back to Round).
    pub tip: String,
    pub warning: Option<String>,
}

/// Save to `brush-presets/<name>.scbrush` (numbered if taken). Returns the stored preset.
pub fn save_brush_preset(lib: &UserLibrary, preset: &BrushPreset) -> Result<BrushPreset, String> {
    if preset.material.textures.values().any(|id| !preset.images.iter().any(|image| image.id == *id && !image.encoded.is_empty())) {
        return Err("brush preset has a missing channel image".into());
    }
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
    if !(1..=2).contains(&p.version) { return Err("brush preset was saved by an unsupported version".into()); }
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
