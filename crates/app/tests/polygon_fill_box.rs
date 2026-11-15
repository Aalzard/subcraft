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
    use subcraft_paint::{PaintError, PaintTarget, StrokeMode};
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Paint)).unwrap();
    assert_eq!(s.stroke_blocker(StrokeMode::Paint), None, "a paint layer takes strokes");
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
    // A paint stroke on a fill layer paints on a new paint layer: nothing refuses it.
    assert_eq!(s.stroke_blocker(StrokeMode::Paint), None);
    assert_eq!(s.stroke_blocker(StrokeMode::Erase), Some(PaintError::NotAPaintLayer), "erasing a fill layer's content is refused");
    s.brush.target = PaintTarget::MaskBase;
    assert_eq!(s.stroke_blocker(StrokeMode::Paint), Some(PaintError::NoMask), "its mask, when it has none");
    // And the brush really does paint on a fill layer through a new paint layer.
    s.brush.target = PaintTarget::Content;
    let layers = s.selected_texture_set().layers.len();
    assert!(s.ensure_paint_layer().is_some());
    assert_eq!(s.selected_texture_set().layers.len(), layers + 1);
}

#[test]
fn the_colour_eyedropper_reads_the_base_colour_under_the_cursor() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    let cam = Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    let on = cam.project(Vec3::new(0.2, 0.2, 1.0)).unwrap().truncate();
    let picked = s.sample_colour_at(&cam, on).expect("the model is under the cursor");
    let ts = s.selected_texture_set().id;
    let uv = s.pick(&cam, on).unwrap().uv;
    assert_eq!(Some(picked), s.sample_uv(ts, uv, ChannelKind::BaseColor).map(|v| v.0), "the composite's base colour there");
    assert_eq!(s.sample_colour_at(&cam, Vec2::new(2.0, 2.0)), None, "off the model");
}

#[test]
fn a_fill_box_counts_what_it_would_fill_without_filling() {
    let s = Session::new(primitives::cube(2.0), 64);
    let cam = Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    let entries = s.history().entries().len();
    let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(512.0, 512.0));
    use subcraft_paint::PolygonHitTest as H;
    // The whole front face, visible only: 4 x 4 quads.
    assert_eq!(s.polygon_fill_box_count(&cam, a, b, PolygonFillMode::Polygon, true, H::Intersection), (16, 32));
    assert_eq!(s.polygon_fill_box_count(&cam, a, b, PolygonFillMode::Triangle, true, H::Intersection), (32, 32));
    assert_eq!(s.polygon_fill_box_count(&cam, a, b, PolygonFillMode::UvIsland, true, H::Intersection).0, 1);
    assert_eq!(s.polygon_fill_box_count(&cam, Vec2::ZERO, Vec2::splat(4.0), PolygonFillMode::Polygon, true, H::Intersection), (0, 0), "a box off the model");
    assert_eq!(s.history().entries().len(), entries, "counting fills nothing");
}
