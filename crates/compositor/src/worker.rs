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
//! Evaluation runs on the worker's own rayon pool (one thread fewer than the machine has), so
//! parallel work started by the UI thread (brush stamps) never queues behind composite tiles.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use subcraft_core::TextureSetId;
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
                }
                geometry.insert(id, set.geometry.clone());
            }
        }
        let mut out = Vec::new();
        pool.install(|| {
            for set in &snap.sets {
                let cache = caches.get_mut(&set.texture_set.id).expect("inserted for every set");
                let ctx = EvalContext { geometry: &set.geometry, resources: &snap.resources, mesh_maps: &set.mesh_maps };
                let tiles = cache.update_within(&set.texture_set, &ctx, Some(SLICE));
                if !tiles.is_empty() {
                    let composite = cache.composite().expect("updated").clone();
                    out.push((set.texture_set.id, Published { composite, tiles, changed: cache.last_changed().clone() }));
                }
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
        if !working && st.next.is_none() {
            st.done = *generation;
        }
        shared.changed.notify_all();
    }
}
