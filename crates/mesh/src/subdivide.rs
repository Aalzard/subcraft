//! Uniform midpoint subdivision: the tessellation used by displacement (mesh export with
//! displacement, the 3D view's displacement preview).

use std::collections::HashMap;

use glam::Vec4;

use crate::{Mesh, Submesh};

/// Split every triangle into four at its edge midpoints. Edge midpoints are shared between the
/// two triangles of an edge, so the surface stays closed; submesh ranges scale by four.
pub fn midpoint(mesh: &Mesh) -> Mesh {
    let mut out = mesh.clone();
    out.indices.clear();
    out.tri_material.clear();
    out.tri_object.clear();
    let mut mids: HashMap<(u32, u32), u32> = HashMap::new();
    let mut mid = |out: &mut Mesh, a: u32, b: u32| -> u32 {
        let key = (a.min(b), a.max(b));
        *mids.entry(key).or_insert_with(|| {
            let (a, b) = (a as usize, b as usize);
            out.positions.push((mesh.positions[a] + mesh.positions[b]) * 0.5);
            if mesh.normals.len() == mesh.positions.len() {
                out.normals.push(((mesh.normals[a] + mesh.normals[b]) * 0.5).normalize_or(mesh.normals[a]));
            }
            if mesh.uvs.len() == mesh.positions.len() {
                out.uvs.push((mesh.uvs[a] + mesh.uvs[b]) * 0.5);
            }
            if mesh.tangents.len() == mesh.positions.len() {
                let (ta, tb) = (mesh.tangents[a], mesh.tangents[b]);
                let xyz = ((ta.truncate() + tb.truncate()) * 0.5).normalize_or(ta.truncate());
                out.tangents.push(Vec4::new(xyz.x, xyz.y, xyz.z, ta.w));
            }
            (out.positions.len() - 1) as u32
        })
    };
    for t in 0..mesh.triangle_count() {
        let [a, b, c] = mesh.tri(t);
        let (ab, bc, ca) = (mid(&mut out, a, b), mid(&mut out, b, c), mid(&mut out, c, a));
        for tri in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
            out.indices.extend_from_slice(&tri);
            out.tri_material.push(mesh.tri_material.get(t).copied().unwrap_or(0));
            out.tri_object.push(mesh.tri_object.get(t).copied().flatten());
        }
    }
    out.submeshes = mesh.submeshes.iter().map(|s| Submesh { material: s.material, first_triangle: s.first_triangle * 4, triangle_count: s.triangle_count * 4 }).collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_level_quadruples_triangles_and_keeps_attributes_interpolated() {
        let cube = crate::primitives::cube(2.0);
        let s = midpoint(&cube);
        assert_eq!(s.triangle_count(), cube.triangle_count() * 4);
        assert_eq!(s.positions.len(), s.normals.len());
        assert_eq!(s.positions.len(), s.uvs.len());
        assert_eq!(s.submeshes.iter().map(|m| m.triangle_count).sum::<u32>() as usize, s.triangle_count());
        // Midpoints stay on the flat faces of the cube.
        for p in &s.positions {
            assert!(p.abs().max_element() <= 1.0 + 1e-6 && (p.abs().max_element() - 1.0).abs() < 1e-6, "{p}");
        }
    }
}
