//! CPU compositor: per-tile evaluation of a texture set's layer tree (ADR-0003).
//!
//! Output is one composite [`TiledImage`] per texture-set channel (f16): 3-component channels
//! (base color, emissive, normal) are stored as 4 components `rgb + alpha`, scalar channels as 1.
//! Normals are stored encoded (0..1). Tiles are evaluated independently (rayon), so evaluation
//! is deterministic and any subset of tiles can be refreshed — [`CompositeCache`] uses this to
//! re-evaluate only tiles touched since the last update.

pub mod blend;
pub mod cache;
pub mod strength;
pub mod mask;
pub mod tile;
mod util;

use std::collections::BTreeMap;

use rayon::prelude::*;
use subcraft_core::ChannelKind;
use subcraft_doc::{ResourceStore, TextureSet};
use subcraft_image::{TILE_TEXELS, TiledImage};
use subcraft_materials::{MeshMaps, SampleContext, TextureSetGeometry, TileGeometry};

pub use blend::{BlendTarget, blend_fn, composite};
pub use cache::CompositeCache;
pub use tile::evaluate_tile;

/// Everything outside the [`TextureSet`] that evaluation reads.
#[derive(Clone, Copy)]
pub struct EvalContext<'a> {
    /// Geometry of the texture set at its resolution.
    pub geometry: &'a TextureSetGeometry,
    pub resources: &'a ResourceStore,
    pub mesh_maps: &'a MeshMaps,
}

/// Per-tile view of the evaluation inputs.
pub struct TileContext<'a> {
    pub geo: &'a TileGeometry,
    pub set_geometry: &'a TextureSetGeometry,
    pub sample: SampleContext<'a>,
}

/// Number of stored components of a channel's composite image.
pub fn composite_comps(kind: ChannelKind) -> usize {
    if kind.components() == 3 { 4 } else { 1 }
}

/// Composited channel images of one texture set.
#[derive(Clone, Debug, PartialEq)]
pub struct Composite {
    pub resolution: u32,
    pub channels: BTreeMap<ChannelKind, TiledImage>,
}

impl Composite {
    /// Allocate images for every channel of `ts`, filled with the channel defaults.
    pub fn new(ts: &TextureSet) -> Self {
        let r = ts.resolution;
        let channels = ts
            .channel_kinds()
            .map(|k| {
                let d = k.default_value().0;
                (k, TiledImage::new(r, r, composite_comps(k), [d[0], d[1], d[2], 1.0]))
            })
            .collect();
        Self { resolution: r, channels }
    }

    pub fn channel(&self, kind: ChannelKind) -> Option<&TiledImage> {
        self.channels.get(&kind)
    }

    pub fn tile_count(&self) -> usize {
        let t = self.resolution.div_ceil(subcraft_image::TILE_SIZE) as usize;
        t * t
    }

    /// Evaluate `tiles` of `ts` in parallel and store the results.
    pub fn evaluate_tiles(&mut self, ts: &TextureSet, ctx: &EvalContext, tiles: &[usize]) {
        assert_eq!(ctx.geometry.resolution, ts.resolution, "geometry resolution must match the texture set");
        let results: Vec<(usize, Vec<(ChannelKind, Vec<[f32; 4]>)>)> = tiles
            .par_iter()
            .map(|&t| {
                let geo = ctx.geometry.tile(t);
                let tc = TileContext {
                    geo: &geo,
                    set_geometry: ctx.geometry,
                    sample: SampleContext { resources: ctx.resources, mesh_maps: ctx.mesh_maps },
                };
                (t, evaluate_tile(ts, &tc))
            })
            .collect();
        let mut buf = Vec::with_capacity(TILE_TEXELS * 4);
        for (t, channels) in results {
            for (kind, px) in channels {
                let Some(img) = self.channels.get_mut(&kind) else { continue };
                let comps = img.comps();
                buf.clear();
                for p in &px {
                    buf.extend_from_slice(&p[..comps]);
                }
                img.write_tile(t, &buf);
            }
        }
    }
}

/// Evaluate every tile of a texture set.
pub fn composite_full(ts: &TextureSet, ctx: &EvalContext) -> Composite {
    let mut c = Composite::new(ts);
    let all: Vec<usize> = (0..c.tile_count()).collect();
    c.evaluate_tiles(ts, ctx, &all);
    c
}
