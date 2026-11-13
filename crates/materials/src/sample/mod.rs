//! Per-tile fill-source evaluation.
//!
//! A [`FillSource`] is evaluated for all texels of one 64² tile at once, given the tile's
//! [`TileGeometry`]. Output texels are `[v0, v1, v2, alpha]` (straight alpha, linear values for
//! color channels; scalar channels use `v0`).

pub mod bitmap;
pub mod field;
pub mod geometry;
pub mod edge_distance;
pub mod mesh_maps;
pub mod projection;

use glam::Vec2;
use subcraft_doc::{FillSource, ProceduralParams, Projection, ResourceStore};
use subcraft_image::TILE_TEXELS;

pub use bitmap::{BitmapSampler, MipChain, bitmap_tile, coverage_tile, mip_chain, projection_coverage, projection_covers};
pub use geometry::{GeometryCache, TextureSetGeometry, TileGeometry};
pub use mesh_maps::MeshMaps;
pub use projection::{planar_coords, sample_projected, sample_projected_sc, triplanar_weights};

use crate::pattern;

/// External data fills may read.
#[derive(Clone, Copy)]
pub struct SampleContext<'a> {
    pub resources: &'a ResourceStore,
    pub mesh_maps: &'a MeshMaps,
}

/// Evaluate `source` for every texel of a tile into `out` (`TILE_TEXELS` entries).
///
/// `linearize` decodes sRGB bitmaps; pass `ChannelKind::is_color()` (never for data channels
/// or masks). A missing bitmap resource evaluates fully transparent. The projection's Filtering,
/// UV Wrap and Shape crop apply (see [`bitmap`]); texels it does not cover are transparent.
pub fn eval_fill_tile(
    source: &FillSource,
    linearize: bool,
    projection: &Projection,
    geo: &TileGeometry,
    ctx: &SampleContext,
    out: &mut [[f32; 4]],
) {
    eval_fill_tile_filtered(source, linearize, projection, geo, ctx, subcraft_image::TextureFilter::Bilinear, out);
}

pub fn eval_fill_tile_filtered(
    source: &FillSource, linearize: bool, projection: &Projection, geo: &TileGeometry,
    ctx: &SampleContext, filter: subcraft_image::TextureFilter, out: &mut [[f32; 4]],
) {
    debug_assert_eq!(out.len(), TILE_TEXELS);
    match source {
        FillSource::Uniform { value } => {
            out.fill([value.0[0], value.0[1], value.0[2], 1.0]);
        }
        FillSource::Bitmap { resource } => {
            let Some(bmp) = ctx.resources.bitmap(*resource) else {
                out.fill([0.0; 4]);
                return;
            };
            bitmap_tile(bmp, linearize, projection, filter == subcraft_image::TextureFilter::Nearest, geo, out);
        }
        FillSource::Procedural { params, low, high } if params.colored => {
            let fields: [_; 3] = std::array::from_fn(|c| procedural_tile(&ProceduralParams { seed: params.seed.wrapping_add(c as u32), colored: false, ..*params }, projection, geo));
            for (i, o) in out.iter_mut().enumerate() {
                for c in 0..3 {
                    o[c] = low.0[c] + (high.0[c] - low.0[c]) * fields[c][i];
                }
                o[3] = 1.0;
            }
            clip_alpha(projection, geo, out);
        }
        FillSource::Procedural { params, low, high } => {
            let field = procedural_tile(params, projection, geo);
            let wear = scratch_wear(params, geo, ctx);
            for (i, (o, &t)) in out.iter_mut().zip(field.iter()).enumerate() {
                let t = wear.as_ref().map_or(t, |w| t * w[i]);
                let mut v = [0.0; 4];
                for c in 0..3 {
                    v[c] = low.0[c] + (high.0[c] - low.0[c]) * t;
                }
                v[3] = 1.0;
                *o = v;
            }
            clip_alpha(projection, geo, out);
        }
        FillSource::MeshMap { map } => {
            let mut buf = vec![[0.0f32; 3]; TILE_TEXELS];
            ctx.mesh_maps.read_tile(*map, geo, &mut buf);
            for (o, v) in out.iter_mut().zip(&buf) {
                *o = [v[0], v[1], v[2], 1.0];
            }
        }
        // Anchors are read by the compositor's stage graph, which knows the whole texture set
        // (ADR-0045). Sampled alone, the source is transparent.
        FillSource::Anchor { .. } => out.fill([0.0; 4]),
    }
}

/// Make a procedural transparent where its projection does not cover the texel (UV Wrap other
/// than Repeat, Shape crop). Nothing to do at the defaults.
fn clip_alpha(projection: &Projection, geo: &TileGeometry, out: &mut [[f32; 4]]) {
    if let Some(cover) = coverage_tile(projection, geo) {
        for (o, c) in out.iter_mut().zip(cover) {
            o[3] *= c;
        }
    }
}

/// Per-texel scratch weights from the baked curvature map; `None` when the pattern is not a
/// wear-biased scratch generator or the map is not baked yet.
fn scratch_wear(params: &ProceduralParams, geo: &TileGeometry, ctx: &SampleContext) -> Option<Vec<f32>> {
    if params.kind != subcraft_doc::ProceduralKind::Scratch || params.scratch.wear <= 0.0 || ctx.mesh_maps.get(subcraft_core::MeshMapKind::Curvature).is_none() {
        return None;
    }
    let mut buf = vec![[0.0f32; 3]; TILE_TEXELS];
    ctx.mesh_maps.read_tile(subcraft_core::MeshMapKind::Curvature, geo, &mut buf);
    Some(buf.iter().map(|c| crate::scratch::wear_weight(c[0], params.scratch.wear)).collect())
}

/// Grayscale procedural values of every texel of a tile, cached on the tile geometry: exactly
/// [`eval_procedural`] for each texel, computed once per parameter set.
pub fn procedural_tile(params: &ProceduralParams, projection: &Projection, geo: &TileGeometry) -> std::sync::Arc<field::Field> {
    procedural_tile_at(params, projection, [0.0; 3], geo)
}

/// [`procedural_tile`] with the pattern moved by `offset` in object space (units of the model's
/// longest dimension). Only projections that read the position (triplanar, planar) move.
pub fn procedural_tile_at(params: &ProceduralParams, projection: &Projection, offset: [f32; 3], geo: &TileGeometry) -> std::sync::Arc<field::Field> {
    geo.fields.get_or_compute(field::FieldKey::new(params, projection).at(offset), || {
        let sc = projection.rotation_sin_cos();
        (0..TILE_TEXELS).map(|i| eval_procedural_sc(params, projection, sc, offset, geo, i)).collect()
    })
}

pub fn organic_tile(params: &ProceduralParams, geo: &TileGeometry) -> std::sync::Arc<field::Field> {
    organic_tile_at(params, [0.0; 3], geo)
}

/// [`organic_tile`] with the volume noise moved by `offset` (same units as [`procedural_tile_at`]).
pub fn organic_tile_at(params: &ProceduralParams, offset: [f32; 3], geo: &TileGeometry) -> std::sync::Arc<field::Field> {
    geo.fields.get_or_compute(field::FieldKey::organic(params).at(offset), || (0..TILE_TEXELS).map(|i| {
        let p = geo.position(i);
        crate::organic::eval(params, [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]])
    }).collect())
}

/// The bundled grunge map ([`crate::grunge`]) of every texel of a tile, moved by `offset` (same
/// units as [`procedural_tile_at`]), cached like the other fields.
pub fn grunge_tile_at(params: &ProceduralParams, offset: [f32; 3], geo: &TileGeometry) -> std::sync::Arc<field::Field> {
    geo.fields.get_or_compute(field::FieldKey::grunge(params).at(offset), || (0..TILE_TEXELS).map(|i| {
        let p = geo.position(i);
        crate::grunge::eval(params, [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]], geo.normal(i))
    }).collect())
}

/// Grayscale procedural value of texel `i` through a projection.
#[inline]
pub fn eval_procedural(params: &ProceduralParams, projection: &Projection, geo: &TileGeometry, i: usize) -> f32 {
    eval_procedural_at(params, projection, [0.0; 3], geo, i)
}

#[inline]
fn eval_procedural_at(params: &ProceduralParams, projection: &Projection, offset: [f32; 3], geo: &TileGeometry, i: usize) -> f32 {
    eval_procedural_sc(params, projection, projection.rotation_sin_cos(), offset, geo, i)
}

#[inline]
fn eval_procedural_sc(params: &ProceduralParams, projection: &Projection, sc: (f32, f32), offset: [f32; 3], geo: &TileGeometry, i: usize) -> f32 {
    let p = geo.position(i);
    sample_projected_sc(projection, sc, geo.uv(i), [p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]], geo.normal(i), |p| {
        [pattern::eval(params, Vec2::new(p[0], p[1])), 0.0, 0.0, 0.0]
    })[0]
}

#[cfg(test)]
mod tests;
