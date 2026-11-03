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
}

impl Inner {
    fn remove(&mut self, k: &Key) {
        if let Some(e) = self.entries.remove(k) {
            self.order.remove(&e.stamp);
            self.bytes -= e.bytes;
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
        Self { inner: Mutex::default(), budget: budget_bytes, hits: AtomicUsize::new(0), misses: AtomicUsize::new(0) }
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

    /// Boundaries (top-level layer indices) worth storing: as many as the budget holds, evenly
    /// spaced, nearest the top first. `n` layers, `tiles` per channel, `kinds` stored.
    pub(crate) fn plan(&self, n: usize, tiles: usize, kinds: &[ChannelKind]) -> Vec<usize> {
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
        while i.bytes > self.budget {
            let Some((_, oldest)) = i.order.pop_first() else { break };
            if let Some(e) = i.entries.remove(&oldest) {
                i.bytes -= e.bytes;
            }
        }
    }
}
