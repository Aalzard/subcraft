//! Incremental compositing: diff the texture set against the last evaluated one and refresh only
//! the tiles that can have changed.
//!
//! * pointer-equal layers are skipped (copy-on-write: untouched subtrees are shared);
//! * layers whose parameters are unchanged ([`Layer::same_parameters`]) contribute the tiles that
//!   changed identity in their paint channels and mask base ([`TiledImage::changed_tiles`]);
//! * anything else (parameter, structure, channel list, resolution change) dirties every tile.
//!
//! The returned dirty list is what the renderer re-uploads.
//!
//! Interactive callers can bound the work per call ([`CompositeCache::update_within`]): dirty
//! tiles accumulate in a pending set that is drained round-robin across calls, so a continuous
//! edit (slider drag) never blocks a frame and every tile still refreshes in turn. Pending tiles
//! are always evaluated against the newest texture set, so the result converges exactly to a
//! full update.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use subcraft_doc::{ContentEffectKind, Layer, LayerContent, MaskEffectKind, TextureSet, changed_paint_tiles};

use crate::{Composite, EvalContext};

#[derive(Default)]
pub struct CompositeCache {
    last: Option<TextureSet>,
    composite: Option<Composite>,
    force_all: bool,
    changed: crate::ChangedTiles,
    /// Tiles not yet evaluated against `last`.
    pending: BTreeSet<usize>,
    /// Round-robin position in `pending` for budgeted updates.
    cursor: usize,
    /// Smoothed wall time per tile of a parallel batch (ms), for batch sizing.
    tile_ms: f64,
}

/// Result of diffing two layer trees.
enum Dirty {
    All,
    Tiles(BTreeSet<usize>),
}

impl CompositeCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn composite(&self) -> Option<&Composite> {
        self.composite.as_ref()
    }

    /// Per channel, the tiles whose texels changed in the last [`CompositeCache::update`] (a
    /// subset of the re-evaluated tiles: identical results are not reported).
    pub fn last_changed(&self) -> &crate::ChangedTiles {
        &self.changed
    }

    /// Force a full re-evaluation on the next update (resources, mesh maps or geometry changed).
    pub fn invalidate(&mut self) {
        self.force_all = true;
    }

    /// Whether a budgeted update left tiles to evaluate (call again next frame).
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Bring the composite fully up to date with `ts`; returns the re-evaluated tiles (sorted).
    pub fn update(&mut self, ts: &TextureSet, ctx: &EvalContext) -> Vec<usize> {
        self.update_within(ts, ctx, None)
    }

    /// Like [`CompositeCache::update`], but stop starting new batches once `budget` is spent
    /// (`None`: no limit). Unfinished tiles stay pending for the next call.
    pub fn update_within(&mut self, ts: &TextureSet, ctx: &EvalContext, budget: Option<Duration>) -> Vec<usize> {
        let reusable = !self.force_all
            && self.composite.as_ref().is_some_and(|c| c.resolution == ts.resolution)
            && self.last.as_ref().is_some_and(|l| l.id == ts.id && l.channels == ts.channels);
        let dirty = match (&self.last, reusable) {
            (Some(last), true) if last.albedo_filter != ts.albedo_filter => Dirty::All,
            (Some(last), true) => diff_lists(&last.layers, &ts.layers),
            _ => Dirty::All,
        };
        let composite = match (&mut self.composite, reusable) {
            (Some(c), true) => c,
            (slot, _) => slot.insert(Composite::new(ts)),
        };
        let threads = rayon::current_num_threads().max(1);
        // Newest edits first: a small fresh change (a brush stroke) is evaluated at once, even
        // while tiles of an earlier whole-texture change are still pending or the budget is spent.
        let fresh: Vec<usize> = match dirty {
            Dirty::Tiles(set) if set.len() <= threads * 8 => {
                for t in &set {
                    self.pending.remove(t);
                }
                set.into_iter().collect()
            }
            Dirty::Tiles(set) => {
                self.pending.extend(set);
                Vec::new()
            }
            Dirty::All => {
                self.pending.extend(0..composite.tile_count());
                Vec::new()
            }
        };
        self.last = Some(ts.clone());
        self.force_all = false;
        self.changed = Default::default();
        let start = Instant::now();
        let mut evaluated = Vec::new();
        let mut batches = 0;
        if !fresh.is_empty() {
            for (kind, tiles) in composite.evaluate_tiles(ts, ctx, &fresh) {
                self.changed.entry(kind).or_default().extend(tiles);
            }
            evaluated.extend(fresh);
        }
        while !self.pending.is_empty() {
            let batch: Vec<usize> = match budget {
                None => std::mem::take(&mut self.pending).into_iter().collect(),
                Some(budget) => {
                    // A zero budget only queues work; otherwise at least one batch runs, so
                    // repeated calls always make progress. The rest stays pending.
                    let left = budget.saturating_sub(start.elapsed()).as_secs_f64() * 1e3;
                    if budget.is_zero() || (left <= 0.0 && batches > 0) {
                        break;
                    }
                    batches += 1;
                    // Whole multiples of the thread count, at least one round per call.
                    let fit = if self.tile_ms > 0.0 { (left / self.tile_ms) as usize } else { threads };
                    let n = fit.max(threads).div_ceil(threads) * threads;
                    let batch: Vec<usize> =
                        self.pending.range(self.cursor..).chain(self.pending.range(..self.cursor)).copied().take(n).collect();
                    for t in &batch {
                        self.pending.remove(t);
                    }
                    self.cursor = batch.last().map_or(0, |t| t + 1);
                    batch
                }
            };
            let t0 = Instant::now();
            let changed = composite.evaluate_tiles(ts, ctx, &batch);
            let per_tile = t0.elapsed().as_secs_f64() * 1e3 / batch.len() as f64;
            self.tile_ms = if self.tile_ms > 0.0 { self.tile_ms * 0.7 + per_tile * 0.3 } else { per_tile };
            for (kind, tiles) in changed {
                self.changed.entry(kind).or_default().extend(tiles);
            }
            evaluated.extend(batch);
        }
        for tiles in self.changed.values_mut() {
            tiles.sort_unstable();
        }
        evaluated.sort_unstable();
        evaluated
    }
}

fn diff_lists(old: &[Arc<Layer>], new: &[Arc<Layer>]) -> Dirty {
    let mut tiles = BTreeSet::new();
    if collect_list(old, new, &mut tiles) { Dirty::Tiles(tiles) } else { Dirty::All }
}

/// Adds raster-changed tiles to `out`; returns false if the change is not raster-only.
fn collect_list(old: &[Arc<Layer>], new: &[Arc<Layer>], out: &mut BTreeSet<usize>) -> bool {
    if old.len() != new.len() {
        return false;
    }
    for (a, b) in old.iter().zip(new) {
        if Arc::ptr_eq(a, b) {
            continue;
        }
        if !a.same_parameters(b) {
            return false;
        }
        if let (Some(ma), Some(mb)) = (&a.mask, &b.mask) {
            out.extend(ma.base.changed_tiles(&mb.base));
            // Same parameters, so the same effect list: only paint effects differ, in texels.
            for (ea, eb) in ma.effects.iter().zip(&mb.effects).filter(|(x, y)| !Arc::ptr_eq(x, y)) {
                if let (MaskEffectKind::Paint { image: ia }, MaskEffectKind::Paint { image: ib }) = (&ea.kind, &eb.kind) {
                    out.extend(ia.changed_tiles(ib));
                }
            }
        }
        for (ea, eb) in a.content_effects.iter().zip(&b.content_effects).filter(|(x, y)| !Arc::ptr_eq(x, y)) {
            if let (ContentEffectKind::Paint { channels: ca }, ContentEffectKind::Paint { channels: cb }) = (&ea.kind, &eb.kind) {
                changed_paint_tiles(ca, cb, out);
            }
        }
        match (&a.content, &b.content) {
            (LayerContent::Paint(pa), LayerContent::Paint(pb)) => changed_paint_tiles(&pa.channels, &pb.channels, out),
            (LayerContent::Group(ga), LayerContent::Group(gb)) if !collect_list(&ga.children, &gb.children, out) => return false,
            _ => {}
        }
    }
    true
}
