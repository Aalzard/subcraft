//! Painter's "Export flattened" and "Export mask" from the layer stack: one layer or folder,
//! composited on its own, written one file per channel.
//!
//! Fixed choices, as in Painter: the texture set's resolution, 1 texel of padding, the file name
//! pattern `$textureSet_$layerName_$srcMap(.$udim)` by default, and the file type from the
//! channel: EXR for height and normal, otherwise PNG at the channel's bit depth (float channels
//! EXR). A mask is one gray file in the type the user picks.

use std::path::{Path, PathBuf};

use subcraft_core::ChannelKind;
use subcraft_core::color::linear_to_srgb;
use subcraft_doc::ChannelFormat;
use subcraft_image::dilate::dilate_width;
use subcraft_image::{FileFormat, TiledImage};
use subcraft_mesh::Mesh;

use crate::{ExportError, ExportTextureSet, PatternVars, coverage, expand_pattern_with, sanitize};

/// Painter's default file name for flattened exports.
pub const FLATTENED_PATTERN: &str = "$textureSet_$layerName_$srcMap(.$udim)";

/// File type of a flattened channel: EXR for height and normal, else by the channel's format.
pub fn flattened_format(kind: ChannelKind, format: ChannelFormat) -> FileFormat {
    match (kind, format) {
        (ChannelKind::Height | ChannelKind::Normal, _) => FileFormat::Exr32,
        (_, ChannelFormat::Bit8) => FileFormat::Png8,
        (_, ChannelFormat::Bit16) => FileFormat::Png16,
        (_, ChannelFormat::Float16) => FileFormat::Exr16,
        (_, ChannelFormat::Float32) => FileFormat::Exr32,
    }
}

/// `$srcMap` for a channel: its name without spaces (`BaseColor`, `AmbientOcclusion`).
pub fn src_map_name(kind: ChannelKind) -> String {
    kind.display_name().chars().filter(|c| c.is_alphanumeric()).collect()
}

fn base_name<'a>(set: &'a ExportTextureSet) -> &'a str {
    match set.udim {
        Some(n) => set.name.strip_suffix(&format!(".{n}")).unwrap_or(set.name),
        None => set.name,
    }
}

/// Encode `img` for `format` (sRGB for colour in integer files, height as 0.5 + h/2 in integer
/// files), pad by one texel outside the UV islands, and write it as `<dir>/<name>.<ext>`.
#[allow(clippy::too_many_arguments)]
fn write_one(mesh: &Mesh, set: &ExportTextureSet, kind: Option<ChannelKind>, img: &TiledImage, comps: usize, format: FileFormat, name: &str, dir: &Path) -> Result<PathBuf, ExportError> {
    let size = img.width();
    let c = img.comps();
    let dense = img.to_dense();
    let float = format.is_float();
    let mut data: Vec<f32> = Vec::with_capacity((size * size) as usize * comps);
    for px in dense.chunks_exact(c) {
        for i in 0..comps {
            let v = px[i.min(c - 1)];
            data.push(match kind {
                Some(k) if k.is_color() && !float => linear_to_srgb(v.clamp(0.0, 1.0)),
                Some(ChannelKind::Height) if !float => v * 0.5 + 0.5,
                _ => v,
            });
        }
    }
    let covered = coverage(mesh, set.material_slot, size);
    dilate_width(size as usize, size as usize, comps, &mut data, &covered, 1);
    let file = format!("{}.{}", if name.is_empty() { "layer" } else { name }, format.extension());
    let path = dir.join(&file);
    if let Some(parent) = path.parent() {
        subcraft_core::vfs::create_dir_all(parent).map_err(|e| ExportError::Folder(parent.to_path_buf(), e))?;
    }
    let bytes = subcraft_image::io::encode_image(size, size, comps, &data, format).map_err(|e| ExportError::Write(path.clone(), e.to_string()))?;
    subcraft_core::vfs::write(&path, &bytes).map_err(|e| ExportError::Write(path.clone(), e.to_string()))?;
    Ok(path)
}

fn vars<'a>(set: &'a ExportTextureSet, mesh_name: &'a str, layer: &'a str, src: &'a str) -> PatternVars<'a> {
    PatternVars { mesh: mesh_name, project: mesh_name, texture_set: base_name(set), scene_material: base_name(set), udim: set.udim, layer_name: layer, src_map: src, ..Default::default() }
}

/// Write `channels` of `set` (the composite of one layer alone) as one file per channel.
pub fn write_flattened(mesh: &Mesh, mesh_name: &str, set: &ExportTextureSet, layer_name: &str, channels: &[(ChannelKind, ChannelFormat)], pattern: &str, dir: &Path) -> Result<Vec<PathBuf>, ExportError> {
    let mut out = Vec::new();
    for &(kind, format) in channels {
        let Some(img) = set.channels.get(&kind) else { continue };
        let src = src_map_name(kind);
        let name = expand_pattern_with(pattern, &vars(set, mesh_name, layer_name, &src));
        let comps = if kind.is_color() || kind == ChannelKind::Normal { 3 } else { 1 };
        out.push(write_one(mesh, set, Some(kind), img, comps, flattened_format(kind, format), &name, dir)?);
    }
    Ok(out)
}

/// Write a layer's mask (`mask`: one component, 0..1) as one gray file; `$srcMap` is `mask`.
pub fn write_mask(mesh: &Mesh, mesh_name: &str, set: &ExportTextureSet, layer_name: &str, mask: &TiledImage, format: FileFormat, pattern: &str, dir: &Path) -> Result<PathBuf, ExportError> {
    let name = expand_pattern_with(pattern, &vars(set, mesh_name, layer_name, "mask"));
    write_one(mesh, set, None, mask, 1, format, &name, dir)
}

/// Default file name of a flattened export, for the dialog's preview.
pub fn preview_name(pattern: &str, texture_set: &str, layer: &str, kind: ChannelKind) -> String {
    let src = src_map_name(kind);
    let v = PatternVars { mesh: "", project: "", texture_set, scene_material: texture_set, layer_name: layer, src_map: &src, ..Default::default() };
    let name = expand_pattern_with(pattern, &v);
    if name.is_empty() { sanitize(layer) } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use subcraft_mesh::primitives;

    #[test]
    fn flattened_channels_are_named_typed_and_padded() {
        let mesh = primitives::plane(2.0, 1);
        let mut ch = BTreeMap::new();
        ch.insert(ChannelKind::BaseColor, TiledImage::new(32, 32, 4, [0.5, 0.25, 0.0, 1.0]));
        ch.insert(ChannelKind::Roughness, TiledImage::new(32, 32, 1, [0.7; 4]));
        ch.insert(ChannelKind::Height, TiledImage::new(32, 32, 1, [0.25; 4]));
        let set = ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None };
        let dir = std::env::temp_dir().join(format!("subcraft-flat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let channels = [(ChannelKind::BaseColor, ChannelFormat::Bit8), (ChannelKind::Roughness, ChannelFormat::Bit16), (ChannelKind::Height, ChannelFormat::Bit16), (ChannelKind::Metallic, ChannelFormat::Bit8)];
        let files = write_flattened(&mesh, "M", &set, "Rust layer", &channels, FLATTENED_PATTERN, &dir).unwrap();
        let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Body_Rust_layer_BaseColor.png", "Body_Rust_layer_Roughness.png", "Body_Rust_layer_Height.exr"], "absent channels are skipped");
        let bc = subcraft_image::load_bitmap(&files[0]).unwrap();
        assert_eq!(bc.channels, 3);
        assert!((bc.texel(5, 5, false)[0] - linear_to_srgb(0.5)).abs() < 1.0 / 255.0, "colour is sRGB in PNG");
        let r = subcraft_image::load_bitmap(&files[1]).unwrap();
        assert!(matches!(r.data, subcraft_image::BitmapData::U16(_)), "the channel's bit depth");
        let h = subcraft_image::load_bitmap(&files[2]).unwrap();
        assert!((h.texel(5, 5, false)[0] - 0.25).abs() < 1e-6, "height stays raw in EXR");
        // A mask: one gray file named with $srcMap = mask.
        let mask = TiledImage::new(32, 32, 1, [0.4; 4]);
        let m = write_mask(&mesh, "M", &set, "Rust layer", &mask, FileFormat::Png8, FLATTENED_PATTERN, &dir).unwrap();
        assert_eq!(m.file_name().unwrap(), "Body_Rust_layer_mask.png");
        assert!((subcraft_image::load_bitmap(&m).unwrap().texel(5, 5, false)[0] - 0.4).abs() < 1.0 / 255.0);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn udim_tiles_get_their_number_and_formats_follow_the_channel() {
        assert_eq!(flattened_format(ChannelKind::Normal, ChannelFormat::Bit8), FileFormat::Exr32);
        assert_eq!(flattened_format(ChannelKind::Roughness, ChannelFormat::Float16), FileFormat::Exr16);
        let ch = BTreeMap::new();
        let set = ExportTextureSet { name: "Body.1002", material_slot: 0, udim: Some(1002), channels: &ch, mesh_maps: None };
        assert_eq!(expand_pattern_with(FLATTENED_PATTERN, &vars(&set, "M", "L", "BaseColor")), "Body_L_BaseColor.1002");
        assert_eq!(preview_name(FLATTENED_PATTERN, "Body", "Dirt", ChannelKind::AmbientOcclusion), "Body_Dirt_AmbientOcclusion");
    }
}
