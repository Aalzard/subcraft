//! Several UV sets per mesh: kept from the file, switched per material slot, saved with projects.

use glam::{Mat4, Vec2, Vec3};
use subcraft_mesh::{Corner, Mesh, MeshBuilder, serialize};

/// Two quads side by side, each its own material slot. Set 0 fills 0..1 per quad; set 1 packs
/// both quads into the left/right halves (a lightmap-style layout), and is named by the file.
fn two_quads() -> Mesh {
    let mut b = MeshBuilder::new("quads");
    b.set_uv_set_name(1, "lightmap");
    for (q, name) in ["left", "right"].iter().enumerate() {
        let m = b.material(name);
        let x0 = q as f32 * 1.5;
        let p = [Vec3::new(x0, 0., 0.), Vec3::new(x0 + 1., 0., 0.), Vec3::new(x0 + 1., 1., 0.), Vec3::new(x0, 1., 0.)];
        let uv0 = [Vec2::new(0., 1.), Vec2::new(1., 1.), Vec2::new(1., 0.), Vec2::new(0., 0.)];
        let uv1 = uv0.map(|uv| Vec2::new(0.5 * q as f32 + 0.5 * uv.x, uv.y));
        for tri in [[0, 1, 2], [0, 2, 3]] {
            let c = tri.map(|i| Corner { position: p[i], normal: Some(Vec3::Z), uv: Some(uv0[i]) });
            b.push_triangle_uv_sets(m, c, &[], &[tri.map(|i| Some(uv1[i]))]);
        }
    }
    b.build().unwrap()
}

fn corner_uvs(mesh: &Mesh) -> Vec<Vec2> {
    mesh.indices.iter().map(|&i| mesh.uvs[i as usize]).collect()
}

#[test]
fn every_set_is_kept_and_set_zero_is_what_gets_painted() {
    let mesh = two_quads();
    assert_eq!(mesh.uv_set_count(), 2);
    assert_eq!(mesh.uv_set_name(0), "UV0", "unnamed sets get a number");
    assert_eq!(mesh.uv_set_name(1), "lightmap");
    assert_eq!(mesh.uv_sets[0].corners, corner_uvs(&mesh));
    assert_eq!(mesh.slot_uv_set(0), Some(0));
    assert!(mesh.validate().is_empty());
}

#[test]
fn switching_one_slot_changes_only_its_triangles_and_switching_back_restores_it() {
    let mesh = two_quads();
    let switched = mesh.with_uv_set(&[1], 1).unwrap();
    assert!(switched.validate().is_empty(), "{:?}", switched.validate());
    assert_eq!(switched.slot_uv_set(0), Some(0));
    assert_eq!(switched.slot_uv_set(1), Some(1));
    let (before, after) = (corner_uvs(&mesh), corner_uvs(&switched));
    for t in 0..mesh.triangle_count() {
        for c in 3 * t..3 * t + 3 {
            let want = if mesh.tri_material[t] == 1 { mesh.uv_sets[1].corners[c] } else { before[c] };
            assert_eq!(after[c], want, "corner {c}");
        }
    }
    // Right quad now lives in u 0.5..1.
    let right_u: Vec<f32> = (0..mesh.triangle_count()).filter(|&t| mesh.tri_material[t] == 1).flat_map(|t| switched.tri_uvs(t).map(|uv| uv.x)).collect();
    assert!(right_u.iter().all(|&u| (0.5..=1.0).contains(&u)), "{right_u:?}");
    assert!(switched.tangents.iter().all(|t| t.is_finite() && t.truncate().length() > 0.5));
    // Positions and topology are untouched.
    for t in 0..mesh.triangle_count() {
        assert_eq!(switched.tri(t).map(|i| switched.positions[i as usize]), mesh.tri(t).map(|i| mesh.positions[i as usize]));
    }
    let back = switched.with_uv_set(&[1], 0).unwrap();
    assert_eq!(corner_uvs(&back), before);
    assert!(back.tri_uv_set.is_empty(), "all on set 0 again: nothing to store");
    assert!(mesh.with_uv_set(&[0], 2).is_none(), "no third set");
}

#[test]
fn sets_and_the_choice_round_trip_through_the_project_blob() {
    let switched = two_quads().with_uv_set(&[0], 1).unwrap();
    let back = serialize::decode(&serialize::encode(&switched)).unwrap();
    assert_eq!(back.uv_sets, switched.uv_sets);
    assert_eq!(back.tri_uv_set, switched.tri_uv_set);
    assert_eq!(back.uvs, switched.uvs);
    // A mesh with one set writes exactly what it did before the extension existed.
    let plain = subcraft_mesh::primitives::cube(1.0);
    let bytes = serialize::encode(&plain);
    assert!(!bytes.windows(4).any(|w| w == b"UVST"));
    // Both trailing extensions together still decode (unit first, then sets).
    let mut with_unit = switched.clone();
    with_unit.unit_meters = 0.01;
    let back = serialize::decode(&serialize::encode(&with_unit)).unwrap();
    assert_eq!(back.unit_meters, 0.01);
    assert_eq!(back.uv_sets.len(), 2);
}

#[test]
fn regrouping_and_mirroring_keep_the_sets_in_step_with_the_triangles() {
    let switched = two_quads().with_uv_set(&[1], 1).unwrap();
    // Swap the slot order: triangles move, and their UV-set corners must move with them.
    let names = vec!["B".to_string(), "A".to_string()];
    let slot_of_tri: Vec<u32> = switched.tri_material.iter().map(|&m| 1 - m).collect();
    let (regrouped, _) = switched.regroup(&names, &slot_of_tri, &[]);
    assert!(regrouped.validate().is_empty());
    for t in 0..regrouped.triangle_count() {
        let active = regrouped.tri_uv_set(t);
        assert_eq!(regrouped.uv_sets[active].corners[3 * t..3 * t + 3], regrouped.tri_uvs(t), "triangle {t}");
    }
    let mirrored = switched.transformed(&Mat4::from_scale(Vec3::new(-1., 1., 1.)));
    assert!(mirrored.validate().is_empty());
    for t in 0..mirrored.triangle_count() {
        let active = mirrored.tri_uv_set(t);
        assert_eq!(mirrored.uv_sets[active].corners[3 * t..3 * t + 3], mirrored.tri_uvs(t), "triangle {t}");
    }
}
