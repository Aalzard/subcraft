//! Presets of the painting tools, saved as small JSON files in the user library
//! (`brush-presets/*.scbrush`). Three kinds, as in Painter:
//!
//! - a **tool** preset holds everything: brush, alpha, stencil, material and the tool it was made with;
//! - a **brush** preset holds the brush parameters, the alpha and the stencil, not the material;
//! - a **material** preset holds only the material (values and images).
//!
//! The tip is referenced by name (built-in tips, or tips in the library's `alphas/` folder) rather
//! than embedded: imported tips already live in the library. Channel images and the stencil
//! image are embedded, so a preset works in another project or library.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use subcraft_assets::{BrushAlpha, UserAssetKind, UserLibrary};
use subcraft_paint::{BrushMaterial, BrushSettings, StrokeOptions};

use crate::session::Session;

pub const FORMAT: &str = "subcraft-brush";

/// Newest file version: 3 added the preset kind, the tool and the stencil.
pub const VERSION: u32 = 3;

/// What a preset holds and so what loading it changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetKind {
    /// Brush, alpha, stencil, material and (when stored) the tool. Files older than version 3
    /// are tool presets without a tool.
    #[default]
    Tool,
    /// Brush parameters, alpha and stencil; the material in use is left alone.
    Brush,
    /// The material only; the brush in use is left alone.
    Material,
}

impl PresetKind {
    pub const ALL: [PresetKind; 3] = [PresetKind::Tool, PresetKind::Material, PresetKind::Brush];

    pub fn label(self) -> &'static str {
        match self {
            PresetKind::Tool => "tool preset",
            PresetKind::Brush => "brush preset",
            PresetKind::Material => "material preset",
        }
    }

    pub fn has_brush(self) -> bool { self != PresetKind::Material }
    pub fn has_material(self) -> bool { self != PresetKind::Brush }
}

/// The painting tool a tool preset was made with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetTool {
    Paint,
    Eraser,
    Smudge,
    Clone,
    PolygonFill,
}

impl PresetTool {
    pub fn label(self) -> &'static str {
        match self {
            PresetTool::Paint => "Paint",
            PresetTool::Eraser => "Eraser",
            PresetTool::Smudge => "Smudge",
            PresetTool::Clone => "Clone",
            PresetTool::PolygonFill => "Polygon Fill",
        }
    }
}

/// The stencil of a brush / tool preset: its image (embedded) and placement.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresetStencil {
    pub encoded: Vec<u8>,
    pub center: [f32; 2],
    pub scale: f32,
    pub rotation: f32,
    #[serde(default)]
    pub tiling: subcraft_paint::StencilTiling,
}

impl PresetStencil {
    fn of(s: &subcraft_paint::Stencil) -> Result<Self, String> {
        let (encoded, _) = subcraft_image::encode_bitmap(&s.bitmap).map_err(|e| format!("stencil image: {e}"))?;
        Ok(Self { encoded, center: s.center.to_array(), scale: s.scale, rotation: s.rotation, tiling: s.tiling })
    }

    fn stencil(&self) -> Result<subcraft_paint::Stencil, String> {
        let bitmap = subcraft_image::io::decode_bitmap(&self.encoded, Some("stencil")).map_err(|e| format!("stencil image: {e}"))?;
        Ok(subcraft_paint::Stencil { center: glam::Vec2::from_array(self.center), scale: self.scale, rotation: self.rotation, tiling: self.tiling,
            ..subcraft_paint::Stencil::new(std::sync::Arc::new(bitmap)) })
    }
}

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
    #[serde(default)]
    pub kind: PresetKind,
    /// Tool presets: the tool to switch to when the preset is selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<PresetTool>,
    /// Brush and tool presets: the stencil in use when the preset was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stencil: Option<PresetStencil>,
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
            subcraft_core::vfs::write(&path,&bytes).map_err(|e|e.to_string())?;
            let alpha=BrushAlpha {id:subcraft_core::AssetId::from_name(&format!("alpha/user/{}",path.file_name().and_then(|n|n.to_str()).unwrap_or("tip.png"))),name:display_name(&path,&self.brush.alpha_name),bitmap:Some(bitmap),user_file:Some(path)};
            self.set_brush_alpha(&alpha);saved_tip=Some(alpha);
        }
        let preset=save_brush_preset(lib,&self.brush_preset(name))?;Ok((preset,saved_tip))
    }

    /// Create a preset of `kind` from the current tool in the library ("Create tool / material /
    /// brush preset"). A starter tip in use is saved to the library first, like
    /// [`Session::save_current_brush_preset`]; a material preset needs no tip.
    pub fn create_preset(&mut self, lib: &UserLibrary, name: &str, kind: PresetKind, tool: Option<PresetTool>) -> Result<(BrushPreset, Option<BrushAlpha>), String> {
        let mut saved_tip = None;
        if kind.has_brush() && self.brush.alpha_name.starts_with("Starter: ") {
            // Reuses the tip persistence of the legacy save, without writing a second preset.
            saved_tip = self.persist_starter_tip(lib)?;
        }
        let preset = save_brush_preset(lib, &self.preset_of(name, kind, tool)?)?;
        Ok((preset, saved_tip))
    }

    fn persist_starter_tip(&mut self, lib: &UserLibrary) -> Result<Option<BrushAlpha>, String> {
        let Some(bitmap) = self.brush.alpha.clone() else { return Ok(None) };
        let path = lib.new_file_path(UserAssetKind::Alpha, &self.brush.alpha_name);
        std::fs::create_dir_all(lib.folder(UserAssetKind::Alpha)).map_err(|e| e.to_string())?;
        let (bytes, _) = subcraft_image::encode_bitmap(&bitmap).map_err(|e| e.to_string())?;
        subcraft_core::vfs::write(&path, &bytes).map_err(|e| e.to_string())?;
        let alpha = BrushAlpha { id: subcraft_core::AssetId::from_name(&format!("alpha/user/{}", path.file_name().and_then(|n| n.to_str()).unwrap_or("tip.png"))),
            name: display_name(&path, &self.brush.alpha_name), bitmap: Some(bitmap), user_file: Some(path) };
        self.set_brush_alpha(&alpha);
        Ok(Some(alpha))
    }

    /// "Update from current tool": overwrite a library preset with the current tool, keeping its
    /// kind, name and file. A tool preset takes `tool` as its new tool.
    pub fn update_preset(&mut self, lib: &UserLibrary, existing: &BrushPreset, tool: Option<PresetTool>) -> Result<(BrushPreset, Option<BrushAlpha>), String> {
        let path = existing.user_file.clone().ok_or("built-in presets cannot be updated: duplicate it first")?;
        let mut saved_tip = None;
        if existing.kind.has_brush() && self.brush.alpha_name.starts_with("Starter: ") {
            saved_tip = self.persist_starter_tip(lib)?;
        }
        let tool = if existing.kind == PresetKind::Tool { tool.or(existing.tool) } else { None };
        let mut preset = self.preset_of(&existing.name, existing.kind, tool)?;
        check_images(&preset)?;
        write_preset(&path, &preset)?;
        preset.name = display_name(&path, &existing.name);
        preset.user_file = Some(path);
        Ok((preset, saved_tip))
    }

    /// The current brush and material as a preset: a tool preset without a tool. It is written
    /// as version 2, which says nothing about the stencil, so loading it leaves the stencil alone.
    pub fn brush_preset(&self, name: &str) -> BrushPreset {
        let mut p = self.preset_parts(name, PresetKind::Tool);
        p.version = 2;
        p
    }

    /// The current tool as a preset of `kind`: only the parts that kind holds are stored.
    pub fn preset_of(&self, name: &str, kind: PresetKind, tool: Option<PresetTool>) -> Result<BrushPreset, String> {
        let mut p = self.preset_parts(name, kind);
        p.tool = tool.filter(|_| kind == PresetKind::Tool);
        p.stencil = match (&self.brush.stencil, kind.has_brush()) {
            (Some(s), true) => Some(PresetStencil::of(s)?),
            _ => None,
        };
        Ok(p)
    }

    fn preset_parts(&self, name: &str, kind: PresetKind) -> BrushPreset {
        let b = &self.brush;
        let material = if kind.has_material() { b.material.clone() } else { BrushMaterial::default() };
        BrushPreset {
            format: FORMAT.into(),
            version: VERSION,
            name: name.trim().to_string(),
            tip: (kind.has_brush() && b.alpha.is_some()).then(|| b.alpha_name.clone()),
            settings: if kind.has_brush() { b.settings } else { BrushSettings::default() },
            options: if kind.has_brush() { b.options } else { StrokeOptions::default() },
            images: material.textures.values().copied().chain(material.stamp.as_ref().and_then(|s| s.resource)).collect::<std::collections::BTreeSet<_>>().into_iter()
                .filter_map(|id| self.resources().get(id).map(|r| PresetImage {
                    id, name: r.name.clone(), encoded: r.encoded.as_deref().cloned().unwrap_or_default(),
                })).collect(),
            material,
            kind,
            tool: None,
            stencil: None,
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
                Ok(SelectedBrush { name: p.name.clone(), tip: self.brush.alpha_name.clone(), warning, tool: p.tool.filter(|_| p.kind == PresetKind::Tool), kind: Some(p.kind) })
            }
            ShelfBrush::Tip(a) => {
                self.set_brush_alpha(a);
                Ok(SelectedBrush { name: a.name.clone(), tip: a.name.clone(), warning: None, tool: None, kind: None })
            }
        }
    }

    /// `Err` leaves the brush unchanged; `Ok(Some)` applied with a warning (missing tip).
    /// Only the parts the preset's kind holds are changed.
    fn load_brush_preset(&mut self, p: &BrushPreset, tips: &[BrushAlpha]) -> Result<Option<String>, String> {
        // Decode the stencil before anything changes, so a broken preset leaves the tool as it was.
        let stencil = match (&p.stencil, p.kind.has_brush()) {
            (Some(s), true) => Some(s.stencil().map_err(|e| format!("Preset {e}; brush unchanged"))?),
            _ => None,
        };
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
        if let Some(old) = material.stamp.as_ref().and_then(|s| s.resource) {
            let Some(image) = p.images.iter().find(|image| image.id == old) else {
                return Err("Brush preset is missing its normal-map stamp image; brush unchanged".into());
            };
            let id = self.import_image_bytes(&image.name, image.encoded.clone()).map_err(|e| format!("Brush preset stamp image: {e}; brush unchanged"))?;
            if let Some(s) = material.stamp.as_mut() { s.resource = Some(id); }
        }
        if p.kind == PresetKind::Material {
            self.brush.material = material;
            return Ok(None);
        }
        self.brush.settings = p.settings;
        self.brush.options = p.options;
        if p.kind == PresetKind::Brush {
            // The material in use stays; only a stamp brush (which replaces the tip) is dropped.
            self.clear_brush_stamp();
        } else if !p.material.channels.is_empty() {
            self.brush.material = material;
        } else {
            self.clear_brush_stamp();
        }
        // Presets made since version 3 carry their stencil state: none stored means none in use.
        if p.version >= 3 {
            self.brush.stencil = stencil.map(std::sync::Arc::new);
        }
        let tip = match &p.tip {
            None => tips.iter().find(|a| a.bitmap.is_none()),
            Some(name) => tips.iter().find(|a| &a.name == name),
        };
        match tip {
            Some(a) => {
                self.set_brush_tip(a);
                Ok(None)
            }
            None => {
                if let Some(round) = tips.iter().find(|a| a.bitmap.is_none()) {
                    self.set_brush_tip(round);
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
    /// A tool preset names the tool to switch to.
    pub tool: Option<PresetTool>,
    /// Kind of the preset (`None` for a bare tip).
    pub kind: Option<PresetKind>,
}

/// Save to `brush-presets/<name>.scbrush` (numbered if taken). Returns the stored preset.
pub fn save_brush_preset(lib: &UserLibrary, preset: &BrushPreset) -> Result<BrushPreset, String> {
    check_images(preset)?;
    let name = if preset.name.is_empty() { "Brush" } else { preset.name.as_str() };
    let path = lib.new_file_path(UserAssetKind::BrushPreset, name);
    let dir = lib.folder(UserAssetKind::BrushPreset);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    write_preset(&path, preset)?;
    let mut stored = preset.clone();
    stored.name = display_name(&path, name);
    stored.user_file = Some(path);
    Ok(stored)
}

fn check_images(preset: &BrushPreset) -> Result<(), String> {
    if preset.material.textures.values().copied().chain(preset.material.stamp.as_ref().and_then(|s| s.resource)).any(|id| !preset.images.iter().any(|image| image.id == id && !image.encoded.is_empty())) {
        return Err("brush preset has a missing channel image".into());
    }
    Ok(())
}

fn write_preset(path: &Path, preset: &BrushPreset) -> Result<(), String> {
    let json = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
    subcraft_core::vfs::write(path, json.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

/// Rename a library preset: its file moves to the new name (numbered if taken). Returns the
/// preset as stored. A name that changes nothing returns the preset as it is.
pub fn rename_brush_preset(lib: &UserLibrary, preset: &BrushPreset, new_name: &str) -> Result<BrushPreset, String> {
    let old = preset.user_file.clone().ok_or("built-in presets cannot be renamed: duplicate it first")?;
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("a preset needs a name".into());
    }
    if new_name == preset.name {
        return Ok(preset.clone());
    }
    let mut renamed = preset.clone();
    renamed.name = new_name.to_string();
    let stored = save_brush_preset(lib, &renamed)?;
    if let Err(e) = lib.remove(&old) {
        // Keep one file: the rename did not happen.
        if let Some(new) = &stored.user_file { let _ = lib.remove(new); }
        return Err(e);
    }
    Ok(stored)
}

/// Duplicate a preset (built-in or from the library) into the library as "<name> copy".
pub fn duplicate_brush_preset(lib: &UserLibrary, preset: &BrushPreset) -> Result<BrushPreset, String> {
    let mut copy = preset.clone();
    copy.name = format!("{} copy", preset.name);
    copy.user_file = None;
    save_brush_preset(lib, &copy)
}

/// Favorite assets of the user library, by a stable key (see [`favorite_key`]), kept in
/// `favorites.json` at the library root. Built-in assets can be favorites too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Favorites {
    keys: std::collections::BTreeSet<String>,
}

/// Key of an asset in [`Favorites`]: its category and shelf name (`brush/Dirt Grunge`).
pub fn favorite_key(category: &str, name: &str) -> String {
    format!("{category}/{name}")
}

impl Favorites {
    const FILE: &'static str = "favorites.json";

    /// The library's favorites; a missing or unreadable file is an empty list.
    pub fn load(lib: &UserLibrary) -> Self {
        let keys = std::fs::read_to_string(lib.root().join(Self::FILE)).ok()
            .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
            .map(|v| v.into_iter().collect()).unwrap_or_default();
        Self { keys }
    }

    pub fn save(&self, lib: &UserLibrary) -> Result<(), String> {
        std::fs::create_dir_all(lib.root()).map_err(|e| format!("{}: {e}", lib.root().display()))?;
        let json = serde_json::to_string_pretty(&self.keys.iter().collect::<Vec<_>>()).map_err(|e| e.to_string())?;
        let path = lib.root().join(Self::FILE);
        subcraft_core::vfs::write(&path, json.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn contains(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    /// Add or remove `key`; returns whether it is a favorite now.
    pub fn toggle(&mut self, key: &str) -> bool {
        if self.keys.remove(key) {
            false
        } else {
            self.keys.insert(key.to_string());
            true
        }
    }

    /// A renamed asset keeps its favorite mark.
    pub fn rename(&mut self, old: &str, new: &str) {
        if self.keys.remove(old) {
            self.keys.insert(new.to_string());
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.keys.remove(key);
    }

    pub fn len(&self) -> usize { self.keys.len() }
    pub fn is_empty(&self) -> bool { self.keys.is_empty() }
}

pub fn load_brush_preset(path: &Path) -> Result<BrushPreset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut p: BrushPreset = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if p.format != FORMAT {
        return Err(format!("{}: not a SubCraft brush preset", path.display()));
    }
    if !(1..=VERSION).contains(&p.version) { return Err("brush preset was saved by an unsupported version".into()); }
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

/// Ready-made brush presets on the built-in tips (`subcraft_assets::alphas::builtin`).
pub fn builtin() -> Vec<BrushPreset> {
    use subcraft_core::{ChannelKind::*, ChannelValue};
    let rgb = ChannelValue::rgb;
    let s = ChannelValue::scalar;
    let preset = |name: &str, tip: Option<&str>, settings: BrushSettings, channels: &[(subcraft_core::ChannelKind, ChannelValue)]| BrushPreset {
        format: FORMAT.into(),
        version: 2,
        name: name.into(),
        tip: tip.map(Into::into),
        settings,
        options: StrokeOptions::default(),
        material: BrushMaterial::with(channels),
        images: Vec::new(),
        kind: PresetKind::Tool,
        tool: None,
        stencil: None,
        user_file: None,
    };
    let d = BrushSettings::default();
    vec![
        preset("Soft Airbrush", None, BrushSettings { size: 12.0, hardness: 0.0, flow: 0.15, spacing: 0.08, ..d }, &[]),
        preset(
            "Dirt Grunge",
            Some("Grunge"),
            BrushSettings { size: 10.0, flow: 0.45, spacing: 0.3, size_jitter: 0.35, angle_jitter_deg: 180.0, ..d },
            &[(BaseColor, rgb(0.06, 0.045, 0.03)), (Roughness, s(0.92)), (Metallic, s(0.0))],
        ),
        preset(
            "Rust Spatter",
            Some("Splatter"),
            BrushSettings { size: 8.0, flow: 0.8, spacing: 0.6, size_jitter: 0.5, angle_jitter_deg: 180.0, position_jitter: 0.4, ..d },
            &[(BaseColor, rgb(0.28, 0.09, 0.03)), (Roughness, s(0.85)), (Metallic, s(0.0))],
        ),
        preset(
            "Bare Metal Scratches",
            Some("Scratches"),
            BrushSettings { size: 6.0, flow: 0.8, spacing: 0.4, angle_jitter_deg: 25.0, follow_path: true, ..d },
            &[(BaseColor, rgb(0.56, 0.57, 0.58)), (Roughness, s(0.3)), (Metallic, s(1.0))],
        ),
        preset(
            "Spray Paint",
            Some("Spray"),
            BrushSettings { size: 9.0, flow: 0.35, spacing: 0.15, ..d },
            &[(BaseColor, rgb(0.6, 0.6, 0.6)), (Roughness, s(0.5)), (Metallic, s(0.0))],
        ),
        preset(
            "Engraved Line",
            Some("Line"),
            BrushSettings { size: 1.5, hardness: 0.9, spacing: 0.05, follow_path: true, ..d },
            &[(Height, s(-0.4))],
        ),
        preset(
            "Raised Rivet",
            None,
            BrushSettings { size: 2.0, hardness: 0.3, spacing: 4.0, ..d },
            &[(Height, s(0.6))],
        ),
    ]
}
