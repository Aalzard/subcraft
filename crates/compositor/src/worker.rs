//! Background compositing (ADR-0018): a worker thread owns every texture set's
//! [`CompositeCache`] and evaluates immutable snapshots, so a cold procedural field (tens of
//! milliseconds for one 64² tile, which cannot be split across frames) never costs a UI frame.
//!
//! * [`CompositeWorker::submit`] hands over the newest [`Snapshot`]. A snapshot the worker has not
//!   started yet is replaced (latest edit wins); tiles still pending from earlier snapshots are
//!   evaluated against the newest one, so the result converges exactly to a full update.
//! * The worker evaluates in short slices and publishes after each one: a cheap copy of the
//!   composite (tiles are shared, copy-on-write) and the tiles evaluated / changed since the
//!   last [`CompositeWorker::take`]. The UI uploads exactly those tiles.
//! * [`CompositeWorker::finish`] blocks until the newest snapshot is completely evaluated
//!   (export, bake, save and tests need the exact full composite).
//!
//! * Quick previews: when a snapshot dirties many tiles, a low-resolution composite of it is
//!   published first (stretched over the dirty tiles) and then refined tile by tile to the exact
//!   result. A preview in progress is finished even if a newer snapshot arrives (a slider sends
//!   one per frame; cancelling would restart it forever), and the newest one is previewed next.
//! * Previews only cover what changed: whenever the worker is idle it keeps a low-resolution
//!   composite of the exact state (the baseline). A preview then stretches only the tiles, per
//!   channel, whose low-resolution texels differ from the baseline; everything else keeps showing
//!   the crisp exact tiles and is not uploaded again. Dragging a generator slider on a 4096² set
//!   so republishes the tiles it changes instead of every tile of every channel.
//!
//! Evaluation runs on the worker's own rayon pool (one thread fewer than the machine has), so
//! parallel work started by the UI thread (brush stamps) never queues behind composite tiles.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use subcraft_core::{ChannelKind, TextureSetId};
use subcraft_doc::{ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};

use crate::{ChangedTiles, Composite, CompositeCache, EvalContext};

/// Work per slice before the worker publishes progress and looks for a newer snapshot.
const SLICE: Duration = Duration::from_millis(8);

/// Everything one texture set's evaluation reads, owned (cheap: layers, tiles and resources are
/// shared).
#[derive(Clone)]
pub struct SetSnapshot {
    pub texture_set: TextureSet,
    pub geometry: Arc<TextureSetGeometry>,
    pub mesh_maps: MeshMaps,
    /// The same geometry at [`preview_resolution`] of this set (`None`: no quick previews).
    pub preview_geometry: Option<Arc<TextureSetGeometry>>,
}

/// Resolution of the quick preview for a texture set of `resolution` texels, if one is worth it:
/// a power-of-two fraction that lands on 512.
pub fn preview_resolution(resolution: u32) -> Option<u32> {
    let mut low = resolution;
    while low > PREVIEW_EDGE && low % 2 == 0 {
        low /= 2;
    }
    (low == PREVIEW_EDGE && low < resolution).then_some(low)
}

/// Edge of the quick preview.
const PREVIEW_EDGE: u32 = 512;
/// A preview is never made for fewer stale tiles than this.
const PREVIEW_MIN_TILES: usize = 128;

/// Stale tiles from which a quick preview is worth making for a set of `tiles` tiles: a quarter
/// of the set, at least [`PREVIEW_MIN_TILES`]. A preview re-blends the whole stack at low
/// resolution, which pays off when an edit dirties most of the texture (a slider, a blend mode).
/// A brush stroke dirties the tiles under the brush, again on every frame: with the fixed
/// 128-tile threshold a large brush on a 4096² set (4096 tiles) got a full-stack preview per
/// frame, and the exact tiles (cheap for a stroke: only the layers above the last cached
/// boundary) were skipped for as long as the pen kept moving, so the paint showed late and blocky.
pub fn preview_min_tiles(tiles: usize) -> usize {
    PREVIEW_MIN_TILES.max(tiles / 4)
}

/// The inputs of every texture set at one document state.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub sets: Vec<SetSnapshot>,
    pub resources: ResourceStore,
    /// Texture sets to re-evaluate completely (their mesh maps changed).
    pub invalidate: BTreeSet<TextureSetId>,
}

/// One texture set's progress since the last [`CompositeWorker::take`].
#[derive(Clone, Debug)]
pub struct Published {
    /// The newest composite (complete only once the worker is idle).
    pub composite: Composite,
    /// Re-evaluated tiles (sorted).
    pub tiles: Vec<usize>,
    /// Per channel, the tiles whose texels changed (sorted): what to upload.
    pub changed: ChangedTiles,
}

impl Published {
    fn merge(&mut self, newer: Published) {
        if newer.composite.resolution != self.composite.resolution {
            *self = newer; // a new tiling: the older tile indices mean nothing now
            return;
        }
        self.composite = newer.composite;
        merge_sorted(&mut self.tiles, newer.tiles);
        for (kind, tiles) in newer.changed {
            merge_sorted(self.changed.entry(kind).or_default(), tiles);
        }
    }
}

fn merge_sorted(into: &mut Vec<usize>, more: Vec<usize>) {
    into.extend(more);
    into.sort_unstable();
    into.dedup();
}

#[derive(Default)]
struct State {
    /// Newest snapshot, not yet picked up by the worker.
    next: Option<Snapshot>,
    /// Generation of the newest submitted snapshot.
    submitted: u64,
    /// Generation of the newest completely evaluated snapshot.
    done: u64,
    /// Snapshots the worker started evaluating (the rest were replaced before it got to them).
    started: u64,
    published: HashMap<TextureSetId, Published>,
    /// Caches handed over at spawn, taken by the thread when it starts.
    adopt: HashMap<TextureSetId, CompositeCache>,
    shutdown: bool,
    /// The worker thread panicked.
    failed: bool,
    /// Quick previews published so far.
    previews: u64,
    /// Baselines (quick composites of an idle exact state) made so far.
    baselines: u64,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Handle of the compositing thread; dropping it stops the thread.
pub struct CompositeWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl CompositeWorker {
    /// Start the worker, continuing from `caches` (composites evaluated so far; they move to the
    /// worker on success).
    pub fn spawn(caches: &mut HashMap<TextureSetId, CompositeCache>) -> std::io::Result<Self> {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).saturating_sub(1).max(1);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("composite-{i}"))
            .build()
            .map_err(std::io::Error::other)?;
        let state = State { adopt: std::mem::take(caches), ..Default::default() };
        let shared = Arc::new(Shared { state: Mutex::new(state), changed: Condvar::new() });
        let worker = shared.clone();
        match std::thread::Builder::new().name("compositor".into()).spawn(move || run(&worker, &pool)) {
            Ok(thread) => Ok(Self { shared, thread: Some(thread) }),
            Err(e) => {
                *caches = std::mem::take(&mut shared.lock().adopt);
                Err(e)
            }
        }
    }

    /// Evaluate `snapshot` next, replacing any snapshot the worker has not started.
    pub fn submit(&self, mut snapshot: Snapshot) {
        let mut st = self.lock();
        if let Some(replaced) = st.next.take() {
            snapshot.invalidate.extend(replaced.invalidate);
        }
        st.next = Some(snapshot);
        st.submitted += 1;
        self.shared.changed.notify_all();
    }

    /// The newest submitted snapshot is completely evaluated.
    pub fn idle(&self) -> bool {
        let st = self.lock();
        st.done == st.submitted
    }

    /// Results published since the last call are waiting.
    pub fn has_published(&self) -> bool {
        !self.lock().published.is_empty()
    }

    /// Take everything published since the last call.
    pub fn take(&self) -> HashMap<TextureSetId, Published> {
        std::mem::take(&mut self.lock().published)
    }

    /// How many submitted snapshots the worker started on (diagnostics and tests).
    pub fn snapshots_started(&self) -> u64 {
        self.lock().started
    }

    /// Quick previews published so far (diagnostics and tests).
    pub fn previews_published(&self) -> u64 {
        self.lock().previews
    }

    /// Preview baselines made so far (diagnostics and tests: a baseline is made after the worker
    /// becomes idle, so a test waits for it before the next submission).
    pub fn baselines_made(&self) -> u64 {
        self.lock().baselines
    }

    /// Block until the newest submitted snapshot is completely evaluated.
    pub fn finish(&self) {
        let mut st = self.lock();
        while st.done != st.submitted && !st.failed {
            st = self.shared.changed.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        if st.failed {
            drop(st);
            panic!("the compositing worker panicked");
        }
    }

    /// The state, re-raising a worker panic on the calling thread (the crash reporter sees it).
    fn lock(&self) -> MutexGuard<'_, State> {
        let st = self.shared.lock();
        if st.failed {
            drop(st);
            panic!("the compositing worker panicked");
        }
        st
    }
}

impl Drop for CompositeWorker {
    fn drop(&mut self) {
        self.shared.lock().shutdown = true;
        self.shared.changed.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Marks the worker failed (waking any waiter) if its thread unwinds.
struct PanicGuard<'a>(&'a Shared);

impl Drop for PanicGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.lock().failed = true;
            self.0.changed.notify_all();
        }
    }
}

fn run(shared: &Shared, pool: &rayon::ThreadPool) {
    let _guard = PanicGuard(shared);
    let mut caches = std::mem::take(&mut shared.lock().adopt);
    let mut current: Option<(u64, Snapshot)> = None;
    let mut geometry: HashMap<TextureSetId, Arc<TextureSetGeometry>> = HashMap::new();
    let mut working = false;
    // What is shown while previews are pending: the exact composite with preview tiles on top.
    let mut displays: HashMap<TextureSetId, Composite> = HashMap::new();
    // Per channel, the displayed tiles that show a preview instead of the exact composite.
    let mut previewed: HashMap<TextureSetId, HashMap<ChannelKind, BTreeSet<usize>>> = HashMap::new();
    // Quick composite of the exact state, made when the worker last became idle, and the sets whose
    // exact composite changed since (their baseline is remade at the next idle).
    let mut baselines: HashMap<TextureSetId, Composite> = HashMap::new();
    let mut outdated: BTreeSet<TextureSetId> = BTreeSet::new();
    loop {
        let fresh = {
            let mut st = shared.lock();
            loop {
                if st.shutdown {
                    return;
                }
                if let Some(next) = st.next.take() {
                    st.started += 1;
                    current = Some((st.submitted, next));
                    break true;
                }
                if working {
                    break false;
                }
                st = shared.changed.wait(st).unwrap_or_else(|e| e.into_inner());
            }
        };
        let (generation, snap) = current.as_ref().expect("a snapshot was taken");
        if fresh {
            caches.retain(|id, _| snap.sets.iter().any(|s| s.texture_set.id == *id));
            geometry.retain(|id, _| caches.contains_key(id));
            for set in &snap.sets {
                let id = set.texture_set.id;
                let cache = caches.entry(id).or_default();
                // Other geometry (a new mesh) changes every tile, like changed mesh maps.
                let same_geometry = geometry.get(&id).is_none_or(|g| Arc::ptr_eq(g, &set.geometry));
                if snap.invalidate.contains(&id) || !same_geometry {
                    cache.invalidate();
                    baselines.remove(&id);
                    outdated.insert(id);
                }
                geometry.insert(id, set.geometry.clone());
            }
        }
        displays.retain(|id, _| caches.contains_key(id));
        previewed.retain(|id, _| caches.contains_key(id));
        baselines.retain(|id, _| caches.contains_key(id));
        outdated.retain(|id| caches.contains_key(id));
        let mut out = Vec::new();
        let mut previewed_now = false;
        if fresh {
            for set in &snap.sets {
                let (id, ts) = (set.texture_set.id, &set.texture_set);
                let Some(low_geo) = set.preview_geometry.as_ref().filter(|g| Some(g.resolution) == preview_resolution(ts.resolution)) else { continue };
                let cache = caches.get(&id).expect("inserted for every set");
                let stale = cache.stale_tiles(ts);
                let tiles = (ts.resolution.div_ceil(subcraft_image::TILE_SIZE) as usize).pow(2);
                if stale.len() < preview_min_tiles(tiles) {
                    continue;
                }
                let mut low = ts.clone();
                low.resolution = low_geo.resolution;
                let ctx = EvalContext { geometry: low_geo, resources: &snap.resources, mesh_maps: &set.mesh_maps };
                // A preview in progress is finished even when a newer snapshot arrives: a slider
                // sends one every frame, and a 4096² preview takes about 20 ms, so cancelling made
                // every preview start over and nothing showed until the drag paused. The newest
                // snapshot is previewed right after.
                let cancel = || shared.lock().shutdown;
                let Some(quick) = pool.install(|| Composite::preview(&low, &ctx, ts.resolution, &cancel)) else { continue };
                let exact = cache.composite().filter(|c| c.resolution == ts.resolution && c.channels.keys().eq(quick.channels.keys()));
                let mut display = match displays.remove(&id) {
                    Some(d) if d.resolution == ts.resolution => d,
                    _ => {
                        previewed.remove(&id);
                        exact.cloned().unwrap_or_else(|| Composite::new(ts))
                    }
                };
                let changed = match exact {
                    Some(exact) => {
                        let (changed, overlaid) = display.overlay_preview_against(&quick, &stale, exact, baselines.get(&id));
                        // Stale tiles not overlaid now show exactly what `exact` holds.
                        let shown = previewed.entry(id).or_default();
                        for kind in display.channels.keys() {
                            let set = shown.entry(*kind).or_default();
                            for t in &stale {
                                set.remove(t);
                            }
                            set.extend(overlaid.get(kind).into_iter().flatten().copied());
                        }
                        shown.retain(|_, set| !set.is_empty());
                        changed
                    }
                    None => {
                        display.overlay_preview(&quick, &stale);
                        let shown = previewed.entry(id).or_default();
                        for kind in display.channels.keys() {
                            shown.entry(*kind).or_default().extend(stale.iter().copied());
                        }
                        display.channels.keys().map(|k| (*k, stale.clone())).collect()
                    }
                };
                out.push((id, Published { composite: display.clone(), tiles: stale, changed }));
                displays.insert(id, display);
                shared.lock().previews += 1;
                previewed_now = true;
            }
        }
        // While a slider keeps moving, a newer snapshot is usually waiting by now: preview it next
        // instead of spending a slice on exact tiles that snapshot dirties again.
        let skip_exact = previewed_now && shared.lock().next.is_some();
        pool.install(|| {
            for set in snap.sets.iter().filter(|_| !skip_exact) {
                let id = set.texture_set.id;
                let cache = caches.get_mut(&id).expect("inserted for every set");
                let ctx = EvalContext { geometry: &set.geometry, resources: &snap.resources, mesh_maps: &set.mesh_maps };
                let tiles = cache.update_within(&set.texture_set, &ctx, Some(SLICE));
                if tiles.is_empty() {
                    continue;
                }
                outdated.insert(id);
                let exact = cache.composite().expect("updated");
                let mut changed = cache.last_changed().clone();
                let composite = match displays.get_mut(&id) {
                    Some(display) if display.resolution == exact.resolution => {
                        display.copy_tiles_from(exact, &tiles);
                        // Tiles showing a preview must be uploaded even when the exact result
                        // equals the one before the edit.
                        let pending = previewed.entry(id).or_default();
                        for (kind, set) in pending.iter_mut() {
                            for t in &tiles {
                                if set.remove(t) {
                                    changed.entry(*kind).or_default().push(*t);
                                }
                            }
                        }
                        pending.retain(|_, set| !set.is_empty());
                        for list in changed.values_mut() {
                            list.sort_unstable();
                            list.dedup();
                        }
                        let shown = display.clone();
                        if pending.is_empty() {
                            displays.remove(&id);
                        }
                        shown
                    }
                    _ => {
                        displays.remove(&id);
                        previewed.remove(&id);
                        exact.clone()
                    }
                };
                out.push((id, Published { composite, tiles, changed }));
            }
        });
        working = caches.values().any(CompositeCache::has_pending);
        let mut st = shared.lock();
        for (id, p) in out {
            match st.published.get_mut(&id) {
                Some(old) => old.merge(p),
                None => {
                    st.published.insert(id, p);
                }
            }
        }
        let idle = !working && st.next.is_none();
        if idle {
            st.done = *generation;
        }
        shared.changed.notify_all();
        drop(st);
        if idle {
            // The exact composites are complete: remember the quick composites of those that
            // changed, so the next preview can tell which tiles it changes. Cancelled by the next
            // snapshot (those baselines are then missing, and that preview stretches every stale
            // tile, as before).
            for set in &snap.sets {
                let (id, ts) = (set.texture_set.id, &set.texture_set);
                if baselines.contains_key(&id) && !outdated.contains(&id) {
                    continue;
                }
                baselines.remove(&id);
                let Some(low_geo) = set.preview_geometry.as_ref().filter(|g| Some(g.resolution) == preview_resolution(ts.resolution)) else {
                    outdated.remove(&id);
                    continue;
                };
                let mut low = ts.clone();
                low.resolution = low_geo.resolution;
                let ctx = EvalContext { geometry: low_geo, resources: &snap.resources, mesh_maps: &set.mesh_maps };
                let cancel = || {
                    let st = shared.lock();
                    st.next.is_some() || st.shutdown
                };
                match pool.install(|| Composite::preview(&low, &ctx, ts.resolution, &cancel)) {
                    Some(base) => {
                        baselines.insert(id, base);
                        outdated.remove(&id);
                        shared.lock().baselines += 1;
                    }
                    None => {
                        // The other changed sets keep no stale baseline either.
                        for id in &outdated {
                            baselines.remove(id);
                        }
                        break;
                    }
                }
            }
        }
    }
}
