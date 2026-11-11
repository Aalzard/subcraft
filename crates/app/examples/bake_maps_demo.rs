//! Inputs and a comparison sheet for the bake / mesh-map captures:
//! `cargo run --release -p subcraft-app --example bake_maps_demo -- <folder>` writes
//! * `bevel_low.obj` / `bevel_high.obj`: a cube with averaged normals and a rounded-edge cube (high poly),
//! * `crate.obj` and `crate_{normal,ambient_occlusion,curvature,id,position}.png`: the sample
//!   crate's maps baked low on low, to import into a new project by file name,
//! * `curvature_sheet.png`: normal map | curvature read off the image | curvature along the surface,
//!   for the bevelled cube (every edge a UV seam) and a cylinder with a ring (seam on the right).

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use subcraft_bake::normal_curvature::{NormalCurvatureSettings, curvature_from_normal_map};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::MeshMapKind;
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::import::write_obj;
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder, primitives};

fn rounded_box(r: f32, n: u32) -> Mesh {
    let mut b = MeshBuilder::new("Rounded");
    let m = b.material("Rounded");
    let inner = 1.0 - r;
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
            let point = |i: u32, j: u32| {
                let mut p = Vec3::ZERO;
                p[axis] = sign;
                p[ua] = -1.0 + 2.0 * i as f32 / n as f32;
                p[va] = -1.0 + 2.0 * j as f32 / n as f32;
                let c = p.clamp(Vec3::splat(-inner), Vec3::splat(inner));
                let dir = (p - c).normalize();
                Corner { position: c + dir * r, normal: Some(dir), uv: Some(Vec2::ZERO) }
            };
            for j in 0..n {
                for i in 0..n {
                    for mut c in [[point(i, j), point(i + 1, j), point(i + 1, j + 1)], [point(i, j), point(i + 1, j + 1), point(i, j + 1)]] {
                        let g = (c[1].position - c[0].position).cross(c[2].position - c[0].position);
                        if g.dot(c[0].position + c[1].position + c[2].position) < 0.0 { c.swap(1, 2); }
                        b.push_triangle(m, c);
                    }
                }
            }
        }
    }
    b.build_for_baking().unwrap()
}

/// Low poly of the bevel bake: a cube with averaged corner normals (projection rays fan out round
/// the edges, the usual low-poly setup for baking a bevel), each face its own UV island.
fn smooth_cube() -> Mesh {
    let mut b = MeshBuilder::new("Cube");
    let m = b.material("Cube");
    let mut island = 0;
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
            let origin = Vec2::new(0.02 + (island % 3) as f32 / 3.0, 0.02 + (island / 3) as f32 / 2.0);
            island += 1;
            let point = |i: u32, j: u32| {
                let mut p = Vec3::ZERO;
                p[axis] = sign;
                p[ua] = if i == 0 { -1.0 } else { 1.0 };
                p[va] = if j == 0 { -1.0 } else { 1.0 };
                Corner { position: p, normal: Some(p.normalize()), uv: Some(origin + Vec2::new(i as f32, j as f32) * 0.29) }
            };
            for mut c in [[point(0, 0), point(1, 0), point(1, 1)], [point(0, 0), point(1, 1), point(0, 1)]] {
                let g = (c[1].position - c[0].position).cross(c[2].position - c[0].position);
                if g[axis] * sign < 0.0 { c.swap(1, 2); }
                b.push_triangle(m, c);
            }
        }
    }
    b.build().unwrap()
}

fn cylinder(segments: u32, rows: u32, radius: impl Fn(f32) -> f32, slope: impl Fn(f32) -> f32, uv: bool) -> Mesh {
    let mut b = MeshBuilder::new("Cylinder");
    let m = b.material("Cylinder");
    let point = |i: u32, j: u32| {
        let phi = TAU * (i % segments) as f32 / segments as f32;
        let y = -1.0 + 2.0 * j as f32 / rows as f32;
        let r = radius(y);
        Corner {
            position: Vec3::new(r * phi.cos(), y, r * phi.sin()),
            normal: Some(Vec3::new(phi.cos(), -slope(y), phi.sin()).normalize()),
            uv: uv.then(|| Vec2::new(0.02 + 0.96 * i as f32 / segments as f32, 0.02 + 0.96 * (1.0 - j as f32 / rows as f32))),
        }
    };
    for j in 0..rows {
        for i in 0..segments {
            for mut c in [[point(i, j), point(i + 1, j), point(i + 1, j + 1)], [point(i, j), point(i + 1, j + 1), point(i, j + 1)]] {
                let g = (c[1].position - c[0].position).cross(c[2].position - c[0].position);
                let mid = c[0].position + c[1].position + c[2].position;
                if g.dot(Vec3::new(mid.x, 0.0, mid.z)) < 0.0 { c.swap(1, 2); }
                b.push_triangle(m, c);
            }
        }
    }
    if uv { b.build().unwrap() } else { b.build_for_baking().unwrap() }
}

fn bake_maps(low: &Mesh, high: Option<&Mesh>, size: u32, maps: &[MeshMapKind], cage: [f32; 2]) -> std::collections::BTreeMap<MeshMapKind, TiledImage> {
    let lb = Bvh::build(low);
    let hb = high.map(Bvh::build);
    let input = BakeInput { low: Geometry { mesh: low, bvh: &lb }, high: high.zip(hb.as_ref()).map(|(m, b)| Geometry { mesh: m, bvh: b }), material_slot: 0 };
    let s = BakeSettings { size, maps: maps.to_vec(), cage_front: cage[0], cage_rear: cage[1], use_gpu: false, ..Default::default() };
    bake(&input, &s, &Progress::new()).unwrap()
}

/// Curvature straight from image neighbours of the tangent-space normal map (what breaks at
/// seams and on curved low polys), for the comparison column.
fn image_space_curvature(normal: &TiledImage) -> TiledImage {
    let (w, h) = (normal.width(), normal.height());
    let mut out = TiledImage::new(w, h, 1, [0.5, 0.0, 0.0, 0.0]);
    let n = |x: i64, y: i64| normal.get(x.clamp(0, w as i64 - 1) as u32, y.clamp(0, h as i64 - 1) as u32);
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            // OpenGL normal maps: +Y (green) points up, image rows go down.
            let dx = n(x + 1, y)[0] - n(x - 1, y)[0];
            let dy = n(x, y - 1)[1] - n(x, y + 1)[1];
            let v = 0.5 + 0.5 * ((dx + dy) * 2.0).clamp(-1.0, 1.0);
            out.set(x as u32, y as u32, [v, 0.0, 0.0, 0.0]);
        }
    }
    out
}

fn rgb(img: &TiledImage) -> Vec<f32> {
    let d = img.to_dense();
    if img.comps() == 3 { d } else { d.iter().flat_map(|v| [*v; 3]).collect() }
}

fn write_png(path: &Path, img: &TiledImage) {
    let comps = img.comps();
    save_image(path, img.width(), img.height(), comps, &img.to_dense(), FileFormat::Png8).unwrap();
}

fn main() {
    let out: PathBuf = std::env::args().nth(1).unwrap_or_else(|| "bake_maps_demo".into()).into();
    std::fs::create_dir_all(&out).unwrap();
    const S: u32 = 256;

    // Bevelled cube: low = cube with averaged normals (six UV islands), high = rounded edges.
    let low = smooth_cube();
    let high = rounded_box(0.15, 96);
    write_obj(&low, &out.join("bevel_low.obj")).unwrap();
    write_obj(&high, &out.join("bevel_high.obj")).unwrap();
    let cube_normal = bake_maps(&low, Some(&high), S, &[MeshMapKind::Normal], [0.01, 0.06]).remove(&MeshMapKind::Normal).unwrap();
    let cube_surface = curvature_from_normal_map(&low, 0, &cube_normal, S, &NormalCurvatureSettings::default(), None).unwrap();
    let cube_image = image_space_curvature(&cube_normal);

    // Cylinder with a raised ring baked into its normal map.
    let (y0, width, bump) = (0.2f32, 0.12f32, 0.03f32);
    let radius = move |y: f32| 1.0 + bump * (-((y - y0) / width).powi(2)).exp();
    let slope = move |y: f32| bump * (-((y - y0) / width).powi(2)).exp() * (-2.0 * (y - y0) / (width * width));
    let cyl_low = cylinder(64, 8, |_| 1.0, |_| 0.0, true);
    let cyl_high = cylinder(256, 400, radius, slope, false);
    let cyl_normal = bake_maps(&cyl_low, Some(&cyl_high), S, &[MeshMapKind::Normal], [0.03, 0.01]).remove(&MeshMapKind::Normal).unwrap();
    let cyl_surface = curvature_from_normal_map(&cyl_low, 0, &cyl_normal, S, &NormalCurvatureSettings::default(), None).unwrap();
    let cyl_image = image_space_curvature(&cyl_normal);

    // Sheet: 3 columns x 2 rows of S² tiles with a 8 px gap.
    let gap = 8u32;
    let (sw, sh) = (3 * S + 4 * gap, 2 * S + 3 * gap);
    let mut sheet = vec![0.12f32; (sw * sh * 3) as usize];
    for (row, tiles) in [[&cube_normal, &cube_image, &cube_surface], [&cyl_normal, &cyl_image, &cyl_surface]].iter().enumerate() {
        for (col, img) in tiles.iter().enumerate() {
            let px = rgb(img);
            let (ox, oy) = (gap + col as u32 * (S + gap), gap + row as u32 * (S + gap));
            for y in 0..S {
                for x in 0..S {
                    let s = ((y * S + x) * 3) as usize;
                    let d = (((oy + y) * sw + ox + x) * 3) as usize;
                    sheet[d..d + 3].copy_from_slice(&px[s..s + 3]);
                }
            }
        }
    }
    save_image(&out.join("curvature_sheet.png"), sw, sh, 3, &sheet, FileFormat::Png8).unwrap();

    // The sample crate's maps, to import by file name into a new project.
    let crate_mesh = primitives::sample_crate();
    write_obj(&crate_mesh, &out.join("crate.obj")).unwrap();
    let maps = bake_maps(&crate_mesh, None, 1024, &[MeshMapKind::Normal, MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature, MeshMapKind::Id, MeshMapKind::Position], [0.01, 0.01]);
    for (kind, name) in [(MeshMapKind::Normal, "normal"), (MeshMapKind::AmbientOcclusion, "ambient_occlusion"), (MeshMapKind::Curvature, "curvature"), (MeshMapKind::Id, "id"), (MeshMapKind::Position, "position")] {
        write_png(&out.join(format!("crate_{name}.png")), &maps[&kind]);
    }
    println!("wrote {}", out.display());
}
