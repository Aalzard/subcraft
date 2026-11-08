//! Writes the Bake-window alignment test pair into a folder (default `docs/samples/highpoly-align`):
//! `crate_low.obj`, a plain low-poly box with UVs, and `crate_high_misaligned.obj`, a detailed
//! rounded crate with plank grooves and rivets that has been deliberately scaled 1.35x and moved,
//! the way an AI image-to-3D result usually arrives. Deterministic; run with
//! `cargo run -p subcraft-mesh --example make_alignment_demo [out-dir]`.

use glam::{Mat4, Vec3};
use subcraft_mesh::{Corner, MeshBuilder, primitives};
use subcraft_mesh::import::write_obj;

const HALF: Vec3 = Vec3::new(0.8, 0.5, 0.5);
const ROUND: f32 = 0.06;

/// A point on the box surface -> rounded, with plank grooves and rivets pushed along the normal.
fn detail(p: Vec3) -> Vec3 {
    let inner = HALF - Vec3::splat(ROUND);
    let c = p.clamp(-inner, inner);
    let n = (p - c).normalize_or_zero();
    let mut q = c + n * ROUND;
    let mut relief = 0.0;
    // Grooves between planks (every 0.2 along the long axis on the big faces), rivets near the corners.
    if n.z.abs() > 0.9 || n.y.abs() > 0.9 {
        let f = (q.x / 0.2).rem_euclid(1.0);
        let t = f.min(1.0 - f) * 0.2; // distance to the nearest groove line
        relief -= 0.012 * (-(t / 0.03).powi(2)).exp();
    }
    for sx in [-1.0, 1.0] { for sy in [-1.0, 1.0] {
        let d = Vec3::new(q.x - sx * 0.68, q.y - sy * 0.38, 0.0);
        if n.z.abs() > 0.9 { relief += 0.015 * (-(d.x * d.x + d.y * d.y) / 0.0015).exp(); }
    } }
    q += n * relief;
    q
}

fn main() -> std::io::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "docs/samples/highpoly-align".into());
    std::fs::create_dir_all(&dir)?;
    let low = primitives::atlas_box(HALF * 2.0, 2, "CrateLow");
    write_obj(&low, &std::path::Path::new(&dir).join("crate_low.obj"))?;

    let base = primitives::atlas_box(HALF * 2.0, 56, "CrateHigh");
    let mut b = MeshBuilder::new("CrateHigh");
    let m = b.material("CrateHigh");
    b.object("CrateHigh");
    let wrong = Mat4::from_translation(Vec3::new(0.35, -0.2, 0.25)) * Mat4::from_scale(Vec3::splat(1.35));
    for t in 0..base.triangle_count() {
        let tri = base.tri(t).map(|i| i as usize);
        let c = |i: usize| Corner { position: wrong.transform_point3(detail(base.positions[i])), normal: None, uv: Some(base.uvs[i]) };
        b.push_triangle(m, [c(tri[0]), c(tri[1]), c(tri[2])]);
    }
    let high = b.build().expect("high poly");
    write_obj(&high, &std::path::Path::new(&dir).join("crate_high_misaligned.obj"))?;
    println!("wrote {dir}: low {} tris, high {} tris", low.triangle_count(), high.triangle_count());
    Ok(())
}
