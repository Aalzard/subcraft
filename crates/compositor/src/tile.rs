//! Evaluation of a layer tree for one tile.
//!
//! Every channel keeps a running accumulation of straight `(value, alpha)` texels. The root starts
//! at the channel's default value with alpha 1; isolated folders start transparent; passthrough
//! folders let their children act directly on the accumulation below and then lerp the result
//! by folder opacity × mask.

use std::sync::Arc;

use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, ContentEffect, ContentEffectKind, Layer, LayerContent, TextureSet};
use subcraft_image::TILE_TEXELS;
use subcraft_materials::sample::eval_fill_tile_filtered;

use crate::TileContext;
use crate::blend::{BlendTarget, composite};
use crate::mask::eval_mask;
use crate::util::read_tile_rgba;

/// Running accumulation of one channel over a tile.
#[derive(Clone)]
struct Acc {
    kind: ChannelKind,
    target: BlendTarget,
    px: Vec<[f32; 4]>,
}

impl Acc {
    fn root(kind: ChannelKind) -> Self {
        let d = kind.default_value().0;
        Self { kind, target: BlendTarget::for_channel(kind), px: vec![[d[0], d[1], d[2], 1.0]; TILE_TEXELS] }
    }

    fn transparent(kind: ChannelKind) -> Self {
        Self { kind, target: BlendTarget::for_channel(kind), px: vec![[0.0; 4]; TILE_TEXELS] }
    }
}

/// Composite all layers of `ts` for one tile. Returns, per texture-set channel (in
/// `ts.channels` order), the accumulated `[v0, v1, v2, alpha]` texels.
pub fn evaluate_tile(ts: &TextureSet, tc: &TileContext) -> Vec<(ChannelKind, Vec<[f32; 4]>)> {
    let mut accs: Vec<Acc> = ts.channel_kinds().map(Acc::root).collect();
    let active = vec![true; accs.len()];
    apply_stack(&ts.layers, &mut accs, &active, tc);
    accs.into_iter().map(|a| (a.kind, a.px)).collect()
}

fn apply_stack(layers: &[Arc<Layer>], accs: &mut [Acc], active: &[bool], tc: &TileContext) {
    for layer in layers {
        apply_layer(layer, accs, active, tc);
    }
}

/// Isolated authored content for a UV thumbnail, before its own mask/blend/opacity/visibility.
/// Folder children retain their masks and blends. Uses the production evaluator and channel rules.
pub fn preview_content(layer: &Layer, channel: ChannelKind, tc: &TileContext) -> Vec<[f32; 4]> {
    let mut source = layer.clone();
    source.visible = true;
    source.mask = None;
    source.parts = None;
    source.blend.insert(channel, subcraft_doc::ChannelBlend::new(BlendMode::Normal));
    let mut acc = [Acc::transparent(channel)];
    apply_layer(&source, &mut acc, &[true], tc);
    std::mem::take(&mut acc[0].px)
}

#[inline]
fn effective_mode(mode: BlendMode) -> BlendMode {
    if mode == BlendMode::Passthrough { BlendMode::Normal } else { mode }
}

fn apply_layer(layer: &Layer, accs: &mut [Acc], active: &[bool], tc: &TileContext) {
    if !layer.visible {
        return;
    }
    // Channels this layer can affect at all.
    let live: Vec<bool> = accs.iter().zip(active).map(|(a, &on)| on && layer.blend_for(a.kind).is_active()).collect();
    if !live.contains(&true) {
        return;
    }
    let mut mask = eval_mask(layer.mask.as_ref(), tc);
    // Part mask: the layer only shows on its chosen mesh parts.
    if let Some(parts) = layer.parts.as_ref().filter(|p| p.enabled) {
        if parts.parts.is_empty() {
            return; // visible nowhere
        }
        let m = mask.get_or_insert_with(|| vec![1.0; TILE_TEXELS]);
        for (i, v) in m.iter_mut().enumerate() {
            *v *= parts.factor(tc.geo.region(i, parts.kind));
        }
    }
    // Fully masked out in this tile: blending with zero coverage changes nothing, so skip the
    // (possibly expensive) content evaluation entirely. Exact, not an approximation.
    if mask.as_ref().is_some_and(|m| m.iter().all(|&v| v <= 0.0)) {
        return;
    }
    let mask_at = |i: usize| mask.as_ref().map_or(1.0, |m| m[i]);
    let mut content = vec![[0.0f32; 4]; TILE_TEXELS];

    match &layer.content {
        LayerContent::Paint(paint) => {
            for (acc, _) in accs.iter_mut().zip(&live).filter(|(_, l)| **l) {
                let Some(img) = paint.channels.get(&acc.kind) else {
                    // No base raster on this channel: effects still apply to transparent content.
                    if layer.content_effects.is_empty() {
                        continue;
                    }
                    content.fill([0.0; 4]);
                    apply_content_effects(&layer.content_effects, acc, &mut content, tc);
                    blend_content(acc, &content, layer, &mask_at);
                    continue;
                };
                read_paint(img, acc.kind, tc, &mut content);
                apply_content_effects(&layer.content_effects, acc, &mut content, tc);
                blend_content(acc, &content, layer, &mask_at);
            }
        }
        LayerContent::Fill(fill) => {
            for (acc, _) in accs.iter_mut().zip(&live).filter(|(_, l)| **l) {
                if !eval_fill(fill, acc.kind, tc, &mut content) {
                    if layer.content_effects.is_empty() {
                        continue;
                    }
                    content.fill([0.0; 4]);
                }
                apply_content_effects(&layer.content_effects, acc, &mut content, tc);
                blend_content(acc, &content, layer, &mask_at);
            }
        }
        LayerContent::Group(group) => {
            let mut child: Vec<Acc> = accs
                .iter()
                .zip(&live)
                .map(|(a, &on)| {
                    if on && layer.blend_for(a.kind).mode == BlendMode::Passthrough {
                        a.clone()
                    } else {
                        Acc::transparent(a.kind)
                    }
                })
                .collect();
            apply_stack(&group.children, &mut child, &live, tc);
            for ((acc, mut inner), _) in accs.iter_mut().zip(child).zip(&live).filter(|(_, l)| **l) {
                apply_content_effects(&layer.content_effects, acc, &mut inner.px, tc);
                let blend = layer.blend_for(acc.kind);
                if blend.mode == BlendMode::Passthrough {
                    for (i, (dst, src)) in acc.px.iter_mut().zip(&inner.px).enumerate() {
                        let f = (blend.opacity * mask_at(i)).clamp(0.0, 1.0);
                        for c in 0..4 {
                            dst[c] += (src[c] - dst[c]) * f;
                        }
                    }
                } else {
                    blend_content(acc, &inner.px, layer, &mask_at);
                }
            }
        }
    }
}

/// Read a paint raster (`components` values followed by alpha) as `[v0, v1, v2, alpha]`.
fn read_paint(img: &subcraft_image::TiledImage, kind: ChannelKind, tc: &TileContext, out: &mut [[f32; 4]]) {
    read_tile_rgba(img, tc.geo, out);
    let n = kind.components();
    for px in out.iter_mut() {
        let a = px[n];
        *px = if n == 1 { [px[0], px[0], px[0], a] } else { [px[0], px[1], px[2], a] };
    }
}

/// Evaluate one channel of a fill as `[v0, v1, v2, alpha]`; false if the channel is off/absent.
fn eval_fill(fill: &subcraft_doc::FillContent, kind: ChannelKind, tc: &TileContext, out: &mut [[f32; 4]]) -> bool {
    let Some(ch) = fill.channels.get(&kind).filter(|c| c.enabled) else { return false };
    let filter = if kind == ChannelKind::BaseColor { tc.albedo_filter } else { subcraft_image::TextureFilter::Bilinear };
    eval_fill_tile_filtered(&ch.source, kind.is_color(), &fill.projection, tc.geo, &tc.sample, filter, out);
    crate::strength::apply_strength(kind, ch.strength, out);
    if kind.components() == 1 {
        for px in out.iter_mut() {
            *px = [px[0], px[0], px[0], px[3]];
        }
    }
    true
}

/// Run a layer's content effect stack (bottom to top) on one channel of its content.
///
/// Sources (Paint, Fill) composite their `(value, alpha)` with the effect's channel blend.
/// Adjustments (Levels) blend the transformed value over the content and keep its coverage,
/// so they never make transparent content visible.
fn apply_content_effects(effects: &[Arc<ContentEffect>], acc: &Acc, content: &mut [[f32; 4]], tc: &TileContext) {
    let kind = acc.kind;
    let mut e = vec![[0.0f32; 4]; TILE_TEXELS];
    for effect in effects.iter().filter(|e| e.visible) {
        let blend = effect.blend_for(kind);
        if !blend.is_active() {
            continue;
        }
        let mode = effective_mode(blend.mode);
        match &effect.kind {
            ContentEffectKind::Paint { channels } => {
                let Some(img) = channels.get(&kind) else { continue };
                read_paint(img, kind, tc, &mut e);
            }
            ContentEffectKind::Fill(fill) => {
                if !eval_fill(fill, kind, tc, &mut e) {
                    continue;
                }
            }
            ContentEffectKind::Levels { channel, levels } => {
                if *channel != kind || kind == ChannelKind::Normal {
                    continue;
                }
                for px in content.iter_mut() {
                    let mut dst = [px[0], px[1], px[2], 1.0];
                    let v = [levels.apply(px[0]), levels.apply(px[1]), levels.apply(px[2])];
                    composite(&mut dst, v, blend.opacity, mode, acc.target);
                    px[..3].copy_from_slice(&dst[..3]);
                }
                continue;
            }
            ContentEffectKind::Filter { channel, filter } => {
                if *channel != kind || !filter.supports_channel(kind) || !filter.valid() { continue; }
                for px in content.iter_mut() {
                    let mut dst = [px[0], px[1], px[2], 1.0];
                    let value = subcraft_materials::filter::apply(*filter, [px[0], px[1], px[2]], kind);
                    composite(&mut dst, value, blend.opacity, mode, acc.target);
                    px[..3].copy_from_slice(&dst[..3]);
                }
                continue;
            }
        }
        let ignore_alpha = mode == BlendMode::Replace;
        for (dst, src) in content.iter_mut().zip(&e) {
            let a = if ignore_alpha { 1.0 } else { src[3] } * blend.opacity;
            composite(dst, [src[0], src[1], src[2]], a, mode, acc.target);
        }
    }
}

/// Composite `(value, alpha)` content onto a channel with the layer's mode, opacity and mask.
fn blend_content(acc: &mut Acc, content: &[[f32; 4]], layer: &Layer, mask_at: &dyn Fn(usize) -> f32) {
    let blend = layer.blend_for(acc.kind);
    let mode = effective_mode(blend.mode);
    let ignore_alpha = mode == BlendMode::Replace;
    for (i, (dst, src)) in acc.px.iter_mut().zip(content).enumerate() {
        let a = if ignore_alpha { 1.0 } else { src[3] } * blend.opacity * mask_at(i);
        composite(dst, [src[0], src[1], src[2]], a, mode, acc.target);
    }
}
