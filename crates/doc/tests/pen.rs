use glam::Vec2;
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{Layer, LayerContent, LayerKind, PenAnchor, PenPath, TextureSet};

fn path() -> PenPath {
    PenPath { anchors: vec![PenAnchor::new(Vec2::new(0.1, 0.5)), PenAnchor::new(Vec2::new(0.9, 0.5))], closed: false, radius: 0.02, opacity: 1.0, hardness: 1.0, values: [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))].into(), kind: Default::default(), stamp: None, ribbon: None }
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

#[test]
fn a_stamped_path_checks_its_spacing_and_keeps_its_tip_with_the_project() {
    let mut p = path();
    p.stamp = Some(subcraft_doc::PathStamp::default());
    assert!(p.validate().is_ok());
    p.stamp = Some(subcraft_doc::PathStamp { spacing: 9.0, ..Default::default() });
    assert!(p.validate().is_err(), "spacing beyond 400 %");
    let tip = subcraft_core::ResourceId::new();
    p.stamp = Some(subcraft_doc::PathStamp { alpha: Some(tip), ..Default::default() });
    let ts = subcraft_doc::TextureSet::new("Body", 0, 64);
    let mut layer = subcraft_doc::Layer::new_paint("Pen path", &ts);
    let subcraft_doc::LayerContent::Paint(content) = &mut layer.content else { unreachable!() };
    content.path = Some(p);
    let mut used = Vec::new();
    layer.collect_resources(&mut used);
    assert_eq!(used, vec![tip], "saving keeps the stamped tip image");
}

#[test]
fn a_ribbon_checks_its_tiling_and_its_image_is_kept_with_the_project() {
    let mut p = path();
    p.kind = subcraft_doc::PathKind::Ribbon;
    p.ribbon = Some(subcraft_doc::PathRibbon { tiles: 0.0, ..Default::default() });
    assert!(p.validate().is_err(), "tiling below 0.05");
    let (image, tip) = (subcraft_core::ResourceId::new(), subcraft_core::ResourceId::new());
    p.ribbon = Some(subcraft_doc::PathRibbon { image: Some(image), ..Default::default() });
    p.stamp = Some(subcraft_doc::PathStamp { alpha: Some(tip), ..Default::default() });
    assert!(p.validate().is_ok());
    assert_eq!(p.image_resource(), Some(image), "a ribbon draws with its image");
    p.kind = subcraft_doc::PathKind::Paint;
    assert_eq!(p.image_resource(), Some(tip), "a stamped stroke with its tip");
    let ts = TextureSet::new("Body", 0, 64);
    let mut layer = Layer::new_paint("Pen path", &ts);
    let LayerContent::Paint(content) = &mut layer.content else { unreachable!() };
    content.path = Some(p);
    let mut used = Vec::new();
    layer.collect_resources(&mut used);
    assert_eq!(used, vec![tip, image], "both images survive a save, whichever type is shown");
}

#[test]
fn splitting_a_curved_segment_keeps_its_shape() {
    let mut p = path();
    p.anchors[0].outgoing = Vec2::new(0.1, -0.3);
    p.anchors[1].incoming = Vec2::new(-0.2, 0.25);
    let cubic = |c: [Vec2; 4], t: f32| { let u = 1.0 - t; c[0] * (u * u * u) + c[1] * (3.0 * u * u * t) + c[2] * (3.0 * u * t * t) + c[3] * (t * t * t) };
    let whole = p.segment(0).unwrap();
    let id = p.split_segment(0, 0.3).unwrap();
    assert_eq!(p.anchors.len(), 3);
    assert_eq!(p.anchors[1].id, id);
    assert!((p.anchors[1].position - cubic(whole, 0.3)).length() < 1e-6, "the new point is on the curve");
    let (left, right) = (p.segment(0).unwrap(), p.segment(1).unwrap());
    for k in 0..=10 {
        let t = k as f32 / 10.0;
        assert!((cubic(left, t) - cubic(whole, 0.3 * t)).length() < 1e-5, "left half at {t}");
        assert!((cubic(right, t) - cubic(whole, 0.3 + 0.7 * t)).length() < 1e-5, "right half at {t}");
    }
    assert!(p.is_smooth(id), "the split point carries the curve's tangents");
    assert!(p.split_segment(5, 0.5).is_none(), "no such segment");
}

