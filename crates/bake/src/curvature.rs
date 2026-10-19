//! Curvature from 3D neighbourhoods (seam-independent).
//!
//! For each texel, nearby triangles within `radius` are visited; each contributes the convexity
//! `(n_q − n_p) · d̂` between the texel's normal and the normal at the triangle's closest point,
//! weighted by proximity. Flat areas give 0.5, convex edges approach 1, concave creases 0. Being
//! computed from 3D positions, curvature is continuous across UV seams.

use glam::Vec3;
use subcraft_mesh::{Bvh, Mesh};

/// Closest point on triangle (a, b, c) to p, with barycentrics (Ericson, RTCD 5.1.5).
pub fn closest_point(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> (Vec3, [f32; 3]) {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, [1.0, 0.0, 0.0]);
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, [0.0, 1.0, 0.0]);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (a + ab * v, [1.0 - v, v, 0.0]);
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, [0.0, 0.0, 1.0]);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (a + ac * w, [1.0 - w, 0.0, w]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (b + (c - b) * w, [0.0, 1.0 - w, w]);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    (a + ab * v + ac * w, [1.0 - v - w, v, w])
}

/// Curvature (0..1, 0.5 = flat) at point `p` with normal `n` on `mesh`.
pub fn curvature_at(mesh: &Mesh, bvh: &Bvh, p: Vec3, n: Vec3, radius: f32) -> f32 {
    curvature_on_surface(mesh, bvh, p, n, radius, None)
}

#[derive(Default)]
struct Curvature {
    sum: f32,
    weight: f32,
    nearest: f32,
}

impl Curvature {
    fn add(&mut self, value: f32, weight: f32) {
        self.sum += weight * value;
        self.weight += weight;
        self.nearest = self.nearest.max(weight);
    }

    fn value(&self) -> f32 {
        if self.weight <= 0.0 { return 0.5; }
        0.5 + 0.5 * (self.sum / self.weight).clamp(-1.0, 1.0) * self.nearest
    }
}

/// Prefer intrinsic curvature of the sampled connected surface. Nearby separate components
/// must not cancel its convex/concave corners. Flat surfaces still receive contact curvature
/// (e.g. the floor beside a box). `source` refers to the detail mesh after high-poly projection.
pub(crate) fn curvature_on_surface(
    mesh: &Mesh, bvh: &Bvh, p: Vec3, n: Vec3, radius: f32,
    source: Option<(u32, &[u32])>,
) -> f32 {
    let mut all = Curvature::default();
    let mut own = Curvature::default();
    for t in bvh.query_sphere(p, radius) {
        let [a, b, c] = mesh.tri_positions(t as usize);
        let (q, bary) = closest_point(p, a, b, c);
        let d = q - p;
        let dist = d.length();
        if dist >= radius {
            continue;
        }
        let [ia, ib, ic] = mesh.tri(t as usize).map(|i| i as usize);
        let nq = (mesh.normals[ia] * bary[0] + mesh.normals[ib] * bary[1] + mesh.normals[ic] * bary[2]).normalize_or_zero();
        let dn = nq - n;
        if dn.length_squared() < 1e-6 {
            continue;
        }
        // Measure change across the crease for every neighbor, not just coincident samples.
        // Other triangles on the same neighboring face may be displaced along the crease;
        // averaging those oblique directions made a straight edge pulse with tessellation.
        // Keep the true 3D distance for proximity falloff and the query radius.
        let crease = n.cross(nq);
        let crease_len2 = crease.length_squared();
        let project = |v: Vec3| if crease_len2 > 1e-6 { v - crease * (v.dot(crease) / crease_len2) } else { v };
        let mut across = project(d);
        if across.length_squared() <= (radius * 1e-3).powi(2) {
            // A neighbor farther along this same edge has no reliable across-crease direction
            // either: tiny float error must not become a full negative contribution on normalize.
            across = project((a + b + c) / 3.0 - p);
        }
        if across.length_squared() < 1e-24 { continue; }
        let w = 1.0 - dist.max(radius * 1e-3) / radius;
        let value = dn.dot(across.normalize());
        all.add(value, w);
        if let Some((source, elements)) = source
            && elements[t as usize] == elements[source as usize]
            && mesh.tri_object.get(t as usize) == mesh.tri_object.get(source as usize) {
            own.add(value, w);
        }
    }
    if own.weight > 0.0 { own.value() } else { all.value() }
}
