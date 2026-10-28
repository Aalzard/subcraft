//! Optional evaluation profile: `SUBCRAFT_COMPOSITE_PROFILE=1` counts graph node evaluations and
//! their exclusive time per kind. Off by default (one relaxed atomic load per node).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

static STATE: AtomicU8 = AtomicU8::new(0);
static TOTALS: Mutex<BTreeMap<&'static str, (u64, u64)>> = Mutex::new(BTreeMap::new());

thread_local! {
    static CHILD_NS: Cell<u64> = const { Cell::new(0) };
}

pub fn enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        0 => {
            let on = std::env::var_os("SUBCRAFT_COMPOSITE_PROFILE").is_some();
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        s => s == 2,
    }
}

/// Run `f`, charging its own time (children excluded) to `label`.
pub(crate) fn timed<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    let outer = CHILD_NS.with(|c| c.replace(0));
    let start = Instant::now();
    let out = f();
    let total = start.elapsed().as_nanos() as u64;
    let child = CHILD_NS.with(|c| c.replace(outer + total));
    let mut t = TOTALS.lock().unwrap_or_else(|p| p.into_inner());
    let e = t.entry(label).or_default();
    e.0 += 1;
    e.1 += total.saturating_sub(child);
    out
}

/// Take and reset the totals: `(label, evaluations, exclusive ms summed over threads)`.
pub fn take() -> Vec<(&'static str, u64, f64)> {
    let mut t = TOTALS.lock().unwrap_or_else(|p| p.into_inner());
    let mut v: Vec<_> = std::mem::take(&mut *t).into_iter().map(|(k, (n, ns))| (k, n, ns as f64 / 1e6)).collect();
    v.sort_by(|a, b| b.2.total_cmp(&a.2));
    v
}
