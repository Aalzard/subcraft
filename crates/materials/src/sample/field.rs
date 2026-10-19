//! Per-tile cache of evaluated procedural fields.
//!
//! A procedural pattern through a projection depends only on its parameters and the tile's
//! surface geometry, never on colors, blending, masks or other layers. Evaluating it is by far
//! the most expensive part of compositing (fractal noise per texel), so every tile geometry
//! keeps the fields it has evaluated, keyed by the exact parameter bits. Editing anything else
//! (a generator level, fill colors, opacity, blend modes, levels…) then reuses the fields, and
//! results are bit-identical to evaluating again.
//!
//! Memory is bounded twice: at most [`PER_TILE`] fields per tile (least recently used are
//! dropped) and a process-wide byte budget beyond which new fields are computed uncached.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use parking_lot::Mutex;
use subcraft_doc::{ProceduralParams, Projection};
use subcraft_image::TILE_TEXELS;

/// Fields kept per tile (a typical stack has a handful of procedural fills/noises).
pub const PER_TILE: usize = 8;
/// Process-wide budget for cached field data (all tiles of all texture sets).
pub const BUDGET_BYTES: usize = 768 << 20;

static BYTES: AtomicUsize = AtomicUsize::new(0);
static CLOCK: AtomicU64 = AtomicU64::new(0);

/// Bytes currently held by cached fields (diagnostics and tests).
pub fn cached_bytes() -> usize {
    BYTES.load(Ordering::Relaxed)
}

/// Exact identity of a procedural field: every parameter bit that changes its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldKey([u32; 14]);

impl FieldKey {
    pub fn organic(params: &ProceduralParams) -> Self {
        let mut key = Self::new(params, &Projection::default());
        key.0[6] = u32::MAX; // distinct from every supported projection mode
        key
    }
    pub fn new(params: &ProceduralParams, projection: &Projection) -> Self {
        let p = projection;
        Self([
            params.kind as u32,
            params.scale.to_bits(),
            params.seed,
            params.balance.to_bits(),
            params.contrast.to_bits(),
            params.invert as u32,
            p.mode as u32,
            p.scale[0].to_bits(),
            p.scale[1].to_bits(),
            p.rotation_deg.to_bits(),
            p.offset[0].to_bits(),
            p.offset[1].to_bits(),
            p.triplanar_sharpness.to_bits(),
            p.planar_axis as u32,
        ])
    }
}

/// One tile's evaluated field (`TILE_TEXELS` values). Its bytes count against the budget.
#[derive(Debug)]
pub struct Field(Box<[f32]>);

impl Field {
    fn new(values: Box<[f32]>) -> Self {
        BYTES.fetch_add(values.len() * 4, Ordering::Relaxed);
        Self(values)
    }
}

impl std::ops::Deref for Field {
    type Target = [f32];
    fn deref(&self) -> &[f32] {
        &self.0
    }
}

impl Drop for Field {
    fn drop(&mut self) {
        BYTES.fetch_sub(self.0.len() * 4, Ordering::Relaxed);
    }
}

#[derive(Debug)]
struct Entry {
    key: FieldKey,
    field: Arc<Field>,
    used: u64,
}

#[derive(Debug, Default)]
pub struct FieldCache {
    entries: Mutex<Vec<Entry>>,
}

impl FieldCache {
    /// The cached field for `key`, or `compute()` it (outside the lock) and keep it.
    pub fn get_or_compute(&self, key: FieldKey, compute: impl FnOnce() -> Vec<f32>) -> Arc<Field> {
        let now = CLOCK.fetch_add(1, Ordering::Relaxed);
        if let Some(e) = self.entries.lock().iter_mut().find(|e| e.key == key) {
            e.used = now;
            return e.field.clone();
        }
        let values = compute();
        debug_assert_eq!(values.len(), TILE_TEXELS);
        let field = Arc::new(Field::new(values.into_boxed_slice()));
        let mut entries = self.entries.lock();
        if let Some(e) = entries.iter().find(|e| e.key == key) {
            return e.field.clone(); // another thread finished first
        }
        if entries.len() >= PER_TILE {
            let oldest = entries.iter().enumerate().min_by_key(|(_, e)| e.used).map(|(i, _)| i).unwrap();
            entries.swap_remove(oldest);
        }
        if cached_bytes() <= BUDGET_BYTES {
            entries.push(Entry { key, field: field.clone(), used: now });
        }
        field
    }

    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
