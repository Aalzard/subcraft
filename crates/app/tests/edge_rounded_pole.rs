//! Default model: the sphere's pole, where the pole fan meets the rings, is a smooth surface and
//! must not count as a rounded edge; the bevelled base rim must.
use glam::Vec3;
use subcraft_materials::sample::edge_distance::SurfaceEdges;

#[test]
fn sphere_pole_is_not_a_rounded_edge_but_the_base_rim_is() {
    let mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).unwrap().mesh;
    let extent = mesh.bounds.extent().max_element();
    let normalized = |p: Vec3| (p - mesh.bounds.min) / extent;
    let edges = SurfaceEdges::new(&mesh);
    let (mut pole, mut base) = (f32::INFINITY, f32::INFINITY);
    for t in 0..mesh.triangle_count() {
        let [a, b, c] = mesh.tri_positions(t);
        let p = (a + b + c) / 3.0;
        let d = edges.rounded_distance(t, normalized(p));
        let r = (p.x * p.x + p.z * p.z).sqrt();
        if p.y > 125.0 && r < 20.0 { pole = pole.min(d); }
        if p.y < 30.0 { base = base.min(d); }
    }
    eprintln!("nearest rounded source: pole {pole:.4}, base {base:.4} (fraction of model size)");
    assert!(base < 0.01, "base rim lost its rounded-edge source: {base}");
    assert!(pole > 0.1, "sphere pole counts as a rounded edge: {pole}");
}
