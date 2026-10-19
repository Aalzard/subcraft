//! Per-tile fill-source evaluation.
//!
//! A [`FillSource`] is evaluated for all texels of one 64² tile at once, given the tile's
//! [`TileGeometry`]. Output texels are `[v0, v1, v2, alpha]` (straight alpha, linear values for
//! color channels; scalar channels use `v0`).

pub mod field;
pub mod geometry;
pub mod mesh_maps;
pub mod projection;

use glam::Vec2;
use subcraft_doc::{FillSource, ProceduralParams, Projection, ResourceStore};
use subcraft_image::{TILE_TEXELS, Wrap};

pub use geometry::{GeometryCache, TextureSetGeometry, TileGeometry};
pub use mesh_maps::MeshMaps;
pub use projection::{planar_coords, sample_projected, triplanar_weights};

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
/// or masks). A missing bitmap resource evaluates fully transparent.
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
            for (i, o) in out.iter_mut().enumerate() {
                *o = sample_projected(projection, geo.uv(i), geo.position(i), geo.normal(i), |p| {
                    bmp.sample_filtered(p[0], p[1], Wrap::Repeat, linearize, filter)
                });
            }
        }
        FillSource::Procedural { params, low, high } => {
            let field = procedural_tile(params, projection, geo);
            for (o, &t) in out.iter_mut().zip(field.iter()) {
                let mut v = [0.0; 4];
                for c in 0..3 {
                    v[c] = low.0[c] + (high.0[c] - low.0[c]) * t;
                }
                v[3] = 1.0;
                *o = v;
            }
        }
        FillSource::MeshMap { map } => {
            let mut buf = vec![[0.0f32; 3]; TILE_TEXELS];
            ctx.mesh_maps.read_tile(*map, geo, &mut buf);
            for (o, v) in out.iter_mut().zip(&buf) {
                *o = [v[0], v[1], v[2], 1.0];
            }
        }
    }
}

/// Grayscale procedural values of every texel of a tile, cached on the tile geometry: exactly
/// [`eval_procedural`] for each texel, computed once per parameter set.
pub fn procedural_tile(params: &ProceduralParams, projection: &Projection, geo: &TileGeometry) -> std::sync::Arc<field::Field> {
    geo.fields.get_or_compute(field::FieldKey::new(params, projection), || (0..TILE_TEXELS).map(|i| eval_procedural(params, projection, geo, i)).collect())
}

pub fn organic_tile(params: &ProceduralParams, geo: &TileGeometry) -> std::sync::Arc<field::Field> {
    geo.fields.get_or_compute(field::FieldKey::organic(params), || (0..TILE_TEXELS).map(|i| crate::organic::eval(params, geo.position(i))).collect())
}

/// Grayscale procedural value of texel `i` through a projection.
#[inline]
pub fn eval_procedural(params: &ProceduralParams, projection: &Projection, geo: &TileGeometry, i: usize) -> f32 {
    sample_projected(projection, geo.uv(i), geo.position(i), geo.normal(i), |p| {
        [pattern::eval(params, Vec2::new(p[0], p[1])), 0.0, 0.0, 0.0]
    })[0]
}

#[cfg(test)]
mod tests;
