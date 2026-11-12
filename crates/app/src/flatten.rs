//! Painter's flatten (Ctrl+M) and "Export flattened group to files": a layer or folder evaluated
//! on its own into one raster per channel.
//!
//! The flattened layer is a paint layer holding those rasters (Painter makes a fill layer of
//! bitmaps; a paint layer is the same result without new resources). It takes the source's
//! blending, colour tag and geometry mask; its mask is the source's mask evaluated into a plain
//! base. The source stays, hidden, under it. A folder in Passthrough is evaluated as an isolated
//! folder (its children over transparency), since its result depends on the stack below.

use std::path::{Path, PathBuf};

use subcraft_compositor::{TileContext, mask::eval_mask_scaled, tile::preview_content_context_scaled};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{BlendMode, Layer, LayerContent, Mask, PaintContent, TextureSet, empty_paint_image};
use subcraft_image::{TILE_SIZE, TiledImage};
use subcraft_materials::SampleContext;

use crate::{Command, CommandError, Session};

/// Default file name pattern of the flattened export (Painter's).
pub const DEFAULT_PATTERN: &str = "$textureSet_$layerName_$srcMap";

/// A file name from the export pattern: `$textureSet`, `$layerName` and `$srcMap` (the channel)
/// are replaced; characters a file name cannot hold become `_`.
pub fn file_name(pattern: &str, texture_set: &str, layer: &str, channel: ChannelKind) -> String {
    let name = pattern
        .replace("$textureSet", texture_set)
        .replace("$layerName", layer)
        .replace("$srcMap", &channel.display_name().replace(' ', ""));
    subcraft_export::sanitize(&name)
}

/// Painter's per-channel format: EXR for Height and Normal (precision), 8-bit PNG otherwise.
pub fn format_for(channel: ChannelKind) -> subcraft_image::FileFormat {
    match channel {
        ChannelKind::Height | ChannelKind::Normal => subcraft_image::FileFormat::Exr32,
        _ => subcraft_image::FileFormat::Png8,
    }
}

impl Session {
    /// `layer` evaluated on its own: one raster per channel it blends (value and coverage), its
    /// mask flattened into a plain base. Not inserted anywhere.
    pub fn flattened(&self, layer: LayerId) -> Result<Layer, CommandError> {
        let set = self.texture_set_of(layer).ok_or(CommandError::LayerNotFound)?;
        let ts = self.document().texture_set(set).ok_or(CommandError::TextureSetNotFound)?;
        let source = ts.find(layer).ok_or(CommandError::LayerNotFound)?;
        let mut content = source.clone();
        content.visible = true;
        content.mask = None;
        content.parts = None;
        let res = ts.resolution;
        let geometry = self.scene().geometry.get(ts.material_slot, res);
        let no_maps = subcraft_materials::MeshMaps::new();
        let maps = self.mesh_maps(set).unwrap_or(&no_maps);
        let tiles = res.div_ceil(TILE_SIZE) as usize;
        let mut paint = PaintContent::default();
        // Hidden layers and effects drop out on their own: the evaluation skips them.
        for kind in ts.channel_kinds().filter(|k| source.blend_for(*k).is_active()) {
            let mut img = empty_paint_image(kind, res);
            let comps = img.comps();
            let mut buf = vec![0.0f32; subcraft_image::TILE_TEXELS * comps];
            for t in 0..tiles * tiles {
                let geo = geometry.tile(t);
                let tc = TileContext { albedo_filter: ts.albedo_filter, geo: &geo, set_geometry: &geometry, sample: SampleContext { resources: self.resources(), mesh_maps: maps } };
                let px = preview_content_context_scaled(&content, kind, &tc, 1.0, &ts.layers);
                if px.iter().all(|p| p[3] <= 0.0) {
                    continue;
                }
                for (i, p) in px.iter().enumerate() {
                    let at = i * comps;
                    buf[at..at + comps - 1].copy_from_slice(&p[..comps - 1]);
                    buf[at + comps - 1] = p[3];
                }
                img.write_tile(t, &buf);
            }
            paint.channels.insert(kind, img);
        }
        let mut out = Layer::new_paint(format!("{} (flattened)", source.name), ts);
        out.content = LayerContent::Paint(paint);
        out.blend = source.blend.clone();
        // A paint layer cannot pass through: its rasters already hold the folder's result.
        for b in out.blend.values_mut() {
            if b.mode == BlendMode::Passthrough {
                b.mode = BlendMode::Normal;
            }
        }
        out.parts = source.parts.clone();
        out.color = source.color;
        out.mask = source.mask.as_ref().map(|m| flatten_mask(m, ts, &geometry, self.resources(), maps));
        Ok(out)
    }

    /// Write each channel of `layer`'s flattened result to `folder`, named by `pattern`.
    /// Returns the files written. Painter's padding of 1 px is applied around the coverage.
    pub fn export_flattened(&self, layer: LayerId, folder: &Path, pattern: &str) -> Result<Vec<PathBuf>, String> {
        let flat = self.flattened(layer).map_err(|e| e.to_string())?;
        let set = self.texture_set_of(layer).ok_or("layer not found")?;
        let ts = self.document().texture_set(set).ok_or("texture set not found")?;
        let source = ts.find(layer).ok_or("layer not found")?;
        let LayerContent::Paint(paint) = &flat.content else { unreachable!("flattened layers are paint layers") };
        let mut written = Vec::new();
        for (&kind, img) in &paint.channels {
            let res = img.width() as usize;
            let comps = img.comps();
            let mut dense = img.to_dense();
            let covered: Vec<bool> = dense.chunks(comps).map(|p| p[comps - 1] > 0.0).collect();
            if !covered.contains(&true) {
                continue;
            }
            subcraft_image::dilate::dilate_width(res, res, comps, &mut dense, &covered, 1);
            // Value plus coverage as alpha (gray + alpha for one-component channels).
            let out_comps = comps;
            let mut data = Vec::with_capacity(res * res * out_comps);
            for p in dense.chunks(comps) {
                data.extend_from_slice(&p[..comps - 1]);
                data.push(p[comps - 1].clamp(0.0, 1.0));
            }
            let format = format_for(kind);
            let path = folder.join(format!("{}.{}", file_name(pattern, &ts.name, &source.name, kind), format.extension()));
            subcraft_image::save_image(&path, res as u32, res as u32, out_comps, &data, format).map_err(|e| format!("{}: {e}", path.display()))?;
            written.push(path);
        }
        if let Some(mask) = &flat.mask {
            let res = mask.base.width();
            let gray: Vec<f32> = mask.base.to_dense().chunks(mask.base.comps()).map(|p| p[0].clamp(0.0, 1.0)).collect();
            let path = folder.join(format!("{}.png", subcraft_export::sanitize(&pattern.replace("$textureSet", &ts.name).replace("$layerName", &source.name).replace("$srcMap", "Mask"))));
            subcraft_image::save_image(&path, res, res, 1, &gray, subcraft_image::FileFormat::Png8).map_err(|e| format!("{}: {e}", path.display()))?;
            written.push(path);
        }
        Ok(written)
    }
}

/// A mask evaluated with its effects into a plain base (no effects).
fn flatten_mask(mask: &Mask, ts: &TextureSet, geometry: &subcraft_materials::TextureSetGeometry, resources: &subcraft_doc::ResourceStore, maps: &subcraft_materials::MeshMaps) -> Mask {
    let res = ts.resolution;
    let mut source = mask.clone();
    source.enabled = true;
    let mut base = TiledImage::new(res, res, 1, [0.0; 4]);
    let tiles = res.div_ceil(TILE_SIZE) as usize;
    for t in 0..tiles * tiles {
        let geo = geometry.tile(t);
        let tc = TileContext { albedo_filter: ts.albedo_filter, geo: &geo, set_geometry: geometry, sample: SampleContext { resources, mesh_maps: maps } };
        if let Some(values) = eval_mask_scaled(Some(&source), &tc, 1.0) {
            base.write_tile(t, &values);
        }
    }
    Mask { enabled: mask.enabled, base, effects: Vec::new() }
}

/// The flatten command's work, kept out of `Session::execute`'s match.
pub(crate) fn flatten_layers(s: &mut Session, layers: &[LayerId]) -> Result<(), CommandError> {
    let first = *layers.first().ok_or_else(|| CommandError::Invalid("Select the layers or folders to flatten".into()))?;
    let set = s.texture_set_of(first).ok_or(CommandError::LayerNotFound)?;
    if layers.iter().any(|&l| s.texture_set_of(l) != Some(set)) {
        return Err(CommandError::Invalid("The layers to flatten are in different texture sets".into()));
    }
    let ts = s.document().texture_set(set).ok_or(CommandError::TextureSetNotFound)?;
    let order = crate::clipboard::top_level_in_order(ts, layers);
    let mut made = Vec::new();
    for &id in &order {
        made.push((id, s.flattened(id)?));
    }
    let mut last = None;
    s.edit_layer_set(first, if made.len() == 1 { "Flatten Layer" } else { "Flatten Layers" }, None, |ts| {
        for (source, flat) in made {
            // The source stays under its flattened copy, hidden.
            let l = Session::layer_mut(ts, source)?;
            l.visible = false;
            last = Some(ts.insert_above(Some(source), flat));
        }
        Ok(())
    })?;
    s.execute(Command::SelectLayer(last))
}
