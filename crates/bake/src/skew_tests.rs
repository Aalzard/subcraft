use glam::Vec3;
use subcraft_core::MeshMapKind;
use subcraft_mesh::{Bvh, primitives};

use super::skew::{SkewBase, SkewCorrection, hard_edges};
use super::surface::texels;
use super::*;

const RES: u32 = 128;

#[test]
fn a_cube_has_twelve_hard_edges_and_a_sphere_none() {
    let cube = primitives::cube(2.0);
    // The faces are subdivided: each of the 12 cube edges is split into 4 segments.
    let edges = hard_edges(&cube);
    assert_eq!(edges.len(), 12 * 4, "{}", edges.len());
    for [a, b] in &edges {
        // On a cube edge two coordinates are at +-1.
        for p in [a, b] {
            assert_eq!(p.abs().to_array().iter().filter(|c| (**c - 1.0).abs() < 1e-5).count() >= 2, true, "{p}");
        }
    }
    assert!(hard_edges(&primitives::uv_sphere(1.0, 24, 12)).is_empty());
}

#[test]
fn edge_protection_ramps_from_the_edge() {
    let s = SkewCorrection { enabled: true, edge_contrast: 0.0, ..Default::default() };
    assert_eq!(s.ramp(0.0, 1.0), 0.0);
    assert_eq!(s.ramp(1.0, 1.0), 1.0);
    assert!((s.ramp(0.5, 1.0) - 0.5).abs() < 1e-6);
    let hard = SkewCorrection { edge_contrast: 1.0, ..s };
    assert_eq!(hard.ramp(0.49, 1.0), 0.0);
    assert_eq!(hard.ramp(0.51, 1.0), 1.0);
    let off = SkewCorrection { edge_protection: false, ..s };
    assert_eq!(off.ramp(0.0, 1.0), 1.0);
}

/// Height map of a cube baked from a cube 0.1 larger on every side, and the texels of the +Z face.
fn cube_height(skew: SkewCorrection) -> (Vec<(Vec3, f32)>, f32) {
    let low = primitives::cube(2.0);
    let high = primitives::cube(2.2);
    let (low_bvh, high_bvh) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &low_bvh }, high: Some(Geometry { mesh: &high, bvh: &high_bvh }), material_slot: 0 };
    let diag = low.bounds.diagonal().max(high.bounds.diagonal());
    let s = BakeSettings {
        size: RES, maps: vec![MeshMapKind::Height], use_gpu: false, average_normals: true, cage_front: 0.15, cage_rear: 0.02,
        height: HeightNormalization::Manual { distance: 0.2 / diag }, skew, ..BakeSettings::default()
    };
    let height = bake(&input, &s, &Progress::new()).unwrap().remove(&MeshMapKind::Height).unwrap();
    let face = texels(&low, 0, RES).into_iter().filter(|t| t.position.z > 0.999).map(|t| (t.position, height.get(t.index % RES, t.index / RES)[0])).collect();
    // Value of a straight ray: 0.1 above the surface on a 0.2 range.
    (face, 0.75)
}

#[test]
fn skew_correction_straightens_the_rays_on_flat_faces() {
    let (averaged, straight) = cube_height(SkewCorrection::default());
    let (corrected, _) = cube_height(SkewCorrection { enabled: true, edge_protection: false, ..Default::default() });
    let worst = |v: &[(Vec3, f32)]| v.iter().map(|(_, h)| (h - straight).abs()).fold(0.0f32, f32::max);
    // Averaged rays lean towards the corners and cross the 0.1 gap at a slant (longer).
    assert!(worst(&averaged) > 0.05, "averaged rays are skewed ({})", worst(&averaged));
    // Straight rays measure the gap exactly everywhere on the face.
    assert!(worst(&corrected) < 0.01, "corrected rays are straight ({})", worst(&corrected));
}

#[test]
fn edge_protection_keeps_averaged_rays_at_hard_edges() {
    let (averaged, straight) = cube_height(SkewCorrection::default());
    let protected = SkewCorrection { enabled: true, edge_protection: true, edge_distance: 0.1, edge_contrast: 0.0, base: SkewBase::Mesh };
    let (corrected, _) = cube_height(protected);
    let reach = 0.1 * primitives::cube(2.2).bounds.diagonal();
    for ((p, a), (_, c)) in averaged.iter().zip(&corrected) {
        let to_edge = (1.0 - p.x.abs()).min(1.0 - p.y.abs());
        if to_edge > reach * 1.05 {
            assert!((c - straight).abs() < 0.01, "straight beyond the edge distance at {p}: {c}");
        } else if to_edge < reach * 0.02 {
            assert!((c - a).abs() < 0.01, "averaged on the hard edge at {p}: {c} vs {a}");
        }
    }
}

#[test]
fn the_correction_is_full_away_from_the_edges() {
    let cube = primitives::cube(2.0);
    let field = super::skew::SkewField::new(&cube, SkewCorrection { enabled: true, ..Default::default() }, cube.bounds.diagonal());
    let reach = 0.03 * cube.bounds.diagonal();
    for (p, want) in [(Vec3::new(0.0, 0.0, 1.0), 1.0), (Vec3::new(0.75, 0.75, 1.0), 1.0), (Vec3::new(0.99, 0.0, 1.0), 0.0)] {
        let d = field.edge_distance(p);
        let a = field.amount(p);
        assert!((a - want).abs() < 0.05, "{p}: distance {d} (reach {reach}) amount {a}");
    }
}
