use glam::{Vec2, Vec3};
use subcraft_core::MeshMapKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, Corner, MeshBuilder, primitives};

use super::paint_maps::PaintMap;
use super::skew::SkewCorrection;
use super::surface::texels;
use super::*;

const RES: u32 = 64;

fn gray(w: u32, h: u32, f: impl Fn(u32, u32) -> f32) -> PaintMap {
    let mut img = TiledImage::new(w, h, 1, [0.0; 4]);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, [f(x, y); 4]);
        }
    }
    PaintMap::new(img)
}

#[test]
fn a_paint_map_is_read_bilinearly_at_texel_centres() {
    let m = gray(2, 1, |x, _| x as f32);
    assert_eq!(m.sample(0.0, 0.5), 0.0, "clamped at the border");
    assert_eq!(m.sample(1.0, 0.5), 1.0);
    assert!((m.sample(0.5, 0.5) - 0.5).abs() < 1e-6);
    // A 4-texel bake row over the 2-texel map: centres at 1/8, 3/8, 5/8, 7/8.
    let row: Vec<f32> = (0..4).map(|i| m.at_texel(i, 4)).collect();
    assert_eq!(row, vec![0.0, 0.25, 0.75, 1.0]);
    assert_eq!(gray(1, 1, |_, _| 0.75).at_texel(5, 8), 0.75, "a 1 x 1 map is uniform");
}

/// Opacity of a plane under a card 0.05 above it, with a cage too close to reach the card.
/// `None` when every ray missed (the bake refuses).
fn pushed_opacity(push_map: Option<PaintMap>, push: f32) -> Option<(usize, usize, usize, usize)> {
    let low = primitives::plane(2.0, 4);
    let mut b = MeshBuilder::new("card");
    let m = b.material("card");
    let c = |x: f32, z: f32| Corner { position: Vec3::new(x, 0.05, z), normal: Some(Vec3::Y), uv: Some(Vec2::ZERO) };
    b.push_triangle(m, [c(-1.5, -1.5), c(-1.5, 1.5), c(1.5, 1.5)]);
    b.push_triangle(m, [c(-1.5, -1.5), c(1.5, 1.5), c(1.5, -1.5)]);
    let high = b.build_for_baking().unwrap();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let diag = low.bounds.diagonal().max(high.bounds.diagonal());
    let s = BakeSettings {
        size: RES, maps: vec![MeshMapKind::Opacity], use_gpu: false, cage_front: 0.03 / diag, cage_rear: 0.01 / diag,
        cage_push_map: push_map, cage_push: push, ..BakeSettings::default()
    };
    let map = match bake(&input, &s, &Progress::new()) {
        Err(BakeError::NoProjection) => return None,
        r => r.unwrap().remove(&MeshMapKind::Opacity).unwrap(),
    };
    // (hits, misses) on the left half (U < 0.5) and on the right half.
    let mut n = (0, 0, 0, 0);
    for t in texels(&low, 0, RES) {
        let (x, hit) = (t.index % RES, map.get(t.index % RES, t.index / RES)[0] > 0.5);
        match (x < RES / 2 - 2, x > RES / 2 + 2, hit) {
            (true, _, true) => n.0 += 1,
            (true, _, false) => n.1 += 1,
            (_, true, true) => n.2 += 1,
            (_, true, false) => n.3 += 1,
            _ => {}
        }
    }
    Some(n)
}

#[test]
fn the_cage_push_map_moves_the_cage_out_only_where_it_is_painted() {
    // Without a push the 0.03 cage stays under the card at 0.05: every ray misses.
    assert_eq!(pushed_opacity(None, 1.0), None);
    // White on the left half (U < 0.5) and push 2: the cage moves out to 0.09 there.
    let left = gray(RES, RES, |x, _| if x < RES / 2 { 1.0 } else { 0.0 });
    let (lh, lm, rh, rm) = pushed_opacity(Some(left.clone()), 2.0).unwrap();
    assert!(lm == 0 && lh > 500, "left: {lh} hits, {lm} misses");
    assert!(rh == 0 && rm > 500, "right: {rh} hits, {rm} misses");
    // A push of 0 turns the map off.
    assert_eq!(pushed_opacity(Some(left), 0.0), None);
}

/// Height map of a cube baked from a cube 0.1 larger on every side: the +Z face's values.
fn cube_height(skew: SkewCorrection, skew_map: Option<PaintMap>) -> Vec<f32> {
    let low = primitives::cube(2.0);
    let high = primitives::cube(2.2);
    let (low_bvh, high_bvh) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &low_bvh }, high: Some(Geometry { mesh: &high, bvh: &high_bvh }), material_slot: 0 };
    let diag = low.bounds.diagonal().max(high.bounds.diagonal());
    let s = BakeSettings {
        size: RES, maps: vec![MeshMapKind::Height], use_gpu: false, average_normals: true, cage_front: 0.15, cage_rear: 0.02,
        height: HeightNormalization::Manual { distance: 0.2 / diag }, skew, skew_map, ..BakeSettings::default()
    };
    let height = bake(&input, &s, &Progress::new()).unwrap().remove(&MeshMapKind::Height).unwrap();
    texels(&low, 0, RES).into_iter().filter(|t| t.position.z > 0.999).map(|t| height.get(t.index % RES, t.index / RES)[0]).collect()
}

#[test]
fn the_skew_map_scales_the_correction() {
    let on = SkewCorrection { enabled: true, edge_protection: false, ..Default::default() };
    let averaged = cube_height(SkewCorrection::default(), None);
    let corrected = cube_height(on, None);
    assert_ne!(averaged, corrected, "the correction changes the face");
    assert_eq!(cube_height(on, Some(gray(1, 1, |_, _| 1.0))), corrected, "white: full correction");
    assert_eq!(cube_height(on, Some(gray(1, 1, |_, _| 0.0))), averaged, "black: averaged rays");
    // Half gray lands between the two.
    let half = cube_height(on, Some(gray(1, 1, |_, _| 0.5)));
    let straight = 0.75;
    let err = |v: &[f32]| v.iter().map(|h| (h - straight).abs()).sum::<f32>();
    assert!(err(&corrected) < err(&half) && err(&half) < err(&averaged), "{} {} {}", err(&corrected), err(&half), err(&averaged));
    // Without skew correction the map does nothing.
    assert_eq!(cube_height(SkewCorrection::default(), Some(gray(1, 1, |_, _| 1.0))), averaged);
}
