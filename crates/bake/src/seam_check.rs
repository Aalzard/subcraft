//! Hard edges without a UV seam.
//!
//! A tangent-space normal map cannot be right across an edge whose normals are split (a hard
//! edge) while its UVs are continuous: both faces share texels there but need different tangent
//! frames, which shows as a visible line after baking. The usual rule is "every hard edge is a
//! UV seam"; this finds the edges that break it, so the overlay can point at them before a bake.

use std::collections::BTreeMap;

use glam::Vec3;
use subcraft_mesh::Mesh;

/// An edge of the low poly whose normals are split while its UVs are not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HardEdge {
    pub a: Vec3,
    pub b: Vec3,
    /// Average of the two faces' normals (to lift a drawn line off the surface).
    pub normal: Vec3,
}

/// Normals closer than this (about one degree) count as the same normal.
const SAME_NORMAL_COS: f32 = 0.99985;
const SAME_UV: f32 = 1e-5;

/// Every edge shared by exactly two triangles whose vertex normals differ across it (hard) while
/// the UVs and the material slot are the same on both sides (no seam). Open borders and edges
/// with more than two faces are not judged. The result is in a stable order.
pub fn hard_edges_without_seams(mesh: &Mesh) -> Vec<HardEdge> {
    // `+ 0.0` folds -0.0 into 0.0 so both zeros share a key.
    let key = |i: u32| (mesh.positions[i as usize] + Vec3::ZERO).to_array().map(f32::to_bits);
    // Per edge (by position, lower key first): the triangles on it with their own two vertices.
    let mut edges: BTreeMap<([u32; 3], [u32; 3]), Vec<(usize, u32, u32)>> = BTreeMap::new();
    for t in 0..mesh.triangle_count() {
        let v = mesh.tri(t);
        for i in 0..3 {
            let (a, b) = (v[i], v[(i + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            let (edge, a, b) = if ka < kb { ((ka, kb), a, b) } else { ((kb, ka), b, a) };
            edges.entry(edge).or_default().push((t, a, b));
        }
    }
    let normal = |i: u32| mesh.normals[i as usize];
    let uv = |i: u32| mesh.uvs.get(i as usize).copied().unwrap_or_default();
    let slot = |t: usize| mesh.tri_material.get(t).copied().unwrap_or(0);
    let mut out = Vec::new();
    for sides in edges.values() {
        let &[(t0, a0, b0), (t1, a1, b1)] = sides.as_slice() else { continue };
        let hard = normal(a0).dot(normal(a1)) < SAME_NORMAL_COS || normal(b0).dot(normal(b1)) < SAME_NORMAL_COS;
        let seam = slot(t0) != slot(t1) || (uv(a0) - uv(a1)).length() > SAME_UV || (uv(b0) - uv(b1)).length() > SAME_UV;
        if hard && !seam {
            let n = (mesh.geometric_normal(t0) + mesh.geometric_normal(t1)).normalize_or(mesh.geometric_normal(t0));
            out.push(HardEdge { a: mesh.positions[a0 as usize], b: mesh.positions[b0 as usize], normal: n });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;
    use subcraft_mesh::{Corner, MeshBuilder, primitives};

    /// Two unit quads folded at 90 degrees along the x axis: a floor (y = 0, z in 0..1) and a
    /// wall (z = 0, y in 0..1). `hard`: each face keeps its own normal; `seam`: the wall's UVs
    /// are a separate island.
    fn fold(hard: bool, seam: bool) -> Mesh {
        let mut b = MeshBuilder::new("fold");
        let m = b.material("m");
        let smooth = Vec3::new(0.0, 1.0, 1.0).normalize();
        let floor_n = if hard { Vec3::Y } else { smooth };
        let wall_n = if hard { Vec3::Z } else { smooth };
        let c = |p: Vec3, n: Vec3, uv: Vec2| Corner { position: p, normal: Some(n), uv: Some(uv) };
        // Floor: u = x / 2, v = 0.5 + z / 2. The fold edge (z = 0) is at v = 0.5.
        let f = |x: f32, z: f32| c(Vec3::new(x, 0.0, z), if z == 0.0 { floor_n } else { Vec3::Y }, Vec2::new(x * 0.5, 0.5 + z * 0.5));
        b.push_triangle(m, [f(0.0, 0.0), f(0.0, 1.0), f(1.0, 1.0)]);
        b.push_triangle(m, [f(0.0, 0.0), f(1.0, 1.0), f(1.0, 0.0)]);
        // Wall: continuous with the floor at the fold (v = 0.5 - y / 2), or moved away as its own island.
        let shift = if seam { Vec2::new(0.5, 0.0) } else { Vec2::ZERO };
        let w = |x: f32, y: f32| c(Vec3::new(x, y, 0.0), if y == 0.0 { wall_n } else { Vec3::Z }, Vec2::new(x * 0.5, 0.5 - y * 0.5) + shift);
        b.push_triangle(m, [w(0.0, 0.0), w(1.0, 0.0), w(1.0, 1.0)]);
        b.push_triangle(m, [w(0.0, 0.0), w(1.0, 1.0), w(0.0, 1.0)]);
        b.build().unwrap()
    }

    #[test]
    fn a_hard_edge_with_continuous_uvs_is_reported_once() {
        let edges = hard_edges_without_seams(&fold(true, false));
        assert_eq!(edges.len(), 1, "{edges:?}");
        let e = edges[0];
        // The fold: from (0, 0, 0) to (1, 0, 0), in either direction.
        let (lo, hi) = if e.a.x < e.b.x { (e.a, e.b) } else { (e.b, e.a) };
        assert!(lo.length() < 1e-6 && (hi - Vec3::X).length() < 1e-6, "{e:?}");
        assert!((e.normal - Vec3::new(0.0, 1.0, 1.0).normalize()).length() < 1e-4, "between the two faces: {:?}", e.normal);
    }

    #[test]
    fn seams_and_smooth_edges_are_fine() {
        assert!(hard_edges_without_seams(&fold(true, true)).is_empty(), "a hard edge on a UV seam is the rule");
        assert!(hard_edges_without_seams(&fold(false, false)).is_empty(), "a smooth edge needs no seam");
        assert!(hard_edges_without_seams(&fold(false, true)).is_empty());
        // A cube with one island per face: every hard edge is a seam. A sphere has no hard edge.
        assert!(hard_edges_without_seams(&primitives::cube(2.0)).is_empty());
        assert!(hard_edges_without_seams(&primitives::uv_sphere(1.0, 16, 8)).is_empty());
    }

    #[test]
    fn the_order_is_stable() {
        let mesh = fold(true, false);
        assert_eq!(hard_edges_without_seams(&mesh), hard_edges_without_seams(&mesh));
    }
}
