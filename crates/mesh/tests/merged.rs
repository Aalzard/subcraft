//! `Mesh::merged`: several meshes appended into one (baking against several high-poly files).

use subcraft_mesh::{Mesh, primitives};

#[test]
fn merged_meshes_keep_every_part_with_shifted_indices() {
    let a = primitives::plane(2.0, 2);
    let b = primitives::cube(1.0).transformed(&glam::Mat4::from_translation(glam::Vec3::new(5.0, 0.0, 0.0)));
    let m = Mesh::merged(&[&a, &b]);
    assert_eq!(m.vertex_count(), a.vertex_count() + b.vertex_count());
    assert_eq!(m.triangle_count(), a.triangle_count() + b.triangle_count());
    assert_eq!((m.normals.len(), m.uvs.len(), m.tangents.len()), (m.vertex_count(), m.vertex_count(), m.vertex_count()));
    assert!(m.indices.iter().all(|&i| (i as usize) < m.vertex_count()));
    assert_eq!(m.tri_material.len(), m.triangle_count());
    assert_eq!(m.materials.len(), a.materials.len() + b.materials.len());
    // The second mesh's triangles follow the first's, with their own vertices and materials.
    let first_b = a.triangle_count();
    assert_eq!(m.tri_positions(first_b), b.tri_positions(0));
    assert_eq!(m.tri_material[first_b], b.tri_material[0] + a.materials.len() as u32);
    assert_eq!(m.tri_positions(0), a.tri_positions(0));
    // Submeshes cover every triangle once.
    assert_eq!(m.submeshes.iter().map(|s| s.triangle_count as usize).sum::<usize>(), m.triangle_count());
    for s in &m.submeshes {
        let range = s.first_triangle as usize..(s.first_triangle + s.triangle_count) as usize;
        assert!(range.clone().all(|t| m.tri_material[t] == s.material), "{s:?}");
    }
    assert!(m.tri_object.is_empty() || m.tri_object.len() == m.triangle_count());
    assert!(m.bounds.min.x <= -1.0 && m.bounds.max.x >= 5.5);
    // Each triangle remembers its file.
    assert_eq!(m.file_count(), 2);
    assert_eq!((m.file_of(0), m.file_of(first_b), m.file_of(m.triangle_count() - 1)), (0, 1, 1));
    assert!(Mesh::merged(&[&a]).tri_file.is_empty() && a.file_count() == 1 && a.file_of(0) == 0);
    // One mesh merges to itself; none to an empty mesh.
    assert_eq!(Mesh::merged(&[&a]).positions, a.positions);
    assert_eq!(Mesh::merged(&[]).triangle_count(), 0);
}
