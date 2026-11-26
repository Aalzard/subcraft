use glam::{Vec2, Vec3};
use subcraft_core::MeshMapKind;
use subcraft_mesh::{Bvh, Corner, MeshBuilder, primitives};

use super::surface::texels;
use super::*;

const RES: u32 = 64;

fn settings() -> BakeSettings {
    BakeSettings { size: RES, maps: vec![MeshMapKind::Opacity], use_gpu: false, cage_front: 0.02, cage_rear: 0.02, ..BakeSettings::default() }
}

#[test]
fn opacity_is_off_in_a_default_bake() {
    assert!(!BakeSettings::default().maps.contains(&MeshMapKind::Opacity));
    assert!(MeshMapKind::ALL.contains(&MeshMapKind::Opacity));
    assert_eq!(MeshMapKind::from_key("opacity"), Some(MeshMapKind::Opacity));
}

#[test]
fn opacity_is_white_where_the_high_poly_covers_and_black_where_rays_miss() {
    // Low: a 2 x 2 plane (U follows X). High: a card over its left half only, just above it.
    let low = primitives::plane(2.0, 4);
    let mut b = MeshBuilder::new("card");
    let m = b.material("card");
    let c = |x: f32, z: f32| Corner { position: Vec3::new(x, 0.01, z), normal: Some(Vec3::Y), uv: Some(Vec2::ZERO) };
    b.push_triangle(m, [c(-1.0, -1.0), c(-1.0, 1.0), c(0.0, 1.0)]);
    b.push_triangle(m, [c(-1.0, -1.0), c(0.0, 1.0), c(0.0, -1.0)]);
    let high = b.build_for_baking().unwrap();
    let (lb, hb) = (Bvh::build(&low), Bvh::build(&high));
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &lb }, high: Some(Geometry { mesh: &high, bvh: &hb }), material_slot: 0 };
    let map = bake(&input, &settings(), &Progress::new()).unwrap().remove(&MeshMapKind::Opacity).unwrap();
    let (mut covered, mut open) = (0, 0);
    for t in texels(&low, 0, RES) {
        let v = map.get(t.index % RES, t.index / RES)[0];
        if t.position.x < -0.1 {
            assert_eq!(v, 1.0, "under the card at {}", t.position);
            covered += 1;
        } else if t.position.x > 0.1 {
            assert_eq!(v, 0.0, "beside the card at {}", t.position);
            open += 1;
        }
    }
    assert!(covered > 100 && open > 100, "{covered} {open}");
}

#[test]
fn opacity_low_on_low_is_fully_covered() {
    let low = primitives::plane(2.0, 4);
    let bvh = Bvh::build(&low);
    let input = BakeInput { low: Geometry { mesh: &low, bvh: &bvh }, high: None, material_slot: 0 };
    let map = bake(&input, &settings(), &Progress::new()).unwrap().remove(&MeshMapKind::Opacity).unwrap();
    for t in texels(&low, 0, RES) {
        assert_eq!(map.get(t.index % RES, t.index / RES)[0], 1.0);
    }
}
