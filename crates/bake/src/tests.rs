use glam::{Vec2, Vec3};
use subcraft_core::MeshMapKind;
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder, primitives};

use super::*;

const RES: u32 = 128;

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
fn height_and_opacity_from_high_poly() {
    let low = primitives::plane(2.0, 1);
    let high = bumped_plane();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    // Cage reaches 0.2 above and below the plane.
    let front = 0.2 / low.bounds.diagonal().max(high.bounds.diagonal());
    let s = BakeSettings {
        size: RES,
        maps: vec![MeshMapKind::Height, MeshMapKind::Opacity],
        cage_front: front,
        cage_rear: front,
        ..BakeSettings::default()
    };
    let maps = bake(&input, &s, &Progress::new()).unwrap();
    let (height, opacity) = (&maps[&MeshMapKind::Height], &maps[&MeshMapKind::Opacity]);
    let ts = texels(&low, 0, RES);
    let center = ts.iter().min_by(|a, b| a.position.length().total_cmp(&b.position.length())).unwrap();
    // Bump top is 0.15 above the plane: 0.5 + 0.5 · 0.15 / 0.2 = 0.875.
    let h = at(height, center)[0];
    assert!((h - 0.875).abs() < 0.03, "bump top height {h}");
    for t in ts.iter().filter(|t| Vec2::new(t.position.x, t.position.z).length() > 0.8) {
        assert!((at(height, t)[0] - 0.5).abs() < 0.01, "flat area at 0.5");
    }
    assert!(ts.iter().all(|t| at(opacity, t)[0] == 1.0), "high poly covers the whole plane");
}

#[test]
fn opacity_is_zero_where_the_high_poly_is_missing() {
    let low = primitives::plane(2.0, 1);
    // High poly: only the x < 0 half of the plane.
    let mut b = MeshBuilder::new("Half");
    let m = b.material("M");
    let c = |x: f32, z: f32| corner(Vec3::new(x, 0.0, z), Vec2::new(x, z));
    b.push_triangle(m, [c(-1.0, -1.0), c(-1.0, 1.0), c(0.0, 1.0)]);
    b.push_triangle(m, [c(-1.0, -1.0), c(0.0, 1.0), c(0.0, -1.0)]);
    let high = b.build().unwrap();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let s = BakeSettings { size: RES, maps: vec![MeshMapKind::Opacity], ..BakeSettings::default() };
    let op = &bake(&input, &s, &Progress::new()).unwrap()[&MeshMapKind::Opacity];
    for t in texels(&low, 0, RES) {
        if t.position.x < -0.05 {
            assert_eq!(at(op, &t)[0], 1.0);
        } else if t.position.x > 0.05 {
            assert_eq!(at(op, &t)[0], 0.0);
        }
    }
}

#[test]
fn bent_normals_lean_away_from_occluders() {
    let mesh = floor_with_box();
    let maps = bake_low(&mesh, &[MeshMapKind::BentNormal], 0);
    let bent = &maps[&MeshMapKind::BentNormal];
    let ts = texels(&mesh, 0, RES);
    let decode = |c: [f32; 4]| Vec3::new(c[0] * 2.0 - 1.0, c[1] * 2.0 - 1.0, c[2] * 2.0 - 1.0);
    // Floor right next to the box's +X face: open directions lean toward +X.
    let near: Vec<Vec3> =
        ts.iter().filter(|t| (0.52..0.6).contains(&t.position.x) && t.position.z.abs() < 0.3).map(|t| decode(at(bent, t))).collect();
    assert!(!near.is_empty());
    let avg = near.iter().copied().sum::<Vec3>() / near.len() as f32;
    assert!(avg.x > 0.2, "bent normal leans away from the box ({avg:?})");
    for t in ts.iter().filter(|t| t.position.x.abs() > 1.6 && t.position.z.abs() > 1.6) {
        let b = decode(at(bent, t));
        assert!(b.y > 0.97, "open floor bends straight up ({b:?})");
    }
}

/// Low: plane `A_low` (UV left half) and plane `C_low` far away (UV right half, no high partner).
/// High: `A_HIGH` coincides with A; `B_high` is a sloped quad floating just above A.
fn named_scene() -> (Mesh, Mesh) {
    let mut lb = MeshBuilder::new("Low");
    let m = lb.material("M");
    let quad = |b: &mut MeshBuilder, x0: f32, u0: f32| {
        let c = |x: f32, z: f32| corner_n(Vec3::new(x0 + x, 0.0, z), Vec3::Y, Vec2::new(u0 + (x + 1.0) * 0.24, 0.01 + (z + 1.0) * 0.49));
        b.push_triangle(m, [c(-1.0, -1.0), c(-1.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(-1.0, -1.0), c(1.0, 1.0), c(1.0, -1.0)]);
    };
    lb.object("A_low");
    quad(&mut lb, 0.0, 0.01);
    lb.object("C_low");
    quad(&mut lb, 10.0, 0.51);
    let low = lb.build().unwrap();

    let mut hb = MeshBuilder::new("High");
    let m = hb.material("M");
    let c = |x: f32, y: f32, z: f32| corner(Vec3::new(x, y, z), Vec2::new(0.5, 0.5));
    hb.object("A_HIGH");
    hb.push_triangle(m, [c(-1.0, 0.0, -1.0), c(-1.0, 0.0, 1.0), c(1.0, 0.0, 1.0)]);
    hb.push_triangle(m, [c(-1.0, 0.0, -1.0), c(1.0, 0.0, 1.0), c(1.0, 0.0, -1.0)]);
    hb.object("B_high");
    // Rises from y = 0.02 to 0.06 across x in [-0.5, 0.5]: a clearly sloped normal.
    hb.push_triangle(m, [c(-0.5, 0.02, -0.5), c(-0.5, 0.02, 0.5), c(0.5, 0.06, 0.5)]);
    hb.push_triangle(m, [c(-0.5, 0.02, -0.5), c(0.5, 0.06, 0.5), c(0.5, 0.06, -0.5)]);
    (low, hb.build().unwrap())
}

#[test]
fn match_by_mesh_name_ignores_other_parts_inside_the_cage() {
    let (low, high) = named_scene();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let front = 0.1 / low.bounds.diagonal().max(high.bounds.diagonal());
    let base = BakeSettings {
        size: RES,
        maps: vec![MeshMapKind::Normal, MeshMapKind::Opacity, MeshMapKind::Height],
        cage_front: front,
        cage_rear: front,
        ..BakeSettings::default()
    };
    let ts = texels(&low, 0, RES);
    let under_b: Vec<&Texel> = ts.iter().filter(|t| t.position.x.abs() < 0.3 && t.position.z.abs() < 0.3).collect();
    let on_c: Vec<&Texel> = ts.iter().filter(|t| t.position.x > 5.0).collect();
    assert!(under_b.len() > 50 && on_c.len() > 50);

    // Always: A's texels under B pick up B's sloped normal.
    let always = bake(&input, &base, &Progress::new()).unwrap();
    let tilted = under_b.iter().filter(|t| (at(&always[&MeshMapKind::Normal], t)[0] - 0.5).abs() > 0.005).count();
    assert!(tilted > under_b.len() / 2, "Always mode bleeds B into A ({tilted}/{})", under_b.len());

    // By mesh name: A only sees A_HIGH (case-insensitive suffixes) and stays flat; C has no partner.
    let named = bake(&input, &BakeSettings { match_mode: MatchMode::ByMeshName, ..base.clone() }, &Progress::new()).unwrap();
    for t in &under_b {
        let n = at(&named[&MeshMapKind::Normal], t);
        assert!((n[0] - 0.5).abs() < 0.002 && (n[1] - 0.5).abs() < 0.002, "A stays flat: {n:?}");
        assert!((at(&named[&MeshMapKind::Height], t)[0] - 0.5).abs() < 0.01);
        assert_eq!(at(&named[&MeshMapKind::Opacity], t)[0], 1.0);
    }
    for t in &on_c {
        assert_eq!(at(&named[&MeshMapKind::Opacity], t)[0], 0.0, "C_low has no high partner");
        let n = at(&named[&MeshMapKind::Normal], t);
        assert!((n[2] - 1.0).abs() < 0.01, "unmatched texels bake flat");
    }
}

#[test]
fn name_matching_report_and_suffixes() {
    use crate::matching::{UNMATCHED, base_name, match_by_name};
    assert_eq!(base_name("Bolt_low", "_low"), "bolt");
    assert_eq!(base_name("  BOLT_HIGH ", "_high"), "bolt");
    assert_eq!(base_name("_low", "_low"), "_low", "a bare suffix stays a name");
    assert_eq!(base_name("Body", "_low"), "body");
    let (low, high) = named_scene();
    let g = match_by_name(&low, &high, "_low", "_high");
    assert_eq!(g.report.matched, vec!["a".to_string()]);
    assert_eq!(g.report.unmatched_low, vec!["C_low".to_string()]);
    assert_eq!(g.report.unmatched_high, vec!["B_high".to_string()]);
    assert_eq!(g.low.len(), low.triangle_count());
    assert_eq!(g.high.iter().filter(|&&x| x == UNMATCHED).count(), 2);
}

#[test]
fn id_by_material_and_object() {
    let (low, _) = named_scene();
    let ts = texels(&low, 0, RES);
    let bvh = Bvh::build(&low);
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &bvh }, high: None, material_slot: 0 };
    let bake_id = |source| {
        let s = BakeSettings { size: RES, maps: vec![MeshMapKind::Id], id_source: source, ..BakeSettings::default() };
        bake(&input, &s, &Progress::new()).unwrap().remove(&MeshMapKind::Id).unwrap()
    };
    let distinct = |img: &TiledImage| {
        let mut c: Vec<[u32; 3]> =
            ts.iter().map(|t| at(img, t)).map(|c| [(c[0] * 255.0) as u32, (c[1] * 255.0) as u32, (c[2] * 255.0) as u32]).collect();
        c.sort();
        c.dedup();
        c.len()
    };
    assert_eq!(distinct(&bake_id(IdSource::MaterialSlot)), 1, "one material, one color");
    assert_eq!(distinct(&bake_id(IdSource::MeshObject)), 2, "two objects, two colors");
}

#[test]
fn maps_are_emitted_one_by_one_in_order() {
    let mesh = primitives::cube(2.0);
    let bvh = Bvh::build(&mesh);
    let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let maps = vec![MeshMapKind::Opacity, MeshMapKind::Curvature, MeshMapKind::Height];
    let progress = Progress::new();
    let mut seen = Vec::new();
    bake_each(&input, &settings(&maps), &progress, |k, img| {
        assert_eq!(img.comps(), k.components());
        seen.push(k);
    })
    .unwrap();
    assert_eq!(seen, maps);
    assert_eq!(progress.fraction(), 1.0);
    assert_eq!(progress.current_map(), None);
}
