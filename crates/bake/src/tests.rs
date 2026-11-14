use glam::{Vec2, Vec3};
use subcraft_core::MeshMapKind;
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder, primitives};

use super::*;

const RES: u32 = 128;

#[test]
fn streamed_maps_are_complete_padded_ordered_and_exactly_match_sync() {
    let mesh = primitives::plane(2., 1);
    let bvh = Bvh::build(&mesh);
    let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let settings = BakeSettings { size: 32, ao_samples: 4, thickness_samples: 4, ..BakeSettings::default() };
    let mut events = Vec::new();
    let streamed = bake_streamed(&input, &settings, &Progress::new(), |kind, map| {
        assert_eq!((map.width(), map.height(), map.comps()), (32, 32, kind.components()));
        assert!(map.to_dense().iter().all(|v| v.is_finite()));
        events.push((kind, map.clone()));
    }).unwrap();
    assert_eq!(events.iter().map(|(k, _)| *k).collect::<Vec<_>>(), settings.maps);
    let sync = bake(&input, &settings, &Progress::new()).unwrap();
    assert_eq!(streamed, sync);
    for (kind, image) in events { assert_eq!(&image, &sync[&kind]); }
    for p in streamed[&MeshMapKind::Normal].to_dense().as_chunks::<3>().0 { assert_eq!(p, &[0.5, 0.5, 1.]); }
}

#[test]
fn cancelling_between_maps_never_publishes_an_incomplete_map() {
    let mesh = primitives::plane(2., 1);
    let bvh = Bvh::build(&mesh);
    let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let progress = Progress::new();
    let mut events = Vec::new();
    let result = bake_streamed(&input, &settings(&[MeshMapKind::Normal, MeshMapKind::AmbientOcclusion]), &progress, |k, img| {
        events.push((k, img.clone())); progress.cancel();
    });
    assert_eq!(result, Err(BakeError::Cancelled));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, MeshMapKind::Normal);
    assert_eq!(events[0].1.get(64, 64)[..3], [0.5, 0.5, 1.]);
}

fn settings(maps: &[MeshMapKind]) -> BakeSettings {
    BakeSettings { size: RES, maps: maps.to_vec(), ao_samples: 64, ..BakeSettings::default() }
}

fn bake_low(mesh: &Mesh, maps: &[MeshMapKind], slot: u32) -> BTreeMap<MeshMapKind, TiledImage> {
    let bvh = Bvh::build(mesh);
    let input = BakeInput { low: Geometry { mesh, bvh: &bvh }, high: None, material_slot: slot };
    bake(&input, &settings(maps), &Progress::new()).unwrap()
}

fn at(img: &TiledImage, t: &Texel) -> [f32; 4] {
    img.get(t.index % RES, t.index / RES)
}

fn corner(p: Vec3, uv: Vec2) -> Corner {
    Corner { position: p, normal: None, uv: Some(uv) }
}

fn corner_n(p: Vec3, n: Vec3, uv: Vec2) -> Corner {
    Corner { position: p, normal: Some(n), uv: Some(uv) }
}

/// A 4×4 floor (UV left half) with a 1×1×1 box standing on it (UV right half, other material
/// slot so the floor bakes alone in slot 0).
fn floor_with_box() -> Mesh {
    let mut b = MeshBuilder::new("FloorBox");
    let floor = b.material("Floor");
    let boxm = b.material("Box");
    let s = 8;
    for i in 0..s {
        for j in 0..s {
            let (x0, x1) = (-2.0 + 4.0 * j as f32 / s as f32, -2.0 + 4.0 * (j + 1) as f32 / s as f32);
            let (z0, z1) = (-2.0 + 4.0 * i as f32 / s as f32, -2.0 + 4.0 * (i + 1) as f32 / s as f32);
            let uv = |x: f32, z: f32| Vec2::new((x + 2.0) / 4.0 * 0.98 + 0.01, (z + 2.0) / 4.0 * 0.98 + 0.01);
            let p = |x: f32, z: f32| corner_n(Vec3::new(x, 0.0, z), Vec3::Y, uv(x, z));
            b.push_triangle(floor, [p(x0, z0), p(x0, z1), p(x1, z1)]);
            b.push_triangle(floor, [p(x0, z0), p(x1, z1), p(x1, z0)]);
        }
    }
    let cube = primitives::atlas_box(Vec3::ONE, 2, "Box");
    for t in 0..cube.triangle_count() {
        let pos = cube.tri_positions(t);
        let uvs = cube.tri_uvs(t);
        let n = cube.geometric_normal(t);
        let c = |k: usize| corner_n(pos[k] + Vec3::new(0.0, 0.5, 0.0), n, uvs[k]);
        b.push_triangle(boxm, [c(0), c(1), c(2)]);
    }
    b.build().unwrap()
}

#[test]
fn ao_open_plane_is_unoccluded() {
    let mesh = primitives::plane(2.0, 2);
    let maps = bake_low(&mesh, &[MeshMapKind::AmbientOcclusion], 0);
    let ao = &maps[&MeshMapKind::AmbientOcclusion];
    for t in texels(&mesh, 0, RES).iter().step_by(37) {
        assert!(at(ao, t)[0] > 0.99, "open plane texel AO {}", at(ao, t)[0]);
    }
}

#[test]
fn ao_and_curvature_darken_the_contact_with_a_box() {
    let mesh = floor_with_box();
    let maps = bake_low(&mesh, &[MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature], 0);
    let (ao, curv) = (&maps[&MeshMapKind::AmbientOcclusion], &maps[&MeshMapKind::Curvature]);
    let ts = texels(&mesh, 0, RES);
    let dist_to_box = |p: Vec3| (p.x.abs() - 0.5).max(p.z.abs() - 0.5).max(0.0);
    let near: Vec<&Texel> = ts.iter().filter(|t| dist_to_box(t.position) < 0.06 && dist_to_box(t.position) > 0.0).collect();
    let far: Vec<&Texel> = ts.iter().filter(|t| dist_to_box(t.position) > 1.2).collect();
    assert!(!near.is_empty() && !far.is_empty());
    let mean = |v: &[&Texel], img: &TiledImage| v.iter().map(|t| at(img, t)[0]).sum::<f32>() / v.len() as f32;
    let (ao_near, ao_far) = (mean(&near, ao), mean(&far, ao));
    assert!(ao_far > 0.97, "far floor unoccluded ({ao_far})");
    assert!(ao_near < ao_far - 0.15, "floor next to the box is occluded ({ao_near} vs {ao_far})");
    let (c_near, c_far) = (mean(&near, curv), mean(&far, curv));
    assert!((c_far - 0.5).abs() < 0.01, "flat floor curvature {c_far}");
    assert!(c_near < 0.4, "concave crease is dark ({c_near})");
}

#[test]
fn curvature_brightens_cube_edges_and_keeps_faces_flat() {
    let mesh = primitives::cube(2.0);
    let maps = bake_low(&mesh, &[MeshMapKind::Curvature], 0);
    let curv = &maps[&MeshMapKind::Curvature];
    let radius = 0.02 * mesh.bounds.diagonal();
    let mut edge = Vec::new();
    let mut center = Vec::new();
    for t in texels(&mesh, 0, RES) {
        let p = t.position.abs();
        let near_one = [p.x, p.y, p.z].iter().filter(|&&c| 1.0 - c < radius * 0.4).count();
        if near_one >= 2 {
            edge.push(at(curv, &t)[0]);
        } else if [p.x, p.y, p.z].iter().filter(|&&c| c < 0.5).count() == 2 {
            center.push(at(curv, &t)[0]);
        }
    }
    let avg = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(edge.len() > 20 && center.len() > 100);
    assert!(avg(&edge) > 0.7, "convex edges bright ({})", avg(&edge));
    assert!(center.iter().all(|c| (c - 0.5).abs() < 1e-3), "face centers flat");
}

#[test]
fn crate_contact_cannot_cancel_its_own_corner_curvature() {
    for (legacy_metadata, high_poly) in [(false, false), (true, false), (false, true)] {
        let mut mesh = primitives::sample_crate();
        if legacy_metadata { mesh.tri_object.clear(); mesh.objects.clear(); }
        let bvh = Bvh::build(&mesh);
        let geo = Geometry { mesh: &mesh, bvh: &bvh };
        let elements = subcraft_mesh::topology::mesh_elements(&mesh);
        let mut bounds = vec![(Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)); 7];
        for (tri, &element) in elements.iter().enumerate() {
            for p in mesh.tri_positions(tri) {
                let (min, max) = &mut bounds[element as usize]; *min = min.min(p); *max = max.max(p);
            }
        }
        for slot in 0..if high_poly { 1 } else { 2 } {
            let maps = bake(&BakeInput { low: geo, high: high_poly.then_some(geo), material_slot: slot },
                &settings(&[MeshMapKind::Curvature]), &Progress::new()).unwrap();
            let mut checked = 0;
            for t in texels(&mesh, slot, RES) {
                let p = t.position;
                let (min, max) = bounds[elements[t.tri as usize] as usize];
                let delta = (p - min).abs().min((p - max).abs());
                let on_edge = delta.to_array().into_iter().filter(|d| *d < 1e-5).count() >= 2;
                // Projection onto the same crate can legitimately hit another covering part.
                // Check the exposed top/front edge for the high-poly path, all edges for low-on-low.
                let exposed = p.y > 0.49999 && p.z > 0.49999 && p.x.abs() > 0.66 && p.x.abs() < 0.799;
                if on_edge && (!high_poly || exposed) {
                    let value = at(&maps[&MeshMapKind::Curvature], &t)[0];
                    assert!(value > 0.97, "crate edge curvature {value} at {p:?}, slot={slot}, legacy={legacy_metadata}, high={high_poly}");
                    checked += 1;
                }
            }
            assert!(checked >= if high_poly { 6 } else { 100 }, "must exercise complete low-on-low edges and exposed high-poly corners");
        }
    }
}

#[test]
fn parts_buried_under_a_surface_do_not_bake_fake_convex_dashes() {
    // Crate bands pass just behind the corner posts' outer faces. Their hidden edges used to
    // bake convex dashes onto the posts, which Edges masks turned into detached wear marks.
    let mesh = primitives::sample_crate();
    let maps = bake_low(&mesh, &[MeshMapKind::Curvature], 1);
    let radius = settings(&[MeshMapKind::Curvature]).curvature_radius * mesh.bounds.diagonal();
    let mut checked = 0;
    for t in texels(&mesh, 1, RES) {
        let p = t.position;
        let (dx, dz) = ((p.x.abs() - 0.8).abs(), (p.z.abs() - 0.5).abs());
        // Outer post faces, away from the post's own vertical edges and top/bottom rims.
        let lateral = if (p.x.abs() - 0.87).abs() < 1e-4 { dz } else if (p.z.abs() - 0.57).abs() < 1e-4 { dx } else { continue };
        if lateral < 0.07 - radius && p.y.abs() < 0.54 - radius {
            let value = at(&maps[&MeshMapKind::Curvature], &t)[0];
            assert!(value < 0.52, "buried band reads as a convex edge on a post: {value} at {p:?}");
            checked += 1;
        }
    }
    assert!(checked > 100, "only {checked} post texels checked");
}

#[test]
fn curvature_stays_connected_at_exact_cube_corners_and_uv_rims() {
    for subdivisions in [1, 4, 13] {
        let mesh = primitives::atlas_box(Vec3::splat(2.0), subdivisions, "Corner regression");
        let bvh = Bvh::build(&mesh);
        let radius = 0.02 * mesh.bounds.diagonal();
        for normal in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
            for x in [-1.0, 1.0] { for y in [-1.0, 1.0] { for z in [-1.0, 1.0] {
                let corner = Vec3::new(x, y, z);
                if corner.dot(normal) < 0.0 { continue; }
                let near = corner * (1.0 - radius * 0.001) + normal * (radius * 0.001);
                let approaching = curvature::curvature_at(&mesh, &bvh, near, normal, radius);
                let exact = curvature::curvature_at(&mesh, &bvh, corner, normal, radius);
                assert!(exact > 0.97 && (exact - approaching).abs() < 0.03,
                    "corner drops at {corner:?}, {normal:?}, subdivision {subdivisions}: {approaching} -> {exact}");
            } } }
        }
        let maps = bake_low(&mesh, &[MeshMapKind::Curvature], 0);
        let mut checked = 0;
        for t in texels(&mesh, 0, RES) {
            if (Vec3::ONE - t.position.abs()).max_element() < radius * 0.1 {
                let value = at(&maps[&MeshMapKind::Curvature], &t)[0];
                assert!(value > 0.93, "baked corner/rim at {:?} has a gap: {value}", t.position);
                checked += 1;
            }
        }
        assert!(checked >= 12, "must exercise the clamped island-corner rim");
    }
}

#[test]
fn corner_curvature_preserves_concavity_flat_surfaces_and_scale() {
    for size in [0.002, 2.0, 2000.0] {
        let mut mesh = primitives::atlas_box(Vec3::splat(size), 4, "Concave corner");
        for normal in &mut mesh.normals { *normal = -*normal; }
        let bvh = Bvh::build(&mesh);
        let radius = 0.02 * mesh.bounds.diagonal();
        let corner = Vec3::splat(size * 0.5);
        for normal in [-Vec3::X, -Vec3::Y, -Vec3::Z] {
            let value = curvature::curvature_at(&mesh, &bvh, corner, normal, radius);
            assert!(value < 0.03, "inward corner must remain concave at size {size}: {value}");
        }
        let plane = primitives::plane(size, 4);
        let bvh = Bvh::build(&plane);
        for point in [Vec3::ZERO, Vec3::new(size * 0.5, 0.0, size * 0.5)] {
            assert_eq!(curvature::curvature_at(&plane, &bvh, point, Vec3::Y, radius), 0.5);
        }
    }
}

#[test]
fn low_on_low_normals_are_flat_and_world_normals_match_faces() {
    let mesh = primitives::cube(2.0);
    let maps = bake_low(&mesh, &[MeshMapKind::Normal, MeshMapKind::WorldSpaceNormal, MeshMapKind::Position], 0);
    for t in texels(&mesh, 0, RES).iter().step_by(11) {
        let n = at(&maps[&MeshMapKind::Normal], t);
        assert!((n[0] - 0.5).abs() < 2e-3 && (n[1] - 0.5).abs() < 2e-3 && (n[2] - 1.0).abs() < 2e-3, "{n:?}");
        let w = at(&maps[&MeshMapKind::WorldSpaceNormal], t);
        let expect = t.normal * 0.5 + Vec3::splat(0.5);
        assert!((Vec3::new(w[0], w[1], w[2]) - expect).length() < 3e-3);
        let p = at(&maps[&MeshMapKind::Position], t);
        assert!(p[..3].iter().all(|v| (0.0..=1.0).contains(v)));
    }
}

/// High poly: a finely tessellated plane with a smooth bump in the middle.
fn bumped_plane() -> Mesh {
    let mut b = MeshBuilder::new("High");
    let m = b.material("High");
    let s = 64;
    let h = |x: f32, z: f32| 0.15 * (-(x * x + z * z) / 0.08).exp();
    let p = |i: u32, j: u32| {
        let (x, z) = (j as f32 / s as f32 * 2.0 - 1.0, i as f32 / s as f32 * 2.0 - 1.0);
        corner(Vec3::new(x, h(x, z), z), Vec2::new((x + 1.0) * 0.5, (z + 1.0) * 0.5))
    };
    for i in 0..s {
        for j in 0..s {
            b.push_triangle(m, [p(i, j), p(i + 1, j), p(i + 1, j + 1)]);
            b.push_triangle(m, [p(i, j), p(i + 1, j + 1), p(i, j + 1)]);
        }
    }
    b.build().unwrap()
}

#[test]
fn normal_map_from_high_poly_captures_the_bump() {
    let low = primitives::plane(2.0, 1);
    let high = bumped_plane();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let s = BakeSettings { size: RES, maps: vec![MeshMapKind::Normal], cage_front: 0.1, cage_rear: 0.05, ..BakeSettings::default() };
    let maps = bake(&input, &s, &Progress::new()).unwrap();
    let nm = &maps[&MeshMapKind::Normal];
    let mut tilted = 0;
    for t in texels(&low, 0, RES) {
        let n = at(nm, &t);
        let r = Vec2::new(t.position.x, t.position.z).length();
        if r > 0.8 {
            assert!((n[0] - 0.5).abs() < 0.02 && (n[1] - 0.5).abs() < 0.02, "flat area stays flat: {n:?}");
        }
        if (0.15..0.3).contains(&r) && ((n[0] - 0.5).abs() > 0.1 || (n[1] - 0.5).abs() > 0.1) {
            tilted += 1;
        }
    }
    assert!(tilted > 100, "bump slope recorded in the normal map ({tilted} texels)");
}

#[test]
fn a_high_poly_outside_the_cage_is_an_actionable_failure() {
    let low = primitives::plane(2., 1);
    let mut high = primitives::plane(2., 1);
    for p in &mut high.positions { p.y += 10.; }
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let error = bake(&input, &settings(&[MeshMapKind::Normal]), &Progress::new()).unwrap_err();
    assert_eq!(error, BakeError::NoProjection);
    assert!(error.to_string().contains("Cage front / rear"));
}

#[test]
fn partial_high_poly_misses_do_not_index_low_triangles_into_high_ids() {
    let low = primitives::plane(2., 8);
    let mut high = MeshBuilder::new("partial");
    let mat = high.material("high");
    high.push_triangle(mat, [corner(Vec3::new(-1., 0.01, -1.), Vec2::ZERO),
        corner(Vec3::new(-1., 0.01, 1.), Vec2::Y), corner(Vec3::new(1., 0.01, -1.), Vec2::X)]);
    let high = high.build().unwrap();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let maps = bake(&input, &settings(&[MeshMapKind::Id]), &Progress::new()).unwrap();
    let id = &maps[&MeshMapKind::Id];
    assert!(id.to_dense().iter().all(|v| v.is_finite()));
    let a = id.get(16, 16);
    let b = id.get(112, 112);
    assert_ne!(a, b, "misses retain neutral IDs instead of a false high-poly triangle");
}

/// QA 2026-11: a thickness distance of 0 divided 0 by 0 and filled the map with NaN.
#[test]
fn thickness_with_a_zero_distance_is_finite() {
    let mesh = primitives::plane(2., 1);
    let bvh = Bvh::build(&mesh);
    let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let s = BakeSettings { thickness_distance: 0.0, use_gpu: false, ..settings(&[MeshMapKind::Thickness]) };
    let maps = bake(&input, &s, &Progress::new()).unwrap();
    assert!(maps[&MeshMapKind::Thickness].to_dense().iter().all(|v| v.is_finite()), "NaN in the thickness map");
}

#[test]
fn thickness_is_bright_on_thin_parts() {
    let slab = primitives::thin_slab(2.0, 2.0, 0.05);
    let cube = primitives::cube(2.0);
    let center = |m: &Mesh| {
        let maps = bake_low(m, &[MeshMapKind::Thickness], 0);
        let ts = texels(m, 0, RES);
        let t = ts.iter().min_by(|a, b| (a.position.x.abs() + a.position.y.abs()).total_cmp(&(b.position.x.abs() + b.position.y.abs()))).unwrap();
        at(&maps[&MeshMapKind::Thickness], t)[0]
    };
    let (thin, thick) = (center(&slab), center(&cube));
    assert!(thin > 0.8 && thick < thin - 0.3, "thin {thin} vs thick {thick}");
}

#[test]
fn id_colors_differ_per_element_and_cancel_stops() {
    let mesh = primitives::sample_crate();
    let maps = bake_low(&mesh, &[MeshMapKind::Id], 1);
    let id = &maps[&MeshMapKind::Id];
    let mut colors: Vec<[u32; 3]> =
        texels(&mesh, 1, RES).iter().map(|t| at(id, t)).map(|c| [(c[0] * 100.0) as u32, (c[1] * 100.0) as u32, (c[2] * 100.0) as u32]).collect();
    colors.sort();
    colors.dedup();
    assert!(colors.len() >= 6, "trim posts and bands get distinct ID colors ({})", colors.len());

    let bvh = Bvh::build(&mesh);
    let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let p = Progress::new();
    p.cancel();
    assert_eq!(bake(&input, &settings(&[MeshMapKind::AmbientOcclusion]), &p).unwrap_err(), BakeError::Cancelled);
}

#[test]
fn height_map_records_the_bump_from_a_high_poly_and_is_flat_without_one() {
    let low = primitives::plane(2.0, 1);
    let high = bumped_plane();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let diag = low.bounds.diagonal();
    let s = BakeSettings { size: RES, maps: vec![MeshMapKind::Height], cage_front: 0.1, cage_rear: 0.05, ..BakeSettings::default() };
    let maps = bake(&input, &s, &Progress::new()).unwrap();
    let height = &maps[&MeshMapKind::Height];
    assert_eq!(height.comps(), 1);
    let (mut centre, mut flat) = (0.0f32, 0.0f32);
    let (mut nc, mut nf) = (0, 0);
    for t in texels(&low, 0, RES) {
        let v = at(height, &t)[0];
        assert!((0.0..=1.0).contains(&v));
        let r = Vec2::new(t.position.x, t.position.z).length();
        if r < 0.05 { centre += v; nc += 1; }
        if r > 0.8 { flat += v; nf += 1; }
    }
    let (centre, flat) = (centre / nc as f32, flat / nf as f32);
    assert!((flat - 0.5).abs() < 0.02, "the flat surround stays at 0.5: {flat}");
    // The bump is 0.15 tall; the front cage is 0.1 of the diagonal, so the peak reads 0.5 + 0.5 * 0.15 / (0.1 * diag).
    let expected = 0.5 + 0.5 * (0.15 / (0.1 * diag)).min(1.0);
    assert!((centre - expected).abs() < 0.04, "peak height {centre} vs {expected}");
    // Low on low: nothing stands above the surface.
    let flat_maps = bake_low(&low, &[MeshMapKind::Height], 0);
    for t in texels(&low, 0, RES).iter().step_by(31) {
        assert_eq!(at(&flat_maps[&MeshMapKind::Height], t)[0], 0.5);
    }
}

#[test]
fn bent_normal_leans_away_from_a_wall_and_equals_the_normal_in_the_open() {
    let open = primitives::plane(2.0, 2);
    let maps = bake_low(&open, &[MeshMapKind::BentNormal], 0);
    let bent = &maps[&MeshMapKind::BentNormal];
    assert_eq!(bent.comps(), 3);
    for t in texels(&open, 0, RES).iter().step_by(37) {
        let b = at(bent, t);
        assert!((b[0] - 0.5).abs() < 0.12 && (b[1] - 0.5).abs() < 0.12 && b[2] > 0.9, "open plane bends to the normal: {b:?}");
    }
    // Floor beside a box: the open sky is away from the box, so the bent normal tilts that way.
    let mesh = floor_with_box();
    let maps = bake_low(&mesh, &[MeshMapKind::BentNormal], 0);
    let bent = &maps[&MeshMapKind::BentNormal];
    let (mut tilt_away, mut count) = (0.0f32, 0);
    for t in texels(&mesh, 0, RES) {
        let to_box = Vec3::new(t.position.x.clamp(-0.5, 0.5) - t.position.x, 0.0, t.position.z.clamp(-0.5, 0.5) - t.position.z);
        let d = to_box.length();
        if d > 0.0 && d < 0.1 {
            let b = at(bent, &t);
            assert!(b.iter().all(|v| v.is_finite()));
            // Tangent frame of the floor is not world aligned, so test the tilt magnitude instead of a sign.
            tilt_away += Vec2::new(b[0] - 0.5, b[1] - 0.5).length();
            count += 1;
        }
    }
    assert!(count > 20 && tilt_away / count as f32 > 0.1, "texels beside the box lean: {}", tilt_away / count.max(1) as f32);
}

mod ray_backend {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use subcraft_mesh::ray_batch::{BatchControl, RayBatchBackend, RayBatchError, RayMode, RayTexel};

    use super::*;
    use crate::bake_streamed_with;

    /// Answers every texel with a constant and reports how it was called.
    struct Fake {
        result: Result<f32, RayBatchError>,
        calls: AtomicUsize,
    }

    impl RayBatchBackend for Fake {
        fn description(&self) -> String {
            "fake".into()
        }
        fn run(&self, _: &Bvh, count: usize, texel: &(dyn Fn(usize) -> RayTexel + Sync), rays: u32, _: RayMode, control: &dyn BatchControl)
            -> Result<Vec<f32>, RayBatchError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            assert!(rays >= 1);
            let first = texel(0);
            assert!(first.axis.iter().all(|v| v.is_finite()));
            control.progress(0.5);
            self.result.clone().map(|v| vec![v; count])
        }
    }

    fn run(backend: Option<&dyn RayBatchBackend>, use_gpu: bool, maps: &[MeshMapKind]) -> (BTreeMap<MeshMapKind, TiledImage>, Vec<crate::MapReport>) {
        let mesh = primitives::cube(2.0);
        let bvh = Bvh::build(&mesh);
        let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
        let progress = Progress::new();
        let s = BakeSettings { use_gpu, ..settings(maps) };
        let out = bake_streamed_with(&input, &s, &progress, backend, |_, _| {}).unwrap();
        assert!((progress.fraction() - 1.0).abs() < 1e-6, "progress ends complete: {}", progress.fraction());
        (out, progress.reports())
    }

    #[test]
    fn a_backend_result_is_used_for_ao_and_thickness_only() {
        let fake = Fake { result: Ok(0.25), calls: AtomicUsize::new(0) };
        let (maps, reports) = run(Some(&fake), true, &[MeshMapKind::AmbientOcclusion, MeshMapKind::Thickness, MeshMapKind::Position]);
        assert_eq!(fake.calls.load(Ordering::Relaxed), 2);
        assert_eq!(maps[&MeshMapKind::AmbientOcclusion].get(10, 10)[0], 0.25);
        assert_eq!(maps[&MeshMapKind::Thickness].get(10, 10)[0], 0.25);
        let backends: Vec<_> = reports.iter().map(|r| (r.kind, r.backend.as_str())).collect();
        assert_eq!(backends, [(MeshMapKind::AmbientOcclusion, "fake"), (MeshMapKind::Thickness, "fake"), (MeshMapKind::Position, "CPU")]);
    }

    #[test]
    fn the_preference_off_never_calls_the_backend() {
        let fake = Fake { result: Ok(0.25), calls: AtomicUsize::new(0) };
        let (maps, reports) = run(Some(&fake), false, &[MeshMapKind::AmbientOcclusion]);
        assert_eq!(fake.calls.load(Ordering::Relaxed), 0);
        assert_ne!(maps[&MeshMapKind::AmbientOcclusion].get(10, 10)[0], 0.25);
        assert_eq!(reports[0].backend, "CPU");
    }

    #[test]
    fn a_failing_backend_falls_back_to_the_exact_cpu_result() {
        let fake = Fake { result: Err(RayBatchError::Unsupported("device lost".into())), calls: AtomicUsize::new(0) };
        let (gpu_failed, reports) = run(Some(&fake), true, &[MeshMapKind::AmbientOcclusion, MeshMapKind::Thickness]);
        let (cpu, _) = run(None, true, &[MeshMapKind::AmbientOcclusion, MeshMapKind::Thickness]);
        assert_eq!(fake.calls.load(Ordering::Relaxed), 2);
        assert_eq!(gpu_failed, cpu);
        assert!(reports.iter().all(|r| r.backend == "CPU (GPU failed: device lost)"), "{reports:?}");
    }

    #[test]
    fn cancelling_inside_the_backend_cancels_the_bake() {
        let fake = Fake { result: Err(RayBatchError::Cancelled), calls: AtomicUsize::new(0) };
        let mesh = primitives::cube(2.0);
        let bvh = Bvh::build(&mesh);
        let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
        let r = bake_streamed_with(&input, &settings(&[MeshMapKind::AmbientOcclusion]), &Progress::new(), Some(&fake), |_, _| {});
        assert_eq!(r.unwrap_err(), BakeError::Cancelled);
    }
}

#[test]
fn partial_cage_misses_and_name_matching_on_the_cpu_are_warned_about() {
    let low = primitives::plane(2., 8);
    let mut high = MeshBuilder::new("partial");
    let mat = high.material("high");
    high.push_triangle(mat, [corner(Vec3::new(-1., 0.01, -1.), Vec2::ZERO),
        corner(Vec3::new(-1., 0.01, 1.), Vec2::Y), corner(Vec3::new(1., 0.01, -1.), Vec2::X)]);
    let high = high.build().unwrap();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let progress = Progress::new();
    bake(&input, &settings(&[MeshMapKind::Id]), &progress).unwrap();
    let warnings = progress.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("Cage:") && warnings[0].contains("Cage front / rear") && warnings[0].contains('%'), "{warnings:?}");
    // Low on low never warns, and the list is reset by every bake.
    let flat = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: None, material_slot: 0 };
    bake(&flat, &settings(&[MeshMapKind::Id]), &progress).unwrap();
    assert!(progress.warnings().is_empty());
}

#[test]
fn smoothing_removes_ray_noise_but_not_gradients_and_ignores_uncovered_texels() {
    let size = 16usize;
    let covered: Vec<bool> = (0..size * size).map(|i| i % size < 12).collect();
    // A ramp plus alternating +-0.1 noise; uncovered texels hold garbage that must not leak in.
    let mut dense: Vec<f32> = (0..size * size).map(|i| {
        let (x, y) = (i % size, i / size);
        if x >= 12 { 99.0 } else { x as f32 / 20.0 + if (x + y) % 2 == 0 { 0.1 } else { -0.1 } }
    }).collect();
    let before = dense.clone();
    crate::smooth_covered(size, 1, &mut dense, &covered);
    let roughness = |d: &[f32]| (1..11).map(|x| (d[5 * size + x + 1] - d[5 * size + x]).abs()).fold(0.0f32, f32::max);
    assert!(roughness(&dense) < roughness(&before) * 0.35, "{} vs {}", roughness(&dense), roughness(&before));
    assert!(dense.iter().enumerate().all(|(i, v)| !covered[i] || *v < 1.0), "garbage leaked");
    assert!((dense[5 * size + 6] - 6.0 / 20.0).abs() < 0.06);
}

/// The ray-miss heatmap marks exactly the texels the baker reports as cage misses: a high poly
/// covering only half the plane misses the other half; a bigger cage cannot invent a surface.
#[test]
fn cage_misses_mark_texels_without_a_high_poly_surface() {
    let low = primitives::plane(2.0, 4);
    // High poly: the x < 0 half of the plane, slightly raised.
    let mut b = MeshBuilder::new("half");
    let m = b.material("half");
    let p = |x: f32, z: f32| Corner { position: Vec3::new(x, 0.005, z), normal: Some(Vec3::Y), uv: None };
    b.push_triangle(m, [p(-1.0, -1.0), p(0.0, 1.0), p(0.0, -1.0)]);
    b.push_triangle(m, [p(-1.0, -1.0), p(-1.0, 1.0), p(0.0, 1.0)]);
    let high = b.build_for_baking().unwrap();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let s = BakeSettings { cage_front: 0.01, cage_rear: 0.01, ..Default::default() };
    let r = cage_misses(&input, &s, 64).unwrap();
    let frac = r.missed as f32 / r.covered as f32;
    assert!((frac - 0.5).abs() < 0.06, "half the texels miss ({frac})");
    for t in texels(&low, 0, 64) {
        let v = r.image.get(t.index % 64, t.index / 64)[0];
        if t.position.x < -0.05 { assert_eq!(v, 0.0, "hit at {}", t.position); }
        if t.position.x > 0.05 { assert_eq!(v, 1.0, "miss at {}", t.position); }
    }
    // A cage too thin to reach the raised surface: everything misses.
    let thin = BakeSettings { cage_front: 0.0005, cage_rear: 0.0005, ..Default::default() };
    let r = cage_misses(&input, &thin, 32).unwrap();
    assert_eq!(r.missed, r.covered);
    // Without a high poly nothing is reported.
    let none = BakeInput { high: None, ..input };
    assert_eq!(cage_misses(&none, &s, 32).unwrap().missed, 0);
}

/// A plane low poly under a high poly made of two quads: the x < 0 one is object "left" with
/// material "Red" (file colour red) and green vertex colours, the x > 0 one is object "right" with
/// material "Blue" and yellow vertex colours.
fn two_part_high(vertex_colors: bool, material_colors: bool) -> Mesh {
    let mut b = MeshBuilder::new("parts");
    let p = |x: f32, z: f32| Corner { position: Vec3::new(x, 0.005, z), normal: Some(Vec3::Y), uv: None };
    // A small gap in the middle keeps the two quads separate mesh elements.
    for (name, material, x0, x1, kd, vc) in [("left", "Red", -1.0f32, -0.01f32, [1.0f32, 0.0, 0.0], [0.0f32, 1.0, 0.0, 1.0]), ("right", "Blue", 0.01, 1.0, [0.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0])] {
        b.object(name);
        let m = b.material(material);
        if material_colors {
            b.set_material_color(m, kd);
        }
        for tri in [[p(x0, -1.0), p(x1, 1.0), p(x1, -1.0)], [p(x0, -1.0), p(x0, 1.0), p(x1, 1.0)]] {
            if vertex_colors {
                b.push_triangle_colored(m, tri, [vc; 3]);
            } else {
                b.push_triangle(m, tri);
            }
        }
    }
    b.build_for_baking().unwrap()
}

/// The ID map's colour left and right of the middle of the plane, with the bake's warnings.
fn id_bake(high: &Mesh, source: IdSource) -> ([f32; 3], [f32; 3], Vec<String>) {
    id_bake_with(high, &BakeSettings { id_source: source, ..settings(&[MeshMapKind::Id]) })
}

/// [`id_bake`] with explicit settings (which must ask for the ID map).
fn id_bake_with(high: &Mesh, s: &BakeSettings) -> ([f32; 3], [f32; 3], Vec<String>) {
    let low = primitives::plane(2.0, 4);
    let (lb, hb) = (Bvh::build(&low), Bvh::build(high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: high, bvh: &hb }), material_slot: 0 };
    let progress = Progress::new();
    let maps = bake(&input, s, &progress).unwrap();
    let id = &maps[&MeshMapKind::Id];
    let (mut left, mut right) = (None, None);
    for t in texels(&low, 0, RES) {
        let c = at(id, &t);
        let c = [c[0], c[1], c[2]];
        // Every texel of a side has that side's colour (away from the gap in the middle and
        // from the rim, whose texels lie just outside the high poly).
        if t.position.x.abs() > 0.95 || t.position.z.abs() > 0.95 {
            continue;
        }
        if t.position.x < -0.1 {
            assert_eq!(*left.get_or_insert(c), c, "left side is one colour");
        } else if t.position.x > 0.1 {
            assert_eq!(*right.get_or_insert(c), c, "right side is one colour");
        }
    }
    // The gap between the two quads leaves a column of texels without a high surface, which the
    // bake reports on its own; the tests here look at the warnings about colour sources.
    let warnings = progress.warnings().into_iter().filter(|w| !w.starts_with("Cage:")).collect();
    (left.unwrap(), right.unwrap(), warnings)
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 2e-3)
}

#[test]
fn id_map_takes_its_colours_from_the_chosen_source() {
    let high = two_part_high(true, true);
    let (l, r, w) = id_bake(&high, IdSource::VertexColor);
    assert!(close(l, [0.0, 1.0, 0.0]) && close(r, [1.0, 1.0, 0.0]), "vertex colours: {l:?} {r:?}");
    assert!(w.is_empty(), "{w:?}");
    let (l, r, w) = id_bake(&high, IdSource::MaterialColor);
    assert!(close(l, [1.0, 0.0, 0.0]) && close(r, [0.0, 0.0, 1.0]), "material colours: {l:?} {r:?}");
    assert!(w.is_empty(), "{w:?}");
    // Generated colours: the two quads are separate elements and separate objects.
    let (ml, mr, _) = id_bake(&high, IdSource::MeshId);
    assert!(!close(ml, mr), "one colour per mesh element");
    let (sl, sr, _) = id_bake(&high, IdSource::Submesh);
    assert!(!close(sl, sr), "one colour per object");
    assert_eq!(BakeSettings::default().id_source, IdSource::MeshId, "the default is today's behaviour");
}

#[test]
fn id_sources_the_model_lacks_fall_back_to_generated_colours_with_a_warning() {
    let plain = two_part_high(false, false);
    let (ml, mr, w) = id_bake(&plain, IdSource::MeshId);
    assert!(w.is_empty(), "{w:?}");
    let (l, r, w) = id_bake(&plain, IdSource::VertexColor);
    assert!(close(l, ml) && close(r, mr), "no vertex colours: the mesh-element colours");
    assert!(w.iter().any(|w| w.contains("no vertex colours")), "{w:?}");
    let (sl, sr, _) = id_bake(&plain, IdSource::Submesh);
    let (l, r, w) = id_bake(&plain, IdSource::MaterialColor);
    assert!(close(l, sl) && close(r, sr), "no material colours: the per-object colours");
    assert!(w.iter().any(|w| w.contains("materials have no colours")), "{w:?}");
    // A placed (mirrored) high poly keeps each corner's colour.
    let high = two_part_high(true, false);
    let mirrored = high.transformed(&glam::Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
    let (l, r, _) = id_bake(&mirrored, IdSource::VertexColor);
    assert!(close(l, [1.0, 1.0, 0.0]) && close(r, [0.0, 1.0, 0.0]), "mirrored: the sides swap, {l:?} {r:?}");
}

/// Bake a 2 x 2 plane (y = 0, facing up) against `high` with `settings`.
fn bake_plane(high: &Mesh, settings: &BakeSettings) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let low = primitives::plane(2.0, 4);
    let (lb, hb) = (Bvh::build(&low), Bvh::build(high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: high, bvh: &hb }), material_slot: 0 };
    bake(&input, settings, &Progress::new())
}

#[test]
fn dilation_pads_up_to_its_width_and_diffusion_fills_the_rest() {
    // One quad whose UVs cover the middle quarter of the texture: texels 24..40 of 64.
    let mut b = MeshBuilder::new("island");
    let m = b.material("m");
    let c = |x: f32, z: f32| corner(Vec3::new(x, 0.0, z), Vec2::new(0.375 + 0.125 * (x + 1.0), 0.375 + 0.125 * (z + 1.0)));
    b.push_triangle(m, [c(-1.0, -1.0), c(1.0, 1.0), c(1.0, -1.0)]);
    b.push_triangle(m, [c(-1.0, -1.0), c(-1.0, 1.0), c(1.0, 1.0)]);
    let mesh = b.build().unwrap();
    let bvh = Bvh::build(&mesh);
    let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let id = |dilation: u32, diffusion: bool| {
        let s = BakeSettings { size: 64, maps: vec![MeshMapKind::Id], dilation, diffusion, ..Default::default() };
        bake(&input, &s, &Progress::new()).unwrap().remove(&MeshMapKind::Id).unwrap()
    };
    let rgb = |img: &TiledImage, x: u32, y: u32| { let v = img.get(x, y); [v[0], v[1], v[2]] };
    let neutral = MeshMapKind::Id.neutral_value();
    let plain = id(4, false);
    // (36, 30) and the gutter texels below lie on the same triangle's side of the quad's diagonal.
    let island = rgb(&plain, 36, 30);
    assert!(!close(island, neutral), "the island has an ID colour: {island:?}");
    assert_eq!(rgb(&plain, 42, 32), island, "2 texels outside the island: inside the dilation");
    assert_eq!(rgb(&plain, 60, 32), neutral, "20 texels outside: beyond a dilation of 4, left neutral without diffusion");
    assert_eq!(rgb(&id(30, false), 60, 32), island, "a wider dilation reaches it");
    let diffused = id(4, true);
    assert_eq!(rgb(&diffused, 42, 32), island, "diffusion leaves the dilated texels alone");
    assert!(close(rgb(&diffused, 60, 32), island), "diffusion spreads the one colour everywhere: {:?}", rgb(&diffused, 60, 32));
    assert!((BakeSettings::default().dilation, BakeSettings::default().diffusion) == (16, true));
}

#[test]
fn antialiasing_supersamples_and_keeps_the_output_size() {
    let high = two_part_high(true, false);
    let base = BakeSettings { id_source: IdSource::VertexColor, ..settings(&[MeshMapKind::Id, MeshMapKind::Normal]) };
    let plain = bake_plane(&high, &base).unwrap();
    let smooth = bake_plane(&high, &BakeSettings { antialiasing: Antialiasing::X2, ..base.clone() }).unwrap();
    for kind in [MeshMapKind::Id, MeshMapKind::Normal] {
        assert_eq!((smooth[&kind].width(), smooth[&kind].height()), (RES, RES), "{kind:?} keeps the asked size");
    }
    // Away from the colour border both bakes agree; the flat normal stays a unit vector.
    for (x, y) in [(20, 40), (100, 88), (30, 20), (110, 100)] {
        let (a, b) = (plain[&MeshMapKind::Id].get(x, y), smooth[&MeshMapKind::Id].get(x, y));
        assert!(close([a[0], a[1], a[2]], [b[0], b[1], b[2]]), "ID at {x},{y}: {a:?} {b:?}");
        let n = smooth[&MeshMapKind::Normal].get(x, y);
        assert!(close([n[0], n[1], n[2]], [0.5, 0.5, 1.0]), "normal at {x},{y}: {n:?}");
    }
    assert_eq!(Antialiasing::ALL.map(Antialiasing::factor), [1, 2, 4, 8]);
    // The supersampled bake never exceeds the size limit.
    let factor = |size: u32, antialiasing: Antialiasing| supersampling(&BakeSettings { size, antialiasing, ..Default::default() });
    assert_eq!((factor(1024, Antialiasing::X8), factor(4096, Antialiasing::X8), factor(MAX_SUPERSAMPLED_SIZE, Antialiasing::X2), factor(2048, Antialiasing::None)), (8, 2, 1, 1));
}

#[test]
fn averaged_normals_tilt_the_rays_at_hard_edges_only_when_asked() {
    let cube = primitives::cube(2.0);
    let diag = cube.bounds.diagonal();
    let samples = texels(&cube, 0, 64);
    assert!(!samples.is_empty());
    let hard = Cage::new(&cube, 0, 64, &BakeSettings::default(), diag);
    let averaged = Cage::new(&cube, 0, 64, &BakeSettings { average_normals: true, ..Default::default() }, diag);
    let mut tilted = 0;
    for t in &samples {
        // The cube's own normals: every ray runs straight down its face.
        let ray = hard.ray(&cube, t);
        assert!(ray.dir.dot(t.geometric_normal) < -0.999, "hard: {:?} on {:?}", ray.dir, t.geometric_normal);
        assert!((ray.origin - (t.position + t.geometric_normal * ray.front)).length() < 1e-4);
        // Averaged: still from outside towards the surface, tilted towards the edges.
        let ray = averaged.ray(&cube, t);
        assert!((ray.dir.length() - 1.0).abs() < 1e-4 && ray.dir.dot(t.geometric_normal) < -0.3, "averaged: {:?} on {:?}", ray.dir, t.geometric_normal);
        assert!((ray.origin + ray.dir * ray.front - t.position).length() < 1e-4, "the ray passes through the low-poly point");
        if ray.dir.dot(t.geometric_normal) > -0.95 { tilted += 1; }
    }
    assert!(tilted * 4 > samples.len(), "rays near the edges lean over them: {tilted} of {}", samples.len());
    // At a corner the averaged normal is the cube's diagonal, shared by the three faces.
    let normals = averaged_normals(&cube);
    for (p, n) in cube.positions.iter().zip(&normals).filter(|(p, _)| p.abs().min_element() > 0.99) {
        assert!((*n - p.normalize()).length() < 1e-3, "{p:?}: {n:?}");
    }
}

#[test]
fn ignore_backface_skips_high_poly_faces_that_face_away() {
    // The high poly just under the plane, turned to face down: the rays see its back.
    let high = two_part_high(false, false).transformed(&glam::Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)));
    assert!(high.geometric_normal(0).y < -0.9);
    let both = settings(&[MeshMapKind::Id]);
    assert!(bake_plane(&high, &both).is_ok(), "both sides are hit by default");
    let front_only = BakeSettings { ignore_backface: true, ..both.clone() };
    assert_eq!(bake_plane(&high, &front_only).err(), Some(BakeError::NoProjection));
    // Facing up it is hit either way, and the ray-miss check follows the same rule.
    let up = two_part_high(false, false);
    assert!(bake_plane(&up, &front_only).is_ok());
    let low = primitives::plane(2.0, 4);
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let misses = cage_misses(&input, &front_only, 32).unwrap();
    assert_eq!(misses.missed, misses.covered);
    assert!(cage_misses(&input, &both, 32).unwrap().missed < misses.covered / 2);
}

#[test]
fn a_custom_cage_decides_where_the_rays_start() {
    // The high poly floats half a unit above the plane, far beyond the distance-based cage.
    let high = two_part_high(false, false).transformed(&glam::Mat4::from_translation(Vec3::new(0.0, 0.5, 0.0)));
    let distance = settings(&[MeshMapKind::Id, MeshMapKind::Height]);
    assert_eq!(bake_plane(&high, &distance).err(), Some(BakeError::NoProjection));
    // A cage: the low poly (same UVs) lifted above the high poly.
    let cage = primitives::plane(2.0, 4).transformed(&glam::Mat4::from_translation(Vec3::Y));
    let caged = BakeSettings { cage: Some(CageMesh(std::sync::Arc::new(cage))), ..distance.clone() };
    let maps = bake_plane(&high, &caged).unwrap();
    let id = maps[&MeshMapKind::Id].get(30, 40);
    assert!(!close([id[0], id[1], id[2]], MeshMapKind::Id.neutral_value()), "the high poly is reached: {id:?}");
    assert!(maps[&MeshMapKind::Height].get(30, 40)[0] > 0.9, "and it stands above the low poly");
    assert_eq!(caged.cage, caged.clone().cage, "a cage equals itself");
    assert_ne!(caged.cage, Some(CageMesh(std::sync::Arc::new(primitives::plane(2.0, 4)))));
}

/// Ambient occlusion at the middle of a 2 x 2 plane that faces down, baked onto itself.
fn ao_under_plane(ao: AoOptions) -> f32 {
    let low = primitives::plane(2.0, 4).transformed(&glam::Mat4::from_rotation_x(std::f32::consts::PI));
    assert!(low.geometric_normal(0).y < -0.9, "the plane faces down");
    let bvh = Bvh::build(&low);
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &bvh }, high: None, material_slot: 0 };
    let s = BakeSettings { ao, use_gpu: false, ..settings(&[MeshMapKind::AmbientOcclusion]) };
    bake(&input, &s, &Progress::new()).unwrap()[&MeshMapKind::AmbientOcclusion].get(64, 64)[0]
}

#[test]
fn ambient_occlusion_options_shape_the_rays() {
    assert!(AoOptions::default().is_plain());
    assert_eq!(ao_under_plane(AoOptions::default()), 1.0, "nothing under the plane");
    // A floor 0.05 diagonals under it: nearly every ray of the hemisphere reaches it.
    let floor = AoOptions { ground_plane: true, ground_offset: 0.05, ..Default::default() };
    assert!(!floor.is_plain());
    let dark = ao_under_plane(floor);
    assert!(dark < 0.12, "the floor occludes: {dark}");
    // Uniform rays: more of them graze the horizon and escape.
    let uniform = ao_under_plane(AoOptions { distribution: RayDistribution::Uniform, ..floor });
    assert!(uniform > dark + 0.08 && uniform < 0.4, "uniform {uniform} vs cosine {dark}");
    // A narrow cone only looks straight down: fully occluded.
    assert!(ao_under_plane(AoOptions { spread_angle: 20.0, ..floor }) < 0.01);
    // Attenuation: the floor is 0.2 of the maximum distance away, so it counts for less.
    let linear = ao_under_plane(AoOptions { attenuation: AoAttenuation::Linear, ..floor });
    let smooth = ao_under_plane(AoOptions { attenuation: AoAttenuation::Smooth, ..floor });
    assert!(linear > dark + 0.1 && linear < 0.8, "linear {linear}");
    assert!(smooth > dark && smooth < linear, "smooth {smooth} keeps near occluders darker than linear {linear}");
    // A minimum distance beyond the floor straight below ignores most of it.
    let far_only = ao_under_plane(AoOptions { min_distance: 0.1, ..floor });
    assert!(far_only > dark + 0.5, "min distance {far_only}");
    assert_eq!((AoAttenuation::None.weight(0.3, 1.0), AoAttenuation::Linear.weight(0.25, 1.0), AoAttenuation::Smooth.weight(0.5, 1.0)), (1.0, 0.75, 0.5));
}

#[test]
fn cone_directions_stay_inside_the_cone_and_match_the_cosine_hemisphere() {
    let n = Vec3::new(0.3, 0.9, -0.2).normalize();
    let (mut cone, mut hemi) = (Vec::new(), Vec::new());
    surface::cone_directions(n, 64, 0.37, 180.0, false, &mut cone);
    surface::cosine_directions(n, 64, 0.37, &mut hemi);
    for (a, b) in cone.iter().zip(&hemi) { assert!((*a - *b).length() < 1e-4, "{a:?} {b:?}"); }
    for uniform in [false, true] {
        surface::cone_directions(n, 64, 0.11, 60.0, uniform, &mut cone);
        assert_eq!(cone.len(), 64);
        let limit = 30f32.to_radians().cos() - 1e-4;
        assert!(cone.iter().all(|d| (d.length() - 1.0).abs() < 1e-4 && d.dot(n) >= limit), "uniform {uniform}");
    }
    // Uniform spreads wider on average than cosine weighting.
    let mean = |uniform: bool| { let mut d = Vec::new(); surface::cone_directions(n, 256, 0.0, 180.0, uniform, &mut d); d.iter().map(|d| d.dot(n)).sum::<f32>() / 256.0 };
    assert!((mean(false) - 2.0 / 3.0).abs() < 0.02 && (mean(true) - 0.5).abs() < 0.02, "{} {}", mean(false), mean(true));
}

#[test]
fn position_map_normalization_and_single_axis() {
    let mesh = primitives::plane(2.0, 4);
    let bvh = Bvh::build(&mesh);
    let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let position = |position: PositionOptions| {
        let s = BakeSettings { position, ..settings(&[MeshMapKind::Position]) };
        bake(&input, &s, &Progress::new()).unwrap().remove(&MeshMapKind::Position).unwrap()
    };
    let boxed = position(PositionOptions::default());
    let sphere = position(PositionOptions { normalization: PositionNormalization::BoundingSphere, ..Default::default() });
    let z_only = position(PositionOptions { single_axis: Some(2), ..Default::default() });
    let radius = mesh.bounds.diagonal() * 0.5;
    for t in texels(&mesh, 0, RES) {
        let p = t.position;
        // Rim texels lie just outside the plane, where the map is clamped.
        if p.x.abs() > 1.0 || p.z.abs() > 1.0 { continue; }
        let (b, s, z) = (at(&boxed, &t), at(&sphere, &t), at(&z_only, &t));
        // Box: each axis spans 0..1 (the flat Y axis sits at the box's floor).
        assert!((b[0] - (p.x + 1.0) * 0.5).abs() < 2e-3 && (b[2] - (p.z + 1.0) * 0.5).abs() < 2e-3, "box {b:?} at {p}");
        // Sphere: one scale for all axes, the centre at 0.5.
        let expect = |v: f32| 0.5 + v / (2.0 * radius);
        assert!((s[0] - expect(p.x)).abs() < 2e-3 && (s[1] - 0.5).abs() < 2e-3 && (s[2] - expect(p.z)).abs() < 2e-3, "sphere {s:?} at {p}");
        // Single axis: gray, the value of that axis.
        assert!(z[0] == z[1] && z[1] == z[2] && (z[0] - b[2]).abs() < 1e-6, "single axis {z:?} vs {b:?}");
    }
}

#[test]
fn height_map_can_use_a_manual_range() {
    let high = two_part_high(false, false);
    let low = primitives::plane(2.0, 4);
    let diag = low.bounds.diagonal().max(high.bounds.diagonal());
    let height = |height: HeightNormalization| {
        let s = BakeSettings { height, ..settings(&[MeshMapKind::Height]) };
        bake_plane(&high, &s).unwrap()[&MeshMapKind::Height].get(30, 40)[0]
    };
    // The high poly stands 0.005 above the plane. Cage: over the front reach of 0.01 diagonals.
    let cage = height(HeightNormalization::Cage);
    assert!((cage - (0.5 + 0.5 * 0.005 / (0.01 * diag))).abs() < 5e-3, "cage {cage}");
    // Manual: white at 0.02 above.
    let manual = height(HeightNormalization::Manual { distance: 0.02 / diag });
    assert!((manual - 0.625).abs() < 5e-3, "manual {manual}");
    // A range shorter than the offset saturates.
    assert!(height(HeightNormalization::Manual { distance: 0.004 / diag }) > 0.999);
}

#[test]
fn id_colour_generators() {
    let high = two_part_high(false, false);
    let ids = |id_generator: IdGenerator| {
        let s = BakeSettings { id_generator, ..settings(&[MeshMapKind::Id]) };
        let (l, r, _) = id_bake_with(&high, &s);
        (l, r)
    };
    let gray = |c: [f32; 3]| c[0] == c[1] && c[1] == c[2];
    let (l, r) = ids(IdGenerator::Grayscale);
    assert!(gray(l) && gray(r) && !close(l, r), "grayscale {l:?} {r:?}");
    let (l, r) = ids(IdGenerator::Random);
    assert!(!gray(l) && !gray(r) && !close(l, r), "random {l:?} {r:?}");
    let (l, r) = ids(IdGenerator::HueShift);
    assert!(!gray(l) && !close(l, r), "hue shift {l:?} {r:?}");
    assert_eq!(BakeSettings::default().id_generator, IdGenerator::HueShift, "the default is the colours so far");
    assert_eq!(IdGenerator::HueShift.color(3), subcraft_core::color::hsv_to_rgb([(3.0f32 * 0.618_034).fract(), 0.65, 0.9]));
    // No generator gives black, the ID map's empty value.
    for g in IdGenerator::ALL { for i in 0..64 { assert!(g.color(i).iter().all(|c| *c >= 0.1 && *c <= 1.0), "{g:?} {i}"); } }
}

#[test]
fn the_id_map_reads_the_chosen_vertex_color_set() {
    // The two-part high poly with a second colour set: blue on the left part, white on the right.
    let mut high = two_part_high(true, false);
    let second: Vec<[f32; 4]> = (0..high.triangle_count()).flat_map(|t| {
        let left = high.tri_positions(t)[0].x < 0.0;
        [if left { [0.0, 0.0, 1.0, 1.0] } else { [1.0, 1.0, 1.0, 1.0] }; 3]
    }).collect();
    high.extra_color_sets.push(second);
    assert_eq!(high.color_set_count(), 2);
    let with_set = |set: u32| BakeSettings { id_source: IdSource::VertexColor, vertex_color_set: set, ..settings(&[MeshMapKind::Id]) };
    let (l, r, w) = id_bake_with(&high, &with_set(0));
    assert!(close(l, [0.0, 1.0, 0.0]) && close(r, [1.0, 1.0, 0.0]) && w.is_empty(), "set 0: {l:?} {r:?} {w:?}");
    let (l, r, w) = id_bake_with(&high, &with_set(1));
    assert!(close(l, [0.0, 0.0, 1.0]) && close(r, [1.0, 1.0, 1.0]) && w.is_empty(), "set 1: {l:?} {r:?} {w:?}");
    // A set the model lacks: generated colours, and the warning names the sets it has.
    let (l, r, w) = id_bake_with(&high, &with_set(4));
    let (ml, mr, _) = id_bake(&high, IdSource::MeshId);
    assert!(close(l, ml) && close(r, mr), "fallback to the mesh-element colours");
    assert!(w.iter().any(|w| w.contains("2 vertex colour set(s), not set 5")), "{w:?}");
    // A mirrored high poly keeps every set's colours on their corners.
    let mirrored = high.transformed(&glam::Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
    let (l, r, _) = id_bake_with(&mirrored, &with_set(1));
    assert!(close(l, [1.0, 1.0, 1.0]) && close(r, [0.0, 0.0, 1.0]), "mirrored: {l:?} {r:?}");
}

/// Issue #466: low-poly texels whose rays miss the high poly (next to its open edges, or in a gap
/// between its parts) were written black on the ID map, and dilation spread them into lines.
#[test]
fn id_map_texels_that_miss_the_high_poly_take_a_neighbours_id() {
    // The two-part high poly has a gap in the middle and ends exactly at the plane's rim, so the
    // middle column and the rim texels (which reach just past the plane) miss it.
    let high = two_part_high(false, false);
    let low = primitives::plane(2.0, 4);
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let progress = Progress::new();
    let maps = bake(&input, &settings(&[MeshMapKind::Id]), &progress).unwrap();
    assert!(progress.warnings().iter().any(|w| w.starts_with("Cage:")), "the setup has misses: {:?}", progress.warnings());
    let id = &maps[&MeshMapKind::Id];
    let black = MeshMapKind::Id.neutral_value();
    let (mut left, mut right) = (None, None);
    for t in texels(&low, 0, RES) {
        let c = at(id, &t);
        let c = [c[0], c[1], c[2]];
        assert!(!close(c, black), "no empty ID inside the island, at {} ({c:?})", t.position);
        if t.position.x < -0.1 { left.get_or_insert(c); }
        if t.position.x > 0.1 { right.get_or_insert(c); }
        // Every texel has one of the two parts' IDs, never something in between.
        if let (Some(l), Some(r)) = (left, right) {
            assert!(close(c, l) || close(c, r), "at {}: {c:?} is neither {l:?} nor {r:?}", t.position);
        }
    }
    // The gutters follow the islands: no black within the dilation either.
    for y in 0..RES {
        for x in 0..RES {
            let v = id.get(x, y);
            assert!(!close([v[0], v[1], v[2]], black), "black gutter texel at {x},{y}");
        }
    }
}
