use glam::{Vec2, Vec3};
use subcraft_core::MeshMapKind;
use subcraft_mesh::{Bvh, Corner, MeshBuilder};

use super::*;

const RES: u32 = 32;

/// AO in the middle of a 2 x 2 floor (facing up) under a card 0.1 above it that also faces up,
/// so the floor's rays reach the card's back.
fn ao_under_card(ao: AoOptions, card_faces_down: bool) -> f32 {
    let mut b = MeshBuilder::new("scene");
    let floor = b.material("floor");
    let card = b.material("card");
    let c = |x: f32, y: f32, z: f32, n: Vec3| Corner { position: Vec3::new(x, y, z), normal: Some(n), uv: Some(Vec2::new((x + 1.0) / 2.0, (z + 1.0) / 2.0)) };
    let up = Vec3::Y;
    b.push_triangle(floor, [c(-1.0, 0.0, -1.0, up), c(-1.0, 0.0, 1.0, up), c(1.0, 0.0, 1.0, up)]);
    b.push_triangle(floor, [c(-1.0, 0.0, -1.0, up), c(1.0, 0.0, 1.0, up), c(1.0, 0.0, -1.0, up)]);
    let (n, y) = (if card_faces_down { -up } else { up }, 0.1);
    let mut quad = [[c(-1.0, y, -1.0, n), c(-1.0, y, 1.0, n), c(1.0, y, 1.0, n)], [c(-1.0, y, -1.0, n), c(1.0, y, 1.0, n), c(1.0, y, -1.0, n)]];
    if card_faces_down {
        for t in &mut quad { t.swap(1, 2); }
    }
    for t in quad { b.push_triangle(card, t); }
    let mesh = b.build_for_baking().unwrap();
    assert!(mesh.geometric_normal(2).y * if card_faces_down { -1.0 } else { 1.0 } > 0.9, "card winding");
    let bvh = Bvh::build(&mesh);
    let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
    let s = BakeSettings { size: RES, maps: vec![MeshMapKind::AmbientOcclusion], ao_samples: 64, ao_distance: 0.5, use_gpu: false, ao, ..BakeSettings::default() };
    bake(&input, &s, &Progress::new()).unwrap()[&MeshMapKind::AmbientOcclusion].get(RES / 2, RES / 2)[0]
}

#[test]
fn ignore_backface_lets_ao_rays_through_the_back_of_faces() {
    let on = AoOptions { ignore_backface: true, ..Default::default() };
    assert!(!on.is_plain(), "the GPU kernels do not know it: the CPU bakes it");
    // The card's back faces the floor: it occludes unless backfaces are ignored.
    let dark = ao_under_card(AoOptions::default(), false);
    assert!(dark < 0.2, "the card occludes: {dark}");
    let open = ao_under_card(on, false);
    assert!(open > 0.99, "ignored backface: {open}");
    // A card facing the floor still occludes with the option on.
    let facing = ao_under_card(on, true);
    assert!(facing < 0.2, "front faces always occlude: {facing}");
}
