//! Per-tile cache of evaluated procedural fields.
//!
//! A procedural pattern through a projection depends only on its parameters and the tile's
//! surface geometry, never on colors, blending, masks or other layers. Evaluating it is by far
//! the most expensive part of compositing (fractal noise per texel), so every tile geometry
//! keeps the fields it has evaluated, keyed by the exact parameter bits. Editing anything else
//! (a generator level, fill colors, opacity, blend modes, levels…) then reuses the fields, and
//! results are bit-identical to evaluating again.
//!
//! Memory is bounded twice: at most [`PER_TILE`] fields per tile and a process-wide byte budget
//! ([`set_budget`]; the app sizes it from the machine's memory).
//!
//! Replacement is scan resistant: a stack whose tiles need more fields than fit used to evict, on
//! every tile, the field the next layer was about to read (a cyclic access pattern defeats plain
//! least-recently-used), so at 4K every generator recomputed its noise on every slider step. Now
//! a field is only replaced once it has gone unused for [`STALE_AFTER`] reads of its tile (a
//! pattern whose parameters changed, say); while every cached field is still in use, the extra
//! one is computed uncached and the rest keep hitting.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;
use subcraft_doc::{ProceduralParams, Projection};
use subcraft_image::TILE_TEXELS;

/// Fields kept per tile (fills, generator breakup / grain / chips, edge distance...). A 40-layer
/// stack of procedural fills reads about 80 per tile (a pattern per channel), so a limit below
/// that recomputed most fill noise on every edit even when the byte budget had room; the budget,
/// not this count, is what bounds memory at large resolutions. A lookup scans the tile's list,
/// well under a microsecond next to the ~0.3 ms a field costs to evaluate.
pub const PER_TILE: usize = 128;
/// Default process-wide budget for cached field data (all tiles of all texture sets).
pub const BUDGET_BYTES: usize = 768 << 20;
/// Reads of a tile after which a field not read in between counts as stale (replaceable).
pub const STALE_AFTER: u64 = 2 * PER_TILE as u64;

static BYTES: AtomicUsize = AtomicUsize::new(0);
static BUDGET: AtomicUsize = AtomicUsize::new(BUDGET_BYTES);

/// Change the process-wide budget (bytes). Fields already cached stay until replaced.
pub fn set_budget(bytes: usize) {
    BUDGET.store(bytes, Ordering::Relaxed);
}

/// The process-wide budget (bytes).
pub fn budget() -> usize {
    BUDGET.load(Ordering::Relaxed)
}

/// A budget for a machine with `total_memory` bytes of RAM: a twelfth of it, at least the default
/// and at most 8 GiB. A 4096² set holds 4096 tiles of 16 KB per field, so a 40-layer stack of
/// procedural fills (about 80 fields per tile) needs about 5 GB; machines below 48 GB keep the
/// share they had.
pub fn budget_for_memory(total_memory: u64) -> usize {
    (total_memory / 12).clamp(BUDGET_BYTES as u64, 8 << 30) as usize
}

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
    pub fn grunge(params: &ProceduralParams) -> Self {
        let mut key = Self::new(params, &Projection::default());
        key.0[6] = u32::MAX - 2; // the bundled grunge map, distinct from organic and edge distance
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
            // The axis, mixed with the cylinder angle and symmetry (0 at their defaults, so
            // existing projections keep their keys).
            p.planar_axis as u32 ^ projection_extra_bits(p),
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

/// Hash of the cylinder angle, the symmetry, the 3D transform and (Warp mode) the warp grid of a
/// projection; 0 at their defaults outside Warp mode.
fn projection_extra_bits(p: &Projection) -> u32 {
    use subcraft_doc::FillSymmetry;
    let t = p.effective_transform3();
    let warp = p.mode == subcraft_doc::ProjectionMode::Warp;
    if p.cylinder_angle == 360.0 && p.symmetry.is_none() && t.is_identity() && !warp {
        return 0;
    }
    let mut values = vec![p.cylinder_angle.to_bits()];
    if !t.is_identity() {
        values.push(3);
        values.extend(t.offset.iter().chain(&t.rotation_deg).chain(&t.scale).map(|v| v.to_bits()));
    }
    if warp {
        // The field is sampled through the grid: a moved or split point changes it (QA 2026-11:
        // the cached field of the old grid kept being drawn, and two Warp fills shared one).
        let grid = p.warp_grid();
        values.extend([4, grid.cols() as u32, grid.rows() as u32]);
        for (i, point) in grid.points().iter().enumerate() {
            let [u, v] = grid.grid_coords(i);
            values.extend([point[0].to_bits(), point[1].to_bits(), point[2].to_bits(), u.to_bits(), v.to_bits()]);
        }
    }
    match p.symmetry {
        FillSymmetry::None => {}
        FillSymmetry::Mirror { axis, position } => values.extend([1, axis as u32, position.to_bits()]),
        FillSymmetry::Radial { axis, count, flip, center } => values.extend([2, axis as u32, count, flip as u32, center[0].to_bits(), center[1].to_bits(), center[2].to_bits()]),
    }
    // Each value is mixed with a full avalanche step, and the 24 bits kept are the top ones. The
    // old multiply-and-rotate fold shifted its best-mixed bits out with `<< 8`: cylinder angles
    // 120 and 144 (and 90 other groups of whole angles) got the same key, so one angle was drawn
    // with the cached field of the other.
    let mut h = 0x811c_9dc5u32;
    for v in values {
        h ^= v;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^= h >> 13;
        h = h.wrapping_mul(0xc2b2_ae35);
        h ^= h >> 16;
    }
    ((h >> 8) | 1) << 8
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
struct Entries {
    list: Vec<Entry>,
    /// Reads of this tile's fields so far (the clock of `Entry::used`).
    reads: u64,
}

#[derive(Debug, Default)]
pub struct FieldCache {
    entries: Mutex<Entries>,
}

impl FieldCache {
    /// The cached field for `key`, or `compute()` it (outside the lock) and keep it if there is room
    /// (see the module documentation for what gets replaced).
    pub fn get_or_compute(&self, key: FieldKey, compute: impl FnOnce() -> Vec<f32>) -> Arc<Field> {
        {
            let mut e = self.entries.lock();
            e.reads += 1;
            let now = e.reads;
            if let Some(hit) = e.list.iter_mut().find(|x| x.key == key) {
                hit.used = now;
                return hit.field.clone();
            }
        }
        let values = compute();
        debug_assert_eq!(values.len(), TILE_TEXELS);
        let field = Arc::new(Field::new(values.into_boxed_slice()));
        let mut e = self.entries.lock();
        if let Some(x) = e.list.iter().find(|x| x.key == key) {
            return x.field.clone(); // another thread finished first
        }
        let now = e.reads;
        let entry = Entry { key, field: field.clone(), used: now };
        if e.list.len() < PER_TILE && cached_bytes() <= budget() {
            e.list.push(entry);
        } else if let Some((i, oldest)) = e.list.iter().enumerate().min_by_key(|(_, x)| x.used)
            && now.saturating_sub(oldest.used) > STALE_AFTER
        {
            // Replacing frees as much as it takes, so this is fine over budget too.
            e.list[i] = entry;
        }
        field
    }

    pub fn len(&self) -> usize {
        self.entries.lock().list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod warp_key_tests {
    use super::*;
    use subcraft_doc::{ProceduralKind, ProjectionMode};

    /// QA 2026-11: the key ignored the warp grid, so a moved point kept the old cached field.
    #[test]
    fn the_warp_grid_is_part_of_the_field_key() {
        let params = ProceduralParams::new(ProceduralKind::Clouds);
        let flat = Projection { mode: ProjectionMode::Warp, ..Projection::default() };
        let mut grid = flat.warp_grid();
        let p = grid.points()[0];
        assert!(grid.set_point(0, [p[0] + 0.1, p[1], p[2]]));
        let moved = Projection { warp: Some(grid), ..flat };
        assert_ne!(FieldKey::new(&params, &flat), FieldKey::new(&params, &moved));
        let mut split = flat.warp_grid();
        assert!(split.split_column(0));
        assert_ne!(FieldKey::new(&params, &flat), FieldKey::new(&params, &Projection { warp: Some(split), ..flat }));
        assert_eq!(FieldKey::new(&params, &moved), FieldKey::new(&params, &moved.clone()), "stable");
        // Other modes keep their keys (a stored grid is not read there).
        let planar = Projection { mode: ProjectionMode::Planar, ..Projection::default() };
        assert_eq!(FieldKey::new(&params, &planar), FieldKey::new(&params, &Projection { warp: Some(grid), ..planar }));
    }
}
