//! Material filters (`ContentEffectKind::Material`) on one channel of one tile.
//!
//! The noise comes from `subcraft_materials::matfx` (cached per tile, shared by every channel);
//! this file adds what needs the compositor: the edge wear mask (the Edge wear generator, so the
//! damage follows the same owner-approved model), the baked occlusion, and the blend onto the
//! content below.
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_doc::{ChannelBlend, Generator, GeneratorKind, MaskEffectKind, MaterialFilter};
use subcraft_image::TILE_TEXELS;
use subcraft_materials::matfx::{self, PATTERN, Surface};

use crate::TileContext;
use crate::blend::{BlendTarget, composite};

/// Detail Edge Wear: micro convexity (0..1, see `stages::micro_detail_tile`) times this is the
/// wear a ridge asks for at Painted detail 1. A soft hammered ridge measures about 0.25, a crisp
/// painted line saturates.
const DETAIL_WEAR_GAIN: f32 = 4.0;
/// Half width of the fade around the wear threshold (1 - Wear level).
const DETAIL_WEAR_BAND: f32 = 0.15;

/// The Edge wear generator a filter places its damage with.
fn edge_generator(filter: &MaterialFilter) -> Option<Generator> {
    let mut g = Generator::new(GeneratorKind::Edges);
    let (level, distance, scale, seed) = match *filter {
        MaterialFilter::EdgeDamages { amount, spread, scale, seed, .. } => (amount, spread, scale, seed),
        // A thin line: a third of the default Edge Damages reach.
        MaterialFilter::DetailEdgeWear { level, grunge_scale, seed, .. } => (level, 0.8, grunge_scale * 2.0, seed),
        // Rust and peeling only lean on the edges: a fixed, generous band.
        MaterialFilter::RustWeathering { edges, scale, seed, .. } if edges > 0.0 => (0.6, 3.5, scale * 5.0, seed),
        MaterialFilter::PeelingPaint { edge_bias, scale, seed, .. } if edge_bias > 0.0 => (0.65, 4.0, scale * 4.0, seed),
        _ => return None,
    };
    g.level = level.clamp(0.0, 1.0);
    g.noise.scale = scale.clamp(0.5, 256.0);
    g.noise.seed = seed;
    if let Some(s) = g.edge_spread.as_mut() {
        s.distance = distance.clamp(0.0, 12.5);
    }
    g.micro_details = filter.micro_details();
    Some(g)
}

fn edge_mask(filter: &MaterialFilter, tc: &TileContext, details: Option<&[[f32; 4]]>) -> Option<Vec<f32>> {
    let g = edge_generator(filter)?;
    let zeros = vec![0.0f32; TILE_TEXELS];
    let mut out = vec![[0.0f32; 4]; TILE_TEXELS];
    crate::mask::evaluate_effect(&MaskEffectKind::Generator(g), tc, &zeros, &mut out, None, details);
    Some(out.iter().map(|v| v[0]).collect())
}

fn occlusion(filter: &MaterialFilter, tc: &TileContext, details: Option<&[[f32; 4]]>) -> Option<Vec<f32>> {
    // HBAO: the micro occlusion of the relief below (1 = open).
    if let MaterialFilter::Hbao { .. } = *filter {
        return details.map(|d| d.iter().map(|d| 1.0 - d[1]).collect());
    }
    let MaterialFilter::RustWeathering { cavities, .. } = *filter else { return None };
    if cavities <= 0.0 || tc.sample.mesh_maps.get(MeshMapKind::AmbientOcclusion).is_none() {
        return None;
    }
    let mut map = vec![[0.0f32; 3]; TILE_TEXELS];
    tc.sample.mesh_maps.read_tile(MeshMapKind::AmbientOcclusion, tc.geo, &mut map);
    Some(map.iter().map(|v| 1.0 - v[0]).collect())
}

/// Lay `value` over a straight `(value, alpha)` texel with coverage `a`: the filter adds its own
/// coverage where the content below has none (a fill without that channel).
#[inline]
fn over(src: [f32; 4], value: [f32; 3], a: f32) -> [f32; 4] {
    let a = a.clamp(0.0, 1.0);
    let alpha = src[3] + (1.0 - src[3]) * a;
    if alpha <= 1e-12 {
        return [0.0; 4];
    }
    let mut out = [0.0, 0.0, 0.0, alpha];
    for c in 0..3 {
        out[c] = (src[c] * src[3] * (1.0 - a) + value[c] * a) / alpha;
    }
    out
}

/// Apply `filter` to `content` (the channel `kind` of the content below the effect) for one
/// tile. `metallic` is the Metallic content at the same point of the stack, for filters that
/// read it (`None`: treated as 0, a dielectric). `details` are the micro details of the Height
/// and Normal at that point (`[curvature, occlusion, ..]`), for [`MaterialFilter::micro_details`].
pub(crate) fn apply(filter: &MaterialFilter, kind: ChannelKind, blend: ChannelBlend, content: &mut [[f32; 4]], tc: &TileContext, metallic: Option<&[[f32; 4]]>, details: Option<&[[f32; 4]]>) {
    if !filter.valid() || !filter.writes(kind) || !blend.is_active() {
        return;
    }
    let opacity = blend.opacity.clamp(0.0, 1.0);
    let mode = crate::tile::effective_mode(blend.mode);
    let target = BlendTarget::for_channel(kind);
    // Blend the filter's result for a texel onto what was there, by the effect's mode and opacity.
    let finish = |src: [f32; 4], new: [f32; 4]| -> [f32; 4] {
        let mut mixed = [src[0], src[1], src[2], 1.0];
        composite(&mut mixed, [new[0], new[1], new[2]], 1.0, mode, target);
        let alpha = src[3] + (new[3] - src[3]) * opacity;
        if alpha <= 1e-12 {
            return [0.0; 4];
        }
        let mut out = [0.0, 0.0, 0.0, alpha];
        for c in 0..3 {
            out[c] = (src[c] * src[3] * (1.0 - opacity) + mixed[c] * new[3] * opacity) / alpha;
        }
        out
    };
    if let MaterialFilter::PbrValidate { strict, flag_partial_metal, flat } = *filter {
        for (i, px) in content.iter_mut().enumerate() {
            let new = match kind {
                ChannelKind::BaseColor => {
                    let m = metallic.map_or(0.0, |m| m[i][0] * m[i][3]);
                    let base = [px[0], px[1], px[2]];
                    let c = matfx::validate_color(base, matfx::validate(base, m, strict, flag_partial_metal));
                    [c[0], c[1], c[2], px[3]]
                }
                ChannelKind::Roughness if flat => [1.0, 1.0, 1.0, 1.0],
                ChannelKind::Metallic if flat => [0.0, 0.0, 0.0, 1.0],
                _ => continue,
            };
            *px = finish(*px, new);
        }
        return;
    }
    if tc.geo.is_empty() {
        return;
    }
    let pattern = matfx::pattern_tile(filter, tc.geo);
    // Shut Line: the extra input is the blurred coverage of the layer's strokes.
    let (strokes, details) = if filter.coverage_blur().is_some() { (details, None) } else { (None, details) };
    let edges = match strokes {
        Some(c) => Some(c.iter().map(|c| c[3]).collect()),
        None => edge_mask(filter, tc, details),
    };
    // Detail Edge Wear: ridges of the Height / Normal detail below wear like mesh edges.
    let edges = match (*filter, details, edges) {
        (MaterialFilter::DetailEdgeWear { level, detail, .. }, Some(d), Some(mut e)) if detail > 0.0 => {
            let lo = (1.0 - level - DETAIL_WEAR_BAND).max(0.02);
            for (e, d) in e.iter_mut().zip(d) {
                let ridge = ((d[0] - 0.5) * 2.0 * DETAIL_WEAR_GAIN * detail).clamp(0.0, 1.0);
                let t = ((ridge - lo) / (2.0 * DETAIL_WEAR_BAND)).clamp(0.0, 1.0);
                *e = e.max(t * t * (3.0 - 2.0 * t));
            }
            Some(e)
        }
        (_, _, e) => e,
    };
    let occluded = occlusion(filter, tc, details);
    let faces = matches!(filter, MaterialFilter::WaterDrops { .. });
    for (i, px) in content.iter_mut().enumerate() {
        let surface = Surface { edge: edges.as_ref().map_or(0.0, |e| e[i]), occlusion: occluded.as_ref().map_or(0.0, |o| o[i]), up: if faces { tc.geo.normal(i)[1] } else { 0.0 } };
        let r = matfx::respond(filter, &pattern[i * PATTERN..(i + 1) * PATTERN], surface);
        let src = *px;
        // What the filter works on: the content below with its coverage scaled by `keep`.
        let kept = [src[0], src[1], src[2], src[3] * r.keep.clamp(0.0, 1.0)];
        let new = match kind {
            ChannelKind::BaseColor => {
                let gained = [kept[0] * r.color_gain, kept[1] * r.color_gain, kept[2] * r.color_gain, kept[3]];
                over(gained, r.color, r.color_alpha)
            }
            ChannelKind::Roughness => {
                let b = (kept[0] + r.roughness_add).clamp(0.0, 1.0);
                over([b, b, b, kept[3]], [r.roughness; 3], r.roughness_alpha)
            }
            ChannelKind::Metallic => over(kept, [r.metallic; 3], r.metallic_alpha),
            ChannelKind::AmbientOcclusion => {
                if r.occlusion == 0.0 {
                    continue;
                }
                // Uncovered Ambient Occlusion is open (1).
                let v = (src[0] * src[3] + (1.0 - src[3])) * (1.0 - r.occlusion);
                [v, v, v, 1.0]
            }
            ChannelKind::Height => {
                if r.height == 0.0 && r.keep == 1.0 {
                    continue;
                }
                // Height adds: the relief sits on whatever height is below, covered or not.
                let h = kept[0] * kept[3] + r.height;
                if r.height == 0.0 { [kept[0], kept[0], kept[0], kept[3]] } else { [h, h, h, 1.0] }
            }
            _ => continue,
        };
        *px = finish(src, new);
    }
}

/// Whether any visible layer carries a material filter that reads another channel (PBR Validate
/// reads Metallic for the Base Color): those need the stage graph and whole-set invalidation.
pub(crate) fn reads_across(layers: &[std::sync::Arc<subcraft_doc::Layer>]) -> bool {
    use subcraft_doc::{ContentEffectKind, LayerContent};
    layers.iter().filter(|l| l.visible).any(|l| {
        l.content_effects.iter().any(|e| e.visible && matches!(e.kind, ContentEffectKind::Material(m) if m.reads_metallic() && m.valid()))
            || matches!(&l.content, LayerContent::Group(g) if reads_across(&g.children))
    })
}
