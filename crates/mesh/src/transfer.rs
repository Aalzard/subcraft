//! Surface correspondence between two UV layouts of (nearly) the same surface.
//!
//! When a model is replaced by an edited version whose UVs moved, painted texels have to follow
//! the surface, not the texture coordinates. For every texel of the new layout this module finds
//! the 3D surface point the texel shows (UV rasterization), the closest point on the old surface
//! (BVH nearest-point query restricted to one old material slot) and that point's old UV. Image
//! code then resamples old rasters at those UVs; nothing here touches pixels.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use glam::{Vec2, Vec3};
use rayon::prelude::*;
use subcraft_core::IRect;

use crate::bvh::Bvh;
use crate::mesh::Mesh;
use crate::uv_raster::{raster_triangle, triangle_rect, wrap_to_tile};

/// Where texels come from: one material slot of the old mesh.
#[derive(Clone, Copy)]
pub struct TransferSource<'a> {
    pub mesh: &'a Mesh,
    /// BVH over all of `mesh`'s triangles (queries are filtered to `material`).
    pub bvh: &'a Bvh,
    pub material: u32,
}

/// Where texels go: triangles of the new mesh painted by one texture set.
#[derive(Clone)]
pub struct TransferTarget<'a> {
    pub mesh: &'a Mesh,
    pub triangles: Range<usize>,
    pub resolution: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct CorrespondenceOptions {
    /// Square block size the result is grouped in (the texture engine's tile size).
    pub tile_size: u32,
    /// Conservative rim around target islands, in texels, so bilinear reads at island borders
    /// find transferred data instead of the empty gutter.
    pub expand: f32,
    /// Target points farther than this from the old surface receive nothing (new geometry).
    pub max_distance: f32,
}

/// For each texel of the target layout, the source UV that shows the same surface point.
pub struct UvCorrespondence {
    pub resolution: u32,
    pub tile_size: u32,
    tiles_x: u32,
    /// Per target tile (row-major): source UV per texel, NaN where nothing maps.
    tiles: Vec<Option<Box<[[f32; 2]]>>>,
}

impl UvCorrespondence {
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn tiles_x(&self) -> u32 {
        self.tiles_x
    }

    /// Source UVs of tile `index` (`tile_size²` entries, NaN = no data), if any texel maps.
    pub fn tile(&self, index: usize) -> Option<&[[f32; 2]]> {
        self.tiles.get(index)?.as_deref()
    }

    /// Number of target texels that receive data.
    pub fn mapped_texels(&self) -> usize {
        self.tiles.iter().flatten().map(|t| t.iter().filter(|uv| !uv[0].is_nan()).count()).sum()
    }

    /// Share of mapped texels whose source UV lies more than `tolerance` texels from the texel
    /// itself: 0 when the UV layout did not change, close to 1 when every island moved. Build
    /// with `expand = 0` for this: rim texels on seams may resolve to the neighbouring island.
    pub fn moved_fraction(&self, tolerance: f32) -> f32 {
        let (mut mapped, mut moved) = (0usize, 0usize);
        let r = self.resolution as f32;
        let n = self.tile_size;
        for (i, tile) in self.tiles.iter().enumerate() {
            let Some(tile) = tile else { continue };
            let (tx, ty) = (i as u32 % self.tiles_x, i as u32 / self.tiles_x);
            for (k, uv) in tile.iter().enumerate() {
                if uv[0].is_nan() {
                    continue;
                }
                mapped += 1;
                let x = (tx * n + k as u32 % n) as f32 + 0.5;
                let y = (ty * n + k as u32 / n) as f32 + 0.5;
                if (Vec2::new(uv[0] * r, uv[1] * r) - Vec2::new(x, y)).length() > tolerance {
                    moved += 1;
                }
            }
        }
        if mapped == 0 { 0.0 } else { moved as f32 / mapped as f32 }
    }
}

/// Build the correspondence. `done` counts finished target tiles (progress; the total is
/// `ceil(resolution / tile_size)²`); returns `None` when `cancel` was raised.
pub fn build_correspondence(
    source: TransferSource<'_>,
    target: TransferTarget<'_>,
    options: CorrespondenceOptions,
    done: &AtomicUsize,
    cancel: &AtomicBool,
) -> Option<UvCorrespondence> {
    let res = target.resolution;
    let n = options.tile_size;
    let tiles_x = res.div_ceil(n);
    let tile_count = (tiles_x * tiles_x) as usize;
    // Bin target triangles per tile so each tile rasterizes only what touches it.
    let mut bins: Vec<Vec<usize>> = vec![Vec::new(); tile_count];
    for t in target.triangles.clone() {
        let r = triangle_rect(target.mesh.tri_uvs(t), res, res, options.expand);
        if r.is_empty() {
            continue;
        }
        for ty in (r.y0 as u32 / n)..=((r.y1 as u32 - 1) / n) {
            for tx in (r.x0 as u32 / n)..=((r.x1 as u32 - 1) / n) {
                bins[(ty * tiles_x + tx) as usize].push(t);
            }
        }
    }
    let normals: Vec<Vec3> = (0..source.mesh.triangle_count()).map(|t| source.mesh.geometric_normal(t)).collect();
    let in_slot = |t: u32| source.mesh.tri_material[t as usize] == source.material;
    let tiles: Vec<Option<Box<[[f32; 2]]>>> = bins
        .par_iter()
        .enumerate()
        .map(|(i, bin)| {
            let out = if bin.is_empty() || cancel.load(Ordering::Relaxed) {
                None
            } else {
                let (tx, ty) = (i as u32 % tiles_x, i as u32 / tiles_x);
                let region = IRect::new((tx * n) as i32, (ty * n) as i32, ((tx + 1) * n) as i32, ((ty + 1) * n) as i32);
                map_tile(&source, &target, &options, bin, region, &normals, &in_slot)
            };
            done.fetch_add(1, Ordering::Relaxed);
            out
        })
        .collect();
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    Some(UvCorrespondence { resolution: res, tile_size: n, tiles_x, tiles })
}

fn map_tile(
    source: &TransferSource<'_>,
    target: &TransferTarget<'_>,
    options: &CorrespondenceOptions,
    bin: &[usize],
    region: IRect,
    normals: &[Vec3],
    in_slot: &(dyn Fn(u32) -> bool + Sync),
) -> Option<Box<[[f32; 2]]>> {
    let n = options.tile_size as usize;
    let res = target.resolution;
    // Nearest triangle per texel (inside beats rim), as in `rasterize_surface_region`.
    let mut best = vec![f32::INFINITY; n * n];
    let mut point = vec![(0u32, [0.0f32; 3]); n * n];
    for &t in bin {
        raster_triangle(target.mesh.tri_uvs(t), res, res, region, options.expand, |h| {
            let i = (h.y - region.y0) as usize * n + (h.x - region.x0) as usize;
            let d = h.distance.max(0.0);
            if d < best[i] {
                best[i] = d;
                point[i] = (t as u32, h.bary);
            }
        });
    }
    let mut out = vec![[f32::NAN; 2]; n * n];
    let mut any = false;
    for i in 0..n * n {
        if !best[i].is_finite() {
            continue;
        }
        let (t, b) = point[i];
        let sp = target.mesh.interpolate(t as usize, b[1], b[2]);
        // Prefer old triangles facing the same way, so the two sides of a thin shell keep their
        // own paint; fall back to any side (strongly reshaped areas).
        let facing = |s: u32| in_slot(s) && normals[s as usize].dot(sp.normal) > 0.0;
        let Some(hit) = source.bvh.closest_point(sp.position, options.max_distance, facing).or_else(|| source.bvh.closest_point(sp.position, options.max_distance, in_slot)) else {
            continue;
        };
        let (uv, _) = wrap_to_tile(source.mesh.tri_uvs(hit.tri as usize));
        let s = uv[0] * (1.0 - hit.u - hit.v) + uv[1] * hit.u + uv[2] * hit.v;
        out[i] = [s.x, s.y];
        any = true;
    }
    any.then(|| out.into_boxed_slice())
}

/// Texels of a `resolution²` layout covered by `triangles` of `mesh` (grown by `expand` texels).
pub fn uv_coverage(mesh: &Mesh, triangles: Range<usize>, resolution: u32, expand: f32) -> Vec<bool> {
    let mut out = vec![false; (resolution * resolution) as usize];
    let clip = IRect::from_size(resolution, resolution);
    for t in triangles {
        raster_triangle(mesh.tri_uvs(t), resolution, resolution, clip, expand, |h| {
            out[(h.y as u32 * resolution + h.x as u32) as usize] = true;
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn build(old: &Mesh, new: &Mesh, res: u32, expand: f32) -> UvCorrespondence {
        let bvh = Bvh::build(old);
        let source = TransferSource { mesh: old, bvh: &bvh, material: 0 };
        let target = TransferTarget { mesh: new, triangles: new.material_triangles(0), resolution: res };
        let options = CorrespondenceOptions { tile_size: 64, expand, max_distance: f32::INFINITY };
        build_correspondence(source, target, options, &AtomicUsize::new(0), &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn same_layout_maps_every_texel_onto_itself() {
        let cube = primitives::cube(1.0);
        // Only texels inside islands: rim texels sit on seams, where the other island is as close.
        let corr = build(&cube, &cube, 128, 0.0);
        assert!(corr.mapped_texels() > 0);
        assert!(corr.moved_fraction(1.0) < 0.02, "moved {}", corr.moved_fraction(1.0));
    }

    #[test]
    fn mirrored_layout_is_detected_as_moved() {
        let cube = primitives::cube(1.0);
        let mut flipped = cube.clone();
        for uv in &mut flipped.uvs {
            uv.x = 1.0 - uv.x;
        }
        let corr = build(&cube, &flipped, 128, 1.5);
        assert!(corr.moved_fraction(1.0) > 0.5, "moved {}", corr.moved_fraction(1.0));
        // Every mapped texel points back at the mirrored source texel.
        let tile = (0..corr.tile_count()).find_map(|i| corr.tile(i).map(|t| (i, t))).unwrap();
        let (tx, ty) = (tile.0 as u32 % corr.tiles_x(), tile.0 as u32 / corr.tiles_x());
        for (k, uv) in tile.1.iter().enumerate().filter(|(_, uv)| !uv[0].is_nan()).step_by(37) {
            let x = (tx * 64 + k as u32 % 64) as f32 + 0.5;
            let y = (ty * 64 + k as u32 / 64) as f32 + 0.5;
            // Rim texels are clamped onto their triangle, so allow the rim width.
            assert!((uv[0] * 128.0 - (128.0 - x)).abs() < 2.0, "u {} vs {}", uv[0] * 128.0, 128.0 - x);
            assert!((uv[1] * 128.0 - y).abs() < 2.0);
        }
    }

    #[test]
    fn coverage_marks_only_islands() {
        let plane = primitives::plane(1.0, 1);
        let cov = uv_coverage(&plane, 0..plane.triangle_count(), 32, 0.0);
        assert!(cov.iter().all(|&c| c), "a full-square plane covers everything");
    }
}
