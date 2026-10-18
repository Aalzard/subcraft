//! Stroke targets: the images a stroke writes, their pre-stroke snapshots, and the texel update.
//!
//! Every write recomputes texels from the PRE-STROKE snapshot (an O(1) copy-on-write clone taken
//! when the stroke begins) and the stroke coverage, so repeated passes over the same texels never
//! compound: the stroke reaches at most `stroke_opacity`.

use std::collections::BTreeMap;
use std::sync::Arc;

use subcraft_core::{ChannelKind, EffectId, LayerId, ResourceId, normal};
use subcraft_doc::{ContentEffectKind, LayerContent, MaskEffectKind, TextureSet, empty_paint_image};
use subcraft_image::{TILE_TEXELS, TiledImage};

use crate::settings::{BrushMaterial, NormalPaintBlend, PaintTarget, StrokeMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKey {
    Channel(ChannelKind),
    MaskBase,
    EffectChannel(EffectId, ChannelKind),
    MaskEffect(EffectId),
}

impl ImageKey {
    /// The material channel this image holds (`None` for mask rasters).
    pub fn channel(self) -> Option<ChannelKind> {
        match self {
            ImageKey::Channel(k) | ImageKey::EffectChannel(_, k) => Some(k),
            ImageKey::MaskBase | ImageKey::MaskEffect(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintError {
    LayerNotFound,
    /// Content strokes need a paint layer.
    NotAPaintLayer,
    /// The raster is derived from editable anchors, so a brush must not overwrite it.
    EditablePath,
    NoMask,
    /// The selected effect is missing or is not a Paint effect.
    NotAPaintEffect,
    /// No enabled brush channel exists in the texture set.
    NothingToPaint,
    /// The target image or geometry resolution differs from the texture set's.
    ResolutionMismatch,
}

impl std::fmt::Display for PaintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PaintError::LayerNotFound => "layer not found",
            PaintError::NotAPaintLayer => "only paint layers can be painted",
            PaintError::EditablePath => "use the Pen tool to edit this path, or create a paint layer for brushing",
            PaintError::NoMask => "layer has no mask",
            PaintError::NotAPaintEffect => "the selected effect is not a Paint effect",
            PaintError::NothingToPaint => "no enabled brush channel in this texture set",
            PaintError::ResolutionMismatch => "resolution mismatch",
        };
        f.write_str(s)
    }
}

impl std::error::Error for PaintError {}

pub struct TargetImage {
    pub key: ImageKey,
    pub snapshot: TiledImage,
    /// Brush value (replicated for scalars).
    pub value: [f32; 3],
    /// Number of value components; images with alpha store it after them.
    pub value_comps: usize,
    pub has_alpha: bool,
    pub texture: Option<ResourceId>,
    pub normal_blend: NormalPaintBlend,
}

/// Snapshot the images a stroke will write.
pub fn begin_targets(ts: &TextureSet, layer: LayerId, target: PaintTarget, material: &BrushMaterial) -> Result<Vec<TargetImage>, PaintError> {
    let l = ts.find(layer).ok_or(PaintError::LayerNotFound)?;
    let res = ts.resolution;
    let check = |img: &TiledImage| if img.width() == res && img.height() == res { Ok(()) } else { Err(PaintError::ResolutionMismatch) };
    match target {
        PaintTarget::MaskBase => {
            let mask = l.mask.as_ref().ok_or(PaintError::NoMask)?;
            check(&mask.base)?;
            Ok(vec![TargetImage {
                key: ImageKey::MaskBase,
                snapshot: mask.base.clone(),
                value: [material.mask_value; 3],
                value_comps: 1,
                has_alpha: false,
                texture: None,
                normal_blend: NormalPaintBlend::Replace,
            }])
        }
        PaintTarget::MaskEffect(id) => {
            let mask = l.mask.as_ref().ok_or(PaintError::NoMask)?;
            let effect = mask.effects.iter().find(|e| e.id == id).ok_or(PaintError::NotAPaintEffect)?;
            let MaskEffectKind::Paint { image } = &effect.kind else { return Err(PaintError::NotAPaintEffect) };
            check(image)?;
            // Paint effects hold (value, alpha): painting adds coverage, erasing removes it.
            Ok(vec![TargetImage {
                key: ImageKey::MaskEffect(id),
                snapshot: image.clone(),
                value: [material.mask_value; 3],
                value_comps: 1,
                has_alpha: true,
                texture: None,
                normal_blend: NormalPaintBlend::Replace,
            }])
        }
        PaintTarget::Content => {
            let LayerContent::Paint(paint) = &l.content else { return Err(PaintError::NotAPaintLayer) };
            if paint.path.is_some() { return Err(PaintError::EditablePath); }
            channel_targets(ts, &paint.channels, material, ImageKey::Channel)
        }
        PaintTarget::ContentEffect(id) => {
            let effect = l.content_effects.iter().find(|e| e.id == id).ok_or(PaintError::NotAPaintEffect)?;
            let ContentEffectKind::Paint { channels } = &effect.kind else { return Err(PaintError::NotAPaintEffect) };
            channel_targets(ts, channels, material, |k| ImageKey::EffectChannel(id, k))
        }
    }
}

/// One target per enabled brush channel of the texture set, snapshotting `channels`.
fn channel_targets(
    ts: &TextureSet,
    channels: &BTreeMap<ChannelKind, TiledImage>,
    material: &BrushMaterial,
    key: impl Fn(ChannelKind) -> ImageKey,
) -> Result<Vec<TargetImage>, PaintError> {
    let res = ts.resolution;
    let mut out = Vec::new();
    for (&kind, ch) in material.channels.iter().filter(|(k, c)| c.enabled && ts.has_channel(**k)) {
        let snapshot = channels.get(&kind).cloned().unwrap_or_else(|| empty_paint_image(kind, res));
        if snapshot.width() != res || snapshot.height() != res {
            return Err(PaintError::ResolutionMismatch);
        }
        let n = kind.components();
        let value = if n == 1 { [ch.value.0[0]; 3] } else { ch.value.0 };
        out.push(TargetImage { key: key(kind), snapshot, value, value_comps: n, has_alpha: true,
            texture: material.textures.get(&kind).copied(), normal_blend: material.normal_blend });
    }
    if out.is_empty() {
        return Err(PaintError::NothingToPaint);
    }
    Ok(out)
}

impl TargetImage {
    /// New texels of `tile` from the snapshot and coverage, with `k = cov × stroke_opacity`.
    ///
    /// * paint (straight value + alpha): source-over in premultiplied space, i.e. the lerp
    ///   of premultiplied color toward the brush value; alpha → `a + k(1 − a)`;
    /// * erase: alpha → `a(1 − k)`, value kept;
    /// * mask base (no alpha): `lerp(v, brush, k)`; erasing lerps back toward the mask fill.
    pub fn compute_tile(&self, tile: usize, cov: &[f32], opacity: f32, mode: StrokeMode, source: Option<&[[f32; 4]]>) -> Vec<f32> {
        let comps = self.snapshot.comps();
        let mut buf = vec![0.0f32; TILE_TEXELS * comps];
        self.snapshot.read_tile(tile, &mut buf);
        let n = self.value_comps;
        let fill = self.snapshot.fill();
        for (i, (px, &c)) in buf.chunks_exact_mut(comps).zip(cov).enumerate() {
            let src = source.map_or([self.value[0], self.value[1], self.value[2], 1.0], |s| s[i]);
            // Erasing is independent of the current material image, just like flat-value erasing.
            let texture_alpha = if mode == StrokeMode::Paint { src[3].clamp(0.0, 1.0) } else { 1.0 };
            let k = (c * opacity * texture_alpha).clamp(0.0, 1.0);
            if k <= 0.0 {
                continue;
            }
            match (self.has_alpha, mode) {
                (true, StrokeMode::Paint) => {
                    let a0 = px[n];
                    let a = a0 + k * (1.0 - a0);
                    let mut value = [src[0], src[1], src[2]];
                    let is_normal = self.key.channel() == Some(ChannelKind::Normal);
                    if is_normal && self.normal_blend == NormalPaintBlend::Detail && a0 > 0.0 {
                        let combined = normal::reoriented([px[0], px[1], px[2]], value);
                        // Transparent backdrop contributes only the source, as in W3C blending.
                        for j in 0..3 { value[j] += a0 * (combined[j] - value[j]); }
                    }
                    for i in 0..n {
                        px[i] = (px[i] * a0 * (1.0 - k) + value[i] * k) / a;
                    }
                    if is_normal {
                        let unit = normal::encode(normal::decode([px[0], px[1], px[2]]));
                        px[..3].copy_from_slice(&unit);
                    }
                    px[n] = a;
                }
                (true, StrokeMode::Erase) => px[n] *= 1.0 - k,
                (false, mode) => {
                    let to = if mode == StrokeMode::Paint { self.value[0] } else { fill[0] };
                    px[0] += (to - px[0]) * k;
                }
            }
        }
        buf
    }
}

/// Write computed tiles into the live layer (creating channel images on first use).
pub fn store(ts: &mut TextureSet, layer: LayerId, writes: &[(ImageKey, usize, Vec<f32>)]) -> Result<(), PaintError> {
    let res = ts.resolution;
    let l = ts.find_mut(layer).ok_or(PaintError::LayerNotFound)?;
    for (key, tile, data) in writes {
        match key {
            ImageKey::MaskBase => l.mask.as_mut().ok_or(PaintError::NoMask)?.base.write_tile(*tile, data),
            ImageKey::Channel(kind) => {
                let LayerContent::Paint(p) = &mut l.content else { return Err(PaintError::NotAPaintLayer) };
                p.channel_mut(*kind, res).write_tile(*tile, data);
            }
            ImageKey::EffectChannel(id, kind) => {
                let effect = l.content_effects.iter_mut().find(|e| e.id == *id).ok_or(PaintError::NotAPaintEffect)?;
                Arc::make_mut(effect).kind.paint_channel_mut(*kind, res).ok_or(PaintError::NotAPaintEffect)?.write_tile(*tile, data);
            }
            ImageKey::MaskEffect(id) => {
                let mask = l.mask.as_mut().ok_or(PaintError::NoMask)?;
                let effect = mask.effects.iter_mut().find(|e| e.id == *id).ok_or(PaintError::NotAPaintEffect)?;
                let MaskEffectKind::Paint { image } = &mut Arc::make_mut(effect).kind else { return Err(PaintError::NotAPaintEffect) };
                image.write_tile(*tile, data);
            }
        }
    }
    Ok(())
}
