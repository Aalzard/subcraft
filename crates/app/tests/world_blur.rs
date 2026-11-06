//! World-space blur of a paint layer: a radius in model units, one undo step, honest errors.
use glam::Vec3;
use subcraft_app::{Camera, Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_mesh::primitives;

fn painted() -> (Session, subcraft_core::LayerId) {
    let mut s = Session::new(primitives::cube(2.0), 128);
    s.brush.material = subcraft_paint::BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let layer = s.selection().layer.unwrap();
    let cam = Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.05, 100.0, [512, 512]);
    s.brush.settings.size = 8.0;
    s.brush.settings.hardness = 1.0;
    s.begin_stroke().unwrap();
    for k in 0..=4 {
        let p = cam.project(Vec3::new(-0.4 + k as f32 * 0.2, 0.0, 1.0)).unwrap().truncate();
        s.stroke_to(&cam, p, false).unwrap();
    }
    s.end_stroke();
    (s, layer)
}

fn alpha_stats(s: &Session, layer: subcraft_core::LayerId) -> (f32, f32, usize) {
    let LayerContent::Paint(p) = &s.selected_texture_set().find(layer).unwrap().content else { panic!() };
    let img = &p.channels[&ChannelKind::BaseColor];
    let c = img.comps();
    let a: Vec<f32> = img.to_dense().chunks_exact(c).map(|p| p[c - 1]).collect();
    (a.iter().sum(), a.iter().cloned().fold(0.0, f32::max), a.iter().filter(|&&v| v > 0.01).count())
}

#[test]
fn world_blur_spreads_the_paint_in_one_undo_step_and_keeps_its_total() {
    let (mut s, layer) = painted();
    let before = s.document().clone();
    let (sum0, max0, count0) = alpha_stats(&s, layer);
    assert!(count0 > 50 && max0 > 0.9, "the stroke painted something: {count0} {max0}");
    let entries = s.history().entries().len();
    s.world_blur_layer(layer, 0.15).unwrap();
    assert_eq!(s.history().entries().len(), entries + 1, "one undo step");
    let (sum1, max1, count1) = alpha_stats(&s, layer);
    assert!(count1 > count0 && max1 < max0, "the stroke widens and softens: {count0} -> {count1}, {max0} -> {max1}");
    // Texel density differs between cube faces and seams, but the painted amount stays the same order.
    assert!((sum1 / sum0 - 1.0).abs() < 0.35, "painted amount roughly kept: {sum0} -> {sum1}");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
}

#[test]
fn world_blur_refuses_bad_radii_non_paint_layers_and_empty_layers() {
    let (mut s, layer) = painted();
    let before = s.document().clone();
    for radius in [0.0, -1.0, f32::NAN, f32::INFINITY, 100.0] {
        assert!(s.world_blur_layer(layer, radius).is_err(), "radius {radius}");
    }
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let empty = s.selection().layer.unwrap();
    assert!(s.world_blur_layer(empty, 0.1).unwrap_err().to_string().contains("no painted channels"));
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let fill = s.selection().layer.unwrap();
    assert!(s.world_blur_layer(fill, 0.1).unwrap_err().to_string().contains("paint layer"));
    s.execute(Command::Undo).unwrap(); s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before, "failed blurs recorded nothing");
}
