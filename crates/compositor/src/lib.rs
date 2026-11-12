//! CPU compositor: per-tile evaluation of a texture set's layer tree (ADR-0003).
//!
//! Output is one composite [`TiledImage`] per texture-set channel (f16): 3-component channels
//! (base color, emissive, normal) are stored as 4 components `rgb + alpha`, scalar channels as 1.
//! Normals are stored encoded (0..1). Tiles are evaluated independently (rayon), so evaluation
//! is deterministic and any subset of tiles can be refreshed — [`CompositeCache`] uses this to
//! re-evaluate only tiles touched since the last update.

pub mod blend;
pub mod cache;
pub mod gpu;
pub mod prefix;
pub mod strength;
pub mod mask;
pub mod tile;
mod util;
mod stages;
pub mod profile;
pub mod worker;

use std::collections::BTreeMap;

use rayon::prelude::*;
use subcraft_core::ChannelKind;
use subcraft_doc::{ResourceStore, TextureSet};
use half::f16;
use half::slice::HalfFloatSliceExt;
use std::sync::Arc;
use subcraft_image::{TILE_SIZE, TILE_TEXELS, Tile, TileSlot, TiledImage, slot_data_mut};
use subcraft_materials::{MeshMaps, SampleContext, TextureSetGeometry, TileGeometry};

pub use blend::{BlendTarget, blend_fn, composite};
pub use cache::CompositeCache;
pub use prefix::{PrefixCache, PrefixStats};
pub use tile::evaluate_tile;
pub use worker::{CompositeWorker, Published, SetSnapshot, Snapshot, preview_resolution};

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
                (k, TiledImage::new(r, r, composite_comps(k), [d[0], d[1], d[2], ts.base_alpha()]))
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
        self.evaluate_tiles_with_stages(ts, ctx, tiles, &stages::StageCache::default(), None)
    }

    pub(crate) fn evaluate_tiles_with_stages(
        &mut self,
        ts: &TextureSet,
        ctx: &EvalContext,
        tiles: &[usize],
        cache: &stages::StageCache,
        prefix: Option<(&PrefixCache, &[usize])>,
    ) -> ChangedTiles {
        // Optional GPU backend (ADR-0044): blends what it supports, the CPU evaluates the rest.
        // A tile with a cached layer prefix is cheaper to resume on the CPU than to re-blend whole.
        let (gpu_tiles, mut cpu_tiles): (Vec<usize>, Vec<usize>) = match prefix {
            Some((p, plan)) if !plan.is_empty() => {
                let kinds: Vec<ChannelKind> = ts.channel_kinds().collect();
                tiles.iter().partition(|&&t| !p.covers(&kinds, plan, t))
            }
            _ => (tiles.to_vec(), Vec::new()),
        };
        let (mut changed, left) = gpu::evaluate_tiles(self, ts, ctx, &gpu_tiles);
        cpu_tiles.extend(left);
        if cpu_tiles.is_empty() {
            return changed;
        }
        for (kind, tiles) in self.evaluate_tiles_inner(ts, ctx, &cpu_tiles, cache, prefix, 1.0, None) {
            let entry = changed.entry(kind).or_default();
            entry.extend(tiles);
            entry.sort_unstable();
        }
        changed
    }

    /// Quick preview of `full` at `ts.resolution` (a fraction of the authored one): layers are
    /// sampled by UV, spatial radii shrink with the resolution. Returns `None` when `cancel`
    /// fired (the partial result is dropped). Never touches the caches of the full evaluation.
    pub fn preview(ts: &TextureSet, ctx: &EvalContext, full_resolution: u32, cancel: &(dyn Fn() -> bool + Sync)) -> Option<Composite> {
        let mut c = Composite::new(ts);
        let all: Vec<usize> = (0..c.tile_count()).collect();
        let scale = ts.resolution as f32 / full_resolution as f32;
        c.evaluate_tiles_inner(ts, ctx, &all, &stages::StageCache::default(), None, scale, Some(cancel));
        (!cancel()).then_some(c)
    }

    /// Overwrite `tiles` of this (full resolution) composite with `low` stretched to fit (nearest
    /// texel), for a preview shown until the exact tiles arrive.
    pub fn overlay_preview(&mut self, low: &Composite, tiles: &[usize]) {
        let kinds: Vec<ChannelKind> = self.channels.keys().copied().collect();
        for kind in kinds {
            let (Some(img), Some(src)) = (self.channels.get(&kind), low.channels.get(&kind)) else { continue };
            let res = self.resolution;
            let fresh: Vec<(usize, TileSlot)> = tiles.par_iter().map(|&t| (t, Some(stretched_tile(src, img, res, t)))).collect();
            let img = self.channels.get_mut(&kind).expect("checked above");
            let slots = img.slots_mut();
            for (t, slot) in fresh {
                slots.slots[t] = slot;
            }
        }
    }

    /// Show the quick preview `low` on `tiles` of this displayed composite, but only where it
    /// says something changed: a tile (per channel) whose low-resolution texels equal those of
    /// `baseline` (the quick composite of the state `exact` holds) shows the `exact` tile, crisp,
    /// instead of the blocky stretch. Without a baseline every listed tile is stretched.
    ///
    /// Returns per channel the tiles whose displayed texels changed (what to upload) and the
    /// tiles that now differ from `exact` (still waiting for their exact evaluation).
    pub fn overlay_preview_against(&mut self, low: &Composite, tiles: &[usize], exact: &Composite, baseline: Option<&Composite>) -> (ChangedTiles, ChangedTiles) {
        let mut changed = ChangedTiles::new();
        let mut overlaid = ChangedTiles::new();
        let res = self.resolution;
        let kinds: Vec<ChannelKind> = self.channels.keys().copied().collect();
        for kind in kinds {
            let (Some(shown), Some(src), Some(exact_img)) = (self.channels.get(&kind), low.channels.get(&kind), exact.channels.get(&kind)) else { continue };
            let base = baseline.filter(|b| b.resolution == low.resolution).and_then(|b| b.channels.get(&kind));
            // (tile, new slot, differs from exact); `None` slot: the display already shows it.
            let work: Vec<(usize, Option<TileSlot>, bool)> = tiles
                .par_iter()
                .map(|&t| {
                    let unchanged = base.is_some_and(|b| same_low_block(src, b, res, t));
                    let target = if unchanged { exact_img.slot(t).clone() } else { Some(stretched_tile(src, shown, res, t)) };
                    let current = shown.slot(t);
                    let same = match (current, &target) {
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a.data == b.data,
                        (None, None) => true,
                        _ => false,
                    };
                    (t, (!same).then_some(target), !unchanged)
                })
                .collect();
            let img = self.channels.get_mut(&kind).expect("checked above");
            let slots = img.slots_mut();
            let (mut c, mut o) = (Vec::new(), Vec::new());
            for (t, slot, differs) in work {
                if let Some(slot) = slot {
                    slots.slots[t] = slot;
                    c.push(t);
                }
                if differs {
                    o.push(t);
                }
            }
            if !c.is_empty() {
                changed.insert(kind, c);
            }
            if !o.is_empty() {
                overlaid.insert(kind, o);
            }
        }
        (changed, overlaid)
    }

    /// Copy the listed tiles of every channel from `other` (same resolution).
    pub fn copy_tiles_from(&mut self, other: &Composite, tiles: &[usize]) {
        for (kind, img) in self.channels.iter_mut() {
            if let Some(src) = other.channels.get(kind) {
                for &t in tiles {
                    img.set_slot(t, src.slot(t).clone());
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_tiles_inner(
        &mut self,
        ts: &TextureSet,
        ctx: &EvalContext,
        tiles: &[usize],
        cache: &stages::StageCache,
        prefix: Option<(&PrefixCache, &[usize])>,
        scale: f32,
        cancel: Option<&(dyn Fn() -> bool + Sync)>,
    ) -> ChangedTiles {
        let graph = stages::needs_graph(&ts.layers).then(|| {
            let g = stages::Graph::new_scaled(ts, *ctx, cache, scale);
            match prefix {
                Some((p, plan)) => g.with_prefix(p, plan),
                None => g,
            }
        });
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
        let mut work: Vec<_> = per_tile.into_iter().enumerate().filter(|(t, _)| wanted[*t]).collect();
        // Neighborhood filters read the tiles around each tile. Rayon hands every thread a run of
        // consecutive entries, so list them in square blocks: a thread's tiles then sit together
        // and share their neighbors' intermediate results instead of recomputing them.
        let n = (ctx.geometry.resolution.div_ceil(subcraft_image::TILE_SIZE)) as usize;
        let block = block_edge();
        work.sort_by_key(|(t, _)| ((t / n) / block, (t % n) / block, t / n, t % n));
        let changed: Vec<(usize, Vec<ChannelKind>)> = work
            .into_par_iter()
            .with_max_len(1)
            .map(|(t, mut slots)| {
                if cancel.is_some_and(|c| c()) {
                    return (t, Vec::new());
                }
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
                let values = if let Some(graph) = &graph { graph.tile(t) } else {
                    tile::evaluate_tile_local_cached(ts, &tc, t, prefix).into_iter().map(|(k,p)|(k,std::sync::Arc::new(p))).collect()
                };
                for (kind, px) in values {
                    let Some((_, comps, fill, slot)) = slots.iter_mut().find(|s| s.0 == kind) else { continue };
                    let n = TILE_TEXELS * *comps;
                    for (o, p) in buf.chunks_exact_mut(*comps).zip(px.iter()) {
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
        cache.trim();
        let mut out = ChangedTiles::new();
        for (t, kinds) in changed {
            for kind in kinds {
                out.entry(kind).or_default().push(t);
            }
        }
        for tiles in out.values_mut() {
            tiles.sort_unstable();
        }
        out
    }
}

/// Low-resolution texel (column or row) shown at full-resolution texel `x` of a preview stretch
/// (nearest texel, as the preview has always been sampled).
#[inline]
fn low_texel(x: u32, full: u32, low: u32) -> u32 {
    (((x as f32) + 0.5) / (full as f32 / low as f32)) as u32
}

/// The f16 texel `(x, y)` of `img` (its fill where the tile is unallocated).
#[inline]
fn texel_f16<'a>(img: &'a TiledImage, fill: &'a [f16], x: u32, y: u32) -> &'a [f16] {
    let comps = img.comps();
    let t = img.tile_index(x / TILE_SIZE, y / TILE_SIZE);
    match img.tile(t) {
        Some(tile) => {
            let at = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize * comps;
            &tile.data[at..at + comps]
        }
        None => fill,
    }
}

fn fill_f16(img: &TiledImage) -> Vec<f16> {
    img.fill()[..img.comps()].iter().map(|&v| f16::from_f32(v)).collect()
}

/// Tile `t` of a `full`-resolution image shaped like `like`, filled with `low` stretched (nearest
/// texel). Texels are copied as stored (f16), so they equal what decoding and re-encoding gave.
fn stretched_tile(low: &TiledImage, like: &TiledImage, full: u32, t: usize) -> Arc<Tile> {
    let comps = like.comps();
    debug_assert_eq!(comps, low.comps());
    let (tx, ty) = like.tile_coords(t);
    let (x0, y0) = (tx * TILE_SIZE, ty * TILE_SIZE);
    let lr = low.width();
    let fill = fill_f16(low);
    let cols: Vec<u32> = (0..TILE_SIZE).map(|x| low_texel(x0 + x, full, lr).min(lr - 1)).collect();
    let mut data = vec![f16::ZERO; TILE_TEXELS * comps];
    for y in 0..TILE_SIZE {
        let sy = low_texel(y0 + y, full, lr).min(lr - 1);
        let row = &mut data[(y * TILE_SIZE) as usize * comps..][..TILE_SIZE as usize * comps];
        for (o, &sx) in row.chunks_exact_mut(comps).zip(&cols) {
            o.copy_from_slice(texel_f16(low, &fill, sx, sy));
        }
    }
    Arc::new(Tile { data: data.into_boxed_slice() })
}

/// Whether two low-resolution images hold identical bits in every texel that full-resolution
/// tile `t` stretches from (so a preview of `a` there shows the same as one of `b`).
fn same_low_block(a: &TiledImage, b: &TiledImage, full: u32, t: usize) -> bool {
    if a.comps() != b.comps() || a.width() != b.width() {
        return false;
    }
    let lr = a.width();
    let n = full.div_ceil(TILE_SIZE);
    let (tx, ty) = ((t as u32) % n, (t as u32) / n);
    let (x0, y0) = (tx * TILE_SIZE, ty * TILE_SIZE);
    let (sx0, sx1) = (low_texel(x0, full, lr).min(lr - 1), low_texel(x0 + TILE_SIZE - 1, full, lr).min(lr - 1));
    let (sy0, sy1) = (low_texel(y0, full, lr).min(lr - 1), low_texel(y0 + TILE_SIZE - 1, full, lr).min(lr - 1));
    let (fa, fb) = (fill_f16(a), fill_f16(b));
    for sy in sy0..=sy1 {
        for sx in sx0..=sx1 {
            let (pa, pb) = (texel_f16(a, &fa, sx, sy), texel_f16(b, &fb, sx, sy));
            if pa.iter().zip(pb).any(|(x, y)| x.to_bits() != y.to_bits()) {
                return false;
            }
        }
    }
    true
}

/// Edge (in tiles) of the square blocks tiles are scheduled in.
fn block_edge() -> usize {
    std::env::var("SUBCRAFT_TILE_BLOCK").ok().and_then(|v| v.parse().ok()).filter(|&b| b >= 1).unwrap_or(8)
}

/// Evaluate every tile of a texture set.
pub fn composite_full(ts: &TextureSet, ctx: &EvalContext) -> Composite {
    let mut c = Composite::new(ts);
    let all: Vec<usize> = (0..c.tile_count()).collect();
    c.evaluate_tiles(ts, ctx, &all);
    c
}
