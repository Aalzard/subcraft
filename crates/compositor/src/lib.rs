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
pub mod worker;

use std::collections::BTreeMap;

use rayon::prelude::*;
use subcraft_core::ChannelKind;
use subcraft_doc::{ResourceStore, TextureSet};
use half::f16;
use half::slice::HalfFloatSliceExt;
use subcraft_image::{TILE_TEXELS, TileSlot, TiledImage, slot_data_mut};
use subcraft_materials::{MeshMaps, SampleContext, TextureSetGeometry, TileGeometry};

pub use blend::{BlendTarget, blend_fn, composite};
pub use cache::CompositeCache;
pub use tile::evaluate_tile;
pub use worker::{CompositeWorker, Published, SetSnapshot, Snapshot};

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
    pub albedo_filter: subcraft_image::TextureFilter,
    pub geo: &'a TileGeometry,
    pub set_geometry: &'a TextureSetGeometry,
    pub sample: SampleContext<'a>,
}

/// Number of stored components of a channel's composite image.
pub fn composite_comps(kind: ChannelKind) -> usize {
    if kind.components() == 3 { 4 } else { 1 }
}

/// Per channel, the composite tiles whose texels changed (sorted).
pub type ChangedTiles = BTreeMap<ChannelKind, Vec<usize>>;

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

    /// Evaluate `tiles` of `ts` in parallel and store the results. Each worker writes its own
    /// tiles' slots directly (no collected intermediate copies, no serial f16 encoding pass).
    ///
    /// Returns, per channel, the tiles whose stored texels actually changed (sorted): a tile that
    /// re-evaluates to identical bits keeps its storage, so callers upload only real changes.
    pub fn evaluate_tiles(&mut self, ts: &TextureSet, ctx: &EvalContext, tiles: &[usize]) -> ChangedTiles {
        assert_eq!(ctx.geometry.resolution, ts.resolution, "geometry resolution must match the texture set");
        let count = self.tile_count();
        let mut wanted = vec![false; count];
        for &t in tiles {
            wanted[t] = true;
        }
        // Per wanted tile: (channel, components, fill, slot) for every composite channel.
        let mut per_tile: Vec<Vec<(ChannelKind, usize, [f32; 4], &mut TileSlot)>> = (0..count).map(|_| Vec::new()).collect();
        for (&kind, img) in self.channels.iter_mut() {
            let slots = img.slots_mut();
            let (comps, fill) = (slots.comps, slots.fill);
            for (t, slot) in slots.slots.iter_mut().enumerate().filter(|(t, _)| wanted[*t]) {
                per_tile[t].push((kind, comps, fill, slot));
            }
        }
        // Only the wanted tiles, one task each: filtering after rayon splits the whole index range
        // would hand a budgeted batch (consecutive tiles) to one or two threads.
        let work: Vec<_> = per_tile.into_iter().enumerate().filter(|(t, _)| wanted[*t]).collect();
        let changed: Vec<(usize, Vec<ChannelKind>)> = work
            .into_par_iter()
            .with_max_len(1)
            .map(|(t, mut slots)| {
                let geo = ctx.geometry.tile(t);
                let tc = TileContext {
                    albedo_filter: ts.albedo_filter,
                    geo: &geo,
                    set_geometry: ctx.geometry,
                    sample: SampleContext { resources: ctx.resources, mesh_maps: ctx.mesh_maps },
                };
                let mut buf = vec![0.0f32; TILE_TEXELS * 4];
                let mut encoded = vec![f16::ZERO; TILE_TEXELS * 4];
                let mut changed = Vec::new();
                for (kind, px) in evaluate_tile(ts, &tc) {
                    let Some((_, comps, fill, slot)) = slots.iter_mut().find(|s| s.0 == kind) else { continue };
                    let n = TILE_TEXELS * *comps;
                    for (o, p) in buf.chunks_exact_mut(*comps).zip(&px) {
                        o.copy_from_slice(&p[..*comps]);
                    }
                    encoded[..n].convert_from_f32_slice(&buf[..n]);
                    if slot.as_ref().is_some_and(|tile| tile.data[..] == encoded[..n]) {
                        continue; // identical texels: keep the storage, nothing to upload
                    }
                    slot_data_mut(slot, *comps, fill).copy_from_slice(&encoded[..n]);
                    changed.push(kind);
                }
                (t, changed)
            })
            .collect();
        let mut out = ChangedTiles::new();
        for (t, kinds) in changed {
            for kind in kinds {
                out.entry(kind).or_default().push(t);
            }
        }
        out
    }
}

/// Evaluate every tile of a texture set.
pub fn composite_full(ts: &TextureSet, ctx: &EvalContext) -> Composite {
    let mut c = Composite::new(ts);
    let all: Vec<usize> = (0..c.tile_count()).collect();
    c.evaluate_tiles(ts, ctx, &all);
    c
}
