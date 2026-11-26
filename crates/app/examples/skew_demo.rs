//! Skew correction picture: `cargo run --release -p subcraft-app --example skew_demo -- out.png`.
//!
//! A hard-edged cube (low poly) baked from the same cube carrying round bolts and a straight
//! domed rivets (high poly), with averaged normals. Left: the plain averaged rays, so bolts near the
//! edges come out sheared and shifted. Middle: skew correction with edge protection. Right: skew
//! correction without edge protection. Only the +Z face's island is shown; white crosses mark
//! where each rivet really is. The apex shift of every rivet is printed.

use std::f32::consts::TAU;
use std::path::PathBuf;

use glam::{Vec2, Vec3};
use subcraft_bake::skew::SkewCorrection;
use subcraft_bake::surface::texels;
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::MeshMapKind;
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::{Bvh, Corner, Mesh, MeshBuilder, primitives};

const RES: u32 = 1024;

fn c(p: Vec3, n: Vec3) -> Corner {
    Corner { position: p, normal: Some(n), uv: Some(Vec2::ZERO) }
}

fn quad(b: &mut MeshBuilder, m: u32, p: [Vec3; 4], n: Vec3) {
    b.push_triangle(m, [c(p[0], n), c(p[1], n), c(p[2], n)]);
    b.push_triangle(m, [c(p[0], n), c(p[2], n), c(p[3], n)]);
}

/// A domed rivet on the +Z face: a spherical cap of base `radius` rising `rise` above the face.
fn bolt(b: &mut MeshBuilder, m: u32, centre: Vec2, radius: f32, rise: f32) {
    let r = (radius * radius + rise * rise) / (2.0 * rise);
    let centre3 = Vec3::new(centre.x, centre.y, 1.0 + rise - r);
    let (seg, rings) = (64u32, 24u32);
    let max_angle = (radius / r).asin();
    let at = |i: u32, j: u32| {
        let (a, polar) = (TAU * i as f32 / seg as f32, max_angle * j as f32 / rings as f32);
        let n = Vec3::new(polar.sin() * a.cos(), polar.sin() * a.sin(), polar.cos());
        (centre3 + n * r, n)
    };
    for j in 0..rings {
        for i in 0..seg {
            let (p00, p10, p11, p01) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            b.push_triangle(m, [c(p00.0, p00.1), c(p01.0, p01.1), c(p11.0, p11.1)]);
            b.push_triangle(m, [c(p00.0, p00.1), c(p11.0, p11.1), c(p10.0, p10.1)]);
        }
    }
}

fn high_poly() -> Mesh {
    let mut b = MeshBuilder::new("High");
    let m = b.material("High");
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let (ua, va) = ((axis + 1) % 3, (axis + 2) % 3);
            let corner = |u: f32, v: f32| {
                let mut p = Vec3::ZERO;
                p[axis] = sign;
                p[ua] = u;
                p[va] = v;
                p
            };
            let mut n = Vec3::ZERO;
            n[axis] = sign;
            let mut q = [corner(-1., -1.), corner(1., -1.), corner(1., 1.), corner(-1., 1.)];
            if sign < 0.0 { q.reverse(); }
            quad(&mut b, m, q, n);
        }
    }
    for (x, y) in [(0.0, 0.0), (0.75, 0.75), (-0.75, 0.75), (0.75, -0.75), (-0.75, -0.75), (0.0, 0.8), (0.8, 0.0)] {
        bolt(&mut b, m, Vec2::new(x, y), 0.12, 0.05);
    }
    b.build_for_baking().unwrap()
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "skew_demo.png".into()));
    let low = primitives::cube(2.0);
    let high = high_poly();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let base = BakeSettings { size: RES, maps: vec![MeshMapKind::Normal], use_gpu: false, average_normals: true, cage_front: 0.04, cage_rear: 0.03, ..BakeSettings::default() };
    let variants = [
        SkewCorrection::default(),
        SkewCorrection { enabled: true, ..Default::default() },
        SkewCorrection { enabled: true, edge_protection: false, ..Default::default() },
    ];
    // The +Z face's island in the atlas.
    let face_texels: Vec<_> = texels(&low, 0, RES).into_iter().filter(|t| t.geometric_normal.z > 0.9).collect();
    let face: Vec<u32> = face_texels.iter().map(|t| t.index).collect();
    let (x0, x1) = (face.iter().map(|i| i % RES).min().unwrap(), face.iter().map(|i| i % RES).max().unwrap());
    let (y0, y1) = (face.iter().map(|i| i / RES).min().unwrap(), face.iter().map(|i| i / RES).max().unwrap());
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let gap = 8;
    let sheet_w = w * 3 + gap * 2;
    let mut sheet = vec![1.0f32; (sheet_w * h * 3) as usize];
    for (k, skew) in variants.into_iter().enumerate() {
        let s = BakeSettings { skew, ..base.clone() };
        let map = bake(&input, &s, &Progress::new()).unwrap().remove(&MeshMapKind::Normal).unwrap();
        // Where each rivet lands: the centroid of its bumped texels against its true centre.
        let mut worst = 0.0f32;
        for (x, y) in [(0.75f32, 0.75f32), (-0.75, 0.75), (0.75, -0.75), (-0.75, -0.75), (0.0, 0.8), (0.8, 0.0), (0.0, 0.0)] {
            // The dome's apex: the rivet texel whose baked normal points straight up.
            let mut apex = (f32::INFINITY, Vec2::ZERO);
            for t in &face_texels {
                let p = Vec2::new(t.position.x, t.position.y);
                let v = map.get(t.index % RES, t.index / RES);
                let tilt = (v[0] - 0.5).abs() + (v[1] - 0.5).abs();
                if p.distance(Vec2::new(x, y)) < 0.1 && tilt < apex.0 {
                    apex = (tilt, p);
                }
            }
            let off = apex.1.distance(Vec2::new(x, y));
            worst = worst.max(off);
            print!("({x:+.2},{y:+.2}) off {off:.4}  ");
        }
        println!("\nvariant {k}: worst rivet apex shift {worst:.4} (rivet radius 0.12)");
        // A thin cross on each rivet's true centre.
        let mut marked = std::collections::HashSet::new();
        for t in &face_texels {
            let p = Vec2::new(t.position.x, t.position.y);
            for (x, y) in [(0.75f32, 0.75f32), (-0.75, 0.75), (0.75, -0.75), (-0.75, -0.75), (0.0, 0.8), (0.8, 0.0), (0.0, 0.0)] {
                let d = p - Vec2::new(x, y);
                if (d.x.abs() < 0.003 && d.y.abs() < 0.05) || (d.y.abs() < 0.003 && d.x.abs() < 0.05) {
                    marked.insert(t.index);
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let mut px = map.get(x0 + x, y0 + y);
                if marked.contains(&((y0 + y) * RES + x0 + x)) {
                    px = [1.0, 1.0, 1.0, 1.0];
                }
                let o = ((y * sheet_w + k as u32 * (w + gap) + x) * 3) as usize;
                sheet[o..o + 3].copy_from_slice(&px[..3]);
            }
        }
    }
    save_image(&out, sheet_w, h, 3, &sheet, FileFormat::Png8).unwrap();
    println!("wrote {} ({sheet_w} x {h})", out.display());
}
