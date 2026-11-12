//! PLY / STL import with small generated fixtures (cube, pyramid) in every encoding, plus malformed input.

use glam::{Vec2, Vec3};
use subcraft_mesh::{BuildError, ImportError, ImportReport, import_mesh, import_mesh_for_baking};

const CUBE_V: [[f32; 3]; 8] =
    [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]];
const CUBE_QUADS: [[u32; 4]; 6] = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];
const PYR_V: [[f32; 3]; 5] = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.], [0.5, 0.5, 1.]];

fn load(name: &str, ext: &str, bytes: &[u8], bake: bool) -> Result<ImportReport, ImportError> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("subcraft-plystl-{}-{name}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join(format!("fixture.{ext}"));
    std::fs::write(&p, bytes).unwrap();
    let r = if bake { import_mesh_for_baking(&p) } else { import_mesh(&p) };
    std::fs::remove_dir_all(&d).ok();
    r
}

#[derive(Clone, Copy, PartialEq)]
enum Enc {
    Ascii,
    Le,
    Be,
}

/// PLY cube with normals, optional uvs (s = x, t = y) and colors; faces are quads.
fn ply_cube(enc: Enc, uvs: bool) -> Vec<u8> {
    let fmt = match enc {
        Enc::Ascii => "ascii",
        Enc::Le => "binary_little_endian",
        Enc::Be => "binary_big_endian",
    };
    let mut h = format!("ply\nformat {fmt} 1.0\ncomment test\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\n");
    if uvs {
        h += "property float s\nproperty float t\n";
    }
    h += "property uchar red\nproperty uchar green\nproperty uchar blue\nelement face 6\nproperty list uchar int vertex_indices\nend_header\n";
    let mut out = h.into_bytes();
    for p in CUBE_V {
        let n = (Vec3::from(p) - Vec3::splat(0.5)).normalize();
        let mut vals = vec![p[0], p[1], p[2], n.x, n.y, n.z];
        if uvs {
            vals.extend([p[0], p[1]]);
        }
        if enc == Enc::Ascii {
            let s: Vec<String> = vals.iter().map(|v| v.to_string()).collect();
            out.extend(format!("{} 255 128 0\n", s.join(" ")).bytes());
        } else {
            for v in vals {
                out.extend(if enc == Enc::Be { v.to_be_bytes() } else { v.to_le_bytes() });
            }
            out.extend([255u8, 128, 0]);
        }
    }
    for q in CUBE_QUADS {
        if enc == Enc::Ascii {
            out.extend(format!("4 {} {} {} {}\n", q[0], q[1], q[2], q[3]).bytes());
        } else {
            out.push(4);
            for i in q {
                out.extend(if enc == Enc::Be { i.to_be_bytes() } else { i.to_le_bytes() });
            }
        }
    }
    out
}

#[test]
fn ply_cube_in_every_encoding() {
    for (name, enc) in [("ascii", Enc::Ascii), ("le", Enc::Le), ("be", Enc::Be)] {
        let r = load(name, "ply", &ply_cube(enc, true), false).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r.mesh.triangle_count(), 12, "{name}: quads are triangulated");
        assert!(r.mesh.positions.len() <= 12, "{name}: shared vertices, got {}", r.mesh.positions.len());
        assert!(r.warnings.iter().any(|w| w.contains("vertex colors")), "{name}");
        assert!(!r.warnings.iter().any(|w| w.contains("no UVs")), "{name}");
        // v is flipped to the top-left origin.
        let i = r.mesh.positions.iter().position(|p| *p == Vec3::new(1., 1., 1.)).unwrap();
        assert_eq!(r.mesh.uvs[i], Vec2::new(1., 0.), "{name}");
        // Given normals are kept: this corner's normal points away from the centre.
        assert!(r.mesh.normals[i].dot(Vec3::splat(1.).normalize()) > 0.99, "{name}");
    }
}

#[test]
fn ply_without_uvs_warns_and_paint_import_reports_clear_error() {
    let bytes = ply_cube(Enc::Le, false);
    let r = load("nouv", "ply", &bytes, true).unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("no UVs, use auto unwrap or provide UVs")), "{:?}", r.warnings);
    assert_eq!(r.mesh.triangle_count(), 12);
    let e = load("nouv2", "ply", &bytes, false).unwrap_err();
    assert!(matches!(e, ImportError::Build(BuildError::NoUvs)));
}

#[test]
fn ply_face_texcoords_and_ngon_fan() {
    let mut s = String::from("ply\nformat ascii 1.0\nelement vertex 5\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nproperty list uchar float texcoord\nend_header\n");
    for (x, y) in [(0., 0.), (2., 0.), (3., 1.), (1.5, 2.), (-1., 1.)] {
        s += &format!("{x} {y} 0\n");
    }
    s += "5 0 1 2 3 4 10 0 0 0.5 0 1 0.5 1 1 0.5 1 0\n";
    let r = load("ngon", "ply", s.as_bytes(), false).unwrap();
    assert_eq!(r.mesh.triangle_count(), 3);
    assert!(!r.warnings.iter().any(|w| w.contains("no UVs")));
}

#[test]
fn ply_malformed_inputs_error_without_panicking() {
    let good = ply_cube(Enc::Le, true);
    for cut in 0..good.len() {
        let _ = load("trunc", "ply", &good[..cut], false);
    }
    assert!(load("t1", "ply", &good[..good.len() - 3], false).is_err());
    assert!(load("t2", "ply", b"", false).is_err());
    assert!(load("t3", "ply", b"ply\nformat ascii 1.0\n", false).is_err());
    assert!(load("t4", "ply", b"ply\nformat ascii 1.0\nelement vertex 2\nproperty float x\nend_header\n1\n2\n", false).is_err());
    assert!(load("t5", "ply", b"ply\nformat weird 1.0\nend_header\n", false).is_err());
    let huge = b"ply\nformat binary_little_endian 1.0\nelement vertex 4000000000\nproperty float x\nproperty float y\nproperty float z\nend_header\n\0\0\0\0";
    assert!(load("t6", "ply", huge, false).is_err());
    let bad_index = b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 99\n";
    assert!(load("t7", "ply", bad_index, true).is_err(), "out-of-range index leaves no triangles");
}

fn stl_tris() -> Vec<[Vec3; 3]> {
    let v = PYR_V.map(Vec3::from);
    vec![
        [v[0], v[2], v[1]],
        [v[0], v[3], v[2]],
        [v[0], v[1], v[4]],
        [v[1], v[2], v[4]],
        [v[2], v[3], v[4]],
        [v[3], v[0], v[4]],
    ]
}

fn stl_binary(tris: &[[Vec3; 3]]) -> Vec<u8> {
    let mut out = vec![0u8; 80];
    out.extend((tris.len() as u32).to_le_bytes());
    for t in tris {
        out.extend([0u8; 12]);
        for p in t {
            for c in p.to_array() {
                out.extend(c.to_le_bytes());
            }
        }
        out.extend([0u8; 2]);
    }
    out
}

fn stl_ascii(tris: &[[Vec3; 3]]) -> Vec<u8> {
    let mut s = String::from("solid pyramid\n");
    for t in tris {
        s += "facet normal 0 0 0\n outer loop\n";
        for p in t {
            s += &format!("  vertex {:e} {:e} {:e}\n", p.x, p.y, p.z);
        }
        s += " endloop\nendfacet\n";
    }
    s += "endsolid pyramid\n";
    s.into_bytes()
}

#[test]
fn stl_pyramid_ascii_and_binary_weld_and_warn() {
    for (name, bytes) in [("bin", stl_binary(&stl_tris())), ("ascii", stl_ascii(&stl_tris()))] {
        let r = load(name, "stl", &bytes, true).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r.mesh.triangle_count(), 6, "{name}");
        let mut uniq: Vec<[u32; 3]> = r.mesh.positions.iter().map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 5, "{name}");
        assert!(r.mesh.positions.len() < 18, "{name}: welded, got {}", r.mesh.positions.len());
        assert!(r.warnings.iter().any(|w| w.contains("no UVs, use auto unwrap or provide UVs")), "{name}");
        assert!(r.mesh.validate().is_empty());
        assert!(matches!(load(name, "stl", &bytes, false), Err(ImportError::Build(BuildError::NoUvs))));
    }
}

#[test]
fn stl_cube_keeps_hard_edges_and_welds_vertices() {
    let v = CUBE_V.map(Vec3::from);
    let mut tris = Vec::new();
    for q in CUBE_QUADS {
        tris.push([v[q[0] as usize], v[q[1] as usize], v[q[2] as usize]]);
        tris.push([v[q[0] as usize], v[q[2] as usize], v[q[3] as usize]]);
    }
    let r = load("cube", "stl", &stl_binary(&tris), true).unwrap();
    assert_eq!(r.mesh.triangle_count(), 12);
    assert!(r.mesh.normals.iter().all(|n| n.abs().max_element() > 0.999), "{:?}", r.mesh.normals);
    assert_eq!(r.mesh.positions.len(), 24, "6 faces x 4 corners");
}

#[test]
fn stl_malformed_inputs_error_without_panicking() {
    let bin = stl_binary(&stl_tris());
    for cut in 0..bin.len() {
        let _ = load("trunc", "stl", &bin[..cut], true);
    }
    assert!(load("a", "stl", &bin[..bin.len() - 7], true).is_err(), "truncated binary");
    assert!(load("b", "stl", b"", true).is_err());
    assert!(load("c", "stl", b"hello world", true).is_err());
    let asc = stl_ascii(&stl_tris());
    for cut in (0..asc.len()).step_by(7) {
        let _ = load("truncasc", "stl", &asc[..cut], true);
    }
    assert!(load("d", "stl", &asc[..asc.len() - 60], true).is_err(), "truncated ascii");
    assert!(load("e", "stl", b"solid x\nfacet normal 0 0 0\nouter loop\nvertex 1 two 3\n", true).is_err());
    let mut lie = bin.clone();
    lie[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(load("f", "stl", &lie, true).is_err());
}

#[test]
fn extensions_are_advertised() {
    assert!(subcraft_mesh::SUPPORTED_EXTENSIONS.contains(&"ply") && subcraft_mesh::SUPPORTED_EXTENSIONS.contains(&"stl"));
}

/// QA 2026-11: an ASCII STL of 84+ bytes was only recognised when it started with exactly
/// lowercase `solid` at byte 0; otherwise bytes 80..84 of the text were read as a triangle count
/// and the file failed as "binary STL is truncated".
#[test]
fn ascii_stl_is_recognised_with_leading_space_a_bom_or_upper_case() {
    let plain = String::from_utf8(stl_ascii(&stl_tris())).unwrap();
    let expected = load("plain", "stl", plain.as_bytes(), true).unwrap().mesh.triangle_count();
    assert!(plain.len() >= 84 && expected > 0);
    let variants = [
        ("leading-newline", format!("\n{plain}").into_bytes()),
        ("leading-spaces", format!("  \t{plain}").into_bytes()),
        ("bom", [b"\xEF\xBB\xBF".as_slice(), plain.as_bytes()].concat()),
        ("upper", plain.to_uppercase().into_bytes()),
    ];
    for (name, bytes) in variants {
        let r = load(name, "stl", &bytes, true).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(r.mesh.triangle_count(), expected, "{name}");
    }
}

/// QA 2026-11: `x as u32` turned a negative (or NaN) index into vertex 0, so a corrupt face was
/// imported as a wrong triangle instead of being skipped with the warning.
#[test]
fn ply_faces_with_negative_indices_are_skipped_not_remapped() {
    let text = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 2\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n3 0 1 2\n3 -1 1 3\n";
    let r = load("neg", "ply", text.as_bytes(), true).unwrap();
    assert_eq!(r.mesh.triangle_count(), 1);
    assert!(r.warnings.iter().any(|w| w.contains("skipped 1 PLY faces")), "{:?}", r.warnings);
}

/// QA 2026-11: every face of an ASCII PLY counted all remaining tokens of the file, so import
/// time grew with the square of the face count (a scan with a few hundred thousand faces hung).
#[test]
fn ascii_ply_import_time_is_linear_in_the_face_count() {
    let grid = |n: usize| {
        let mut s = format!("ply\nformat ascii 1.0\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\nelement face {}\nproperty list uchar int vertex_indices\nend_header\n", (n + 1) * (n + 1), n * n);
        for y in 0..=n {
            for x in 0..=n {
                s += &format!("{x} {y} 0\n");
            }
        }
        for y in 0..n {
            for x in 0..n {
                let a = y * (n + 1) + x;
                s += &format!("4 {} {} {} {}\n", a, a + 1, a + n + 2, a + n + 1);
            }
        }
        s.into_bytes()
    };
    let time = |n: usize| {
        let bytes = grid(n);
        let t = std::time::Instant::now();
        let r = load("grid", "ply", &bytes, true).unwrap();
        assert_eq!(r.mesh.triangle_count(), n * n * 2);
        t.elapsed().as_secs_f64()
    };
    let small = time(60).max(1e-3); // 3 600 faces
    let big = time(240); // 57 600 faces: 16x the faces; the quadratic reader took ~256x as long
    assert!(big < small * 80.0 + 0.5, "16x the faces took {:.0}x the time ({small:.3}s -> {big:.3}s)", big / small);
}

/// QA 2026-11: -0.0 and 0.0 are the same position but have different bits; welding by bits left
/// the shared corner split, with a hard edge through a smooth surface.
#[test]
fn stl_welding_treats_negative_zero_as_zero() {
    let text = "solid a\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nfacet normal 0 0 1\nouter loop\nvertex -0 0 0\nvertex 0 1 0\nvertex -1 0 0.3\nendloop\nendfacet\nendsolid a\n";
    let r = load("negzero", "stl", text.as_bytes(), true).unwrap();
    assert_eq!(r.mesh.triangle_count(), 2);
    // The corner at the origin is written as 0 by one facet and as -0 by the other. Welded, both
    // get the same smooth normal; unwelded, each kept its own face normal (17 degrees apart).
    let at_origin: Vec<Vec3> = r.mesh.positions.iter().zip(&r.mesh.normals).filter(|(p, _)| **p == Vec3::ZERO).map(|(_, n)| *n).collect();
    assert!(!at_origin.is_empty());
    for n in &at_origin {
        assert!(n.distance(at_origin[0]) < 1e-4, "split corner normals: {at_origin:?}");
        assert!(n.z < 0.999, "the corner normal is the blend of both faces, not the flat +z: {n}");
    }
}
