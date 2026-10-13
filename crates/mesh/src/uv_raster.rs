//! UV-space triangle rasterization — the foundation of seam-aware painting and baking.
//!
//! Texel `(x, y)` has its center at UV `((x + 0.5) / w, (y + 0.5) / h)`. A texel is covered when
//! its center lies inside the triangle; with `expand > 0` texels within that many texels of the
//! triangle are also emitted (with barycentrics clamped onto the triangle), so island borders get
//! a conservative rim and bilinear filtering never reads unpainted gutter texels.

use glam::{Vec2, Vec3};
use subcraft_core::IRect;

use crate::mesh::Mesh;

/// Callback data for one rasterized texel.
#[derive(Clone, Copy, Debug)]
pub struct TexelHit {
    pub x: i32,
    pub y: i32,
    /// Barycentric weights (sum to 1, all >= 0).
    pub bary: [f32; 3],
    /// Texel center strictly inside the triangle (not just in the expanded rim).
    pub inside: bool,
    /// Signed distance (texels) from the triangle; <= 0 inside.
    pub distance: f32,
}

/// Translate a UV triangle into the 0..1 tile containing its centroid (UDIM-ready addressing:
/// the integer offset is the tile coordinate).
pub fn wrap_to_tile(uv: [Vec2; 3]) -> ([Vec2; 3], [i32; 2]) {
    let c = (uv[0] + uv[1] + uv[2]) / 3.0;
    let off = c.floor();
    ([uv[0] - off, uv[1] - off, uv[2] - off], [off.x as i32, off.y as i32])
}

/// Texel-space bounding rectangle of a UV triangle (after tile wrapping), grown by `expand`.
pub fn triangle_rect(uv: [Vec2; 3], width: u32, height: u32, expand: f32) -> IRect {
    let (uv, _) = wrap_to_tile(uv);
    let s = Vec2::new(width as f32, height as f32);
    let p = uv.map(|t| t * s);
    let min = p[0].min(p[1]).min(p[2]) - Vec2::splat(expand + 0.5);
    let max = p[0].max(p[1]).max(p[2]) + Vec2::splat(expand + 0.5);
    IRect::new(min.x.floor() as i32, min.y.floor() as i32, max.x.ceil() as i32 + 1, max.y.ceil() as i32 + 1)
        .intersect(&IRect::from_size(width, height))
}

/// Rasterize one UV triangle clipped to `clip`.
pub fn raster_triangle(
    uv: [Vec2; 3],
    width: u32,
    height: u32,
    clip: IRect,
    expand: f32,
    mut f: impl FnMut(TexelHit),
) {
    let (uv, _) = wrap_to_tile(uv);
    let s = Vec2::new(width as f32, height as f32);
    let p = uv.map(|t| t * s);
    let area = (p[1] - p[0]).perp_dot(p[2] - p[0]);
    if area.abs() < 1e-12 {
        return;
    }
    let rect = triangle_rect(uv, width, height, expand).intersect(&clip);
    if rect.is_empty() {
        return;
    }
    // Edge i is opposite vertex i: from p[i+1] to p[i+2].
    let edges = [(p[1], p[2]), (p[2], p[0]), (p[0], p[1])];
    let lens = edges.map(|(a, b)| (b - a).length().max(1e-12));
    let sign = area.signum();
    for y in rect.y0..rect.y1 {
        let py = y as f32 + 0.5;
        for x in rect.x0..rect.x1 {
            let q = Vec2::new(x as f32 + 0.5, py);
            let mut e = [0.0f32; 3];
            let mut dist = f32::NEG_INFINITY;
            for i in 0..3 {
                let (a, b) = edges[i];
                e[i] = (b - a).perp_dot(q - a) * sign;
                // Positive inside; distance outside the edge line, in texels.
                dist = dist.max(-e[i] / lens[i]);
            }
            // Edge-line distance is a lower bound of the true distance: cheap reject first.
            if dist > expand {
                continue;
            }
            let inside = e[0] >= 0.0 && e[1] >= 0.0 && e[2] >= 0.0;
            let (bary, distance) = if inside {
                ([e[0] / area.abs(), e[1] / area.abs(), e[2] / area.abs()], dist)
            } else {
                // True distance to the triangle; edge lines alone would let the rim spike out
                // far beyond acute vertices (miter effect) into neighbouring islands.
                let (b, d) = closest_on_triangle(p, q);
                if d > expand {
                    continue;
                }
                (b, d)
            };
            f(TexelHit { x, y, bary, inside, distance });
        }
    }
}

/// Barycentrics of the closest point on triangle `p` to an outside point `q`, and its distance.
fn closest_on_triangle(p: [Vec2; 3], q: Vec2) -> ([f32; 3], f32) {
    let mut best = (f32::INFINITY, [1.0, 0.0, 0.0]);
    for i in 0..3 {
        let (a, b) = (p[i], p[(i + 1) % 3]);
        let ab = b - a;
        let t = ((q - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
        let c = a + ab * t;
        let d = c.distance_squared(q);
        if d < best.0 {
            let mut w = [0.0f32; 3];
            w[i] = 1.0 - t;
            w[(i + 1) % 3] = t;
            best = (d, w);
        }
    }
    (best.1, best.0.sqrt())
}

/// Per-texel surface data for a texture set, at a given resolution.
/// `position.w` / `normal.w` hold coverage (1 = covered by some triangle).
pub struct SurfaceTexels {
    pub width: u32,
    pub height: u32,
    pub position: Vec<[f32; 4]>,
    pub normal: Vec<[f32; 4]>,
    pub triangle: Vec<u32>,
}

impl SurfaceTexels {
    pub const NO_TRIANGLE: u32 = u32::MAX;

    pub fn covered(&self, i: usize) -> bool {
        self.triangle[i] != Self::NO_TRIANGLE
    }
}

/// Rasterize all triangles in `tris` into dense surface maps (used by bakers, fill-layer
/// projections and export coverage). Texels strictly inside a triangle win over rim texels.
pub fn rasterize_surface(mesh: &Mesh, tris: std::ops::Range<usize>, width: u32, height: u32, expand: f32) -> SurfaceTexels {
    let n = (width * height) as usize;
    let mut out = SurfaceTexels {
        width,
        height,
        position: vec![[0.0; 4]; n],
        normal: vec![[0.0; 4]; n],
        triangle: vec![SurfaceTexels::NO_TRIANGLE; n],
    };
    let mut best_dist = vec![f32::INFINITY; n];
    let full = IRect::from_size(width, height);
    for t in tris {
        let uv = mesh.tri_uvs(t);
        raster_triangle(uv, width, height, full, expand, |h| {
            let i = (h.y as u32 * width + h.x as u32) as usize;
            let d = h.distance.max(0.0);
            if d < best_dist[i] {
                best_dist[i] = d;
                let sp = mesh.interpolate(t, h.bary[1], h.bary[2]);
                out.position[i] = [sp.position.x, sp.position.y, sp.position.z, 1.0];
                out.normal[i] = [sp.normal.x, sp.normal.y, sp.normal.z, 1.0];
                out.triangle[i] = t as u32;
            }
        });
    }
    out
}

/// Interpolate a 3D position from barycentrics (helper for kernels holding only positions).
#[inline]
pub fn bary_point(p: &[Vec3; 3], b: [f32; 3]) -> Vec3 {
    p[0] * b[0] + p[1] * b[1] + p[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn half_square_covers_half_the_texels() {
        let uv = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)];
        let mut inside = 0;
        raster_triangle(uv, 64, 64, IRect::from_size(64, 64), 0.0, |h| {
            assert!(h.inside);
            let s: f32 = h.bary.iter().sum();
            assert!((s - 1.0).abs() < 1e-4);
            inside += 1;
        });
        // 64*64/2 minus the diagonal texels whose centers fall outside.
        assert!((2000..=2100).contains(&inside), "{inside}");
    }

    #[test]
    fn expansion_adds_rim_with_clamped_barycentrics() {
        let uv = [Vec2::new(0.25, 0.25), Vec2::new(0.75, 0.25), Vec2::new(0.25, 0.75)];
        let mut n0 = 0;
        raster_triangle(uv, 64, 64, IRect::from_size(64, 64), 0.0, |_| n0 += 1);
        let mut n1 = 0;
        raster_triangle(uv, 64, 64, IRect::from_size(64, 64), 1.5, |h| {
            n1 += 1;
            assert!(h.bary.iter().all(|&b| b >= -1e-6));
            assert!(h.distance <= 1.5);
        });
        assert!(n1 > n0 + 40, "{n0} -> {n1}");
    }

    #[test]
    fn rim_never_exceeds_expand_at_acute_vertices() {
        // Long thin triangle: edge-line distances would spike several texels past its tips.
        let uv = [Vec2::new(0.1, 0.5), Vec2::new(0.9, 0.48), Vec2::new(0.9, 0.52)];
        let (w, h) = (256u32, 256u32);
        let s = Vec2::new(w as f32, h as f32);
        let p = uv.map(|t| t * s);
        let mut rim = 0;
        raster_triangle(uv, w, h, IRect::from_size(w, h), 1.0, |hit| {
            let q = Vec2::new(hit.x as f32 + 0.5, hit.y as f32 + 0.5);
            let (_, true_d) = if hit.inside { ([0.0; 3], 0.0) } else { closest_on_triangle(p, q) };
            assert!(true_d <= 1.0 + 1e-4, "texel ({}, {}) is {true_d} texels from the triangle", hit.x, hit.y);
            if !hit.inside {
                rim += 1;
            }
        });
        assert!(rim > 0);
    }

    #[test]
    fn wrap_moves_triangle_to_unit_tile() {
        let (uv, tile) = wrap_to_tile([Vec2::new(1.1, 2.2), Vec2::new(1.3, 2.2), Vec2::new(1.1, 2.4)]);
        assert_eq!(tile, [1, 2]);
        assert!(uv.iter().all(|t| (0.0..1.0).contains(&t.x) && (0.0..1.0).contains(&t.y)));
    }

    #[test]
    fn cube_surface_coverage_matches_atlas() {
        let mesh = primitives::cube(2.0);
        let s = rasterize_surface(&mesh, 0..mesh.triangle_count(), 96, 64, 0.0);
        let covered = (0..s.triangle.len()).filter(|&i| s.covered(i)).count();
        // 6 cells, each with a 2% margin per side: ~ (1-4*0.02/... ) of the texels.
        let frac = covered as f32 / s.triangle.len() as f32;
        assert!(frac > 0.75 && frac < 0.95, "coverage {frac}");
        // Positions lie on the cube surface (|max coord| == 1).
        for i in 0..s.triangle.len() {
            if s.covered(i) {
                let p = Vec3::new(s.position[i][0], s.position[i][1], s.position[i][2]);
                assert!((p.abs().max_element() - 1.0).abs() < 1e-3, "{p}");
            }
        }
    }
}
