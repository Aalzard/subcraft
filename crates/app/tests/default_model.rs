//! The bundled default model (`assets/models/SubCraft_base.fbx`): what SubCraft starts with.
use subcraft_app::{DEFAULT_RESOLUTION, Session};

#[test]
fn the_bundled_fbx_is_a_valid_paintable_model_and_the_start_session() {
    let s = Session::default_model();
    let mesh = &s.scene().mesh;
    assert_eq!(mesh.name, "SubCraft_base");
    assert_eq!(mesh.triangle_count(), 18_720);
    assert_eq!(mesh.objects.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["Cylinder", "Sphere"]);
    assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
    assert!(!mesh.normals.is_empty(), "the file carries normals");
    // Y-up: the model is taller than zero, stands on y > 0 and is centred on the origin in x/z.
    let (min, max) = (mesh.bounds.min, mesh.bounds.max);
    assert!(min.y > 0.0 && max.y > min.y && (max.x + min.x).abs() < 1e-2 && (max.z + min.z).abs() < 1e-2, "{min:?} {max:?}");
    // All UVs inside one tile (no UDIM, nothing outside 0..1).
    assert!(mesh.uvs.iter().all(|uv| (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)));
    assert_eq!(mesh.udim_tiles(None).len(), 1);
    // Normals agree with the triangle winding (no flipped faces).
    let flipped = (0..mesh.triangle_count()).filter(|&t| {
        let [a, b, c] = mesh.tri(t);
        mesh.geometric_normal(t).dot(mesh.normals[a as usize] + mesh.normals[b as usize] + mesh.normals[c as usize]) <= 0.0
    }).count();
    assert_eq!(flipped, 0);
    // One material, so one texture set, at the default resolution, with a paint layer selected.
    let sets = &s.document().texture_sets;
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].name, "DefaultMaterial");
    assert_eq!(sets[0].resolution, DEFAULT_RESOLUTION);
    assert_eq!(DEFAULT_RESOLUTION, 2048);
    assert!(s.selection().layer.is_some());
    assert!(s.mesh_source().is_none(), "bundled model has no file to reload");
}

#[test]
fn uv_islands_do_not_overlap() {
    let s = Session::default_model();
    let mesh = &s.scene().mesh;
    const N: usize = 1024;
    let mut owner = vec![u32::MAX; N * N];
    let (mut clashes, mut covered) = (0usize, 0usize);
    for t in 0..mesh.triangle_count() {
        let uv = mesh.tri_uvs(t).map(|p| p * N as f32);
        let (min, max) = (uv[0].min(uv[1]).min(uv[2]), uv[0].max(uv[1]).max(uv[2]));
        let edge = |a: glam::Vec2, b: glam::Vec2, p: glam::Vec2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        let area = edge(uv[0], uv[1], uv[2]);
        if area.abs() < 1e-6 { continue; }
        for y in (min.y.floor() as usize)..=((max.y.ceil() as usize).min(N - 1)) {
            for x in (min.x.floor() as usize)..=((max.x.ceil() as usize).min(N - 1)) {
                let p = glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w = [edge(uv[1], uv[2], p), edge(uv[2], uv[0], p), edge(uv[0], uv[1], p)];
                let inside = if area > 0.0 { w.iter().all(|v| *v >= 0.0) } else { w.iter().all(|v| *v <= 0.0) };
                if !inside { continue; }
                let o = &mut owner[y * N + x];
                if *o == u32::MAX { *o = t as u32; covered += 1; } else if *o != t as u32 { clashes += 1; }
            }
        }
    }
    eprintln!("covered {covered} texels, {clashes} texel hits shared between triangles");
    assert!(clashes * 50 < covered, "UV islands overlap: {clashes} of {covered} texels");
}
