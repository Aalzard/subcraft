//! Picking on the displacement preview: the mesh midpoint-subdivided like the 3D view's preview
//! (see [`crate::subdivide::midpoint`]), its vertices moved along their normals by a height
//! function, and a BVH over the result. A ray hit on it is reported on the *original* mesh
//! (triangle and barycentrics of the point under the displaced surface), so everything that
//! takes a [`RayHit`] of the modelled mesh (picking, the brush, fills) lands where the artist
//! sees the surface, while painting still writes the modelled surface's texels.

use glam::{Vec2, Vec3};

use crate::{Bvh, Mesh, RayHit};

pub struct DisplacedSurface {
    level: u32,
    /// The subdivided, undisplaced mesh.
    sub: Mesh,
    /// Material slot of each subdivided vertex (from the first triangle using it).
    slot: Vec<u32>,
    /// Direction each vertex moves in.
    normals: Vec<Vec3>,
    /// Displaced positions.
    positions: Vec<Vec3>,
    bvh: Bvh,
}

impl DisplacedSurface {
    /// `mesh` subdivided `level` times, not displaced yet.
    pub fn new(mesh: &Mesh, level: u32) -> Self {
        let mut sub = mesh.clone();
        for _ in 0..level {
            sub = crate::subdivide::midpoint(&sub);
        }
        let mut slot = vec![u32::MAX; sub.positions.len()];
        for t in 0..sub.triangle_count() {
            for v in sub.tri(t) {
                if slot[v as usize] == u32::MAX {
                    slot[v as usize] = sub.tri_material.get(t).copied().unwrap_or(0);
                }
            }
        }
        let normals = if sub.normals.len() == sub.positions.len() {
            sub.normals.clone()
        } else {
            let mut acc = vec![Vec3::ZERO; sub.positions.len()];
            for t in 0..sub.triangle_count() {
                let [a, b, c] = sub.tri_positions(t);
                let n = (b - a).cross(c - a);
                for v in sub.tri(t) {
                    acc[v as usize] += n;
                }
            }
            acc.into_iter().map(|n| n.normalize_or(Vec3::Y)).collect()
        };
        let positions = sub.positions.clone();
        let bvh = Bvh::build(&sub);
        Self { level, sub, slot, normals, positions, bvh }
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn triangle_count(&self) -> usize {
        self.sub.triangle_count()
    }

    /// Move every vertex along its normal by `height(material slot, uv) * scale` (the preview's
    /// Height + Displacement x height scale) and refit the BVH.
    pub fn displace(&mut self, height: impl Fn(u32, Vec2) -> f32 + Sync, scale: f32) {
        let uvs = &self.sub.uvs;
        let (src, normals, slot) = (&self.sub.positions, &self.normals, &self.slot);
        let moved = |i: usize| -> Vec3 {
            let (Some(uv), Some(&s)) = (uvs.get(i), slot.get(i)) else { return src[i] };
            if s == u32::MAX {
                return src[i];
            }
            let h = height(s, *uv) * scale;
            if h.is_finite() { src[i] + normals[i] * h } else { src[i] }
        };
        self.positions = (0..src.len()).map(moved).collect();
        let tris: Vec<[Vec3; 3]> = (0..self.sub.triangle_count()).map(|t| self.sub.tri(t).map(|v| self.positions[v as usize])).collect();
        self.bvh.refit(&tris);
    }

    /// Displaced position of a point given on the original mesh (triangle and barycentrics), for
    /// drawing a cursor on the displaced surface. `None` if `tri` is not of this mesh.
    pub fn displaced_point(&self, tri: u32, u: f32, v: f32) -> Option<Vec3> {
        // Find the child triangle containing (u, v) level by level.
        let base = tri as usize;
        if base >= self.sub.triangle_count() >> (2 * self.level) {
            return None;
        }
        let mut corners = [Vec3::X, Vec3::Y, Vec3::Z];
        let b = Vec3::new(1.0 - u - v, u, v);
        let mut sub = base;
        for _ in 0..self.level {
            let children = split(corners);
            let k = (0..4).find(|&k| inside(children[k], b)).unwrap_or(3);
            corners = children[k];
            sub = sub * 4 + k;
        }
        // Barycentrics of `b` in the child, then the displaced child corners.
        let w = solve(corners, b);
        let p = self.sub.tri(sub).map(|i| self.positions[i as usize]);
        Some(p[0] * w.x + p[1] * w.y + p[2] * w.z)
    }

    /// The nearest hit on the displaced surface, reported on the original mesh: `tri` is the
    /// original triangle, `u`/`v` its barycentrics, `t` the distance to the displaced surface.
    /// `accept` filters original triangles.
    pub fn intersect_filtered(&self, origin: Vec3, dir: Vec3, t_min: f32, t_max: f32, accept: impl Fn(u32) -> bool) -> Option<RayHit> {
        let shift = 2 * self.level;
        let hit = self.bvh.intersect_filtered(origin, dir, t_min, t_max, |t| accept(t >> shift))?;
        let corners = corners_in_base(hit.tri, self.level);
        let b = corners[0] * (1.0 - hit.u - hit.v) + corners[1] * hit.u + corners[2] * hit.v;
        Some(RayHit { tri: hit.tri >> shift, t: hit.t, u: b.y, v: b.z, front_face: hit.front_face })
    }

    pub fn intersect(&self, origin: Vec3, dir: Vec3, t_min: f32, t_max: f32) -> Option<RayHit> {
        self.intersect_filtered(origin, dir, t_min, t_max, |_| true)
    }
}

/// The four children of a triangle in the order [`crate::subdivide::midpoint`] makes them.
fn split(c: [Vec3; 3]) -> [[Vec3; 3]; 4] {
    let (ab, bc, ca) = ((c[0] + c[1]) * 0.5, (c[1] + c[2]) * 0.5, (c[2] + c[0]) * 0.5);
    [[c[0], ab, ca], [ab, c[1], bc], [ca, bc, c[2]], [ab, bc, ca]]
}

/// Barycentric coordinates (in the original triangle) of the corners of subdivided triangle
/// `sub`: its index holds the child chosen at each level, two bits per level, coarsest first.
fn corners_in_base(sub: u32, level: u32) -> [Vec3; 3] {
    let mut c = [Vec3::X, Vec3::Y, Vec3::Z];
    for l in (0..level).rev() {
        c = split(c)[((sub >> (2 * l)) & 3) as usize];
    }
    c
}

/// Weights of `b` (barycentrics in the original triangle) relative to the corners `c` of a
/// child triangle (also in barycentrics): a 2D solve in the (u, v) plane.
fn solve(c: [Vec3; 3], b: Vec3) -> Vec3 {
    let (e1, e2, p) = (Vec2::new(c[1].y - c[0].y, c[1].z - c[0].z), Vec2::new(c[2].y - c[0].y, c[2].z - c[0].z), Vec2::new(b.y - c[0].y, b.z - c[0].z));
    let det = e1.perp_dot(e2);
    if det.abs() < 1e-20 {
        return Vec3::new(1.0, 0.0, 0.0);
    }
    let (wu, wv) = (p.perp_dot(e2) / det, e1.perp_dot(p) / det);
    Vec3::new(1.0 - wu - wv, wu, wv)
}

fn inside(c: [Vec3; 3], b: Vec3) -> bool {
    let w = solve(c, b);
    w.min_element() >= -1e-5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn corners_follow_the_subdivision_order() {
        // Level 1: child 1 is (ab, b, bc).
        assert_eq!(corners_in_base(1, 1), [Vec3::new(0.5, 0.5, 0.0), Vec3::Y, Vec3::new(0.0, 0.5, 0.5)]);
        // Level 2: child 3 (the middle) of child 0.
        let c = corners_in_base(3, 2);
        assert_eq!(c, [Vec3::new(0.75, 0.25, 0.0), Vec3::new(0.5, 0.25, 0.25), Vec3::new(0.75, 0.0, 0.25)]);
        // The barycentrics agree with the geometry of the subdivided mesh.
        let tri = primitives::plane(2.0, 1);
        let mut sub = tri.clone();
        for _ in 0..2 {
            sub = crate::subdivide::midpoint(&sub);
        }
        for s in 0..sub.triangle_count() {
            let base = tri.tri_positions(s >> 4);
            let c = corners_in_base(s as u32, 2);
            for (k, p) in sub.tri_positions(s).iter().enumerate() {
                let q = base[0] * c[k].x + base[1] * c[k].y + base[2] * c[k].z;
                assert!(p.distance(q) < 1e-5, "{s} {k}: {p} vs {q}");
            }
        }
    }

    #[test]
    fn a_raised_plane_is_hit_at_its_new_height_and_reported_on_the_flat_one() {
        // A 2 x 2 plane facing +Y, raised by 0.3 everywhere and by more on the u > 0.5 half.
        let plane = primitives::plane(2.0, 1);
        let mut s = DisplacedSurface::new(&plane, 2);
        assert_eq!(s.triangle_count(), plane.triangle_count() * 16);
        // Flat until displaced.
        let down = Vec3::NEG_Y;
        let flat = s.intersect(Vec3::new(0.3, 5.0, 0.2), down, 0.0, f32::INFINITY).unwrap();
        assert!((flat.t - 5.0).abs() < 1e-4);
        s.displace(|_, uv| if uv.x > 0.5 { 1.0 } else { 0.6 }, 0.5);
        for at in [Vec3::new(-0.7, 5.0, 0.4), Vec3::new(0.2, 5.0, -0.6), Vec3::new(0.8, 5.0, 0.8)] {
            let raw = s.intersect(at, down, 0.0, f32::INFINITY).unwrap();
            let base = plane.interpolate(raw.tri as usize, raw.u, raw.v);
            let expected = if base.uv.x > 0.5 { 0.5 } else { 0.3 };
            // Away from the step between the two heights the hit is at the raised height...
            if (base.uv.x - 0.5).abs() > 0.15 {
                assert!((5.0 - raw.t - expected).abs() < 1e-3, "{at}: hit at {} not {expected}", 5.0 - raw.t);
            }
            // ...and its point on the original mesh is straight below it.
            assert!((base.position.x - at.x).abs() < 1e-4 && (base.position.z - at.z).abs() < 1e-4, "{at} -> {}", base.position);
            // Going back up gives the hit point.
            let p = s.displaced_point(raw.tri, raw.u, raw.v).unwrap();
            assert!(p.distance(at + down * raw.t) < 1e-3, "{p} vs {}", at + down * raw.t);
        }
        // A ray beside the (open) plane misses, and the filter sees the original triangles.
        assert!(s.intersect(Vec3::new(1.5, 5.0, 0.0), down, 0.0, f32::INFINITY).is_none());
        assert!(s.intersect_filtered(Vec3::new(0.3, 5.0, 0.2), down, 0.0, f32::INFINITY, |_| false).is_none());
    }

    #[test]
    fn refit_matches_a_fresh_build() {
        let sphere = primitives::uv_sphere(1.0, 16, 8);
        let mut s = DisplacedSurface::new(&sphere, 1);
        s.displace(|_, uv| (uv.x * 12.0).sin() * 0.5 + 0.5, 0.3);
        let tris: Vec<[Vec3; 3]> = (0..s.sub.triangle_count()).map(|t| s.sub.tri(t).map(|v| s.positions[v as usize])).collect();
        let fresh = Bvh::from_triangles(&tris);
        for k in 0..200 {
            let a = k as f32 * 0.37;
            let o = Vec3::new(a.cos(), (a * 0.7).sin(), a.sin()) * 4.0;
            let d = (Vec3::new((a * 1.3).sin() * 0.3, 0.1, 0.0) - o).normalize();
            let (x, y) = (s.bvh.intersect(o, d, 0.0, f32::INFINITY), fresh.intersect(o, d, 0.0, f32::INFINITY));
            assert_eq!(x.map(|h| h.tri), y.map(|h| h.tri));
        }
    }
}
