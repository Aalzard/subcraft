use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_mesh::{Corner, MeshBuilder};

use super::*;

const RES: u32 = 256;

fn corner(p: Vec3, n: Vec3, u: f32, v: f32) -> Corner {
    Corner { position: p, normal: Some(n), uv: Some(Vec2::new(u, v)) }
}

/// A flat quad `x0..x1` by `z0..z1` at height `y`, facing `n`, mapped to UV `u0..u1` by `v0..v1`.
fn quad(b: &mut MeshBuilder, (x0, x1): (f32, f32), (z0, z1): (f32, f32), y: f32, n: Vec3, (u0, u1): (f32, f32), (v0, v1): (f32, f32)) {
    let m = b.material("m");
    let c = |x: f32, z: f32, u: f32, v: f32| corner(Vec3::new(x, y, z), n, u, v);
    let (a, bb, cc, d) = (c(x0, z0, u0, v0), c(x1, z0, u1, v0), c(x1, z1, u1, v1), c(x0, z1, u0, v1));
    b.push_triangle(m, [a, cc, bb]);
    b.push_triangle(m, [a, d, cc]);
}

fn geometry(b: MeshBuilder) -> TextureSetGeometry {
    TextureSetGeometry::new(Arc::new(b.build().unwrap()), 0, RES)
}

/// A 4 x 1 strip mapped onto the whole texture: UV stretches 4:1 (a texel is four times wider than tall on the surface).
fn stretched_strip() -> TextureSetGeometry {
    let mut b = MeshBuilder::new("strip");
    quad(&mut b, (0.0, 4.0), (0.0, 1.0), 0.0, Vec3::Y, (0.0, 1.0), (0.0, 1.0));
    geometry(b)
}

fn raster(f: impl Fn(u32, u32) -> f32) -> Vec<f32> {
    (0..RES * RES).map(|i| f(i % RES, i / RES)).collect()
}

/// World distance between the 10 % and 90 % crossings of a rising profile sampled every `step` world units.
fn rise_width(profile: &[f32], step: f32) -> f32 {
    let cross = |level: f32| {
        let i = profile.windows(2).position(|w| w[0] < level && w[1] >= level).expect("profile crosses the level");
        i as f32 + (level - profile[i]) / (profile[i + 1] - profile[i])
    };
    (cross(0.9) - cross(0.1)) * step
}

#[test]
fn the_blur_is_the_same_width_in_world_units_where_uv_is_stretched() {
    let geo = stretched_strip();
    let radius = 0.15; // world units; a Gaussian with sigma = radius / 3
    let (pitch_x, pitch_z) = (4.0 / RES as f32, 1.0 / RES as f32);
    // A step across the strip (along x, at world x = 2) and a step along it (along z, at z = 0.5).
    let across = world_blur(&geo, &raster(|x, _| if x >= RES / 2 { 1.0 } else { 0.0 }), 1, radius);
    let along = world_blur(&geo, &raster(|_, y| if y >= RES / 2 { 1.0 } else { 0.0 }), 1, radius);
    let row: Vec<f32> = (RES / 2 - 24..RES / 2 + 24).map(|x| across[(RES / 2 * RES + x) as usize]).collect();
    let column: Vec<f32> = (RES / 2 - 60..RES / 2 + 60).map(|y| along[(y * RES + RES / 2) as usize]).collect();
    let (wx, wz) = (rise_width(&row, pitch_x), rise_width(&column, pitch_z));
    let expected = 2.0 * 1.2816 * radius / 3.0;
    assert!((wx - expected).abs() < 0.3 * expected, "x: {wx} vs {expected}");
    assert!((wz - expected).abs() < 0.3 * expected, "z: {wz} vs {expected}");
    assert!((wx / wz - 1.0).abs() < 0.2, "the same 10-90 % width both ways although texels are 4:1: {wx} / {wz}");
}

#[test]
fn flat_values_and_zero_radius_are_unchanged_and_colors_keep_their_coverage() {
    let geo = stretched_strip();
    let flat = vec![0.7; (RES * RES) as usize];
    let blurred = world_blur(&geo, &flat, 1, 0.3);
    assert!(blurred.iter().all(|v| (v - 0.7).abs() < 1e-4));
    let data = raster(|x, _| if x > 100 { 1.0 } else { 0.0 });
    assert_eq!(world_blur(&geo, &data, 1, 0.0), data);
    assert_eq!(world_blur(&geo, &data, 1, f32::NAN), data);
    // Value + coverage raster: painted value 1 beside unpainted (value 0, coverage 0). Premultiplied
    // blurring keeps the value 1 wherever anything is painted and only fades the coverage.
    let paint: Vec<f32> = (0..RES * RES).flat_map(|i| if i % RES > RES / 2 { [1.0, 1.0] } else { [0.0, 0.0] }).collect();
    let out = world_blur(&geo, &paint, 2, 0.15);
    let near = (RES / 2 * RES + RES / 2 + 3) as usize * 2; // just inside the painted side
    let outside = (RES / 2 * RES + RES / 2 - 3) as usize * 2; // just outside it
    assert!(out[near + 1] > 0.4 && out[near + 1] < 1.0, "coverage fades: {}", out[near + 1]);
    assert!((out[near] - 1.0).abs() < 1e-3, "no darkening toward the unpainted side: {}", out[near]);
    assert!(out[outside + 1] > 0.0 && (out[outside] - 1.0).abs() < 1e-3, "paint spreads outward at full value: {:?}", &out[outside..outside + 2]);
}

#[test]
fn blur_crosses_uv_seams_but_not_to_the_other_face_of_a_thin_plate() {
    // Two coplanar quads side by side in 3D (sharing the edge x = 1) whose UV islands are far apart.
    let mut b = MeshBuilder::new("seam");
    quad(&mut b, (0.0, 1.0), (0.0, 1.0), 0.0, Vec3::Y, (0.0, 0.4), (0.0, 1.0));
    quad(&mut b, (1.0, 2.0), (0.0, 1.0), 0.0, Vec3::Y, (0.6, 1.0), (0.0, 1.0));
    let geo = geometry(b);
    let data = raster(|x, _| if x >= (0.5 * RES as f32) as u32 { 1.0 } else { 0.0 });
    let out = world_blur(&geo, &data, 1, 0.2);
    let at = |u: f32, v: f32| out[((v * RES as f32) as u32 * RES + (u * RES as f32) as u32) as usize];
    // World x = 0.95 on the left island is 0.05 from the seam: it receives value from the right island.
    let left_edge = at(0.38, 0.5);
    assert!(left_edge > 0.1 && left_edge < 0.6, "paint crosses the seam: {left_edge}");
    assert!(at(0.05, 0.5) < 0.01, "far from the seam nothing arrives: {}", at(0.05, 0.5));

    // A thin plate: top face (normal +Y) painted 1, bottom face (normal -Y) 0, 0.02 apart.
    let mut b = MeshBuilder::new("plate");
    quad(&mut b, (0.0, 1.0), (0.0, 1.0), 0.02, Vec3::Y, (0.0, 0.5), (0.0, 1.0));
    quad(&mut b, (0.0, 1.0), (0.0, 1.0), 0.0, -Vec3::Y, (0.5, 1.0), (0.0, 1.0));
    let geo = geometry(b);
    let data = raster(|x, _| if x < RES / 2 { 1.0 } else { 0.0 });
    let out = world_blur(&geo, &data, 1, 0.3);
    let (top, bottom) = (out[(RES / 2 * RES + RES / 4) as usize], out[(RES / 2 * RES + 3 * RES / 4) as usize]);
    assert!(top > 0.999 && bottom < 0.001, "the faces do not leak into each other: {top} / {bottom}");
}
