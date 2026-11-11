//! Clone source behaviour (relative / absolute) and per-channel erasing, through the session.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, CloneSourceMode, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_paint::{BrushMaterial, StrokeMode};

const RES: u32 = 256;

fn session() -> Session {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(0.1, 0.9, 0.1))]);
    s.brush.settings.size = 18.0;
    s.brush.settings.hardness = 1.0;
    s
}
fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}
fn screen(cam: &Camera, x: f32, z: f32) -> Vec2 { cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate() }
fn stroke(s: &mut Session, cam: &Camera, points: &[(f32, f32)]) {
    s.begin_stroke().unwrap();
    for &(x, z) in points { s.stroke_to(cam, screen(cam, x, z), false).unwrap(); }
    s.end_stroke();
}
fn raster(s: &Session, k: K) -> TiledImage {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    p.channels[&k].clone()
}
fn at(img: &TiledImage, x: f32, z: f32) -> [f32; 4] {
    img.get(((x / 2.0 + 0.5) * RES as f32) as u32, ((z / 2.0 + 0.5) * RES as f32) as u32)
}

/// One green dab at (-0.5, 0), cloned by two strokes: at (0.5, 0) and then at (0.5, 0.7).
fn second_clone(mode: CloneSourceMode) -> ([f32; 4], [f32; 4]) {
    let mut s = session();
    let cam = camera();
    stroke(&mut s, &cam, &[(-0.5, 0.0)]);
    s.brush.mode = StrokeMode::Clone;
    s.brush.clone_mode = mode;
    assert!(s.set_clone_source(&cam, screen(&cam, -0.5, 0.0)));
    stroke(&mut s, &cam, &[(0.5, 0.0)]);
    stroke(&mut s, &cam, &[(0.5, 0.7)]);
    let img = raster(&s, K::BaseColor);
    (at(&img, 0.5, 0.0), at(&img, 0.5, 0.7))
}

#[test]
fn relative_source_follows_the_brush_and_absolute_returns_to_where_it_was_set() {
    assert_eq!(Session::new(primitives::plane(2.0, 1), 64).brush.clone_mode, CloneSourceMode::Relative, "relative is the default");
    // Relative: the second stroke copies from beside itself, (-0.5, 0.7), where nothing is painted.
    let (first, second) = second_clone(CloneSourceMode::Relative);
    assert!(first[3] > 0.95 && first[1] > 0.5, "{first:?}");
    assert!(second[3] < 1e-4, "relative: an empty source copies nothing: {second:?}");
    // Absolute: the second stroke starts from the source point again and copies the green dab.
    let (first, second) = second_clone(CloneSourceMode::Absolute);
    assert!(first[3] > 0.95 && first[1] > 0.5, "{first:?}");
    assert!(second[3] > 0.95 && second[1] > 0.5 && second[0] < 0.3, "absolute: the dab is cloned again: {second:?}");
}

#[test]
fn absolute_mode_forgets_the_offset_of_the_previous_stroke_only_when_a_stroke_starts() {
    let mut s = session();
    let cam = camera();
    stroke(&mut s, &cam, &[(-0.5, 0.0)]);
    s.brush.mode = StrokeMode::Clone;
    s.brush.clone_mode = CloneSourceMode::Absolute;
    assert!(s.set_clone_source(&cam, screen(&cam, -0.5, 0.0)));
    stroke(&mut s, &cam, &[(0.5, 0.0)]);
    let first = s.clone_offset().expect("the stroke fixed an offset");
    stroke(&mut s, &cam, &[(0.5, 0.7)]);
    let second = s.clone_offset().expect("so did the next one");
    assert!((first - second).length() > 50.0, "each stroke measures its own offset: {first:?} {second:?}");
    // Back to relative: the offset of the last stroke is kept from then on.
    s.brush.clone_mode = CloneSourceMode::Relative;
    stroke(&mut s, &cam, &[(0.2, 0.2)]);
    assert_eq!(s.clone_offset(), Some(second));
}

#[test]
fn the_eraser_removes_only_the_channels_that_are_switched_on() {
    let mut s = session();
    let cam = camera();
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(0.8, 0.2, 0.2)), (K::Roughness, ChannelValue::scalar(0.3))]);
    stroke(&mut s, &cam, &[(-0.3, 0.0), (0.0, 0.0), (0.3, 0.0)]);
    let (color, rough) = (raster(&s, K::BaseColor), raster(&s, K::Roughness));
    assert!(at(&color, 0.0, 0.0)[3] > 0.99 && at(&rough, 0.0, 0.0)[1] > 0.99, "both channels were painted");

    // Erase with only Roughness on.
    s.brush.mode = StrokeMode::Erase;
    s.brush.material.disable(K::BaseColor);
    let entries = s.history().entries().len();
    stroke(&mut s, &cam, &[(-0.3, 0.0), (0.0, 0.0), (0.3, 0.0)]);
    assert_eq!(raster(&s, K::BaseColor), color, "the switched-off channel is untouched, texel for texel");
    let erased = raster(&s, K::Roughness);
    assert!(at(&erased, 0.0, 0.0)[1] < 1e-3, "roughness is erased: {:?}", at(&erased, 0.0, 0.0));
    assert_eq!(at(&erased, 0.9, 0.9), at(&rough, 0.9, 0.9));
    assert_eq!(s.history().entries().len(), entries + 1);

    // With every channel off there is nothing to erase.
    s.brush.material.disable(K::Roughness);
    assert_eq!(s.begin_stroke().unwrap_err(), subcraft_paint::PaintError::NothingToPaint);
}
