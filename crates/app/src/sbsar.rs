use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Resolution for shelf preview bakes (sphere thumbnail; separate from project apply size).
pub const PREVIEW_BAKE_SIZE: u32 = 512;

pub use subcraft_sbsar::{
    bake_sbsar, bake_sbsar_with_overrides, baked_texture_paths, clamp_bake_size, resolve_host,
    BakeManifest as SbsarBakeManifest, SbsarEnumValue, SbsarInput, DEFAULT_BAKE_SIZE,
    MAX_BAKE_SIZE, MIN_BAKE_SIZE,
};

use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{
    BlendMode, ChannelBlend, ContentEffect, ContentEffectKind, FillChannel, FillContent,
    FillSource, Layer, LayerContent, Projection, ProjectionMode, TextureSet,
};
use subcraft_image::{Bitmap, BitmapData};
use subcraft_sbsar::BakeManifest;

use crate::command::Command;
use crate::images::TextureSetImport;
use crate::session::Session;

/// Stored Substance `.sbsar` graph parameters and user overrides for an imported fill layer.
#[derive(Clone, Debug)]
pub struct SbsarLayerParams {
    pub path: PathBuf,
    pub graph_name: String,
    pub graph_index: usize,
    pub size_px: u32,
    pub inputs: Vec<SbsarInput>,
    pub overrides: BTreeMap<String, String>,
    pub pending_rebake: bool,
}

/// True when every pixel in `bm` has the same RGB/Gray value within 1 8-bit step.
pub(crate) fn is_uniform_bitmap(bm: &Bitmap) -> bool {
    let ch = (bm.channels as usize).clamp(1, 4);
    let check_ch = ch.min(3);
    match &bm.data {
        BitmapData::U8(v) => {
            if v.len() < ch {
                return true;
            }
            let first = &v[..check_ch];
            v.chunks_exact(ch)
                .all(|px| (0..check_ch).all(|c| px[c].abs_diff(first[c]) <= 1))
        }
        BitmapData::U16(v) => {
            if v.len() < ch {
                return true;
            }
            let first = &v[..check_ch];
            v.chunks_exact(ch)
                .all(|px| (0..check_ch).all(|c| px[c].abs_diff(first[c]) <= 64))
        }
        BitmapData::F32(v) => {
            if v.len() < ch {
                return true;
            }
            let first = &v[..check_ch];
            v.chunks_exact(ch)
                .all(|px| (0..check_ch).all(|c| (px[c] - first[c]).abs() <= 1e-3))
        }
    }
}

/// Convert a baked channel bitmap into either a direct `FillSource::Uniform` (when the baked map
/// is flat across all pixels, so the user gets direct color/slider controls in Properties) or a
/// `FillSource::Bitmap` when it contains spatial detail.
pub(crate) fn fill_channel_for_baked_map(
    channel: ChannelKind,
    bm: &Bitmap,
    resource: subcraft_core::ResourceId,
) -> FillChannel {
    if is_uniform_bitmap(bm) {
        match channel {
            ChannelKind::BaseColor => {
                let t = bm.texel(0, 0, true);
                return FillChannel::new(FillSource::uniform(ChannelValue::rgb(t[0], t[1], t[2])));
            }
            ChannelKind::Roughness | ChannelKind::Metallic => {
                let t = bm.texel(0, 0, false);
                return FillChannel::new(FillSource::uniform(ChannelValue::scalar(t[0])));
            }
            ChannelKind::Height => {
                let t = bm.texel(0, 0, false);
                // Substance height maps encode 0.5 (127/255) as neutral midlevel.
                if (t[0] - 0.5).abs() <= 0.02 {
                    return FillChannel::disabled(FillSource::uniform(ChannelValue::scalar(0.0)));
                }
                return FillChannel::new(FillSource::uniform(ChannelValue::scalar(
                    (t[0] - 0.5) * 2.0,
                )));
            }
            ChannelKind::Normal => {
                let t = bm.texel(0, 0, false);
                if (t[0] - 0.5).abs() <= 0.02 && (t[1] - 0.5).abs() <= 0.02 && t[2] >= 0.95 {
                    return FillChannel::disabled(FillSource::uniform(
                        ChannelKind::Normal.default_paint_value(),
                    ));
                }
            }
            _ => {}
        }
    }
    FillChannel::new(FillSource::Bitmap { resource })
}

impl Session {
    /// Evaluate a Substance `.sbsar` via the external bake host, then import the maps as one fill layer.
    pub fn import_sbsar(&mut self, path: &Path) -> Result<TextureSetImport, String> {
        self.import_sbsar_with_options(path, DEFAULT_BAKE_SIZE, 0)
    }

    pub fn import_sbsar_at(&mut self, path: &Path, size_px: u32) -> Result<TextureSetImport, String> {
        self.import_sbsar_with_options(path, clamp_bake_size(size_px), 0)
    }

    /// Bake with explicit output size and graph index (see host `manifest.json` `graphs` list).
    pub fn import_sbsar_with_options(
        &mut self,
        path: &Path,
        size_px: u32,
        graph_index: usize,
    ) -> Result<TextureSetImport, String> {
        let clamped_size = clamp_bake_size(size_px);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("material")
            .to_string();
        let cache_root = std::env::temp_dir().join("subcraft-sbsar");
        let bake_dir = cache_root.join(format!(
            "{}-{}-{}-{}",
            stem,
            clamped_size,
            graph_index,
            uuid::Uuid::new_v4()
        ));
        let manifest = bake_sbsar(path, &bake_dir, clamped_size, graph_index)?;
        let paths: Vec<PathBuf> = baked_texture_paths(&bake_dir, &manifest);

        let ts = self.selected_texture_set().clone();
        let layer_name = if !manifest.graph.is_empty() {
            manifest.graph.clone()
        } else {
            stem.clone()
        };
        let (mut layer, mut result) = self.build_sbsar_fill_layer(&layer_name, &paths, &ts)?;
        result.layer_name = layer_name.clone();

        let layer_id = layer.id;
        layer.name = layer_name.clone();
        self.execute(Command::InsertLayer {
            layer: Box::new(layer),
            label: format!("Import SBSAR: {layer_name}"),
        })
        .map_err(|e| e.to_string())?;

        self.sbsar_layers.insert(
            layer_id,
            SbsarLayerParams {
                path: path.to_path_buf(),
                graph_name: manifest.graph.clone(),
                graph_index,
                size_px: clamped_size,
                inputs: manifest.inputs,
                overrides: BTreeMap::new(),
                pending_rebake: false,
            },
        );

        let _ = std::fs::remove_dir_all(&bake_dir);

        log::info!(
            "imported SBSAR {} as {} ({} map(s))",
            path.display(),
            manifest.graph,
            result.used.len()
        );
        Ok(result)
    }

    /// Bake a Substance `.sbsar` material and apply it directly onto an existing layer,
    /// preserving that layer's mask and parts.
    pub fn apply_sbsar_to_layer(
        &mut self,
        layer_id: LayerId,
        path: &Path,
        size_px: u32,
    ) -> Result<TextureSetImport, String> {
        let clamped_size = clamp_bake_size(size_px);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("material")
            .to_string();
        let cache_root = std::env::temp_dir().join("subcraft-sbsar");
        let bake_dir = cache_root.join(format!(
            "{}-{}-0-{}",
            stem,
            clamped_size,
            uuid::Uuid::new_v4()
        ));
        let manifest = bake_sbsar(path, &bake_dir, clamped_size, 0)?;
        let paths: Vec<PathBuf> = baked_texture_paths(&bake_dir, &manifest);

        let ts = self.selected_texture_set().clone();
        let existing = ts
            .find(layer_id)
            .cloned()
            .ok_or_else(|| "target layer not found".to_string())?;
        let layer_name = if !manifest.graph.is_empty() {
            manifest.graph.clone()
        } else {
            stem.clone()
        };
        let (mut layer, mut result) = self.build_sbsar_fill_layer(&layer_name, &paths, &ts)?;
        result.layer_name = layer_name.clone();
        layer.id = layer_id;
        layer.name = format!("Stripe · {layer_name}");
        layer.visible = existing.visible;
        if existing.mask.is_some() {
            layer.mask = existing.mask;
        }
        if existing.parts.is_some() {
            layer.parts = existing.parts;
        }

        self.execute(Command::ReplaceLayer {
            layer: layer_id,
            replacement: Box::new(layer),
            label: format!("Stripe SBSAR: {layer_name}"),
        })
        .map_err(|e| e.to_string())?;

        self.sbsar_layers.insert(
            layer_id,
            SbsarLayerParams {
                path: path.to_path_buf(),
                graph_name: manifest.graph.clone(),
                graph_index: 0,
                size_px: clamped_size,
                inputs: manifest.inputs,
                overrides: BTreeMap::new(),
                pending_rebake: false,
            },
        );

        let _ = std::fs::remove_dir_all(&bake_dir);
        Ok(result)
    }

    /// Re-bake an imported `.sbsar` fill layer using its current parameter `overrides` and update
    /// the layer's channels in place.
    pub fn rebake_sbsar_layer(&mut self, layer_id: LayerId) -> Result<(), String> {
        let Some(params) = self.sbsar_layers.get(&layer_id).cloned() else {
            return Err("layer has no associated SBSAR parameters".into());
        };
        let Some(existing_layer) = self.selected_texture_set().find(layer_id).cloned() else {
            return Err("layer not found in selected texture set".into());
        };
        let existing_projection = match &existing_layer.content {
            LayerContent::Fill(existing_fill) => existing_fill.projection,
            _ => return Err("layer is not a fill layer".into()),
        };

        let stem = params
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("material")
            .to_string();
        let cache_root = std::env::temp_dir().join("subcraft-sbsar");
        let bake_dir = cache_root.join(format!(
            "{}-{}-{}-{}",
            stem,
            params.size_px,
            params.graph_index,
            uuid::Uuid::new_v4()
        ));
        let overrides_vec: Vec<(String, String)> = params
            .overrides
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let manifest = bake_sbsar_with_overrides(
            &params.path,
            &bake_dir,
            params.size_px,
            params.graph_index,
            &overrides_vec,
        )?;
        let paths: Vec<PathBuf> = baked_texture_paths(&bake_dir, &manifest);
        let ts = self.selected_texture_set().clone();
        let (baked_layer, _) = self.build_sbsar_fill_layer(&existing_layer.name, &paths, &ts)?;
        let LayerContent::Fill(baked_fill) = baked_layer.content else {
            unreachable!()
        };

        let _ = std::fs::remove_dir_all(&bake_dir);

        let mut updated_layer = existing_layer;
        updated_layer.content = LayerContent::Fill(FillContent {
            projection: existing_projection,
            channels: baked_fill.channels,
        });

        self.execute(Command::ReplaceLayer {
            layer: layer_id,
            replacement: Box::new(updated_layer),
            label: "Adjust SBSAR Parameter".into(),
        })
        .map_err(|e| e.to_string())?;

        if let Some(stored) = self.sbsar_layers.get_mut(&layer_id) {
            stored.inputs = manifest.inputs;
        }
        Ok(())
    }

    fn build_sbsar_fill_layer(
        &mut self,
        layer_name: &str,
        paths: &[PathBuf],
        ts: &TextureSet,
    ) -> Result<(Layer, TextureSetImport), String> {
        let plan = subcraft_assets::texture_maps::plan(paths);
        let file_name = |p: &Path| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image")
                .to_string()
        };

        let mut result = TextureSetImport {
            layer_name: layer_name.to_string(),
            skipped: plan
                .skipped
                .iter()
                .map(|(p, why)| (file_name(p), why.clone()))
                .collect(),
            ..Default::default()
        };

        let mut layer = Layer::new_fill(layer_name, ts);
        let LayerContent::Fill(fill) = &mut layer.content else {
            unreachable!("new_fill")
        };
        for c in fill.channels.values_mut() {
            c.enabled = false;
        }

        for (path, m) in &plan.maps {
            let name = file_name(path);
            if !ts.has_channel(m.channel) {
                result.skipped.push((
                    name,
                    format!(
                        "the texture set has no {} channel",
                        m.channel.display_name()
                    ),
                ));
                continue;
            }
            let resource = self.import_converted(path, m.conversion)?;
            let channel_val = if let Some(res) = self.resources().get(resource) {
                fill_channel_for_baked_map(m.channel, &res.bitmap, resource)
            } else {
                FillChannel::new(FillSource::Bitmap { resource })
            };
            fill.channels.insert(m.channel, channel_val);
            result.used.push((name, m.channel));
        }

        if result.used.is_empty() {
            return Err("no compatible channels produced by SBSAR bake".into());
        }
        Ok((layer, result))
    }

    /// Bake a Substance `.sbsar` filter (such as `matfinish_powder_coated.sbsar`) and append it
    /// as a non-destructive `ContentEffect` on `layer`.
    pub fn apply_sbsar_as_content_effect(
        &mut self,
        layer: LayerId,
        path: &Path,
        size_px: u32,
    ) -> Result<TextureSetImport, String> {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("filter")
            .to_string();
        let cache_root = std::env::temp_dir().join("subcraft-sbsar");
        let bake_dir = cache_root.join(format!("{}-{}-0-{}", stem, size_px, uuid::Uuid::new_v4()));
        let manifest = bake_sbsar(path, &bake_dir, clamp_bake_size(size_px), 0)?;
        let paths: Vec<PathBuf> = baked_texture_paths(&bake_dir, &manifest);
        let plan = subcraft_assets::texture_maps::plan(&paths);
        let file_name = |p: &Path| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image")
                .to_string()
        };

        let ts = self.selected_texture_set().clone();
        let mut channels = BTreeMap::new();
        for kind in ts.channel_kinds() {
            channels.insert(
                kind,
                FillChannel::disabled(FillSource::uniform(kind.default_paint_value())),
            );
        }

        let mut result = TextureSetImport {
            layer_name: manifest.graph.clone(),
            skipped: plan
                .skipped
                .iter()
                .map(|(p, why)| (file_name(p), why.clone()))
                .collect(),
            ..Default::default()
        };

        for (map_path, m) in &plan.maps {
            let name = file_name(map_path);
            if !ts.has_channel(m.channel) {
                result.skipped.push((
                    name,
                    format!(
                        "the texture set has no {} channel",
                        m.channel.display_name()
                    ),
                ));
                continue;
            }
            let resource = self.import_converted(map_path, m.conversion)?;
            channels.insert(m.channel, FillChannel::new(FillSource::Bitmap { resource }));
            result.used.push((name, m.channel));
        }

        let _ = std::fs::remove_dir_all(&bake_dir);

        if result.used.is_empty() {
            return Err("no compatible channels produced by SBSAR filter".into());
        }

        let is_finish = stem.to_lowercase().contains("finish");
        let projection = Projection {
            mode: if is_finish {
                ProjectionMode::Triplanar
            } else {
                ProjectionMode::Uv
            },
            scale: if is_finish { [4.0, 4.0] } else { [1.0, 1.0] },
            ..Projection::default()
        };

        let mut effect = ContentEffect::new(
            ContentEffectKind::Fill(FillContent {
                projection,
                channels,
            }),
            &ts,
        );
        effect.name = manifest.graph.clone();
        if is_finish {
            if let Some(b) = effect.blend.get_mut(&ChannelKind::Height) {
                b.mode = BlendMode::LinearDodge;
                b.opacity = 1.0;
            }
            if let Some(b) = effect.blend.get_mut(&ChannelKind::Roughness) {
                b.mode = BlendMode::LinearLight;
                b.opacity = 0.35;
            }
            effect.blend.insert(
                ChannelKind::Normal,
                ChannelBlend {
                    mode: BlendMode::NormalMapCombine,
                    opacity: 1.0,
                },
            );
        }

        let clamped_size = clamp_bake_size(size_px);
        let kind = effect.kind.clone();
        self.execute(Command::AddContentEffect { layer, kind })
            .map_err(|e| e.to_string())?;
        let effect_id = self
            .selected_effect()
            .ok_or_else(|| "SBSAR filter effect was not selected after add".to_string())?;
        effect.id = effect_id;
        self.execute(Command::SetContentEffect { layer, effect })
            .map_err(|e| e.to_string())?;

        self.sbsar_effects.insert(
            (layer, effect_id),
            SbsarLayerParams {
                path: path.to_path_buf(),
                graph_name: manifest.graph.clone(),
                graph_index: 0,
                size_px: clamped_size,
                inputs: manifest.inputs,
                overrides: BTreeMap::new(),
                pending_rebake: false,
            },
        );

        Ok(result)
    }

    /// Re-bake a Substance `.sbsar` content effect using its current parameter `overrides`.
    pub fn rebake_sbsar_content_effect(
        &mut self,
        layer_id: LayerId,
        effect_id: subcraft_core::EffectId,
    ) -> Result<(), String> {
        let Some(params) = self.sbsar_effects.get(&(layer_id, effect_id)).cloned() else {
            return Err("effect has no associated SBSAR parameters".into());
        };
        let Some(existing_layer) = self.selected_texture_set().find(layer_id).cloned() else {
            return Err("layer not found in selected texture set".into());
        };
        let Some(existing_effect) = existing_layer
            .content_effects
            .iter()
            .find(|e| e.id == effect_id)
            .cloned()
        else {
            return Err("content effect not found on layer".into());
        };
        let ContentEffectKind::Fill(existing_fill) = &existing_effect.kind else {
            return Err("content effect is not a fill effect".into());
        };
        let existing_projection = existing_fill.projection;

        let stem = params
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("filter")
            .to_string();
        let cache_root = std::env::temp_dir().join("subcraft-sbsar");
        let bake_dir = cache_root.join(format!(
            "{}-{}-{}-{}",
            stem,
            params.size_px,
            params.graph_index,
            uuid::Uuid::new_v4()
        ));
        let overrides_vec: Vec<(String, String)> = params
            .overrides
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let manifest = bake_sbsar_with_overrides(
            &params.path,
            &bake_dir,
            params.size_px,
            params.graph_index,
            &overrides_vec,
        )?;
        let paths: Vec<PathBuf> = baked_texture_paths(&bake_dir, &manifest);
        let plan = subcraft_assets::texture_maps::plan(&paths);
        let ts = self.selected_texture_set().clone();

        let mut channels = BTreeMap::new();
        for kind in ts.channel_kinds() {
            channels.insert(
                kind,
                FillChannel::disabled(FillSource::uniform(kind.default_paint_value())),
            );
        }
        for (map_path, m) in &plan.maps {
            if !ts.has_channel(m.channel) {
                continue;
            }
            let resource = self.import_converted(map_path, m.conversion)?;
            channels.insert(m.channel, FillChannel::new(FillSource::Bitmap { resource }));
        }
        let _ = std::fs::remove_dir_all(&bake_dir);

        let mut updated_effect = (*existing_effect).clone();
        updated_effect.kind = ContentEffectKind::Fill(FillContent {
            projection: existing_projection,
            channels,
        });

        self.execute(Command::SetContentEffect {
            layer: layer_id,
            effect: updated_effect,
        })
        .map_err(|e| e.to_string())?;

        if let Some(stored) = self.sbsar_effects.get_mut(&(layer_id, effect_id)) {
            stored.inputs = manifest.inputs;
        }
        Ok(())
    }
}

/// Bake (or reuse) maps under `cache_dir` for shelf sphere previews.
pub fn ensure_preview_bake(sbsar: &Path, cache_dir: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("{}: {e}", cache_dir.display()))?;
    let manifest_path = cache_dir.join("manifest.json");
    let needs_bake = !manifest_path.is_file()
        || std::fs::metadata(sbsar)
            .and_then(|m| m.modified())
            .ok()
            .zip(std::fs::metadata(&manifest_path).and_then(|m| m.modified()).ok())
            .is_some_and(|(src, man)| src > man);
    if needs_bake {
        bake_sbsar(sbsar, cache_dir, PREVIEW_BAKE_SIZE, 0)?;
    }
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest: BakeManifest =
        serde_json::from_str(&text).map_err(|e| format!("manifest.json: {e}"))?;
    Ok(baked_texture_paths(cache_dir, &manifest))
}
