//! Heal: a clone stroke that copies the source's detail onto the target's tone.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_paint::shape::Shape;
use subcraft_paint::{BrushMaterial, StrokeMode};

const RES: u32 = 256;

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}
fn screen(cam: &Camera, x: f32, z: f32) -> Vec2 {
    cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate()
}
fn grey(v: f32) -> BrushMaterial {
    BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(v, v, v)), (K::Roughness, ChannelValue::scalar(v))])
}
fn raster(s: &Session, k: K) -> TiledImage {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    p.channels[&k].clone()
}
fn at(img: &TiledImage, x: f32, z: f32) -> [f32; 4] {
    img.get(((x / 2.0 + 0.5) * RES as f32) as u32, ((z / 2.0 + 0.5) * RES as f32) as u32)
}

/// Light grey on the right, dark grey on the left with a small white dot at (-0.5, 0); then a
/// clone dab from the dot onto the light side at (0.5, 0).
fn cloned(heal: bool) -> (Session, TiledImage, TiledImage) {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    let cam = camera();
    s.brush.material = grey(0.8);
    s.fill_shape_projected(&cam, screen(&cam, -1.0, -1.0), screen(&cam, 1.0, 1.0), &Shape::default()).unwrap();
    s.brush.material = grey(0.2);
    s.fill_shape_projected(&cam, screen(&cam, -1.0, -1.0), screen(&cam, -0.05, 1.0), &Shape::default()).unwrap();
    s.brush.material = grey(1.0);
    s.brush.settings.size = 3.0;
    s.brush.settings.hardness = 1.0;
    s.begin_stroke().unwrap();
    s.stroke_to(&cam, screen(&cam, -0.5, 0.0), false).unwrap();
    s.end_stroke();
    let before = raster(&s, K::BaseColor);
    s.brush.mode = StrokeMode::Clone;
    s.brush.settings.size = 18.0;
    s.brush.settings.clone_heal = heal;
    assert!(s.set_clone_source(&cam, screen(&cam, -0.5, 0.0)));
    s.begin_stroke().unwrap();
    s.stroke_to(&cam, screen(&cam, 0.5, 0.0), false).unwrap();
    s.end_stroke();
    let after = raster(&s, K::BaseColor);
    (s, before, after)
}

#[test]
fn heal_copies_the_detail_but_keeps_the_target_tone() {
    let (_, before, plain) = cloned(false);
    let (s, _, healed) = cloned(true);
    // Plain clone carries the dark surroundings over; heal keeps the light tone around the dot.
    let around = (0.62, 0.0);
    assert!((at(&plain, around.0, around.1)[0] - 0.2).abs() < 0.03, "plain clone: {:?}", at(&plain, around.0, around.1));
    let v = at(&healed, around.0, around.1)[0];
    assert!((v - 0.8).abs() < 0.03, "healed surroundings keep the target tone: {v}");
    // The dot (the detail) is still there, brighter than its surroundings.
    let dot = at(&healed, 0.5, 0.0)[0];
    assert!(dot > v + 0.15, "the dot was carried over: dot {dot}, around {v}");
    // Data channels heal too (roughness follows the same greys).
    let r = at(&raster(&s, K::Roughness), around.0, around.1)[0];
    assert!((r - 0.8).abs() < 0.03, "roughness healed: {r}");
    // Outside the brush nothing changed.
    assert_eq!(at(&healed, 0.5, 0.7), at(&before, 0.5, 0.7));
}

#[test]
fn a_heal_stroke_is_one_undo_step_and_deterministic() {
    let (mut s, before, healed) = cloned(true);
    assert_eq!(healed, cloned(true).2);
    s.execute(Command::Undo).unwrap();
    assert_eq!(raster(&s, K::BaseColor), before);
}
