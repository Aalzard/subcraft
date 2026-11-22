//! Per-tile cache of other derived per-texel values (layer masks), next to the procedural
//! [`super::field`] cache.
//!
//! Editing a layer re-blends every layer above it, and each of those re-evaluated its mask
//! (generators, procedural mask fills, levels...) although nothing the mask reads had changed.
//! Callers key an entry by the *identity* of everything the value depends on (`Arc` pointers held
//! by the key itself, so copy-on-write guarantees any edit produces a different key and a freed
//! pointer can never be reused while the key is alive). A hit is therefore bit-identical to
//! evaluating again.
//!
//! Memory is bounded by its own budget, half of the field budget ([`super::field::budget`]) and at
//! most 1 GiB, so masks never push out the much more expensive noise fields. Replacement follows
//! the field cache: at most [`PER_TILE`] entries per tile; once full or over budget an entry is
//! only replaced after it has gone unused for [`STALE_AFTER`] reads of its tile. A probe can also report an entry as
//! outdated ([`Probe::Replace`]: the same mask with a repainted tile or rebaked maps), which is then
//! replaced at once so old rasters are not kept alive.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

/// Entries kept per tile.
pub const PER_TILE: usize = 128;
/// Reads of a tile after which an entry not read in between counts as stale (replaceable).
pub const STALE_AFTER: u64 = 2 * PER_TILE as u64;

static BYTES: AtomicUsize = AtomicUsize::new(0);

/// Bytes currently held by cached derived values (diagnostics and tests).
pub fn cached_bytes() -> usize {
    BYTES.load(Ordering::Relaxed)
}

/// The budget for derived values: half the procedural field budget, at most 1 GiB.
pub fn budget() -> usize {
    (super::field::budget() / 2).min(1 << 30)
}

/// One tile's cached values. Its bytes count against [`budget`].
#[derive(Debug)]
pub struct Derived(Box<[f32]>);

impl Derived {
    fn new(values: Vec<f32>) -> Self {
        BYTES.fetch_add(values.len() * 4, Ordering::Relaxed);
        Self(values.into_boxed_slice())
    }
}

impl std::ops::Deref for Derived {
    type Target = [f32];
    fn deref(&self) -> &[f32] {
        &self.0
    }
}

impl Drop for Derived {
    fn drop(&mut self) {
        BYTES.fetch_sub(self.0.len() * 4, Ordering::Relaxed);
    }
}

/// What a probe says about a cached key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// The value was computed from exactly these inputs.
    Hit,
    /// Unrelated.
    Miss,
    /// The same owner with changed inputs: never read again, replace it now.
    Replace,
}

struct Entry {
    key: Box<dyn Any + Send + Sync>,
    value: Arc<Derived>,
    used: u64,
}

#[derive(Default)]
struct Entries {
    list: Vec<Entry>,
    reads: u64,
}

#[derive(Default)]
pub struct DerivedCache {
    entries: Mutex<Entries>,
}

impl std::fmt::Debug for DerivedCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DerivedCache").field("entries", &self.len()).finish()
    }
}

impl DerivedCache {
    /// The cached value whose key `probe` calls a hit, or `compute()` it (outside the lock) and
    /// keep it under `make_key()` if there is room. Keys of other types are never probed.
    pub fn get_or_compute<K: Any + Send + Sync>(&self, probe: impl Fn(&K) -> Probe, make_key: impl FnOnce() -> K, compute: impl FnOnce() -> Vec<f32>) -> Arc<Derived> {
        let scan = |e: &mut Entries| -> (Option<usize>, Option<usize>) {
            let mut replace = None;
            for (i, x) in e.list.iter().enumerate() {
                if let Some(k) = x.key.downcast_ref::<K>() {
                    match probe(k) {
                        Probe::Hit => return (Some(i), None),
                        Probe::Replace => replace = replace.or(Some(i)),
                        Probe::Miss => {}
                    }
                }
            }
            (None, replace)
        };
        {
            let mut e = self.entries.lock();
            e.reads += 1;
            let now = e.reads;
            if let (Some(i), _) = scan(&mut e) {
                e.list[i].used = now;
                return e.list[i].value.clone();
            }
        }
        let value = Arc::new(Derived::new(compute()));
        let mut e = self.entries.lock();
        let now = e.reads;
        match scan(&mut e) {
            (Some(i), _) => return e.list[i].value.clone(), // another thread finished first
            (None, Some(i)) => {
                e.list[i] = Entry { key: Box::new(make_key()), value: value.clone(), used: now };
                return value;
            }
            (None, None) => {}
        }
        if e.list.len() < PER_TILE && cached_bytes() <= budget() {
            e.list.push(Entry { key: Box::new(make_key()), value: value.clone(), used: now });
        } else if let Some((i, oldest)) = e.list.iter().enumerate().min_by_key(|(_, x)| x.used)
            && now.saturating_sub(oldest.used) > STALE_AFTER
        {
            e.list[i] = Entry { key: Box::new(make_key()), value: value.clone(), used: now };
        }
        value
    }

    pub fn len(&self) -> usize {
        self.entries.lock().list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop every entry (and what their keys keep alive).
    pub fn clear(&self) {
        self.entries.lock().list.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_misses_and_replacement() {
        let cache = DerivedCache::default();
        let computed = std::sync::atomic::AtomicUsize::new(0);
        let get = |k: u32, owner: u32| {
            cache.get_or_compute::<(u32, u32)>(
                |&(ko, kv)| if ko != owner { Probe::Miss } else if kv == k { Probe::Hit } else { Probe::Replace },
                || (owner, k),
                || {
                    computed.fetch_add(1, Ordering::Relaxed);
                    vec![k as f32; 4]
                },
            )
        };
        assert_eq!(get(1, 7)[0], 1.0);
        assert_eq!(get(1, 7)[0], 1.0);
        assert_eq!(computed.load(Ordering::Relaxed), 1, "second read is a hit");
        assert_eq!(get(2, 8)[0], 2.0);
        assert_eq!(cache.len(), 2);
        // Same owner, new input: replaced in place, not added.
        assert_eq!(get(3, 7)[0], 3.0);
        assert_eq!(cache.len(), 2);
        assert_eq!(get(3, 7)[0], 3.0);
        assert_eq!(computed.load(Ordering::Relaxed), 3);
        // Keys of another type are never probed.
        let other = cache.get_or_compute::<String>(|_| Probe::Hit, || "x".into(), || vec![9.0]);
        assert_eq!(other[0], 9.0);
        assert_eq!(cache.len(), 3);
        cache.clear();
        assert!(cache.is_empty());
    }
}
