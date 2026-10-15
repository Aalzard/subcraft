use glam::Vec2;
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{Layer, LayerContent, LayerKind, PenAnchor, PenPath, TextureSet};

fn path() -> PenPath {
    PenPath { anchors: vec![PenAnchor::new(Vec2::new(0.1, 0.5)), PenAnchor::new(Vec2::new(0.9, 0.5))], closed: false, radius: 0.02, opacity: 1.0, hardness: 1.0, values: [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))].into() }
}

#[test]
fn cubic_subdivision_contains_curve_and_closed_return_segment() {
    let mut path = path();
    assert_eq!(path.polyline(0.001).unwrap(), [Vec2::new(0.1, 0.5), Vec2::new(0.9, 0.5)]);
    path.anchors[0].outgoing = Vec2::new(0.0, -0.4);
    path.anchors[1].incoming = Vec2::new(0.0, -0.4);
    let points = path.polyline(0.001).unwrap();
    assert!(points.len() > 8); assert!(points.iter().any(|p| (p.x - 0.5).abs() < 0.01 && (p.y - 0.2).abs() < 0.01));
    path.closed = true; assert_eq!(path.polyline(0.001).unwrap().last(), Some(&path.anchors[0].position));
}

#[test]
fn validation_rejects_bad_identity_parameters_and_oversized_paths() {
    let original = path();
    let mut bad = original.clone(); bad.anchors[1].id = bad.anchors[0].id; assert!(bad.validate().is_err());
    let mut bad = original.clone(); bad.anchors[0].incoming.x = f32::NAN; assert!(bad.polyline(0.001).is_err());
    let mut bad = original.clone(); bad.opacity = 2.0; assert!(bad.validate().is_err());
    let mut bad = original.clone(); bad.radius = 0.0; assert!(bad.validate().is_err());
    let mut bad = original.clone(); bad.values.insert(ChannelKind::Height, ChannelValue::scalar(f32::MAX)); assert!(bad.validate().is_err());
    let mut bad = original; bad.anchors = (0..257).map(|_| PenAnchor::new(Vec2::ZERO)).collect(); assert!(bad.validate().is_err());
}

#[test]
fn duplicate_pen_layer_has_fresh_anchor_identity_with_shared_raster_cache() {
    let ts = TextureSet::new("Body", 0, 128); let mut layer = Layer::new_paint("Pen", &ts);
    let LayerContent::Paint(p) = &mut layer.content else { panic!() }; p.path = Some(path());
    p.channel_mut(ChannelKind::BaseColor, 128).set(20, 30, [1.0, 0.0, 0.0, 1.0]);
    let copy = layer.duplicate_with_new_ids(); assert_ne!(layer.id, copy.id); assert_eq!(copy.kind(), LayerKind::Path);
    let (LayerContent::Paint(a), LayerContent::Paint(b)) = (&layer.content, &copy.content) else { panic!() };
    assert_ne!(a.path.as_ref().unwrap().anchors[0].id, b.path.as_ref().unwrap().anchors[0].id);
    assert!(std::sync::Arc::ptr_eq(a.channels[&ChannelKind::BaseColor].tile(0).unwrap(), b.channels[&ChannelKind::BaseColor].tile(0).unwrap()));
}
