//! Mask evaluation: base raster followed by the effect stack.
//!
//! Each visible effect produces a value `e` and coverage `ea` per texel and is composited onto
//! the running mask with its blend mode and opacity (`m = lerp(m, B(m, e), ea·opacity)`).

use glam::Vec3;
use subcraft_core::MeshMapKind;
use subcraft_doc::{Generator, GeneratorKind, Mask, MaskEffectKind, Projection};
use subcraft_image::TILE_TEXELS;
use subcraft_materials::sample::eval_procedural;

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
                for (i, o) in e.iter_mut().enumerate() {
                    let v = procedural.as_ref().map_or(*value, |p| eval_procedural(p, projection, tc.geo, i));
                    *o = [v, 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Generator(g) => eval_generator(g, tc, &mut e),
            MaskEffectKind::Levels(l) => {
                for (o, &v) in e.iter_mut().zip(&m) {
                    *o = [l.apply(v), 1.0, 0.0, 0.0];
                }
            }
            MaskEffectKind::Invert => {
                for (o, &v) in e.iter_mut().zip(&m) {
                    *o = [1.0 - v, 1.0, 0.0, 0.0];
                }
            }
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
    let noise_proj = Projection::default();
    for (i, o) in out.iter_mut().enumerate() {
        let raw = match g.kind {
            GeneratorKind::Edges | GeneratorKind::MeshMap(_) => map[i][0],
            GeneratorKind::Cavity => 1.0 - map[i][0],
            GeneratorKind::Directional => Vec3::from_array(geo.normal(i)).dot(dir) * 0.5 + 0.5,
            GeneratorKind::PositionGradient => (geo.position(i)[1] * set.max_extent / ext_y).clamp(0.0, 1.0),
        };
        let level = if g.noise_amount > 0.0 {
            g.level + (eval_procedural(&g.noise, &noise_proj, geo, i) - 0.5) * g.noise_amount
        } else {
            g.level
        };
        let v = smoothstep(level - half, level + half, raw);
        *o = [if g.invert { 1.0 - v } else { v }, 1.0, 0.0, 0.0];
    }
}
