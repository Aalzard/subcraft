//! Painter's "continue the previous stroke" (A): the stroke-opacity cap spans both strokes.
use glam::{Vec2, Vec3};
use subcraft_app::{Action, Camera, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_mesh::primitives;
use subcraft_paint::BrushMaterial;

const RES: u32 = 128;

fn session() -> Session {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s.brush.settings.size = 10.0;
    s.brush.settings.hardness = 1.0;
    s.brush.settings.flow = 1.0;
    s.brush.settings.stroke_opacity = 0.5;
    s
}

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}

/// One stroke across the middle of the plane.
fn stroke(s: &mut Session, cam: &Camera) {
    let screen = |x: f32| -> Vec2 { cam.project(Vec3::new(x, 0.0, 0.0)).unwrap().truncate() };
    s.begin_stroke().unwrap();
    for i in 0..=12 {
        s.stroke_to(cam, screen(-0.5 + i as f32 / 12.0), false).unwrap();
    }
    s.end_stroke();
}

/// Paint alpha in the middle of the plane.
fn middle(s: &Session) -> f32 {
    let LayerContent::Paint(l) = &s.selected_layer().unwrap().content else { panic!() };
    l.channels[&K::BaseColor].get(RES / 2, RES / 2)[3]
}

#[test]
fn a_second_stroke_builds_up_but_a_continued_one_stays_under_the_stroke_opacity() {
    let cam = camera();
    let mut s = session();
    stroke(&mut s, &cam);
    let one = middle(&s);
    assert!((one - 0.5).abs() < 0.01, "one stroke reaches the stroke opacity: {one}");
    assert!(s.can_continue_stroke(false) && !s.can_continue_stroke(true), "3D strokes continue in the 3D view");

    // A new stroke over it: a second half-opacity layer of paint.
    let mut plain = session();
    stroke(&mut plain, &cam);
    stroke(&mut plain, &cam);
    assert!((middle(&plain) - 0.75).abs() < 0.02, "{}", middle(&plain));

    // Continued: the cap applies to both strokes together.
    let steps = s.history().entries().len();
    s.set_continue_stroke(true);
    stroke(&mut s, &cam);
    assert!((middle(&s) - 0.5).abs() < 0.01, "a continued stroke stays at the stroke opacity: {}", middle(&s));
    assert!(s.history().entries().len() <= steps + 1, "still at most one undo step per stroke");
    // The flag is used once.
    stroke(&mut s, &cam);
    assert!((middle(&s) - 0.75).abs() < 0.02, "the stroke after that is a new one: {}", middle(&s));
}

#[test]
fn a_changed_brush_or_an_undo_starts_a_new_stroke_instead() {
    let cam = camera();
    let mut s = session();
    stroke(&mut s, &cam);
    s.brush.settings.size = 12.0;
    assert!(!s.can_continue_stroke(false), "another brush size is another stroke");
    s.set_continue_stroke(true);
    stroke(&mut s, &cam);
    assert!(middle(&s) > 0.7, "painted as a new stroke: {}", middle(&s));

    let mut s = session();
    stroke(&mut s, &cam);
    s.perform(Action::Undo).unwrap();
    assert!(!s.can_continue_stroke(false), "the stroke was undone");
    s.set_continue_stroke(true);
    stroke(&mut s, &cam);
    assert!((middle(&s) - 0.5).abs() < 0.01, "a fresh stroke on the cleared layer: {}", middle(&s));
}
