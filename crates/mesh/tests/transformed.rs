use glam::{Mat4, Vec3};
use subcraft_mesh::primitives;

#[test]
fn translation_scale_and_rotation_move_positions_normals_and_bounds() {
    let m = primitives::cube(2.0);
    let t = Mat4::from_translation(Vec3::new(1., 2., 3.)) * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2) * Mat4::from_scale(Vec3::splat(0.5));
    let out = m.transformed(&t);
    assert_eq!(out.positions.len(), m.positions.len());
    for (a, b) in m.positions.iter().zip(&out.positions) { assert!((t.transform_point3(*a) - *b).length() < 1e-5); }
    assert!((out.bounds.center() - Vec3::new(1., 2., 3.)).length() < 1e-4);
    assert!((out.bounds.extent() - Vec3::splat(1.)).length() < 1e-4);
    // +X normals become +Y after a quarter turn about Z.
    for (n0, n1) in m.normals.iter().zip(&out.normals) {
        assert!((n1.length() - 1.).abs() < 1e-5);
        if (*n0 - Vec3::X).length() < 1e-5 { assert!((*n1 - Vec3::Y).length() < 1e-4); }
    }
    assert_eq!(out.uvs, m.uvs);
    assert_eq!(out.indices, m.indices);
    assert!(m.positions != out.positions, "the source mesh is untouched");
}

#[test]
fn non_uniform_scale_keeps_normals_perpendicular_to_faces() {
    let m = primitives::cube(1.0);
    let out = m.transformed(&Mat4::from_scale(Vec3::new(4., 1., 0.5)));
    for t in 0..out.triangle_count() {
        let n = out.geometric_normal(t);
        for &i in &out.tri(t) { assert!(out.normals[i as usize].dot(n) > 0.99, "vertex normal follows the face"); }
    }
}

#[test]
fn mirroring_flips_winding_so_faces_still_point_outward() {
    let m = primitives::cube(1.0);
    let out = m.transformed(&Mat4::from_scale(Vec3::new(-1., 1., 1.)));
    for t in 0..out.triangle_count() {
        let n = out.geometric_normal(t);
        let [a, b, c] = out.tri_positions(t);
        assert!(n.dot((a + b + c) / 3.0 - out.bounds.center()) > 0.0, "outward");
    }
}
