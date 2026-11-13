//! Derived per-layer results: the accumulation of a channel after the first `j + 1` top-level
//! layers, per tile, so that editing layer `k` only re-blends the layers above the nearest cached
//! boundary below `k`.
//!
//! * Never saved: it lives inside [`crate::CompositeCache`] and is rebuilt from the document.
//! * Exact: entries are bit-for-bit copies of what a full evaluation produced (planes are stored
//!   losslessly; constant and duplicate planes are merely shared), so a cached recomposite equals
//!   a full one.
//! * Precise invalidation: entries are valid for the top-level layer list they were produced from
//!   (`Arc` identity, copy-on-write guarantees an edit replaces the layer). [`PrefixCache::sync`]
//!   drops the boundaries at or above the first changed layer, and for raster-only edits only the
//!   tiles the incremental diff reports dirty.
//! * Bounded: a byte budget with least-recently-used eviction; which boundaries are stored is
//!   planned from the budget (evenly spaced, newest first).
//! * Read wherever it is valid: the planned boundaries move with the layer count and the measured
//!   entry size, but an entry stored under an earlier plan is still exact, so evaluation resumes
//!   from any stored boundary. Adding a layer on top used to shift every planned boundary by one:
//!   nothing hit, and the first stroke on the new layer re-blended the whole stack per tile.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use subcraft_core::ChannelKind;
use subcraft_doc::Layer;
use subcraft_image::TILE_TEXELS;

enum Plane {
    /// Every texel has these bits.
    Const(u32),
    /// Bitwise equal to a lower plane.
    Same(u8),
    Full(Box<[f32]>),
}

/// One tile of `[f32; 4]` texels, stored per component.
pub(crate) struct Packed {
    planes: [Plane; 4],
}

impl Packed {
    pub(crate) fn pack(px: &[[f32; 4]]) -> Self {
        debug_assert_eq!(px.len(), TILE_TEXELS);
        let data: [Vec<f32>; 4] = std::array::from_fn(|c| px.iter().map(|p| p[c]).collect());
        let bits_eq = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits());
        let mk = |c: usize| -> Plane {
            let d = &data[c];
            let first = d[0].to_bits();
            if d.iter().all(|v| v.to_bits() == first) {
                return Plane::Const(first);
            }
            for lower in 0..c {
                if bits_eq(d, &data[lower]) {
                    return Plane::Same(lower as u8);
                }
            }
            Plane::Full(d.clone().into_boxed_slice())
        };
        Self { planes: [mk(0), mk(1), mk(2), mk(3)] }
    }

    pub(crate) fn unpack(&self) -> Vec<[f32; 4]> {
        let mut out = vec![[0.0f32; 4]; TILE_TEXELS];
        for c in 0..4 {
            let mut at = c;
            while let Plane::Same(l) = &self.planes[at] {
                at = *l as usize;
            }
            match &self.planes[at] {
                Plane::Const(b) => {
                    let v = f32::from_bits(*b);
                    out.iter_mut().for_each(|p| p[c] = v);
                }
                Plane::Full(d) => out.iter_mut().zip(d.iter()).for_each(|(p, v)| p[c] = *v),
                Plane::Same(_) => unreachable!(),
            }
        }
        out
    }

    fn bytes(&self) -> usize {
        64 + self.planes.iter().map(|p| if let Plane::Full(d) = p { d.len() * 4 } else { 0 }).sum::<usize>()
    }
}

type Key = (u16, u32, u32); // (channel, boundary layer index, tile)

fn key(kind: ChannelKind, j: usize, tile: usize) -> Key {
    (kind as u16, j as u32, tile as u32)
}

struct Entry {
    packed: Arc<Packed>,
    stamp: u64,
    bytes: usize,
}

#[derive(Default)]
struct Inner {
    /// The top-level layers every entry belongs to.
    layers: Vec<Arc<Layer>>,
    entries: HashMap<Key, Entry>,
    /// Least recently used first.
    order: BTreeMap<u64, Key>,
    bytes: usize,
    clock: u64,
    /// Whether the entries came from the neighborhood-graph evaluator (the two evaluators are not
    /// guaranteed to round identically, so entries are never mixed).
    graph: bool,
    /// Entries per boundary (which boundaries can be resumed from).
    per_boundary: BTreeMap<u32, usize>,
    /// Boundaries of the current plan: the only ones written. `None` before any plan: all.
    store: Option<BTreeSet<u32>>,
}

impl Inner {
    fn remove(&mut self, k: &Key) {
        if let Some(e) = self.entries.remove(k) {
            self.order.remove(&e.stamp);
            self.bytes -= e.bytes;
            self.forget(k.1);
        }
    }

    /// One entry of boundary `j` is gone.
    fn forget(&mut self, j: u32) {
        if let Some(n) = self.per_boundary.get_mut(&j) {
            *n -= 1;
            if *n == 0 {
                self.per_boundary.remove(&j);
            }
        }
    }

    fn retain(&mut self, mut keep: impl FnMut(&Key) -> bool) {
        let dead: Vec<Key> = self.entries.keys().filter(|k| !keep(k)).copied().collect();
        for k in dead {
            self.remove(&k);
        }
    }
}

/// What changed since the entries were produced.
pub(crate) enum Change<'a> {
    /// Resolution, channels, resources, geometry, filtering: forget everything.
    Everything,
    /// Layer parameters or structure changed: every tile of the changed layers and above.
    Parameters,
    /// Only raster texels of these tiles changed.
    Tiles(&'a BTreeSet<usize>),
}

pub struct PrefixCache {
    inner: Mutex<Inner>,
    budget: usize,
    hits: AtomicUsize,
    misses: AtomicUsize,
    /// The layers' own results (see [`ContentCache`]); same lifetime and budget.
    pub(crate) content: ContentCache,
}

impl Default for PrefixCache {
    fn default() -> Self {
        let mb = std::env::var("SUBCRAFT_LAYER_CACHE_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(2048);
        Self::new(mb << 20)
    }
}

/// Counters, for tests and benchmarks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrefixStats {
    pub entries: usize,
    pub bytes: usize,
    pub hits: usize,
    pub misses: usize,
}

impl PrefixCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self { inner: Mutex::default(), budget: budget_bytes, hits: AtomicUsize::new(0), misses: AtomicUsize::new(0), content: ContentCache::new(budget_bytes) }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn stats(&self) -> PrefixStats {
        let i = self.lock();
        PrefixStats { entries: i.entries.len(), bytes: i.bytes, hits: self.hits.load(Ordering::Relaxed), misses: self.misses.load(Ordering::Relaxed) }
    }

    /// Declare the layer list (and evaluator) the next evaluations run against, dropping what the
    /// change made stale.
    pub(crate) fn sync(&self, layers: &[Arc<Layer>], graph: bool, change: Change) {
        self.content.sync(layers, matches!(change, Change::Everything));
        let mut i = self.lock();
        if i.graph != graph || matches!(change, Change::Everything) {
            *i = Inner { graph, ..Inner::default() };
        }
        let common = i.layers.len().min(layers.len());
        let k = i.layers.iter().zip(layers).position(|(a, b)| !Arc::ptr_eq(a, b)).unwrap_or(common);
        if k != i.layers.len() || k != layers.len() {
            match change {
                Change::Tiles(tiles) if i.layers.len() == layers.len() => i.retain(|key| (key.1 as usize) < k || !tiles.contains(&(key.2 as usize))),
                _ => i.retain(|key| (key.1 as usize) < k),
            }
        }
        i.layers = layers.to_vec();
    }

    /// Boundaries (top-level layer indices) to resume from and to store. Stored from now on: as
    /// many as the budget holds, evenly spaced, nearest the top first. Returned as well: every
    /// boundary that still holds entries from an earlier plan (they stay exact, see the module
    /// documentation); those are read but no longer written, and age out of the budget.
    /// `n` layers, `tiles` per channel, `kinds` stored.
    pub(crate) fn plan(&self, n: usize, tiles: usize, kinds: &[ChannelKind]) -> Vec<usize> {
        let mut plan = self.plan_stored(n, tiles, kinds);
        let mut i = self.lock();
        i.store = Some(plan.iter().map(|&j| j as u32).collect());
        if !plan.is_empty() {
            let earlier: Vec<usize> = i.per_boundary.keys().map(|&j| j as usize).filter(|j| *j + 1 < n && !plan.contains(j)).collect();
            plan.extend(earlier);
        }
        plan
    }

    fn plan_stored(&self, n: usize, tiles: usize, kinds: &[ChannelKind]) -> Vec<usize> {
        if n < 2 || self.budget == 0 {
            return Vec::new();
        }
        // Before anything is stored: color-like channels keep three full planes, scalars one.
        // Afterwards the measured average entry size replaces the estimate (constant and
        // duplicate planes are free, so real entries are usually much smaller).
        let guess: usize = kinds.iter().map(|k| if k.components() == 3 { 3 } else { 1 } * TILE_TEXELS * 4).sum();
        let (stored_bytes, stored_entries) = {
            let i = self.lock();
            (i.bytes, i.entries.len())
        };
        let mut per_tile = guess;
        if stored_entries > 0 {
            per_tile = per_tile.min(stored_bytes / stored_entries * kinds.len());
        }
        let per_tile = per_tile.max(1);
        let per_boundary = (per_tile * tiles).max(1);
        let boundaries = n - 1; // the topmost result is the composite itself
        let fit = self.budget / per_boundary;
        if fit == 0 {
            return Vec::new();
        }
        let spacing = boundaries.div_ceil(fit).max(1);
        (0..fit.min(boundaries)).filter_map(|m| (boundaries - 1).checked_sub(m * spacing)).collect()
    }

    /// Whether some planned boundary holds every channel of `tile` (a resumable prefix).
    pub(crate) fn covers(&self, kinds: &[ChannelKind], plan: &[usize], tile: usize) -> bool {
        let i = self.lock();
        plan.iter().any(|&j| kinds.iter().all(|&k| i.entries.contains_key(&key(k, j, tile))))
    }

    pub(crate) fn get(&self, kind: ChannelKind, j: usize, tile: usize) -> Option<Arc<Packed>> {
        let mut i = self.lock();
        let k = key(kind, j, tile);
        i.clock += 1;
        let stamp = i.clock;
        let Some(e) = i.entries.get_mut(&k) else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        let old = std::mem::replace(&mut e.stamp, stamp);
        let packed = e.packed.clone();
        i.order.remove(&old);
        i.order.insert(stamp, k);
        self.hits.fetch_add(1, Ordering::Relaxed);
        Some(packed)
    }

    pub(crate) fn put(&self, kind: ChannelKind, j: usize, tile: usize, px: &[[f32; 4]]) {
        if self.lock().store.as_ref().is_some_and(|s| !s.contains(&(j as u32))) {
            return; // a boundary of an earlier plan: read only
        }
        let packed = Arc::new(Packed::pack(px));
        let bytes = packed.bytes();
        if bytes > self.budget {
            return;
        }
        let mut i = self.lock();
        let k = key(kind, j, tile);
        i.remove(&k);
        i.clock += 1;
        let stamp = i.clock;
        i.entries.insert(k, Entry { packed, stamp, bytes });
        i.order.insert(stamp, k);
        i.bytes += bytes;
        *i.per_boundary.entry(k.1).or_default() += 1;
        while i.bytes > self.budget {
            let Some((_, oldest)) = i.order.pop_first() else { break };
            if let Some(e) = i.entries.remove(&oldest) {
                i.bytes -= e.bytes;
                i.forget(oldest.1);
            }
        }
    }
}

/// What a top-level layer contributes before it is blended: its content after its effects, per
/// channel, and its mask, per tile.
///
/// An edit of one layer changes what every layer above it is blended onto, but not what those
/// layers are: their content and masks (blurs, medians, generators...) used to be evaluated
/// again for every tile just to be blended once more, which was most of the cost of an edit low
/// in a stack. Here they are kept for as long as the layer itself is unchanged.
///
/// * Exact: entries are lossless copies ([`Packed`]) of what the evaluation produced.
/// * Valid by identity: an entry belongs to one `Arc<Layer>`; copy-on-write replaces the `Arc` on
///   any edit of the layer or of anything inside it. [`ContentCache::sync`] drops the entries of
///   layers that left the stack and keeps the others alive, so an address is never reused.
///   Whatever else a result depends on (geometry, resolution, channels, resources, mesh maps)
///   resets the whole cache, exactly as it resets the prefix entries. Only results that depend on
///   nothing but the layer are stored: the evaluator decides that (no anchors, no passthrough, no
///   compare masks).
/// * Bounded without thrashing: a byte budget; an entry is only evicted once a whole update has
///   passed without reading it. While every entry is in use, new results are simply not stored,
///   so a stack larger than the budget keeps hitting on what it holds instead of evicting, tile
///   after tile, what the next tile is about to read.
pub(crate) struct ContentCache {
    inner: Mutex<ContentInner>,
    budget: usize,
    hits: AtomicUsize,
}

/// Which result of a layer: a channel (`ChannelKind as u16`) or [`CONTENT_MASK`].
pub(crate) type ContentSlot = u16;
pub(crate) const CONTENT_MASK: ContentSlot = u16::MAX;

type ContentKey = (usize, ContentSlot, u32); // (layer address, slot, tile)

struct ContentEntry {
    packed: Arc<Packed>,
    stamp: u64,
    bytes: usize,
    /// The update that last read or wrote it.
    update: u64,
}

#[derive(Default)]
struct ContentInner {
    /// Keeps every layer with entries alive.
    layers: Vec<Arc<Layer>>,
    entries: HashMap<ContentKey, ContentEntry>,
    /// Least recently used first.
    order: BTreeMap<u64, ContentKey>,
    bytes: usize,
    clock: u64,
    update: u64,
}

impl ContentInner {
    fn remove(&mut self, k: &ContentKey) {
        if let Some(e) = self.entries.remove(k) {
            self.order.remove(&e.stamp);
            self.bytes -= e.bytes;
        }
    }
}

impl ContentCache {
    fn new(budget_bytes: usize) -> Self {
        Self { inner: Mutex::default(), budget: budget_bytes, hits: AtomicUsize::new(0) }
    }

    fn lock(&self) -> MutexGuard<'_, ContentInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn enabled(&self) -> bool {
        self.budget > 0
    }

    /// (entries, bytes, hits), for tests and benchmarks.
    pub(crate) fn stats(&self) -> (usize, usize, usize) {
        let i = self.lock();
        (i.entries.len(), i.bytes, self.hits.load(Ordering::Relaxed))
    }

    /// A new update against `layers` begins; `everything`: nothing stored is valid any more.
    fn sync(&self, layers: &[Arc<Layer>], everything: bool) {
        let mut i = self.lock();
        if everything {
            let update = i.update;
            *i = ContentInner { update, ..ContentInner::default() };
        }
        i.update += 1;
        let alive: BTreeSet<usize> = layers.iter().map(|l| Arc::as_ptr(l) as usize).collect();
        let dead: Vec<ContentKey> = i.entries.keys().filter(|k| !alive.contains(&k.0)).copied().collect();
        for k in dead {
            i.remove(&k);
        }
        i.layers = layers.to_vec();
    }

    pub(crate) fn get(&self, layer: usize, slot: ContentSlot, tile: usize) -> Option<Arc<Packed>> {
        let mut i = self.lock();
        let k = (layer, slot, tile as u32);
        i.clock += 1;
        let (stamp, update) = (i.clock, i.update);
        let e = i.entries.get_mut(&k)?;
        let old = std::mem::replace(&mut e.stamp, stamp);
        e.update = update;
        let packed = e.packed.clone();
        i.order.remove(&old);
        i.order.insert(stamp, k);
        self.hits.fetch_add(1, Ordering::Relaxed);
        Some(packed)
    }

    pub(crate) fn put(&self, layer: usize, slot: ContentSlot, tile: usize, px: &[[f32; 4]]) {
        if self.budget == 0 {
            return;
        }
        let packed = Arc::new(Packed::pack(px));
        let bytes = packed.bytes();
        if bytes > self.budget {
            return;
        }
        let mut i = self.lock();
        // Only for a layer of the current stack (its `Arc` is held, so the address is its own).
        if !i.layers.iter().any(|l| Arc::as_ptr(l) as usize == layer) {
            return;
        }
        let k = (layer, slot, tile as u32);
        i.remove(&k);
        while i.bytes + bytes > self.budget {
            let Some((_, &oldest)) = i.order.first_key_value() else { break };
            // Read during this update or the one before: everything stored is in use.
            if i.entries.get(&oldest).is_some_and(|e| e.update + 1 >= i.update) {
                return;
            }
            i.remove(&oldest);
        }
        i.clock += 1;
        let (stamp, update) = (i.clock, i.update);
        i.entries.insert(k, ContentEntry { packed, stamp, bytes, update });
        i.order.insert(stamp, k);
        i.bytes += bytes;
    }
}
