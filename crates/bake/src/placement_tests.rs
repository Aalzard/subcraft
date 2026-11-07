//! Cage distances with known shapes, and high-poly placement. A distance fraction `f` means
//! `f * diagonal` where the diagonal is the larger of the low and (placed) high bounding boxes.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use subcraft_core::{BakePlacement, FitMode, MeshMapKind};
use subcraft_mesh::{Bvh, Mesh, primitives};

use super::*;

const RES: u32 = 64;

fn run(low: &Mesh, high: &Mesh, maps: &[MeshMapKind], front: f32, rear: f32) -> Result<BTreeMap<MeshMapKind, TiledImage>, BakeError> {
    let (lb, hb) = (Bvh::build(low), Bvh::build(high));
    let input = BakeInput { low: Geometry { mesh: low, bvh: &lb }, high: Some(Geometry { mesh: high, bvh: &hb }), material_slot: 0 };
    let s = BakeSettings { size: RES, maps: maps.to_vec(), cage_front: front, cage_rear: rear, ..BakeSettings::default() };
    bake(&input, &s, &Progress::new())
}

fn moved(mesh: &Mesh, m: Mat4) -> Mesh { mesh.transformed(&m) }

/// Mean of the first channel over the texels of `low`.
fn mean(img: &TiledImage, low: &Mesh) -> f32 {
    let t = surface::texels(low, 0, RES);
    t.iter().map(|t| img.get(t.index % RES, t.index / RES)[0]).sum::<f32>() / t.len() as f32
}

#[test]
fn front_cage_misses_when_too_small_and_hits_at_the_expected_distance_when_large_enough() {
    let low = primitives::plane(2., 1);
    let high = moved(&low, Mat4::from_translation(Vec3::Y * 0.1)); // 0.1 above the low poly
    let diag = low.bounds.diagonal();
    // Front reach 0.02 * diag = 0.057 < 0.1: nothing is hit.
    assert_eq!(run(&low, &high, &[MeshMapKind::Height], 0.02, 0.02).unwrap_err(), BakeError::NoProjection);
    // Reach 0.05 * diag = 0.141 > 0.1: hit; the height map reads 0.5 + 0.5 * 0.1 / reach.
    let maps = run(&low, &high, &[MeshMapKind::Height], 0.05, 0.02).unwrap();
    let expected = 0.5 + 0.5 * 0.1 / (0.05 * diag);
    let got = mean(&maps[&MeshMapKind::Height], &low);
    assert!((got - expected).abs() < 1e-3, "{got} vs {expected}");
}

#[test]
fn rear_cage_misses_when_too_small_and_hits_below_the_surface_when_large_enough() {
    let low = primitives::plane(2., 1);
    let high = moved(&low, Mat4::from_translation(-Vec3::Y * 0.1)); // 0.1 below the low poly
    let diag = low.bounds.diagonal();
    assert_eq!(run(&low, &high, &[MeshMapKind::Height], 0.02, 0.02).unwrap_err(), BakeError::NoProjection);
    // Only the rear distance matters for a surface below: a big front alone does not help.
    let maps = run(&low, &high, &[MeshMapKind::Height], 0.02, 0.05).unwrap();
    let expected = 0.5 - 0.5 * 0.1 / (0.05 * diag);
    let got = mean(&maps[&MeshMapKind::Height], &low);
    assert!((got - expected).abs() < 1e-3, "{got} vs {expected}");
}

#[test]
fn a_slightly_bigger_cube_is_hit_on_every_face_at_the_growth_distance() {
    let low = primitives::cube(1.0);
    let high = moved(&low, Mat4::from_scale(Vec3::splat(1.04))); // each face 0.02 outside
    let diag = high.bounds.diagonal().max(low.bounds.diagonal());
    assert_eq!(run(&low, &high, &[MeshMapKind::Height], 0.01, 0.01).unwrap_err(), BakeError::NoProjection, "0.01 * 1.8 = 0.018 < 0.02");
    let maps = run(&low, &high, &[MeshMapKind::Height, MeshMapKind::Normal], 0.03, 0.03).unwrap();
    // Faces dominate the texels; edges / corners may read a slightly different distance.
    let expected = 0.5 + 0.5 * 0.02 / (0.03 * diag);
    let t = surface::texels(&low, 0, RES);
    let height = &maps[&MeshMapKind::Height];
    let close = t.iter().filter(|t| (height.get(t.index % RES, t.index / RES)[0] - expected).abs() < 0.01).count();
    assert!(close * 10 > t.len() * 9, "{close} of {} texels read the face distance {expected}", t.len());
    // The big cube's faces are flat, so the tangent-space normal is straight up everywhere it hit.
    let normal = &maps[&MeshMapKind::Normal];
    let flat = t.iter().filter(|t| { let n = normal.get(t.index % RES, t.index / RES); (n[0] - 0.5).abs() < 0.02 && (n[1] - 0.5).abs() < 0.02 && n[2] > 0.95 }).count();
    assert!(flat * 10 > t.len() * 9, "{flat} flat of {}", t.len());
}

#[test]
fn distance_fractions_follow_the_larger_of_the_two_diagonals() {
    // A high poly 3x as large makes every fraction 3x longer: a displaced copy that the plain low
    // diagonal cage would miss is reached. (Documented in the Bake window: fractions of the diagonal.)
    let low = primitives::plane(2., 1);
    let big = moved(&primitives::plane(2., 1), Mat4::from_scale(Vec3::new(3., 1., 3.)) * Mat4::from_translation(Vec3::Y * 0.1));
    let big_diag = big.bounds.diagonal();
    assert!(big_diag > low.bounds.diagonal() * 2.5);
    let f = 0.1 / big_diag * 1.2; // reach = 1.2 * 0.1 only on the larger diagonal
    assert!(f * low.bounds.diagonal() < 0.1, "would miss on the low diagonal alone");
    assert!(run(&low, &big, &[MeshMapKind::Height], f, f).is_ok());
}

#[test]
fn placement_brings_a_wrongly_placed_high_poly_back_to_the_reference_bake() {
    // Reference: a bump plane on the low plane, baked as modelled.
    let low = primitives::plane(2., 1);
    let reference_high = bumped();
    let reference = run(&low, &reference_high, &[MeshMapKind::Normal, MeshMapKind::Height], 0.1, 0.05).unwrap();
    // "AI" version: the same sculpt moved, scaled 1.5x and offset. Unplaced, it misses.
    let wrong = Arc::new(reference_high.transformed(&(Mat4::from_translation(Vec3::new(0.3, 0.8, -0.2)) * Mat4::from_scale(Vec3::splat(1.5)))));
    assert!(run(&low, &wrong, &[MeshMapKind::Normal], 0.1, 0.05).is_err());
    // Auto align (centre + fit longest side) then restore the half-height difference of the boxes
    // (the bump plane's box is 0.15 tall, centred 0.075 above the low plane).
    let mut p = BakePlacement::default();
    assert!(p.auto_align((low.bounds.min, low.bounds.max), (wrong.bounds.min, wrong.bounds.max), FitMode::LongestAxis));
    assert!((p.scale - 1. / 1.5).abs() < 1e-4);
    p.offset = [0., 0.075, 0.];
    let aligned = place_high(&low, &wrong, &p);
    let maps = run(&low, &aligned, &[MeshMapKind::Normal, MeshMapKind::Height], 0.1, 0.05).unwrap();
    for kind in [MeshMapKind::Normal, MeshMapKind::Height] {
        let (a, b) = (maps[&kind].to_dense(), reference[&kind].to_dense());
        let worst = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0., f32::max);
        assert!(worst < 3e-3, "{kind:?} differs from the reference bake by {worst}");
    }
}

#[test]
fn applying_a_placement_equals_pre_transforming_the_mesh() {
    let low = primitives::plane(2., 1);
    let raw = Arc::new(bumped());
    let p = BakePlacement { offset: [0.02, 0.01, -0.03], scale: 0.9, rotation_deg: [0., 25., 0.], ..BakePlacement::default() };
    let m = p.matrix((low.bounds.min, low.bounds.max), (raw.bounds.min, raw.bounds.max));
    let via_placement = place_high(&low, &raw, &p);
    let manual = raw.transformed(&m);
    assert_eq!(via_placement.positions, manual.positions);
    let a = run(&low, &via_placement, &[MeshMapKind::Normal], 0.1, 0.05).unwrap();
    let b = run(&low, &manual, &[MeshMapKind::Normal], 0.1, 0.05).unwrap();
    assert_eq!(a, b);
    // The identity is not copied, and the loaded mesh is never modified.
    assert!(Arc::ptr_eq(&place_high(&low, &raw, &BakePlacement::default()), &raw));
    assert_eq!(raw.positions, bumped().positions);
}

#[test]
fn offsetting_the_high_poly_changes_the_hit_distance_by_exactly_the_offset() {
    let low = primitives::plane(2., 1);
    let high = Arc::new(low.clone());
    let diag = low.bounds.diagonal();
    for offset in [0.02f32, 0.05, 0.1] {
        let p = BakePlacement { offset: [0., offset, 0.], ..BakePlacement::default() };
        let placed = place_high(&low, &high, &p);
        let maps = run(&low, &placed, &[MeshMapKind::Height], 0.1, 0.05).unwrap();
        let expected = 0.5 + 0.5 * offset / (0.1 * diag);
        assert!((mean(&maps[&MeshMapKind::Height], &low) - expected).abs() < 1e-3, "offset {offset}");
    }
}

fn bumped() -> Mesh {
    use glam::Vec2;
    use subcraft_mesh::{Corner, MeshBuilder};
    let mut b = MeshBuilder::new("High");
    let m = b.material("High");
    let s = 48;
    let h = |x: f32, z: f32| 0.15 * (-(x * x + z * z) / 0.08).exp();
    let p = |i: u32, j: u32| {
        let (x, z) = (j as f32 / s as f32 * 2.0 - 1.0, i as f32 / s as f32 * 2.0 - 1.0);
        Corner { position: Vec3::new(x, h(x, z), z), normal: None, uv: Some(Vec2::new((x + 1.0) * 0.5, (z + 1.0) * 0.5)) }
    };
    for i in 0..s {
        for j in 0..s {
            b.push_triangle(m, [p(i, j), p(i + 1, j), p(i + 1, j + 1)]);
            b.push_triangle(m, [p(i, j), p(i + 1, j + 1), p(i, j + 1)]);
        }
    }
    b.build().unwrap()
}
