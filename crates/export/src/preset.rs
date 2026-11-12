//! User export presets: a versioned JSON file per preset, a library folder of them, and import
//! of Adobe Substance 3D Painter export presets (`.spexp` / export-config JSON).
//!
//! File layout (`*.json`):
//! ```json
//! { "format": "subcraft.export-preset", "version": 1, "preset": { "name": "...", "maps": [...] } }
//! ```
//! The preset object is [`ExportTemplate`]; every field added after version 1 must be optional.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_image::{BitDepth, Container, FileFormat};

use crate::{ExportTemplate, OutputMap, Padding, Source};

pub const FORMAT: &str = "subcraft.export-preset";
pub const VERSION: u32 = 1;
/// File extension of presets in the library and of exported presets.
pub const EXTENSION: &str = "json";

#[derive(Serialize, Deserialize)]
struct PresetFile {
    format: String,
    version: u32,
    preset: ExportTemplate,
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("not valid JSON: {0}")]
    Json(String),
    #[error("not a SubCraft or Painter export preset")]
    Unknown,
    #[error("this preset was saved by a newer SubCraft (version {0})")]
    Version(u32),
    #[error("{0}: {1}")]
    Io(PathBuf, String),
}

/// A preset's JSON file text.
pub fn to_json(preset: &ExportTemplate) -> String {
    let file = PresetFile { format: FORMAT.into(), version: VERSION, preset: preset.clone() };
    serde_json::to_string_pretty(&file).expect("presets always serialize")
}

/// What an import read: the presets, plus notes about Painter features SubCraft dropped.
#[derive(Debug, Default)]
pub struct Imported {
    pub presets: Vec<ExportTemplate>,
    pub warnings: Vec<String>,
}

/// Read a SubCraft preset file or a Painter export preset (`.spexp`, or the export-config JSON
/// of Painter's scripting API: `{"exportPresets": [{"name", "maps": [...]}]}`).
/// `fallback_name` names presets that carry no name (usually the file stem).
pub fn from_json(text: &str, fallback_name: &str) -> Result<Imported, PresetError> {
    let value: Value = serde_json::from_str(text).map_err(|e| PresetError::Json(e.to_string()))?;
    if value.get("format").and_then(Value::as_str) == Some(FORMAT) {
        let file: PresetFile = serde_json::from_value(value).map_err(|e| PresetError::Json(e.to_string()))?;
        if file.version > VERSION {
            return Err(PresetError::Version(file.version));
        }
        return Ok(Imported { presets: vec![file.preset], warnings: Vec::new() });
    }
    painter::import(&value, fallback_name)
}

/// Read a preset file from disk (SubCraft or Painter).
pub fn read_file(path: &Path) -> Result<Imported, PresetError> {
    let text = std::fs::read_to_string(path).map_err(|e| PresetError::Io(path.to_path_buf(), e.to_string()))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Imported preset");
    from_json(&text, stem)
}

/// A fresh preset identity.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The user preset folder (`<library>/export-presets`): one `<uuid>.json` per preset.
#[derive(Clone, Debug)]
pub struct PresetLibrary {
    dir: PathBuf,
}

impl PresetLibrary {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.{EXTENSION}", crate::sanitize(id)))
    }

    /// Every preset in the folder, sorted by name. Unreadable files are reported, not fatal;
    /// a preset without an id gets the file stem as id so it can still be edited and deleted.
    pub fn load(&self) -> (Vec<ExportTemplate>, Vec<String>) {
        let (mut ok, mut errors) = (Vec::new(), Vec::new());
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return (ok, errors) };
        let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))).collect();
        files.sort();
        for path in files {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    errors.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            let value: Result<Value, _> = serde_json::from_str(&text);
            // Only SubCraft preset files live here; Painter files are imported (converted) first.
            if !value.as_ref().is_ok_and(|v| v.get("format").and_then(Value::as_str) == Some(FORMAT)) {
                errors.push(format!("{}: not a SubCraft export preset", path.display()));
                continue;
            }
            match from_json(&text, &stem) {
                Ok(mut imported) => {
                    let mut p = imported.presets.remove(0);
                    p.id = Some(stem);
                    ok.push(p);
                }
                Err(e) => errors.push(format!("{}: {e}", path.display())),
            }
        }
        ok.sort_by_key(|p| p.name.to_lowercase());
        (ok, errors)
    }

    /// Write (create or replace) a user preset. Built-in templates have no id and are refused.
    pub fn save(&self, preset: &ExportTemplate) -> Result<PathBuf, PresetError> {
        let Some(id) = preset.id.as_deref() else {
            return Err(PresetError::Io(self.dir.clone(), "built-in templates are read-only".into()));
        };
        std::fs::create_dir_all(&self.dir).map_err(|e| PresetError::Io(self.dir.clone(), e.to_string()))?;
        let path = self.path(id);
        subcraft_core::vfs::write(&path, to_json(preset).as_bytes()).map_err(|e| PresetError::Io(path.clone(), e.to_string()))?;
        Ok(path)
    }

    pub fn delete(&self, id: &str) -> Result<(), PresetError> {
        let path = self.path(id);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(PresetError::Io(path, e.to_string())),
        }
    }
}

/// A copy of `preset` as a new user preset named `name` (fresh id).
pub fn duplicate(preset: &ExportTemplate, name: &str) -> ExportTemplate {
    ExportTemplate { name: name.to_string(), id: Some(new_id()), ..preset.clone() }
}

/// `base`, or `base (2)`, `base (3)`... whichever no preset in `taken` uses.
pub fn unique_name(base: &str, taken: &[&str]) -> String {
    (1..).map(|n| if n == 1 { base.to_string() } else { format!("{base} ({n})") }).find(|c| !taken.contains(&c.as_str())).expect("unbounded")
}

/// Painter export preset conversion.
mod painter {
    use super::*;

    pub(super) fn import(value: &Value, fallback_name: &str) -> Result<Imported, PresetError> {
        let list: Vec<&Value> = if let Some(arr) = value.get("exportPresets").and_then(Value::as_array) {
            arr.iter().collect()
        } else if value.get("maps").is_some_and(Value::is_array) {
            vec![value]
        } else {
            return Err(PresetError::Unknown);
        };
        let mut out = Imported::default();
        for (i, p) in list.into_iter().enumerate() {
            let name = p.get("name").or_else(|| p.get("exportName")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| if i == 0 { fallback_name.to_string() } else { format!("{fallback_name} {}", i + 1) });
            let mut maps = Vec::new();
            for (mi, m) in p.get("maps").and_then(Value::as_array).into_iter().flatten().enumerate() {
                if let Some(map) = convert_map(m, mi, &name, &mut out.warnings) {
                    maps.push(map);
                }
            }
            // Suffixes are the maps' identity: make them unique.
            for i in 0..maps.len() {
                let taken: Vec<String> = maps[..i].iter().map(|m: &OutputMap| m.suffix.clone()).collect();
                let s = unique_suffix(&maps[i].suffix, &taken);
                maps[i].suffix = s;
            }
            out.presets.push(ExportTemplate { name, maps, id: Some(new_id()), ..Default::default() });
        }
        if out.presets.is_empty() {
            return Err(PresetError::Unknown);
        }
        Ok(out)
    }

    fn unique_suffix(base: &str, taken: &[String]) -> String {
        (1..).map(|n| if n == 1 { base.to_string() } else { format!("{base}{n}") }).find(|c| !taken.contains(c)).expect("unbounded")
    }

    /// Suffix from a Painter file name: the last literal text between tokens
    /// (`$mesh_$textureSet_Normal.$udim` → `Normal`), sanitized; `Map<n>` when there is none.
    fn suffix_of(file_name: &str, index: usize) -> String {
        let mut s = file_name.to_string();
        for tok in ["$textureSet", "$sparseUdim", "$mesh", "$udim", "$colorSpace", "$project"] {
            s = s.replace(tok, "\0");
        }
        let last = s.split('\0').map(|seg| seg.trim_matches(|c: char| !c.is_alphanumeric())).filter(|seg| !seg.is_empty()).last();
        match last {
            Some(seg) => crate::sanitize(seg),
            None => format!("Map{}", index + 1),
        }
    }

    fn comp_index(c: &str) -> Option<usize> {
        match c.to_ascii_uppercase().as_str() {
            "R" | "L" => Some(0),
            "G" => Some(1),
            "B" => Some(2),
            "A" => Some(3),
            _ => None,
        }
    }

    fn document_channel(name: &str) -> Option<(ChannelKind, bool)> {
        use ChannelKind::*;
        Some(match name.to_ascii_lowercase().replace(['_', ' '], "").as_str() {
            "basecolor" | "diffuse" | "albedo" => (BaseColor, false),
            "height" => (Height, false),
            "roughness" => (Roughness, false),
            "glossiness" => (Roughness, true),
            "metallic" | "metalness" => (Metallic, false),
            "normal" => (Normal, false),
            "ambientocclusion" | "ao" => (AmbientOcclusion, false),
            "emissive" | "emission" => (Emissive, false),
            "opacity" => (Opacity, false),
            "specularlevel" | "specular" => (Specular, false),
            "user0" => (User0, false),
            "user1" => (User1, false),
            "user2" => (User2, false),
            "user3" => (User3, false),
            other => {
                let n: usize = other.strip_prefix("user")?.parse().ok()?;
                (*ChannelKind::USER.get(n)?, false)
            }
        })
    }

    fn mesh_map(name: &str) -> Option<MeshMapKind> {
        use MeshMapKind::*;
        Some(match name.to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
            "ambient_occlusion" | "ao" => AmbientOcclusion,
            "curvature" => Curvature,
            "normal_base" | "normal" | "normal_opengl" | "normal_directx" => Normal,
            "world_space_normals" | "world_normal" | "world_space_normal" => WorldSpaceNormal,
            "id" => Id,
            "position" => Position,
            "thickness" => Thickness,
            "height" => Height,
            "bent_normals" | "bent_normal" => BentNormal,
            _ => return None,
        })
    }

    /// One Painter channel entry → a source, or `None` with a warning.
    fn convert_source(ch: &Value, preset: &str, warnings: &mut Vec<String>) -> Option<Source> {
        let kind = ch.get("srcMapType").and_then(Value::as_str).unwrap_or("documentMap").to_ascii_lowercase();
        let name = ch.get("srcMapName").and_then(Value::as_str).unwrap_or_default();
        let src_ch = ch.get("srcChannel").and_then(Value::as_str).unwrap_or("R");
        let gray = src_ch.eq_ignore_ascii_case("L");
        let component = comp_index(src_ch).unwrap_or(0) as u8;
        let lower = name.to_ascii_lowercase();
        match kind.as_str() {
            "documentmap" | "virtualmap" => {
                if lower.starts_with("normal_opengl") || lower.starts_with("normal_directx") {
                    return Some(Source::Normal { component: component.min(2), directx: lower.contains("directx") });
                }
                if lower == "height" {
                    return Some(Source::Height);
                }
                if lower == "mixed_ao" || lower == "ao_mixed" {
                    return Some(Source::ch(ChannelKind::AmbientOcclusion, 0));
                }
                match document_channel(name) {
                    // Painter's opacity channel is the texel's coverage: opacity times what the
                    // stack covers (issue #145), which is SubCraft's effective alpha.
                    Some((ChannelKind::Opacity, false)) => Some(Source::Alpha),
                    Some((kind, invert)) if gray && kind.is_color() => Some(Source::Gray { kind, invert }),
                    Some((kind, invert)) => Some(Source::Channel { kind, component, invert }),
                    None => {
                        warnings.push(format!("{preset}: Painter map \"{name}\" has no SubCraft equivalent (written as black)"));
                        Some(Source::Constant { value: 0.0 })
                    }
                }
            }
            "meshmap" => match mesh_map(name) {
                Some(kind) => Some(Source::MeshMap { kind, component: component.min(kind.components() as u8 - 1), invert: false }),
                None => {
                    warnings.push(format!("{preset}: Painter mesh map \"{name}\" has no SubCraft equivalent (written as black)"));
                    Some(Source::Constant { value: 0.0 })
                }
            },
            "defaultmap" => Some(Source::Constant {
                value: match lower.as_str() {
                    "white" => 1.0,
                    "gray" | "grey" => 0.5,
                    _ => 0.0,
                },
            }),
            other => {
                warnings.push(format!("{preset}: unknown Painter source type \"{other}\" skipped"));
                None
            }
        }
    }

    fn convert_map(m: &Value, index: usize, preset: &str, warnings: &mut Vec<String>) -> Option<OutputMap> {
        let file_name = m.get("fileName").and_then(Value::as_str).unwrap_or_default();
        let channels = m.get("channels").and_then(Value::as_array)?;
        if channels.is_empty() {
            return None;
        }
        // Destination layout: L (+A) → gray (+alpha); otherwise RGB, RGBA when alpha is used.
        let dests: Vec<(String, &Value)> = channels.iter().map(|c| (c.get("destChannel").and_then(Value::as_str).unwrap_or("R").to_ascii_uppercase(), c)).collect();
        let gray = dests.iter().any(|(d, _)| d == "L");
        let alpha = dests.iter().any(|(d, _)| d == "A");
        let layout: Vec<&str> = match (gray, alpha) {
            (true, false) => vec!["L"],
            (true, true) => vec!["L", "A"],
            (false, false) => vec!["R", "G", "B"],
            (false, true) => vec!["R", "G", "B", "A"],
        };
        let components: Vec<Source> = layout
            .iter()
            .map(|slot| {
                dests.iter().find(|(d, _)| d == slot).and_then(|(_, ch)| convert_source(ch, preset, warnings)).unwrap_or(Source::Constant { value: if *slot == "A" { 1.0 } else { 0.0 } })
            })
            .collect();
        let params = m.get("parameters").cloned().unwrap_or(Value::Null);
        let text = |k: &str| params.get(k).and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_i64().map(|n| n.to_string())));
        let container = match text("fileFormat").unwrap_or_else(|| "png".into()).to_ascii_lowercase().as_str() {
            "png" => Container::Png,
            "tga" | "targa" => Container::Tga,
            "exr" => Container::Exr,
            "tif" | "tiff" => Container::Tiff,
            "jpg" | "jpeg" => Container::Jpeg,
            other => {
                warnings.push(format!("{preset}: file format \"{other}\" is not supported, using PNG"));
                Container::Png
            }
        };
        let depth = match text("bitDepth").unwrap_or_else(|| "8".into()).to_ascii_lowercase().as_str() {
            "16" => BitDepth::B16,
            "16f" | "32f" | "32" => BitDepth::F32,
            _ => BitDepth::B8,
        };
        let format = FileFormat::from_parts(container, depth);
        if format.depth() != depth {
            warnings.push(format!("{preset}: {} has no {} files, using {}", container.label(), depth.label(), format.label()));
        }
        let padding = text("paddingAlgorithm").map(|a| match a.to_ascii_lowercase().as_str() {
            "passthrough" => Padding::None,
            "color" | "transparent" => Padding::Texels(params.get("dilationDistance").and_then(Value::as_u64).unwrap_or(16).clamp(1, 256) as u32),
            _ => Padding::Infinite,
        });
        let size = params.get("sizeLog2").and_then(Value::as_u64).filter(|n| *n <= 14).map(|n| 1u32 << n);
        let srgb = components.iter().any(|s| matches!(s, Source::Channel { kind, .. } | Source::Gray { kind, .. } if kind.is_color()));
        let suffix = suffix_of(file_name, index);
        let mut pattern = file_name.replace("$sparseUdim", "$udim");
        for tok in ["$colorSpace", "$project"] {
            if pattern.contains(tok) {
                warnings.push(format!("{preset}: file name token {tok} is not supported and was removed"));
                pattern = pattern.replace(tok, "");
            }
        }
        let pattern = (!pattern.is_empty()).then_some(pattern);
        Some(OutputMap { pattern, padding, size, ..OutputMap::new(&suffix, format, components, srgb) })
    }
}
