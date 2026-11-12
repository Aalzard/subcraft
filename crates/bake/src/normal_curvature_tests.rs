use std::collections::BTreeMap;
use std::f32::consts::TAU;

use glam::{Vec2, Vec3};
use subcraft_core::MeshMapKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder, primitives};

use super::*;
use crate::{BakeInput, BakeSettings, Geometry, bake};

fn corner(p: Vec3, n: Vec3, uv: Option<Vec2>) -> Corner {
    Corner { position: p, normal: Some(n), uv }
}

/// A 2×2×2 box with every edge rounded to radius `r` (the high poly of a bevelled cube).
fn rounded_box(r: f32, n: u32) -> Mesh {
    let mut b = MeshBuilder::new("Rounded");
    let m = b.material("Rounded");
    let inner = 1.0 - r;
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let (u_axis, v_axis) = ((axis + 1) % 3, (axis + 2) % 3);
            let point = |i: u32, j: u32| {
                let mut p = Vec3::ZERO;
                p[axis] = sign;
                p[u_axis] = -1.0 + 2.0 * i as f32 / n as f32;
                p[v_axis] = -1.0 + 2.0 * j as f32 / n as f32;
                let c = p.clamp(Vec3::splat(-inner), Vec3::splat(inner));
                let dir = (p - c).normalize();
                corner(c + dir * r, dir, None)
            };
            let mut face = Vec::new();
            for j in 0..n {
                for i in 0..n {
                    face.push([point(i, j), point(i + 1, j), point(i + 1, j + 1)]);
                    face.push([point(i, j), point(i + 1, j + 1), point(i, j + 1)]);
                }
            }
            for mut c in face {
                let g = (c[1].position - c[0].position).cross(c[2].position - c[0].position);
                let mid = (c[0].position + c[1].position + c[2].position) / 3.0;
                if g.dot(mid) < 0.0 {
                    c.swap(1, 2);
                }
                b.push_triangle(m, c);
            }
        }
    }
    b.build_for_baking().unwrap()
}

/// The low poly for a bevel bake: a 2×2×2 cube with averaged corner normals (so projection rays
/// fan out round the edges, as a low poly baked against a bevel is set up), each face its own UV
/// island in a 3×2 atlas, so every cube edge is a UV seam.
pub(crate) fn smooth_cube() -> Mesh {
    let mut b = MeshBuilder::new("Cube");
    let m = b.material("Cube");
    let mut island = 0;
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let (u_axis, v_axis) = ((axis + 1) % 3, (axis + 2) % 3);
            let origin = Vec2::new(0.02 + (island % 3) as f32 / 3.0, 0.02 + (island / 3) as f32 / 2.0);
            island += 1;
            let point = |i: u32, j: u32| {
                let mut p = Vec3::ZERO;
                p[axis] = sign;
                p[u_axis] = if i == 0 { -1.0 } else { 1.0 };
                p[v_axis] = if j == 0 { -1.0 } else { 1.0 };
                corner(p, p.normalize(), Some(origin + Vec2::new(i as f32, j as f32) * 0.29))
            };
            for mut c in [[point(0, 0), point(1, 0), point(1, 1)], [point(0, 0), point(1, 1), point(0, 1)]] {
                let g = (c[1].position - c[0].position).cross(c[2].position - c[0].position);
                if g[axis] * sign < 0.0 {
                    c.swap(1, 2);
                }
                b.push_triangle(m, c);
            }
        }
    }
    b.build().unwrap()
}

/// Open cylinder along Y (y in -1..1) with radius `radius(y)`; one UV island whose seam runs
/// along the +X side (u = 0 and u = 1 meet there).
fn cylinder(segments: u32, rows: u32, radius: impl Fn(f32) -> f32, slope: impl Fn(f32) -> f32, with_uv: bool) -> Mesh {
    let mut b = MeshBuilder::new("Cylinder");
    let m = b.material("Cylinder");
    let point = |i: u32, j: u32| {
        // The seam column shares bit-identical positions with column 0 (welded, as importers give it).
        let phi = TAU * (i % segments) as f32 / segments as f32;
        let y = -1.0 + 2.0 * j as f32 / rows as f32;
        let r = radius(y);
        let p = Vec3::new(r * phi.cos(), y, r * phi.sin());
        let n = Vec3::new(phi.cos(), -slope(y), phi.sin()).normalize();
        // Keep a margin so the island does not touch the image border.
        let uv = Vec2::new(0.02 + 0.96 * i as f32 / segments as f32, 0.02 + 0.96 * (1.0 - j as f32 / rows as f32));
        corner(p, n, with_uv.then_some(uv))
    };
    for j in 0..rows {
        for i in 0..segments {
            for mut c in [[point(i, j), point(i + 1, j), point(i + 1, j + 1)], [point(i, j), point(i + 1, j + 1), point(i, j + 1)]] {
                let g = (c[1].position - c[0].position).cross(c[2].position - c[0].position);
                let mid = (c[0].position + c[1].position + c[2].position) / 3.0;
                if g.dot(Vec3::new(mid.x, 0.0, mid.z)) < 0.0 {
                    c.swap(1, 2);
                }
                b.push_triangle(m, c);
            }
        }
    }
    if with_uv { b.build().unwrap() } else { b.build_for_baking().unwrap() }
}

fn bake_normal(low: &Mesh, high: &Mesh, size: u32, front: f32, rear: f32) -> TiledImage {
    let (lb, hb) = (Bvh::build(low), Bvh::build(high));
    let input = BakeInput { low: Geometry { mesh: low, bvh: &lb }, high: Some(Geometry { mesh: high, bvh: &hb }), material_slot: 0 };
    let settings = BakeSettings { size, maps: vec![MeshMapKind::Normal], cage_front: front, cage_rear: rear, use_gpu: false, ..Default::default() };
    let mut maps: BTreeMap<MeshMapKind, TiledImage> = bake(&input, &settings, &Progress::new()).unwrap();
    maps.remove(&MeshMapKind::Normal).unwrap()
}

fn value(img: &TiledImage, t: &Texel, size: u32) -> f32 {
    img.get(t.index % size, t.index / size)[0]
}

/// Inside texels only (rim texels sit just outside their triangle).
fn interior(mesh: &Mesh, size: u32) -> Vec<Texel> {
    let mut out = texels(mesh, 0, size);
    out.retain(|t| {
        let [a, b, c] = mesh.tri_positions(t.tri as usize);
        barycentric(t.position, a, b, c).iter().all(|&l| l > -1e-4)
    });
    out
}

#[test]
fn walking_across_a_cube_edge_turns_onto_the_next_face_and_crosses_uv_seams() {
    let cube = primitives::cube(2.0);
    let walker = SurfaceWalker::new(&cube);
    // A point on +Z near the +X edge, walking towards +X: it must end on the +X face.
    let start = Vec3::new(0.9, 0.1, 1.0);
    let tri = (0..cube.triangle_count() as u32).find(|&t| {
        let [a, b, c] = cube.tri_positions(t as usize);
        let l = barycentric(start, a, b, c);
        cube.geometric_normal(t as usize).z > 0.9 && l.iter().all(|&x| x >= -1e-5)
    }).unwrap();
    let (end_tri, end, dir) = walker.walk(tri, start, Vec3::X, 0.3);
    assert!(cube.geometric_normal(end_tri as usize).x > 0.9, "ended on the +X face");
    assert!((end - Vec3::new(1.0, 0.1, 0.8)).length() < 1e-3, "surface distance kept: {end}");
    assert!((dir - Vec3::NEG_Z).length() < 1e-3, "direction turned down the side: {dir}");
}

/// A plain cube low poly with a rounded-box high poly: each edge must give one band centred on
/// the edge, the same on every edge (every cube edge is a UV seam here), flat on the faces.
#[test]
fn bevelled_cube_edges_give_one_centred_band_identical_across_every_seam() {
    const SIZE: u32 = 256;
    let r = 0.15;
    let low = smooth_cube();
    let high = rounded_box(r, 96);
    let normal = bake_normal(&low, &high, SIZE, 0.01, 0.06);
    let curv = curvature_from_normal_map(&low, 0, &normal, SIZE, &NormalCurvatureSettings::default(), None).unwrap();
    assert_eq!((curv.width(), curv.comps()), (SIZE, 1));
    // Texel length on the cube faces (each face is about a third of the atlas width).
    let texel = texel_lengths(&low, SIZE)[0];
    let bin = |d: f32| (d / texel).floor() as usize;
    // (edge id, distance bin) -> values
    // (face axis, face sign, edge axis, edge sign, distance bin): one key per side of each edge.
    let mut per_edge: BTreeMap<(usize, i32, usize, i32, usize), Vec<f32>> = BTreeMap::new();
    let mut flat = Vec::new();
    for t in interior(&low, SIZE) {
        let p = t.position;
        let a = p.abs();
        let axis = if a.x >= a.y && a.x >= a.z { 0 } else if a.y >= a.z { 1 } else { 2 };
        let others = [(axis + 1) % 3, (axis + 2) % 3];
        let dist = others.map(|o| 1.0 - a[o]);
        let (near, far) = if dist[0] <= dist[1] { (0, 1) } else { (1, 0) };
        if dist[far] < 0.45 {
            continue; // near a corner
        }
        let v = value(&curv, &t, SIZE);
        if dist[near] > 0.4 {
            flat.push(v);
            continue;
        }
        let edge_axis = others[near];
        let key = (axis, p[axis].signum() as i32, edge_axis, p[edge_axis].signum() as i32, bin(dist[near]));
        per_edge.entry(key).or_default().push(v);
    }
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(flat.len() > 1000);
    let worst_flat = flat.iter().map(|v| (v - 0.5).abs()).fold(0.0, f32::max);
    assert!(worst_flat < 0.02, "faces are flat ({worst_flat})");
    // Profile per distance bin over all 12 edges (24 face sides).
    let bins = bin(0.4);
    let mut sides = Vec::new();
    let mut profile = Vec::new();
    for b in 0..bins {
        let means: Vec<f32> = per_edge.iter().filter(|(k, _)| k.4 == b).map(|(_, v)| mean(v)).collect();
        eprintln!("bin {b}: {} edge sides {means:?}", means.len());
        assert!(means.len() >= 20, "bin {b} sampled on only {} of the 24 edge sides", means.len());
        profile.push(mean(&means));
        sides.push(means);
    }
    eprintln!("bevelled cube profile (edge outwards, texel {texel:.4}): {profile:?}");
    // Every edge side gives the same profile (every edge is a UV seam here). Where the band falls
    // off at the bevel's end the value changes steeply per texel, so a sub-texel shift of that
    // transition between sides is allowed for by the local slope.
    for (b, means) in sides.iter().enumerate() {
        let spread = means.iter().fold(f32::MIN, |m, &x| m.max(x)) - means.iter().fold(f32::MAX, |m, &x| m.min(x));
        let (lo, hi) = (profile[b.saturating_sub(1)], profile[(b + 1).min(bins - 1)]);
        let slope = (hi - lo).abs() / (if b == 0 || b + 1 == bins { 1.0 } else { 2.0 });
        assert!(spread < 0.05 + slope, "edges disagree at distance bin {b}: spread {spread}, local slope {slope} ({means:?})");
    }
    // One band centred on the edge: bright at the edge itself (no dark line or offset band there),
    // its peak inside the bevel, falling away past the peak without a second band.
    let in_bevel = bin(r);
    let (peak_bin, peak) = profile.iter().copied().enumerate().fold((0, f32::MIN), |a, (i, v)| if v > a.1 { (i, v) } else { a });
    assert!(peak > 0.65, "a bright band ({peak})");
    assert!(peak_bin <= in_bevel, "the band peaks inside the bevel (bin {peak_bin}, bevel ends at bin {in_bevel}): {profile:?}");
    assert!(profile[0] >= 0.9 * peak, "bright at the edge itself, not offset from it ({} vs peak {peak}): {profile:?}", profile[0]);
    for w in profile[peak_bin..].windows(2) {
        assert!(w[1] <= w[0] + 0.01, "the profile rises again past the band (a second band): {profile:?}");
    }
    let beyond = bin(r + 3.0 * texel);
    assert!(profile[beyond..].iter().all(|v| (v - 0.5).abs() < 0.03), "flat beyond the bevel: {profile:?}");
}

/// A cylinder with a raised ring baked into its normal map: the ring is one band at the ring's
/// height all the way round, including the columns either side of the UV seam, and the plain
/// cylinder wall reads the same everywhere.
#[test]
fn cylinder_ring_band_is_continuous_across_the_seam_and_centred_on_the_ring() {
    const SIZE: u32 = 256;
    let (y0, width, bump) = (0.2f32, 0.12f32, 0.03f32);
    let radius = move |y: f32| 1.0 + bump * (-((y - y0) / width).powi(2)).exp();
    let slope = move |y: f32| bump * (-((y - y0) / width).powi(2)).exp() * (-2.0 * (y - y0) / (width * width));
    let low = cylinder(64, 8, |_| 1.0, |_| 0.0, true);
    let high = cylinder(256, 400, radius, slope, false);
    let normal = bake_normal(&low, &high, SIZE, 0.03, 0.01);
    let curv = curvature_from_normal_map(&low, 0, &normal, SIZE, &NormalCurvatureSettings::default(), None).unwrap();
    // Rows by height; columns by angle.
    let row_h = 2.0 / (0.96 * SIZE as f32);
    let mut rows: BTreeMap<i32, Vec<(f32, f32)>> = BTreeMap::new();
    for t in interior(&low, SIZE) {
        let p = t.position;
        if p.y.abs() > 0.85 {
            continue; // the open ends stop walks short
        }
        let phi = p.z.atan2(p.x).rem_euclid(TAU);
        rows.entry((p.y / row_h).round() as i32).or_default().push((phi, value(&curv, &t, SIZE)));
    }
    let mut peak = (f32::MIN, 0.0f32);
    let mut wall = Vec::new();
    for (row, values) in &rows {
        let y = *row as f32 * row_h;
        if values.len() < 200 {
            continue; // a partial row at the cut-off height
        }
        let vals: Vec<f32> = values.iter().map(|v| v.1).collect();
        let (lo, hi) = vals.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        // Columns next to the seam (phi near 0 / TAU) must match the rest of the row.
        let seam: Vec<f32> = values.iter().filter(|(phi, _)| phi.min(TAU - phi) < 0.05).map(|v| v.1).collect();
        let mean = vals.iter().sum::<f32>() / vals.len() as f32;
        assert!(!seam.is_empty(), "row {row} has texels at the seam");
        assert!(hi - lo < 0.04, "row at y = {y:.3} varies around the cylinder by {} (seam values {seam:?}, mean {mean})", hi - lo);
        if mean > peak.0 {
            peak = (mean, y);
        }
        if (y - y0).abs() > 3.0 * width {
            wall.push(mean);
        }
    }
    eprintln!("cylinder ring peak {peak:?}");
    assert!(peak.0 > 0.6, "the ring is a bright band ({})", peak.0);
    assert!((peak.1 - y0).abs() < 2.0 * row_h, "band centred on the ring: at y = {} for a ring at {y0}", peak.1);
    let (lo, hi) = wall.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    assert!(hi - lo < 0.01 && lo >= 0.5, "plain wall uniform and slightly convex ({lo}..{hi})");
}

/// A flat normal map on a smooth cylinder: only the low poly's own curvature, identical on both
/// sides of the seam (an image-space derivative would read the seam as a hard edge).
#[test]
fn flat_normal_map_on_a_cylinder_has_no_seam_band() {
    const SIZE: u32 = 128;
    let low = cylinder(48, 6, |_| 1.0, |_| 0.0, true);
    let flat = TiledImage::new(SIZE, SIZE, 3, [0.5, 0.5, 1.0, 0.0]);
    let curv = curvature_from_normal_map(&low, 0, &flat, SIZE, &NormalCurvatureSettings::default(), None).unwrap();
    let vals: Vec<(f32, f32)> = interior(&low, SIZE).iter()
        .filter(|t| t.position.y.abs() < 0.8)
        .map(|t| (t.position.z.atan2(t.position.x).rem_euclid(TAU), value(&curv, t, SIZE)))
        .collect();
    let (lo, hi) = vals.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v.1), hi.max(v.1)));
    assert!(hi - lo < 0.006, "uniform around the cylinder: {lo}..{hi}");
    assert!(lo > 0.5, "convex ({lo})");
}

/// The bake setting derives curvature from the normal map of the same bake, keeping the normal
/// map internal when it was not asked for.
#[test]
fn bake_setting_derives_curvature_from_its_own_normal_map() {
    const SIZE: u32 = 64;
    let low = primitives::cube(2.0);
    let high = rounded_box(0.3, 32);
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let settings = BakeSettings { size: SIZE, maps: vec![MeshMapKind::Curvature], cage_front: 0.01, cage_rear: 0.1, use_gpu: false, curvature_from_normal: true, ..Default::default() };
    let mut published = Vec::new();
    let maps = crate::bake_streamed(&input, &settings, &Progress::new(), |k, _| published.push(k)).unwrap();
    assert_eq!(published, vec![MeshMapKind::Curvature]);
    assert_eq!(maps.keys().copied().collect::<Vec<_>>(), vec![MeshMapKind::Curvature]);
    let normal = bake_normal(&low, &high, SIZE, 0.01, 0.1);
    let direct = curvature_from_normal_map(&low, 0, &normal, SIZE, &NormalCurvatureSettings::default(), None).unwrap();
    assert_eq!(maps[&MeshMapKind::Curvature], direct);
}

