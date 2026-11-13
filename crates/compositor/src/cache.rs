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
    stages: crate::stages::StageCache,
    /// Accumulations after chosen top-level layers (derived data, never saved).
    prefix: crate::PrefixCache,
    /// Top-level boundaries the prefix cache stores this update.
    plan: Vec<usize>,
    /// The channels the pending tiles are evaluated for (`None`: all): the edits since the
    /// composite was last complete only changed what these channels read.
    only: Option<BTreeSet<subcraft_core::ChannelKind>>,
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

    /// Replace the layer-result cache (budget in bytes; 0 disables it).
    pub fn set_layer_cache_budget(&mut self, bytes: usize) {
        self.prefix = crate::PrefixCache::new(bytes);
        self.plan.clear();
    }

    /// (entries, bytes, hits) of the cache of the layers' own content and masks.
    pub fn content_cache_stats(&self) -> (usize, usize, usize) {
        self.prefix.content.stats()
    }

    /// The channels the last update was limited to (`None`: every channel was evaluated).
    pub fn limited_to(&self) -> Option<Vec<subcraft_core::ChannelKind>> {
        self.only.as_ref().map(|k| k.iter().copied().collect())
    }

    /// Statistics of the per-layer result cache.
    pub fn layer_cache_stats(&self) -> crate::PrefixStats {
        self.prefix.stats()
    }

    /// Tiles the next update of `ts` would evaluate (no side effects): everything not yet
    /// evaluated against the document, plus what the edit since the last update dirties.
    pub fn stale_tiles(&self, ts: &TextureSet) -> Vec<usize> {
        let n = ts.resolution.div_ceil(subcraft_image::TILE_SIZE) as usize;
        let reusable = !self.force_all
            && self.composite.as_ref().is_some_and(|c| c.resolution == ts.resolution)
            && self.last.as_ref().is_some_and(|l| l.id == ts.id && l.channels == ts.channels && l.transparent == ts.transparent);
        match (&self.last, reusable) {
            (Some(last), true) if last.albedo_filter == ts.albedo_filter => match diff_lists(&last.layers, &ts.layers, ts.resolution) {
                Dirty::Tiles(mut set) => {
                    set.extend(self.pending.iter().copied());
                    set.into_iter().collect()
                }
                Dirty::All => (0..n * n).collect(),
            },
            _ => (0..n * n).collect(),
        }
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
            && self.last.as_ref().is_some_and(|l| l.id == ts.id && l.channels == ts.channels && l.transparent == ts.transparent);
        let dirty = match (&self.last, reusable) {
            (Some(last), true) if last.albedo_filter != ts.albedo_filter => Dirty::All,
            (Some(last), true) => diff_lists(&last.layers, &ts.layers, ts.resolution),
            _ => Dirty::All,
        };
        if !matches!(&dirty,Dirty::Tiles(set) if set.is_empty()) { self.stages.clear(); }
        let graph_path = crate::stages::needs_graph(&ts.layers);
        let everything = !reusable || self.last.as_ref().is_some_and(|l| l.albedo_filter != ts.albedo_filter);
        // A parameter change that only some channels read (one channel's blend or fill).
        let confined = match (&dirty, &self.last) {
            (Dirty::All, Some(last)) if !everything => confined_kinds(&last.layers, &ts.layers),
            _ => None,
        };
        let change = match &dirty {
            _ if everything => crate::prefix::Change::Everything,
            Dirty::All => match &confined {
                Some(kinds) => crate::prefix::Change::Kinds(kinds),
                None => crate::prefix::Change::Parameters,
            },
            Dirty::Tiles(set) => crate::prefix::Change::Tiles(set),
        };
        self.prefix.sync(&ts.layers, graph_path, change);
        // The pending tiles are evaluated for the union of what the edits since the last complete
        // composite can have changed; anything that is not confined to channels means all of them.
        let had_pending = !self.pending.is_empty();
        self.only = match (&dirty, confined) {
            (Dirty::All, Some(kinds)) if !had_pending => Some(kinds),
            (Dirty::All, Some(kinds)) => self.only.take().map(|mut prev| {
                prev.extend(kinds);
                prev
            }),
            (Dirty::Tiles(set), _) if set.is_empty() && had_pending => self.only.take(),
            _ => None,
        };
        if self.only.as_ref().is_some_and(|only| ts.channel_kinds().all(|k| only.contains(&k))) {
            self.only = None;
        }
        self.prefix.set_only(self.only.as_ref().map(|k| k.iter().copied().collect()));
        self.plan = self.prefix.plan(ts.layers.len(), ts.resolution.div_ceil(subcraft_image::TILE_SIZE).pow(2) as usize, &ts.channel_kinds().collect::<Vec<_>>());
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
            for (kind, tiles) in composite.evaluate_tiles_with_stages(ts, ctx, &fresh, &self.stages, Some((&self.prefix, &self.plan))) {
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
                    let n = fit.max(threads).max(crate::gpu::preferred_batch()).div_ceil(threads) * threads;
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
            let changed = composite.evaluate_tiles_with_stages(ts, ctx, &batch, &self.stages, Some((&self.prefix, &self.plan)));
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

/// The channels a parameter change between two layer lists can affect, when it is confined to
/// per-channel settings: `Some(kinds)` if every changed top-level layer differs from its old self
/// only in the blend (mode, opacity) of some channels and, for a fill layer, in the fill of some
/// channels. A channel's composite reads other channels' settings in three cases, all excluded
/// or widened here: anchor references and Compare Masks (any in the set: `None`), and micro
/// details, whose height and normal change follows the Base Color opacity.
fn confined_kinds(old: &[Arc<Layer>], new: &[Arc<Layer>]) -> Option<BTreeSet<subcraft_core::ChannelKind>> {
    use subcraft_core::ChannelKind;
    if old.len() != new.len() {
        return None;
    }
    let reads_across = |layers: &[Arc<Layer>]| subcraft_doc::anchor::has_references(layers) || crate::stages::has_compare(layers);
    if reads_across(old) || reads_across(new) {
        return None;
    }
    let mut kinds = BTreeSet::new();
    for (o, n) in old.iter().zip(new).filter(|(o, n)| !Arc::ptr_eq(o, n)) {
        if o.id != n.id {
            return None;
        }
        // The old layer with the new per-channel settings must be the new layer.
        let mut probe = (**o).clone();
        let mut changed = BTreeSet::new();
        let keys: BTreeSet<ChannelKind> = o.blend.keys().chain(n.blend.keys()).copied().collect();
        for k in keys {
            if o.blend.get(&k) != n.blend.get(&k) {
                match n.blend.get(&k) {
                    Some(b) => probe.blend.insert(k, *b),
                    None => probe.blend.remove(&k),
                };
                changed.insert(k);
            }
        }
        if let (LayerContent::Fill(pf), LayerContent::Fill(nf)) = (&mut probe.content, &n.content) {
            let keys: BTreeSet<ChannelKind> = pf.channels.keys().chain(nf.channels.keys()).copied().collect();
            for k in keys {
                if pf.channels.get(&k) != nf.channels.get(&k) {
                    match nf.channels.get(&k) {
                        Some(c) => pf.channels.insert(k, c.clone()),
                        None => pf.channels.remove(&k),
                    };
                    changed.insert(k);
                }
            }
        }
        if probe != **n {
            return None;
        }
        if changed.contains(&ChannelKind::BaseColor) && (crate::stages::micro_layer(o) || crate::stages::micro_layer(n)) {
            changed.extend([ChannelKind::Height, ChannelKind::Normal]);
        }
        kinds.extend(changed);
    }
    (!kinds.is_empty()).then_some(kinds)
}

fn diff_lists(old: &[Arc<Layer>], new: &[Arc<Layer>], resolution: u32) -> Dirty {
    let mut tiles = BTreeSet::new();
    let n = resolution.div_ceil(subcraft_image::TILE_SIZE) as usize;
    // Anchor references (ADR-0045) read other layers' content texel by texel, so a raster edit
    // also changes the readers at the same texels; any filter on a reader's path spreads it.
    // Conservatively grow by every filter of the set. Compare Masks reading the layer stack are
    // readers too (QA 2026-11: a blur after one left the neighbouring tiles stale).
    let readers = if subcraft_doc::anchor::has_references(new) || crate::stages::has_compare(new) { crate::stages::halo(new) } else { 0 };
    if collect_list(old, new, &mut tiles, n, readers) { Dirty::Tiles(tiles) } else { Dirty::All }
}

/// Add the tiles within `radius` texels of `tiles` to `out`.
fn expand_into(tiles: &BTreeSet<usize>, radius: u32, n: usize, out: &mut BTreeSet<usize>) {
    let halo = radius.div_ceil(subcraft_image::TILE_SIZE).min(n as u32) as usize;
    if halo == 0 {
        out.extend(tiles);
        return;
    }
    for &tile in tiles {
        let (x, y) = (tile % n, tile / n);
        for yy in y.saturating_sub(halo)..=(y + halo).min(n - 1) {
            for xx in x.saturating_sub(halo)..=(x + halo).min(n - 1) {
                out.insert(yy * n + xx);
            }
        }
    }
}

/// Adds raster-changed tiles (grown by the neighborhood filters that carry the change on) to
/// `out`; returns false if the change is not raster-only.
///
/// A changed texel of layer L reaches the output through L's own filters, those of its parent
/// folders, and those inside folders above it that let the accumulation pass through (their
/// filters see everything below). Blending itself is per texel and filters of other layers act
/// on those layers' own content, so they never spread L's change. `inherited` is the sum of the
/// parent-side supports, in texels.
fn collect_list(old: &[Arc<Layer>], new: &[Arc<Layer>], out: &mut BTreeSet<usize>, n: usize, inherited: u32) -> bool {
    if old.len() != new.len() {
        return false;
    }
    // Support of the folders above each layer (conservatively every visible one).
    let mut above = vec![0u32; new.len()];
    let mut running = 0u32;
    for (i, l) in new.iter().enumerate().rev() {
        above[i] = running;
        // A paint or fill layer in Passthrough (a filter layer) filters everything below it, just
        // like a folder that lets the stack through: its filters carry a change below it on too.
        let passes_through = l.blend.values().any(|b| b.mode == subcraft_doc::BlendMode::Passthrough);
        if l.visible && (l.is_folder() || passes_through) {
            running = running.saturating_add(crate::stages::halo(std::slice::from_ref(l)));
        }
    }
    for (i, (a, b)) in old.iter().zip(new).enumerate() {
        if Arc::ptr_eq(a, b) {
            continue;
        }
        if !a.same_parameters(b) {
            return false;
        }
        let side = inherited.saturating_add(above[i]);
        let mut own = BTreeSet::new();
        if let (Some(ma), Some(mb)) = (&a.mask, &b.mask) {
            own.extend(ma.base.changed_tiles(&mb.base));
            // Same parameters, so the same effect list: only paint effects differ, in texels.
            for (ea, eb) in ma.effects.iter().zip(&mb.effects).filter(|(x, y)| !Arc::ptr_eq(x, y)) {
                if let (MaskEffectKind::Paint { image: ia }, MaskEffectKind::Paint { image: ib }) = (&ea.kind, &eb.kind) {
                    own.extend(ia.changed_tiles(ib));
                }
            }
        }
        for (ea, eb) in a.content_effects.iter().zip(&b.content_effects).filter(|(x, y)| !Arc::ptr_eq(x, y)) {
            if let (ContentEffectKind::Paint { channels: ca }, ContentEffectKind::Paint { channels: cb }) = (&ea.kind, &eb.kind) {
                changed_paint_tiles(ca, cb, &mut own);
            }
        }
        if let (LayerContent::Paint(pa), LayerContent::Paint(pb)) = (&a.content, &b.content) {
            changed_paint_tiles(&pa.channels, &pb.channels, &mut own);
        }
        expand_into(&own, side.saturating_add(crate::stages::own_halo(b)), n, out);
        if let (LayerContent::Group(ga), LayerContent::Group(gb)) = (&a.content, &b.content) {
            let inner = side.saturating_add(crate::stages::own_halo(b));
            if !collect_list(&ga.children, &gb.children, out, n, inner) {
                return false;
            }
        }
    }
    true
}
