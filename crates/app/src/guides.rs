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

/// Radial symmetry guide: the axis line through the (offset) bounds center, a little longer than
/// the mesh, and `count` spokes in the plane through the center, one per painted copy (spoke 0
/// points along the next axis, so copies sit at equal angles from it).
pub(crate) fn radial_guides(mesh: &Mesh, radial: &subcraft_paint::RadialSymmetry, count: u32) -> Vec<[Vec3; 2]> {
    if count < 2 { return Vec::new(); }
    let axis = (radial.axis as usize).min(2);
    let dir = radial.axis_dir();
    let center = mesh.bounds.center() + Vec3::from_array(radial.center_offset);
    let extent = mesh.bounds.extent() * 0.5;
    let half = extent[axis] * 1.25 + mesh.bounds.diagonal() * 0.05;
    let mut lines = vec![[center - dir * half, center + dir * half]];
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let reach = extent[u].max(extent[v]) * 1.2 + mesh.bounds.diagonal() * 0.02;
    let mut base = Vec3::ZERO;
    base[u] = 1.0;
    for k in 0..count {
        let r = glam::Quat::from_axis_angle(dir, std::f32::consts::TAU * k as f32 / count as f32);
        lines.push([center, center + r * base * reach]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_guides_draw_the_axis_and_one_spoke_per_copy_through_the_offset_center() {
        let mut mesh = subcraft_mesh::primitives::cube(2.);
        for p in &mut mesh.positions { *p += Vec3::new(3., 4., 5.); }
        mesh.bounds = subcraft_mesh::Aabb::from_points(mesh.positions.iter().copied());
        let radial = subcraft_paint::RadialSymmetry { enabled: true, axis: 1, count: 6, center_offset: [0.5, 0.0, 0.0] };
        let lines = radial_guides(&mesh, &radial, 6);
        assert_eq!(lines.len(), 7);
        let c = Vec3::new(3.5, 4., 5.);
        assert!((lines[0][0].x - c.x).abs() < 1e-6 && (lines[0][1].z - c.z).abs() < 1e-6 && lines[0][1].y > 5.);
        for (k, s) in lines[1..].iter().enumerate() {
            assert!(s[0].distance(c) < 1e-6);
            assert!((s[1].y - c.y).abs() < 1e-5, "spokes lie in the plane through the center");
            let d = (s[1] - c).normalize();
            let expected = glam::Quat::from_rotation_y(std::f32::consts::TAU * k as f32 / 6.) * Vec3::Z;
            assert!(d.distance(expected) < 1e-5, "spoke {k}: {d} vs {expected}");
        }
        assert!(radial_guides(&mesh, &radial, 1).is_empty());
    }
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
