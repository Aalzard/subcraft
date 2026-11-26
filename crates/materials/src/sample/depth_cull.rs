//! Depth culling of planar projections: a height field of the whole mesh seen from the
//! projector, so only the nearest surface along the projection direction receives the image.

use std::sync::{Arc, Mutex};

use glam::Vec3;
use subcraft_doc::Projection;
use subcraft_mesh::Mesh;

use super::geometry::TileGeometry;

/// Cells per side of the height field.
const GRID: usize = 1024;
/// Height fields kept for reuse (a few layers with depth culling at once).
const CACHE: usize = 6;

/// The mesh's nearest surface toward the projector, over a grid of the projection plane.
pub struct DepthMap {
    /// The axis the projection looks along, its sign toward the projector, the two in-plane axes.
    axis: usize,
    facing: f32,
    plane: [usize; 2],
    min: [f32; 2],
    inv_cell: [f32; 2],
    /// Height toward the projector of the nearest surface per cell (`-inf`: nothing there).
    top: Vec<f32>,
    /// Depth that still counts as the nearest surface: a couple of cells of slope.
    tolerance: f32,
}

impl DepthMap {
    /// Build the height field of `mesh` (positions normalized by `bounds_min` and `max_extent`
    /// like the tile geometry) for `projection`.
    pub fn build(projection: &Projection, mesh: &Mesh, bounds_min: Vec3, max_extent: f32) -> Self {
        let axis = (projection.planar_axis as usize).min(2);
        // The side a planar projection faces (see `Projection::facing_weight`).
        let facing = if axis == 0 { -1.0 } else { 1.0 };
        let plane = match axis { 0 => [1, 2], 1 => [0, 2], _ => [0, 1] };
        let local: Vec<[f32; 3]> = mesh.positions.iter().map(|p| projection.depth_local(((*p - bounds_min) / max_extent).to_array())).collect();
        let (mut min, mut max) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for p in &local {
            for k in 0..2 {
                min[k] = min[k].min(p[plane[k]]);
                max[k] = max[k].max(p[plane[k]]);
            }
        }
        let span = [(max[0] - min[0]).max(1e-6), (max[1] - min[1]).max(1e-6)];
        let inv_cell = [GRID as f32 / span[0], GRID as f32 / span[1]];
        let mut top = vec![f32::NEG_INFINITY; GRID * GRID];
        let cell = |v: f32, k: usize| (((v - min[k]) * inv_cell[k]).floor() as isize).clamp(0, GRID as isize - 1) as usize;
        for t in 0..mesh.triangle_count() {
            let v = [0, 1, 2].map(|k| local[mesh.indices[t * 3 + k] as usize]);
            let q = v.map(|p| [(p[plane[0]] - min[0]) * inv_cell[0], (p[plane[1]] - min[1]) * inv_cell[1], p[axis] * facing]);
            // Each vertex marks its own cell, so a triangle smaller than a cell still counts.
            for (k, p) in v.iter().enumerate() {
                let c = cell(p[plane[1]], 1) * GRID + cell(p[plane[0]], 0);
                top[c] = top[c].max(q[k][2]);
            }
            let x0 = q.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
            let x1 = (q.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max).ceil() as usize).min(GRID);
            let y0 = q.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
            let y1 = (q.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max).ceil() as usize).min(GRID);
            let area = (q[1][0] - q[0][0]) * (q[2][1] - q[0][1]) - (q[2][0] - q[0][0]) * (q[1][1] - q[0][1]);
            if area.abs() < 1e-12 {
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((q[1][0] - px) * (q[2][1] - py) - (q[2][0] - px) * (q[1][1] - py)) / area;
                    let w1 = ((q[2][0] - px) * (q[0][1] - py) - (q[0][0] - px) * (q[2][1] - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let h = w0 * q[0][2] + w1 * q[1][2] + w2 * q[2][2];
                    let c = y * GRID + x;
                    top[c] = top[c].max(h);
                }
            }
        }
        let tolerance = 2.0 * (span[0].max(span[1]) / GRID as f32) + 1e-3;
        Self { axis, facing, plane, min, inv_cell, top, tolerance }
    }

    /// Coverage 0..=1 left by depth culling for a texel at `pos` (normalized object position).
    pub fn weight(&self, projection: &Projection, pos: [f32; 3]) -> f32 {
        let Some(culling) = projection.depth_culled() else { return 1.0 };
        let p = projection.depth_local(pos);
        let x = ((p[self.plane[0]] - self.min[0]) * self.inv_cell[0]).floor();
        let y = ((p[self.plane[1]] - self.min[1]) * self.inv_cell[1]).floor();
        // A texel on the outline lands on the far border (or a rounding step past it).
        let range = -1.0..=GRID as f32;
        if !range.contains(&x) || !range.contains(&y) {
            return 1.0;
        }
        let (x, y) = (x.clamp(0.0, GRID as f32 - 1.0) as usize, y.clamp(0.0, GRID as f32 - 1.0) as usize);
        let top = self.top[y * GRID + x];
        if !top.is_finite() {
            return 1.0;
        }
        culling.weight(top - p[self.axis] * self.facing, self.tolerance)
    }
}

type Key = (usize, usize, String);

fn cache() -> &'static Mutex<Vec<(Key, Arc<DepthMap>)>> {
    static CACHE_CELL: Mutex<Vec<(Key, Arc<DepthMap>)>> = Mutex::new(Vec::new());
    &CACHE_CELL
}

/// The depth map of `projection` over the tile's mesh, built once and shared; `None` without
/// depth culling or mesh.
pub fn for_tile(projection: &Projection, geo: &TileGeometry) -> Option<Arc<DepthMap>> {
    projection.depth_culled()?;
    let (mesh, bounds_min, max_extent) = geo.mesh_frame()?;
    // What the height field depends on: the mesh, the axis, the box transform and the symmetry
    // (not the hardness, so dragging its slider does not rebuild it).
    let key: Key = (Arc::as_ptr(mesh) as usize, mesh.positions.len(), format!("{:?}|{:?}|{:?}|{:?}", mesh.bounds, projection.planar_axis, projection.transform3, projection.effective_symmetry()));
    // Built under the lock without rayon (a parallel build inside a lock reached from rayon
    // jobs can deadlock): other tiles wait for the first build instead of repeating it.
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = c.iter().position(|(k, _)| *k == key) {
        let hit = c.remove(i);
        let map = hit.1.clone();
        c.push(hit);
        return Some(map);
    }
    let map = Arc::new(DepthMap::build(projection, mesh, bounds_min, max_extent));
    if c.len() >= CACHE {
        c.remove(0);
    }
    c.push((key, map.clone()));
    Some(map)
}
