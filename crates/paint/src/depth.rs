//! CPU camera depth map for paint visibility (no GPU readback needed).
//!
//! Stores linear view depth per pixel. Perspective-correct interpolation: `depth/w` and `1/w`
//! are screen-linear for both perspective and orthographic projections.

use std::sync::Arc;

use glam::{Vec2, Vec3};
use parking_lot::Mutex;
use subcraft_mesh::Mesh;

use crate::camera::Camera;

pub struct DepthMap {
    pub width: u32,
    pub height: u32,
    /// Linear view depth; `f32::INFINITY` where nothing was rasterized.
    pub depth: Vec<f32>,
    /// Constant part of the visibility bias (object units).
    pub bias: f32,
}

impl DepthMap {
    /// Rasterize every triangle of `mesh` (all texture sets occlude).
    pub fn render(mesh: &Mesh, camera: &Camera) -> Self {
        let (w, h) = (camera.viewport[0].max(1), camera.viewport[1].max(1));
        let mut depth = vec![f32::INFINITY; (w * h) as usize];
        let vp = camera.view_proj();
        for t in 0..mesh.triangle_count() {
            let p = mesh.tri_positions(t);
            let clip = p.map(|v| vp * v.extend(1.0));
            if clip.iter().any(|c| c.w <= 1e-6 || c.z < 0.0) {
                continue; // crosses the near plane or the eye: skipped (no clipping)
            }
            let s = clip.map(|c| camera.ndc_to_screen(Vec2::new(c.x / c.w, c.y / c.w)));
            let inv_w = clip.map(|c| 1.0 / c.w);
            let d_w = [camera.view_depth(p[0]) * inv_w[0], camera.view_depth(p[1]) * inv_w[1], camera.view_depth(p[2]) * inv_w[2]];
            let area = (s[1] - s[0]).perp_dot(s[2] - s[0]);
            if area.abs() < 1e-9 {
                continue;
            }
            let min = s[0].min(s[1]).min(s[2]);
            let max = s[0].max(s[1]).max(s[2]);
            let x0 = (min.x - 0.5).ceil().max(0.0) as u32;
            let y0 = (min.y - 0.5).ceil().max(0.0) as u32;
            let x1 = ((max.x - 0.5).floor() as i64).min(w as i64 - 1);
            let y1 = ((max.y - 0.5).floor() as i64).min(h as i64 - 1);
            if x1 < x0 as i64 || y1 < y0 as i64 {
                continue;
            }
            for y in y0..=y1 as u32 {
                for x in x0..=x1 as u32 {
                    let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let b0 = (s[2] - s[1]).perp_dot(q - s[1]) / area;
                    let b1 = (s[0] - s[2]).perp_dot(q - s[2]) / area;
                    let b2 = 1.0 - b0 - b1;
                    if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                        continue;
                    }
                    let iw = b0 * inv_w[0] + b1 * inv_w[1] + b2 * inv_w[2];
                    let d = (b0 * d_w[0] + b1 * d_w[1] + b2 * d_w[2]) / iw;
                    let o = (y * w + x) as usize;
                    if d < depth[o] {
                        depth[o] = d;
                    }
                }
            }
        }
        Self { width: w, height: h, depth, bias: 1e-4 * mesh.bounds.diagonal() }
    }

    #[inline]
    fn at(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return f32::INFINITY;
        }
        self.depth[(y as u32 * self.width + x as u32) as usize]
    }

    /// True if surface point `p` with geometric normal `n` is not hidden behind other geometry.
    /// Points outside the viewport are never visible. The bias grows with the surface slope
    /// (one pixel's depth extent), and the 2×2 pixels around the projection are tested
    /// leniently (farthest wins) to avoid self-occlusion at pixel-sized creases.
    pub fn visible(&self, camera: &Camera, p: Vec3, n: Vec3) -> bool {
        let Some(s) = camera.project(p) else { return false };
        if s.x < 0.0 || s.y < 0.0 || s.x >= self.width as f32 || s.y >= self.height as f32 {
            return false;
        }
        let (fx, fy) = ((s.x - 0.5).floor() as i64, (s.y - 0.5).floor() as i64);
        let far = self.at(fx, fy).max(self.at(fx + 1, fy)).max(self.at(fx, fy + 1)).max(self.at(fx + 1, fy + 1));
        if far.is_infinite() {
            return true;
        }
        let cos = n.dot(-camera.projection_dir(p)).abs().max(0.05);
        let tan = (1.0 - cos * cos).max(0.0).sqrt() / cos;
        let pixel = 1.0 / camera.pixels_per_unit(s.z);
        s.z <= far + self.bias + 2.0 * pixel * tan
    }
}

/// Depth maps cached per camera; a camera change renders a new map. Holds a few entries so the
/// mirrored cameras of symmetry painting stay cached alongside the main one.
pub struct DepthCache {
    entries: Mutex<Vec<(Camera, Arc<DepthMap>)>>,
}

const CACHE_ENTRIES: usize = 8;

impl Default for DepthCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DepthCache {
    pub fn new() -> Self {
        Self { entries: Mutex::new(Vec::new()) }
    }

    pub fn get(&self, mesh: &Mesh, camera: &Camera) -> Arc<DepthMap> {
        let mut e = self.entries.lock();
        if let Some(pos) = e.iter().position(|(c, _)| c == camera) {
            let entry = e.remove(pos);
            let map = entry.1.clone();
            e.push(entry);
            return map;
        }
        let map = Arc::new(DepthMap::render(mesh, camera));
        if e.len() >= CACHE_ENTRIES {
            e.remove(0);
        }
        e.push((*camera, map.clone()));
        map
    }

    pub fn len(&self) -> usize {
        self.entries.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Drop all maps (the mesh changed).
    pub fn clear(&self) {
        self.entries.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_mesh::primitives;

    #[test]
    fn cube_depth_and_visibility() {
        let mesh = primitives::cube(2.0);
        let cam = Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 50.0, [128, 128]);
        let map = DepthMap::render(&mesh, &cam);
        let center = map.depth[(64 * 128 + 64) as usize];
        assert!((center - 4.0).abs() < 1e-3, "front face at depth 4, got {center}");
        assert!(map.depth[0].is_infinite());
        assert!(map.visible(&cam, Vec3::new(0.2, 0.1, 1.0), Vec3::Z));
        assert!(!map.visible(&cam, Vec3::new(0.2, 0.1, -1.0), -Vec3::Z), "back face hidden");
    }

    #[test]
    fn cache_reuses_until_camera_changes() {
        let mesh = primitives::cube(2.0);
        let cache = DepthCache::new();
        let a = Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 50.0, [64, 64]);
        let m1 = cache.get(&mesh, &a);
        assert!(Arc::ptr_eq(&m1, &cache.get(&mesh, &a)));
        let b = Camera { viewport: [64, 32], ..a };
        assert!(!Arc::ptr_eq(&m1, &cache.get(&mesh, &b)));
        assert_eq!(cache.len(), 2);
    }
}
