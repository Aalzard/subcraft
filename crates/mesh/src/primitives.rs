//! Procedural meshes: built-in sample geometry and deterministic test fixtures.
//!
//! All primitives carry explicit UV layouts *with seams* — painting tests must cross islands.

use std::f32::consts::{PI, TAU};

use glam::{Vec2, Vec3};

use crate::mesh::{Corner, Mesh, MeshBuilder};

fn corner(p: Vec3, n: Vec3, uv: Vec2) -> Corner {
    Corner { position: p, normal: Some(n), uv: Some(uv) }
}

/// Push a triangle, flipping winding so its geometric normal agrees with `outward`.
fn push_oriented(b: &mut MeshBuilder, m: u32, mut c: [Corner; 3], outward: Vec3) {
    let n = (c[1].position - c[0].position).cross(c[2].position - c[0].position);
    if n.dot(outward) < 0.0 {
        c.swap(1, 2);
    }
    b.push_triangle(m, c);
}

/// UV sphere. One island covering the whole UV square, seam along the -X meridian.
pub fn uv_sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    let mut b = MeshBuilder::new("Sphere");
    let m = b.material("Sphere");
    let vert = |i: u32, j: u32| {
        let theta = PI * i as f32 / rings as f32;
        let phi = TAU * j as f32 / segments as f32;
        let n = Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin());
        corner(n * radius, n, Vec2::new(j as f32 / segments as f32, i as f32 / rings as f32))
    };
    for i in 0..rings {
        for j in 0..segments {
            let (a, bb, c, d) = (vert(i, j), vert(i + 1, j), vert(i + 1, j + 1), vert(i, j + 1));
            let center = (a.position + bb.position + c.position + d.position).normalize_or_zero();
            push_oriented(&mut b, m, [a, bb, c], center);
            push_oriented(&mut b, m, [a, c, d], center);
        }
    }
    b.build().expect("sphere is valid")
}

/// Axis-aligned box whose six faces are separate UV islands in a 3×2 atlas.
pub fn atlas_box(size: Vec3, subdivisions: u32, name: &str) -> Mesh {
    let mut b = MeshBuilder::new(name);
    let m = b.material(name);
    add_atlas_box(&mut b, m, Vec3::ZERO, size, subdivisions, 0.02);
    b.build().expect("box is valid")
}

pub fn cube(size: f32) -> Mesh {
    atlas_box(Vec3::splat(size), 4, "Cube")
}

/// A slab thinner than typical depth precision: front and back faces are separate islands, so
/// any paint-through from the front is detectable on the back island.
pub fn thin_slab(width: f32, height: f32, thickness: f32) -> Mesh {
    atlas_box(Vec3::new(width, height, thickness), 8, "Slab")
}

pub(crate) fn add_atlas_box(b: &mut MeshBuilder, m: u32, center: Vec3, size: Vec3, subdiv: u32, margin: f32) {
    let h = size * 0.5;
    // (normal, u axis, v axis) — v axis points "down" in UV, i.e. -up on the face.
    let faces = [
        (Vec3::Z, Vec3::X, -Vec3::Y),
        (-Vec3::Z, -Vec3::X, -Vec3::Y),
        (Vec3::X, -Vec3::Z, -Vec3::Y),
        (-Vec3::X, Vec3::Z, -Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::Z),
        (-Vec3::Y, Vec3::X, -Vec3::Z),
    ];
    let s = subdiv.max(1);
    for (fi, (n, ua, va)) in faces.into_iter().enumerate() {
        let cell = Vec2::new((fi % 3) as f32 / 3.0, (fi / 3) as f32 / 2.0);
        let cell_size = Vec2::new(1.0 / 3.0, 0.5);
        let uv_of = |a: f32, c: f32| cell + Vec2::new(margin + a * (cell_size.x - 2.0 * margin), margin + c * (cell_size.y - 2.0 * margin));
        let pos_of = |a: f32, c: f32| {
            let ext_u = (ua * h).abs().max_element();
            let ext_v = (va * h).abs().max_element();
            let ext_n = (n * h).abs().max_element();
            center + n * ext_n + ua * ((a * 2.0 - 1.0) * ext_u) + va * ((c * 2.0 - 1.0) * ext_v)
        };
        for i in 0..s {
            for j in 0..s {
                let (a0, a1) = (j as f32 / s as f32, (j + 1) as f32 / s as f32);
                let (c0, c1) = (i as f32 / s as f32, (i + 1) as f32 / s as f32);
                let k = |a: f32, c: f32| corner(pos_of(a, c), n, uv_of(a, c));
                push_oriented(b, m, [k(a0, c0), k(a1, c0), k(a1, c1)], n);
                push_oriented(b, m, [k(a0, c0), k(a1, c1), k(a0, c1)], n);
            }
        }
    }
}

/// Subdivided plane on XZ facing +Y, single island.
pub fn plane(size: f32, subdivisions: u32) -> Mesh {
    let mut b = MeshBuilder::new("Plane");
    let m = b.material("Plane");
    let s = subdivisions.max(1);
    let k = |i: u32, j: u32| {
        let (x, z) = (j as f32 / s as f32, i as f32 / s as f32);
        corner(Vec3::new((x - 0.5) * size, 0.0, (z - 0.5) * size), Vec3::Y, Vec2::new(x, z))
    };
    for i in 0..s {
        for j in 0..s {
            push_oriented(&mut b, m, [k(i, j), k(i, j + 1), k(i + 1, j + 1)], Vec3::Y);
            push_oriented(&mut b, m, [k(i, j), k(i + 1, j + 1), k(i + 1, j)], Vec3::Y);
        }
    }
    b.build().expect("plane is valid")
}

/// Torus around +Y. Seams in both UV directions.
pub fn torus(major: f32, minor: f32, segments: u32, sides: u32) -> Mesh {
    let mut b = MeshBuilder::new("Torus");
    let m = b.material("Torus");
    let k = |i: u32, j: u32| {
        let a = TAU * i as f32 / segments as f32;
        let c = TAU * j as f32 / sides as f32;
        let ring = Vec3::new(a.cos(), 0.0, a.sin());
        let n = ring * c.cos() + Vec3::Y * c.sin();
        corner(ring * major + n * minor, n, Vec2::new(i as f32 / segments as f32, j as f32 / sides as f32))
    };
    for i in 0..segments {
        for j in 0..sides {
            let q = [k(i, j), k(i + 1, j), k(i + 1, j + 1), k(i, j + 1)];
            let outward = (q[0].normal.unwrap() + q[2].normal.unwrap()).normalize_or_zero();
            push_oriented(&mut b, m, [q[0], q[1], q[2]], outward);
            push_oriented(&mut b, m, [q[0], q[2], q[3]], outward);
        }
    }
    b.build().expect("torus is valid")
}

/// Built-in sample asset: a utility crate with two material slots ("Body" and "Trim"),
/// giving two texture sets out of the box.
pub fn sample_crate() -> Mesh {
    let mut b = MeshBuilder::new("SampleCrate");
    let body = b.material("Body");
    let trim = b.material("Trim");
    // Body: the main box. Trim: corner posts and bands, each with its own atlas islands.
    add_atlas_box(&mut b, body, Vec3::ZERO, Vec3::new(1.6, 1.0, 1.0), 6, 0.02);
    let post = Vec3::new(0.14, 1.08, 0.14);
    for (x, z) in [(-0.8, -0.5), (0.8, -0.5), (0.8, 0.5), (-0.8, 0.5)] {
        add_atlas_box_into_region(&mut b, trim, Vec3::new(x, 0.0, z), post, 2, region_of(x, z));
    }
    let band = Vec3::new(1.72, 0.12, 1.12);
    add_atlas_box_into_region(&mut b, trim, Vec3::new(0.0, 0.32, 0.0), band, 4, [0.0, 0.5, 0.5, 0.75]);
    add_atlas_box_into_region(&mut b, trim, Vec3::new(0.0, -0.32, 0.0), band, 4, [0.5, 0.5, 1.0, 0.75]);
    b.build().expect("crate is valid")
}

fn region_of(x: f32, z: f32) -> [f32; 4] {
    let i = match (x > 0.0, z > 0.0) {
        (false, false) => 0,
        (true, false) => 1,
        (true, true) => 2,
        (false, true) => 3,
    };
    let x0 = i as f32 * 0.25;
    [x0, 0.0, x0 + 0.25, 0.5]
}

/// Like [`add_atlas_box`] but squeezes the 3×2 atlas into a UV sub-rectangle `[u0, v0, u1, v1]`.
fn add_atlas_box_into_region(b: &mut MeshBuilder, m: u32, center: Vec3, size: Vec3, subdiv: u32, region: [f32; 4]) {
    let before = b.triangle_count();
    let mut tmp = MeshBuilder::new("tmp");
    let tm = tmp.material("tmp");
    add_atlas_box(&mut tmp, tm, center, size, subdiv, 0.03);
    let _ = before;
    for tri in tmp.take_corners().chunks_exact(3) {
        let remap = |c: Corner| Corner {
            uv: c.uv.map(|uv| Vec2::new(region[0] + uv.x * (region[2] - region[0]), region[1] + uv.y * (region[3] - region[1]))),
            ..c
        };
        b.push_triangle(m, [remap(tri[0]), remap(tri[1]), remap(tri[2])]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_are_valid_and_outward() {
        for mesh in [uv_sphere(1.0, 24, 12), cube(1.0), thin_slab(1.0, 1.0, 0.002), plane(1.0, 4), torus(1.0, 0.3, 24, 12), sample_crate()] {
            assert!(mesh.validate().is_empty(), "{}: {:?}", mesh.name, mesh.validate());
            let c = mesh.bounds.center();
            // Most triangles of convex-ish shapes should face away from the center.
            let outward = (0..mesh.triangle_count())
                .filter(|&t| {
                    let p = mesh.tri_positions(t);
                    mesh.geometric_normal(t).dot((p[0] + p[1] + p[2]) / 3.0 - c) > 0.0
                })
                .count();
            if mesh.name != "Plane" {
                assert!(outward as f32 > 0.6 * mesh.triangle_count() as f32, "{} winding", mesh.name);
            }
            for uv in &mesh.uvs {
                assert!((0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y), "{} uv {uv}", mesh.name);
            }
        }
    }

    #[test]
    fn crate_has_two_material_slots() {
        let m = sample_crate();
        assert_eq!(m.materials.len(), 2);
        assert_eq!(m.materials[0].name, "Body");
        assert_eq!(m.materials[1].name, "Trim");
    }
}
