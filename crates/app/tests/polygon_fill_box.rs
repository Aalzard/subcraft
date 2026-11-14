//! Box polygon fill through the session: one undo step in 3D and in the 2D view.

use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::LayerContent;
use subcraft_mesh::primitives;
use subcraft_paint::PolygonFillMode;

fn base_color_tiles(s: &Session) -> usize {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("paint layer expected") };
    p.channels.get(&ChannelKind::BaseColor).map_or(0, |i| i.allocated_tiles())
}

#[test]
fn a_box_fill_in_the_3d_view_is_one_undo_step() {
    let mut s = Session::new(primitives::cube(2.0), 256);
    let cam = Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    let before = s.document().clone();
    let entries = s.history().entries().len();
    let tiles = s.polygon_fill_box(&cam, Vec2::new(100.0, 100.0), Vec2::new(400.0, 400.0), PolygonFillMode::Polygon, true).unwrap();
    assert!(!tiles.is_empty());
    assert!(base_color_tiles(&s) > 0);
    assert_eq!(s.history().entries().len(), entries + 1, "one history entry");
    assert_eq!(s.history().entries().last().unwrap().label, "Polygon Fill");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before, "undo restores the document exactly");
    s.execute(Command::Redo).unwrap();
    assert!(base_color_tiles(&s) > 0);
}

#[test]
fn a_box_fill_in_the_2d_view_is_one_undo_step_and_misses_fill_nothing() {
    let mut s = Session::new(primitives::cube(2.0), 256);
    let before = s.document().clone();
    let entries = s.history().entries().len();
    assert!(s.polygon_fill_uv_box(Vec2::new(5.0, 5.0), Vec2::new(6.0, 6.0), PolygonFillMode::UvIsland).unwrap().is_empty());
    assert_eq!(s.history().entries().len(), entries, "an empty box adds no history");
    let tiles = s.polygon_fill_uv_box(Vec2::new(0.0, 0.0), Vec2::new(0.2, 0.2), PolygonFillMode::UvIsland).unwrap();
    assert!(!tiles.is_empty());
    assert_eq!(s.history().entries().len(), entries + 1);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
}

#[test]
fn a_loop_fill_in_the_3d_view_is_one_undo_step() {
    let mut s = Session::new(primitives::cube(2.0), 256);
    let cam = Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    let before = s.document().clone();
    let entries = s.history().entries().len();
    let a = cam.project(Vec3::new(-0.25, 0.25, 1.0)).unwrap().truncate();
    let b = cam.project(Vec3::new(0.6, 0.25, 1.0)).unwrap().truncate();
    assert!(s.polygon_fill_loop(&cam, Vec2::new(1.0, 1.0), Vec2::new(30.0, 1.0), PolygonFillMode::Triangle, false, false).unwrap().is_empty());
    assert_eq!(s.history().entries().len(), entries, "a miss adds no history");
    let tiles = s.polygon_fill_loop(&cam, a, b, PolygonFillMode::Triangle, false, false).unwrap();
    assert!(!tiles.is_empty() && base_color_tiles(&s) > 0);
    assert_eq!(s.history().entries().len(), entries + 1);
    assert_eq!(s.history().entries().last().unwrap().label, "Polygon Fill");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
}

#[test]
fn the_stroke_blocker_names_why_a_stroke_would_be_refused() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Paint)).unwrap();
    assert_eq!(s.stroke_blocker(), None, "a paint layer takes strokes");
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
    assert_eq!(s.stroke_blocker(), Some(subcraft_paint::PaintError::NotAPaintLayer), "a fill layer does not");
    s.brush.target = subcraft_paint::PaintTarget::MaskBase;
    assert_eq!(s.stroke_blocker(), Some(subcraft_paint::PaintError::NoMask), "its mask, when it has one");
}
