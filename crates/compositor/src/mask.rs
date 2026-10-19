//! Mask evaluation: base raster followed by the effect stack.
//!
//! Each visible effect produces a value `e` and coverage `ea` per texel and is composited onto
//! the running mask with its blend mode and opacity (`m = lerp(m, B(m, e), ea·opacity)`).

use glam::Vec3;
use subcraft_core::MeshMapKind;
use subcraft_doc::{Generator, GeneratorKind, Mask, MaskEffectKind, Projection, ProjectionMode};
use subcraft_image::TILE_TEXELS;
use subcraft_image::Wrap;
use subcraft_materials::sample::{procedural_tile, sample_projected};

use crate::TileContext;
use crate::blend::{BlendTarget, composite};
use crate::util::read_tile_rgba;

/// Mask values (0..1) for every texel of the tile, or `None` when the mask is absent/disabled.
pub fn eval_mask(mask: Option<&Mask>, tc: &TileContext) -> Option<Vec<f32>> {
    let mask = mask.filter(|m| m.enabled)?;
    let mut px = vec![[0.0f32; 4]; TILE_TEXELS];
    read_tile_rgba(&mask.base, tc.geo, &mut px);
    let mut m: Vec<f32> = px.iter().map(|p| p[0]).collect();
    let mut e = vec![[0.0f32; 4]; TILE_TEXELS];
    for effect in mask.effects.iter().filter(|e| e.visible && e.opacity > 0.0) {
        // e[i] = [value, alpha, _, _]
        match &effect.kind {
            MaskEffectKind::Paint { image } => {
                read_tile_rgba(image, tc.geo, &mut e);
            }
            MaskEffectKind::Fill { value, procedural, projection } => {
                let field = procedural.as_ref().map(|p| procedural_tile(p, projection, tc.geo));
                for (i, o) in e.iter_mut().enumerate() {
                    let v = field.as_ref().map_or(*value, |f| f[i]);
                    *o = [v, 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Generator(g) => eval_generator(g, tc, &mut e),
            MaskEffectKind::Levels(l) => {
                for (o, &v) in e.iter_mut().zip(&m) {
                    *o = [l.apply(v), 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Filter(filter) => {
                let usable = filter.valid() && filter.supports_mask();
                for (o, &value) in e.iter_mut().zip(&m) {
                    *o = [if usable { subcraft_materials::filter::apply_mask(*filter, value) } else { value }, 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Invert => {
                for (o, &v) in e.iter_mut().zip(&m) {
                    *o = [1.0 - v, 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Image { resource, projection, invert } => eval_image(*resource, projection, *invert, tc, &mut e),
        }
        for (mv, ev) in m.iter_mut().zip(&e) {
            let mut dst = [*mv, *mv, *mv, 1.0];
            composite(&mut dst, [ev[0]; 3], ev[1] * effect.opacity, effect.blend, BlendTarget::MASK);
            *mv = dst[0];
        }
    }
    for v in m.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }
    Some(m)
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Generator coverage: a mesh-derived raw value thresholded at `level` (optionally broken up by
/// noise) with a `softness`-wide transition.
fn eval_generator(g: &Generator, tc: &TileContext, out: &mut [[f32; 4]]) {
    if g.kind == GeneratorKind::Edges && let Some(spread) = g.edge_spread {
        eval_edge_spread(g, spread, tc, out);
        return;
    }
    let geo = tc.geo;
    let mut map = vec![[0.0f32; 3]; TILE_TEXELS];
    let map_kind = match g.kind {
        GeneratorKind::Edges => Some(MeshMapKind::Curvature),
        GeneratorKind::Cavity => Some(MeshMapKind::AmbientOcclusion),
        GeneratorKind::MeshMap(k) => Some(k),
        GeneratorKind::Directional | GeneratorKind::PositionGradient => None,
    };
    if let Some(k) = map_kind {
        tc.sample.mesh_maps.read_tile(k, geo, &mut map);
    }
    let dir = Vec3::from_array(g.direction).normalize_or(Vec3::Y);
    let set = tc.set_geometry;
    let ext_y = set.bounds.extent().y.max(1e-12);
    let half = g.softness.max(1e-4) * 0.5;
    // Breakup noise is projected in 3D (triplanar on object position), never in UV: UV noise
    // jumps at every seam and would make wear follow the islands instead of the shape.
    let noise_proj = Projection { mode: ProjectionMode::Triplanar, triplanar_sharpness: 4.0, ..Projection::default() };
    let noise = (g.noise_amount > 0.0).then(|| if g.organic_breakup {
        subcraft_materials::sample::organic_tile(&g.noise, geo)
    } else { procedural_tile(&g.noise, &noise_proj, geo) });
    for (i, o) in out.iter_mut().enumerate() {
        let raw = match g.kind {
            GeneratorKind::Edges | GeneratorKind::MeshMap(_) => map[i][0],
            GeneratorKind::Cavity => 1.0 - map[i][0],
            GeneratorKind::Directional => Vec3::from_array(geo.normal(i)).dot(dir) * 0.5 + 0.5,
            GeneratorKind::PositionGradient => (geo.position(i)[1] * set.max_extent / ext_y).clamp(0.0, 1.0),
        };
        let level = if let Some(noise) = &noise {
            g.level + (noise[i] - 0.5) * g.noise_amount
        } else {
            g.level
        };
        let v = smoothstep(level - half, level + half, raw);
        *o = [if g.invert { 1.0 - v } else { v }, 1.0, 0.0, 0.0];
    }
}

fn eval_edge_spread(g: &Generator, spread: subcraft_doc::EdgeSpread, tc: &TileContext, out: &mut [[f32; 4]]) {
    let distances = tc.set_geometry.edge_distance_tile(tc.geo);
    // Variation changes the boundary even at zero Breakup. Breakup additionally erodes patches;
    // both share one cached non-periodic volume field and the existing Pattern size/Seed.
    let noise = subcraft_materials::sample::organic_tile(&g.noise, tc.geo);
    let mut curvature = vec![[0.0; 3]; TILE_TEXELS];
    tc.sample.mesh_maps.read_tile(MeshMapKind::Curvature, tc.geo, &mut curvature);
    // Spread is a physical reach, independent of the baked-detail threshold. Multiplying by
    // Level made even maximum Spread almost disappear at the owner's Level 0.065 setting.
    let radius = spread.distance.clamp(0.0, 12.5) * 0.01;
    let variation = spread.variation.clamp(0.0, 1.0);
    let breakup = g.noise_amount.clamp(0.0, 1.0);
    let softness = g.softness.clamp(0.02, 1.0);
    for (i, o) in out.iter_mut().enumerate() {
        let n = noise[i];
        // Fractal volume values cluster near 0.5. Expand their useful range so Variation has
        // visible width changes, rather than an almost straight band at small Spread values.
        let boundary = ((n - 0.5) * 3.0).clamp(-0.5, 0.5);
        let reach = radius * (1.0 + variation * boundary * 1.8) * (1.0 - breakup * 0.7 * (1.0 - n));
        let geometric = if reach > 0.0 { 1.0 - smoothstep(reach * (1.0 - softness), reach, distances[i]) } else { 0.0 };
        // Keep baked rounded/high-poly detail, but explicitly exclude the neutral flat baseline.
        // Geometric sources are independent of contact curvature from other crate components.
        let convex = ((curvature[i][0] - 0.5) * 2.0).clamp(0.0, 1.0);
        let threshold = 1.0 - g.level + (n - 0.5) * breakup + boundary * variation * 0.5;
        let baked = if convex > 1e-4 && g.level > 0.0 {
            smoothstep(threshold - softness * 0.5, threshold + softness * 0.5, convex)
        } else { 0.0 };
        let holes = if breakup > 0.0 { smoothstep(breakup * 0.65 - 0.2, breakup * 0.65, n) } else { 1.0 };
        let v = geometric.max(baked) * holes;
        *o = [if g.invert { 1.0 - v } else { v }, 1.0, 0.0, 0.0];
    }
}

/// Image mask effect. With alpha: value 1 (0 inverted) with coverage = alpha, so a logo on a
/// transparent background stamps onto the mask. Without alpha: value = Rec.709 luminance of the
/// raw (not color-transformed) pixels. A missing resource contributes nothing.
fn eval_image(resource: subcraft_core::ResourceId, projection: &Projection, invert: bool, tc: &TileContext, e: &mut [[f32; 4]]) {
    let Some(bmp) = tc.sample.resources.bitmap(resource) else {
        e.fill([0.0; 4]);
        return;
    };
    let has_alpha = bmp.channels == 2 || bmp.channels == 4;
    let geo = tc.geo;
    for (i, o) in e.iter_mut().enumerate() {
        let s = sample_projected(projection, geo.uv(i), geo.position(i), geo.normal(i), |p| bmp.sample(p[0], p[1], Wrap::Repeat, false));
        let (v, a) = if has_alpha { (1.0, s[3]) } else { (0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2], 1.0) };
        *o = [if invert { 1.0 - v } else { v }, a.clamp(0.0, 1.0), 0.0, 0.0];
    }
}
