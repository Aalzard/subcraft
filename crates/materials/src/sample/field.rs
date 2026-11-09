//! Per-tile cache of evaluated procedural fields.
//!
//! A procedural pattern through a projection depends only on its parameters and the tile's
//! surface geometry, never on colors, blending, masks or other layers. Evaluating it is by far
//! the most expensive part of compositing (fractal noise per texel), so every tile geometry
//! keeps the fields it has evaluated, keyed by the exact parameter bits. Editing anything else
//! (a generator level, fill colors, opacity, blend modes, levels…) then reuses the fields, and
//! results are bit-identical to evaluating again.
//!
//! Memory is bounded twice: a per-tile cap with least-recently-used eviction, and a process-wide
//! byte budget beyond which new fields are computed uncached. The cap is the budget shared fairly
//! by every tile holding fields ([`PER_TILE`] at most, [`MIN_PER_TILE`] at least), so on a big set
//! early tiles cannot take the whole budget and leave later tiles with nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use parking_lot::Mutex;
use subcraft_doc::{ProceduralParams, Projection};
use subcraft_image::TILE_TEXELS;

/// Most fields kept per tile. A few smart materials with triplanar fills already need a dozen or
/// more per tile; with fewer slots than a stack uses, least-recently-used eviction drops every field
/// before its next use and each recomposite evaluates all noise again.
pub const PER_TILE: usize = 32;
/// Fewest fields kept per tile, whatever the set size (the cap before the budget was shared).
pub const MIN_PER_TILE: usize = 8;
/// Process-wide budget for cached field data (all tiles of all texture sets).
pub const BUDGET_BYTES: usize = 768 << 20;

/// Tiles currently holding at least one field.
static TILES: AtomicUsize = AtomicUsize::new(0);

/// Fields a tile may keep now: the budget shared by the tiles holding fields (32 at 2048², 12 at
/// 4096² with one texture set), within [`MIN_PER_TILE`]..=[`PER_TILE`].
pub fn per_tile_cap() -> usize {
    let field = TILE_TEXELS * std::mem::size_of::<f32>();
    (BUDGET_BYTES / (TILES.load(Ordering::Relaxed).max(1) * field)).clamp(MIN_PER_TILE, PER_TILE)
}

static BYTES: AtomicUsize = AtomicUsize::new(0);
static CLOCK: AtomicU64 = AtomicU64::new(0);

/// Bytes currently held by cached fields (diagnostics and tests).
pub fn cached_bytes() -> usize {
    BYTES.load(Ordering::Relaxed)
}

/// Exact identity of a procedural field: every parameter bit that changes its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldKey([u32; 16]);

impl FieldKey {
    pub fn edge_distance() -> Self {
        let mut key = Self::new(&ProceduralParams::new(subcraft_doc::ProceduralKind::Clouds), &Projection::default());
        key.0[6] = u32::MAX - 1;
        key
    }
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
            scratch_bits(&params.scratch),
            0,
        ])
    }
    /// The same field sampled `offset` (object-space, in units of the model's longest dimension)
    /// away. A zero offset keeps the original key, so unmoved patterns share their cache entry.
    pub fn at(mut self, offset: [f32; 3]) -> Self {
        if offset != [0.0; 3] {
            self.0[15] = offset.iter().fold(0x811c_9dc5u32, |h, v| (h ^ v.to_bits()).wrapping_mul(0x0100_0193).rotate_left(5)) | 1;
        }
        self
    }
}

/// Hash of every scratch setting (0 for the defaults, so other kinds share keys).
fn scratch_bits(s: &subcraft_doc::ScratchParams) -> u32 {
    if s.is_default() {
        return 0;
    }
    [s.angle_deg, s.spread_deg, s.length, s.length_var, s.width, s.width_var, s.density, s.curvature, s.depth_falloff, s.cluster]
        .iter()
        .fold(0x811c_9dc5u32, |h, v| (h ^ v.to_bits()).wrapping_mul(0x0100_0193).rotate_left(5))
        | 1
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
        let cap = per_tile_cap();
        while entries.len() >= cap {
            let oldest = entries.iter().enumerate().min_by_key(|(_, e)| e.used).map(|(i, _)| i).unwrap();
            entries.swap_remove(oldest);
        }
        if cached_bytes() <= BUDGET_BYTES {
            if entries.is_empty() {
                TILES.fetch_add(1, Ordering::Relaxed);
            }
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

impl Drop for FieldCache {
    fn drop(&mut self) {
        if !self.entries.get_mut().is_empty() {
            TILES.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
