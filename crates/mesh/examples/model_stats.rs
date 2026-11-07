//! Print what the importer makes of a model file: counts, bounds, UV range/overlap, normals.
//! `cargo run -p subcraft-mesh --example model_stats -- model.fbx`
use subcraft_mesh::import_mesh;

fn main() {
    let path = std::env::args().nth(1).expect("model path");
    let r = import_mesh(std::path::Path::new(&path)).expect("import");
    let m = &r.mesh;
    println!("triangles {} vertices {} objects {} materials {:?}", m.triangle_count(), m.vertex_count(), m.objects.len(), m.materials.iter().map(|s| s.name.clone()).collect::<Vec<_>>());
    for o in &m.objects { println!("object {:?}", o); }
    let bb = subcraft_mesh::mesh::Aabb::from_points(m.positions.iter().copied());
    println!("bounds min {:?} max {:?} extent {:?}", bb.min, bb.max, bb.extent());
    let (mut umin, mut umax) = (f32::MAX, f32::MIN);
    let (mut vmin, mut vmax) = (f32::MAX, f32::MIN);
    for uv in &m.uvs { umin = umin.min(uv.x); umax = umax.max(uv.x); vmin = vmin.min(uv.y); vmax = vmax.max(uv.y); }
    println!("uv u {umin}..{umax} v {vmin}..{vmax} tiles {:?}", m.udim_tiles(None).len());
    println!("has normals {} has tangents? stats {:?}", !m.normals.is_empty(), r.stats);
    // normals vs winding
    let (mut agree, mut total) = (0usize, 0usize);
    if !m.normals.is_empty() {
        for t in 0..m.triangle_count() {
            let g = m.geometric_normal(t);
            let [a, b, c] = m.tri(t);
            let n = m.normals[a as usize] + m.normals[b as usize] + m.normals[c as usize];
            total += 1; if g.dot(n) > 0.0 { agree += 1; }
        }
    }
    println!("normals agree with winding {agree}/{total}");
    // outward check: mean dot(face normal, centroid direction)
    let c = bb.center();
    let mut out = 0usize;
    for t in 0..m.triangle_count() { let p = m.tri_positions(t); let ctr = (p[0] + p[1] + p[2]) / 3.0; if m.geometric_normal(t).dot(ctr - c) > 0.0 { out += 1; } }
    println!("faces pointing away from bbox centre {out}/{}", m.triangle_count());
    println!("warnings {:?}", r.warnings);
}
