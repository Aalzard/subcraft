//! Evaluation of a layer tree for one tile.
//!
//! Every channel keeps a running accumulation of straight `(value, alpha)` texels. The root starts
//! at the channel's default value with alpha 1; isolated folders start transparent; passthrough
//! folders let their children act directly on the accumulation below and then lerp the result
//! by folder opacity × mask.

use std::sync::Arc;

use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, Layer, LayerContent, TextureSet};
use subcraft_image::TILE_TEXELS;
use subcraft_materials::eval_fill_tile;

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
    let mask = eval_mask(layer.mask.as_ref(), tc);
    let mask_at = |i: usize| mask.as_ref().map_or(1.0, |m| m[i]);
    let mut content = vec![[0.0f32; 4]; TILE_TEXELS];

    match &layer.content {
        LayerContent::Paint(paint) => {
            for (acc, _) in accs.iter_mut().zip(&live).filter(|(_, l)| **l) {
                let Some(img) = paint.channels.get(&acc.kind) else { continue };
                read_tile_rgba(img, tc.geo, &mut content);
                // Paint images hold `components` values followed by alpha.
                let n = acc.kind.components();
                for px in content.iter_mut() {
                    let a = px[n];
                    *px = if n == 1 { [px[0], px[0], px[0], a] } else { [px[0], px[1], px[2], a] };
                }
                blend_content(acc, &content, layer, &mask_at);
            }
        }
        LayerContent::Fill(fill) => {
            for (acc, _) in accs.iter_mut().zip(&live).filter(|(_, l)| **l) {
                let Some(ch) = fill.channels.get(&acc.kind).filter(|c| c.enabled) else { continue };
                eval_fill_tile(&ch.source, acc.kind.is_color(), &fill.projection, tc.geo, &tc.sample, &mut content);
                if acc.kind.components() == 1 {
                    for px in content.iter_mut() {
                        *px = [px[0], px[0], px[0], px[3]];
                    }
                }
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
            for ((acc, inner), _) in accs.iter_mut().zip(child).zip(&live).filter(|(_, l)| **l) {
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
