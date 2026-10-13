//! Stamp kernel: candidate triangles → tiles → UV rasterization → per-texel coverage.
//!
//! For a batch of stamps, each stamp's candidate triangles of the active texture set are found
//! with a BVH volume query and binned by the texture tiles their UV footprint touches. Tiles are
//! then processed in parallel: each candidate triangle is UV-rasterized (≈1 texel rim, so seams
//! get no gaps), the texel's 3D position and geometric normal are reconstructed, and every stamp
//! relevant to the tile evaluates its footprint there. Overlapping triangles of one stamp combine
//! by max; stamps accumulate into the stroke coverage as `cov += a·(1 − cov)` with `a` = footprint
//! × flow, so coverage never exceeds 1 and the stroke opacity caps the final result.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_core::IRect;
use subcraft_image::{TILE_SIZE, TILE_TEXELS};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::uv_raster::{bary_point, raster_triangle, triangle_rect};
use subcraft_mesh::{Bvh, Mesh};

use crate::camera::Camera;
use crate::depth::DepthMap;
use crate::settings::Alignment;
use crate::stamp::Stamp;

/// Rim (texels) rasterized around each triangle.
pub const KERNEL_EXPAND: f32 = 1.0;

pub struct KernelInput<'a> {
    pub mesh: &'a Mesh,
    pub bvh: &'a Bvh,
    pub geometry: &'a TextureSetGeometry,
    pub alignment: Alignment,
    pub cameras: &'a [Camera],
    /// Depth map per camera (unused for UV alignment).
    pub depth: &'a [Arc<DepthMap>],
    /// Minimum `n · -projection_dir` for a texel to be painted; `None` disables culling.
    pub backface_cos: Option<f32>,
    /// Brush alpha (tip image); `None` = round procedural tip.
    pub alpha: Option<&'a subcraft_image::Bitmap>,
}

/// A rasterized texel of one triangle.
#[derive(Clone, Copy, Debug)]
pub struct TexelSample {
    /// Texel index within its tile.
    pub idx: u16,
    pub p: Vec3,
    pub ng: Vec3,
    pub uv: Vec2,
}

/// Work for one tile: triangles to rasterize and stamps to evaluate.
#[derive(Default, Debug)]
pub struct TileWork {
    pub tris: BTreeSet<u32>,
    pub stamps: BTreeSet<u32>,
}

impl KernelInput<'_> {
    fn in_set(&self, t: u32) -> bool {
        self.mesh.tri_material[t as usize] == self.geometry.material_slot
    }

    /// Triangles of the active texture set the stamp can touch.
    pub fn candidates(&self, s: &Stamp) -> Vec<u32> {
        let reach = crate::stamp::reach(self.alpha.is_some());
        let mut out = match self.alignment {
            Alignment::TangentWrap => self.bvh.query_sphere(s.center, s.radius * reach),
            Alignment::TangentPlanar => self.bvh.query_sphere(s.center, s.radius * reach * std::f32::consts::SQRT_2),
            Alignment::Camera => {
                let cam = &self.cameras[s.camera];
                let r_px = s.radius_px * reach;
                let (lo, hi) = (s.screen - Vec2::splat(r_px), s.screen + Vec2::splat(r_px));
                let overlaps = |pts: &mut dyn Iterator<Item = Vec3>| {
                    let (mut min, mut max) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
                    for p in pts {
                        let Some(q) = cam.project(p) else { return true };
                        min = min.min(q.truncate());
                        max = max.max(q.truncate());
                    }
                    min.x <= hi.x && max.x >= lo.x && min.y <= hi.y && max.y >= lo.y
                };
                let mut v = Vec::new();
                self.bvh.query(
                    |b| {
                        let c = [b.min, b.max];
                        overlaps(&mut (0..8).map(|i| Vec3::new(c[i & 1].x, c[(i >> 1) & 1].y, c[(i >> 2) & 1].z)))
                    },
                    |t| {
                        if overlaps(&mut self.mesh.tri_positions(t as usize).into_iter()) {
                            v.push(t);
                        }
                    },
                );
                v
            }
            Alignment::Uv => {
                let res = self.geometry.resolution as f32;
                let r = s.radius_uv * reach * res + KERNEL_EXPAND + 1.0;
                let c = s.uv * res;
                let rect = IRect::new((c.x - r).floor() as i32, (c.y - r).floor() as i32, (c.x + r).ceil() as i32, (c.y + r).ceil() as i32);
                let mut set = BTreeSet::new();
                for tile in self.tiles_in(rect) {
                    set.extend(self.geometry.tile_triangles(tile).iter().copied());
                }
                set.into_iter().collect()
            }
        };
        out.retain(|&t| self.in_set(t));
        out
    }

    fn tiles_in(&self, r: IRect) -> Vec<usize> {
        let res = self.geometry.resolution as i32;
        let r = r.intersect(&IRect::new(0, 0, res, res));
        if r.is_empty() {
            return Vec::new();
        }
        let ts = TILE_SIZE as i32;
        let tx = self.geometry.tiles_x as usize;
        let mut out = Vec::new();
        for ty in r.y0 / ts..=(r.y1 - 1) / ts {
            for x in r.x0 / ts..=(r.x1 - 1) / ts {
                out.push(ty as usize * tx + x as usize);
            }
        }
        out
    }

    /// Tiles touched by a triangle's rim-expanded UV footprint.
    pub fn triangle_tiles(&self, t: u32) -> Vec<usize> {
        let res = self.geometry.resolution;
        self.tiles_in(triangle_rect(self.mesh.tri_uvs(t as usize), res, res, KERNEL_EXPAND))
    }

    /// Bin a batch of stamps by tile.
    pub fn bin(&self, stamps: &[Stamp]) -> BTreeMap<usize, TileWork> {
        let mut bins: BTreeMap<usize, TileWork> = BTreeMap::new();
        for (si, s) in stamps.iter().enumerate() {
            for t in self.candidates(s) {
                for tile in self.triangle_tiles(t) {
                    let w = bins.entry(tile).or_default();
                    w.tris.insert(t);
                    w.stamps.insert(si as u32);
                }
            }
        }
        bins
    }

    /// UV-rasterize triangles within one tile.
    pub fn rasterize_tile(&self, tile: usize, tris: impl IntoIterator<Item = u32>) -> Vec<TexelSample> {
        let res = self.geometry.resolution;
        let tx = self.geometry.tiles_x as usize;
        let (x0, y0) = ((tile % tx) as i32 * TILE_SIZE as i32, (tile / tx) as i32 * TILE_SIZE as i32);
        let clip = IRect::new(x0, y0, x0 + TILE_SIZE as i32, y0 + TILE_SIZE as i32);
        let mut out = Vec::new();
        for t in tris {
            let pos = self.mesh.tri_positions(t as usize);
            let ng = self.mesh.geometric_normal(t as usize);
            raster_triangle(self.mesh.tri_uvs(t as usize), res, res, clip, KERNEL_EXPAND, |h| {
                out.push(TexelSample {
                    idx: ((h.y - y0) as u32 * TILE_SIZE + (h.x - x0) as u32) as u16,
                    p: bary_point(&pos, h.bary),
                    ng,
                    uv: Vec2::new((h.x as f32 + 0.5) / res as f32, (h.y as f32 + 0.5) / res as f32),
                });
            });
        }
        out
    }

    fn visible(&self, camera: usize, s: &TexelSample) -> bool {
        if self.alignment == Alignment::Uv {
            return true;
        }
        let cam = &self.cameras[camera];
        if let Some(min_cos) = self.backface_cos
            && s.ng.dot(-cam.projection_dir(s.p)) < min_cos
        {
            return false;
        }
        self.depth[camera].visible(cam, s.p, s.ng)
    }

    /// Accumulate the given stamps into a tile's stroke coverage (`TILE_TEXELS` values).
    pub fn accumulate(&self, samples: &[TexelSample], stamps: &[Stamp], tile_stamps: &BTreeSet<u32>, cov: &mut [f32]) {
        debug_assert_eq!(cov.len(), TILE_TEXELS);
        let mut vis: Vec<Option<Vec<bool>>> = vec![None; self.cameras.len()];
        let mut stamp_a = vec![0.0f32; TILE_TEXELS];
        for &si in tile_stamps {
            let st = &stamps[si as usize];
            let v = vis[st.camera].get_or_insert_with(|| samples.iter().map(|s| self.visible(st.camera, s)).collect());
            let cam = &self.cameras[st.camera];
            stamp_a.fill(0.0);
            for (s, &ok) in samples.iter().zip(v.iter()) {
                if !ok {
                    continue;
                }
                let a = st.footprint(self.alignment, cam, s.p, s.uv, self.alpha);
                let slot = &mut stamp_a[s.idx as usize];
                if a > *slot {
                    *slot = a;
                }
            }
            for (c, &a) in cov.iter_mut().zip(&stamp_a) {
                let a = a * st.flow;
                if a > 0.0 {
                    *c += a * (1.0 - *c);
                }
            }
        }
    }
}
