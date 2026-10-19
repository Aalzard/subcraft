//! Object-space mirror-plane intersections. These use the exact whole-mesh center used by
//! mirrored painting; UI/render decide visibility and drawing, never the coordinate frame.
use glam::Vec3;
use subcraft_mesh::Mesh;

pub(crate) fn symmetry_sections(mesh: &Mesh) -> [Vec<[Vec3; 2]>; 3] {
    let center = mesh.bounds.center();
    let eps = mesh.bounds.diagonal().max(1e-6) * 1e-6;
    std::array::from_fn(|axis| {
        let mut lines = Vec::new();
        for tri in mesh.indices.as_chunks::<3>().0 {
            let p = tri.map(|i| mesh.positions[i as usize]);
            let d = p.map(|p| p[axis] - center[axis]);
            if d.iter().all(|d| d.abs() < eps) { continue; }
            let mut hits = [Vec3::ZERO; 3]; let mut count = 0;
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                let mut point = if d[a].abs() < eps { Some(p[a]) }
                    else if d[a] * d[b] < 0. { Some(p[a].lerp(p[b], d[a] / (d[a] - d[b]))) } else { None };
                if let Some(p) = &mut point { p[axis] = center[axis]; }
                if let Some(p) = point && !hits[..count].iter().any(|h| h.distance_squared(p) < eps * eps) { hits[count] = p; count += 1; }
            }
            if count == 2 && hits[0].distance_squared(hits[1]) > eps * eps { lines.push([hits[0], hits[1]]); }
        }
        // A quiet rectangle also identifies the plane beyond the surface, including planar meshes.
        let extent = mesh.bounds.extent() * 0.58 + Vec3::splat(mesh.bounds.diagonal() * 0.025);
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let corners = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)].map(|(su, sv)| {
            let mut p = center; p[u] += su * extent[u]; p[v] += sv * extent[v]; p
        });
        for i in 0..4 { lines.push([corners[i], corners[(i + 1) % 4]]); }
        lines
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mirror_guides_intersect_surfaces_at_the_exact_painting_plane_on_offset_meshes() {
        let mut mesh = subcraft_mesh::primitives::cube(2.);
        for p in &mut mesh.positions { *p += Vec3::new(3., 4., 5.); }
        mesh.bounds = subcraft_mesh::Aabb::from_points(mesh.positions.iter().copied());
        let sections = symmetry_sections(&mesh);
        for axis in 0..3 {
            assert!(sections[axis].len() >= 12);
            for line in &sections[axis] { for p in line { assert!((p[axis] - mesh.bounds.center()[axis]).abs() < 1e-6); } }
        }
    }
}
