//! Erase / Smudge along path: the path runs once as a brush stroke on the paint layer below.
use glam::Vec2;
use subcraft_app::Session;
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::{LayerContent, PenAnchor, PenPath};
use subcraft_paint::{BrushMaterial, StrokeMode};

const RES: u32 = 256;

fn path() -> PenPath {
    PenPath {
        anchors: vec![PenAnchor::new(Vec2::new(0.2, 0.5)), PenAnchor::new(Vec2::new(0.8, 0.5))],
        closed: false, radius: 0.02, hardness: 1.0, opacity: 1.0,
        values: [(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))].into(),
        kind: Default::default(), stamp: None, ribbon: None,
    }
}

fn alpha_at(s: &Session, x: f32, y: f32) -> f32 {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("a paint layer") };
    p.channels.get(&K::BaseColor).map_or(0.0, |img| img.get((x * RES as f32) as u32, (y * RES as f32) as u32)[3])
}

#[test]
fn erase_along_a_path_removes_paint_on_the_layer_in_one_undo_step() {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(0.0, 0.0, 1.0))]);
    s.brush.settings.hardness = 1.0;
    let layer = s.selection().layer.unwrap();
    // Paint along the path first (the same helper in Paint mode), then erase along it.
    s.stroke_along_path(layer, &path(), StrokeMode::Paint).unwrap();
    assert!(alpha_at(&s, 0.5, 0.5) > 0.9, "painted on the path");
    assert_eq!(alpha_at(&s, 0.5, 0.9), 0.0, "and nowhere else");
    let entries = s.history().entries().len();
    s.stroke_along_path(layer, &path(), StrokeMode::Erase).unwrap();
    assert!(alpha_at(&s, 0.5, 0.5) < 0.1, "erased along the path: {}", alpha_at(&s, 0.5, 0.5));
    assert_eq!(s.history().entries().len(), entries + 1, "one undo step");
    assert_eq!(s.brush.mode, StrokeMode::Paint, "the brush keeps its mode");
    assert_eq!(s.selection().layer, Some(layer));
    s.execute(subcraft_app::Command::Undo).unwrap();
    assert!(alpha_at(&s, 0.5, 0.5) > 0.9, "undo brings the paint back");
}
