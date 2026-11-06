//! Issue #95: a mask stroke in the 3D view must stamp the whole brush footprint, like a content
//! stroke with the same brush, not only the stroke's centre line.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, LayerContent};
use subcraft_mesh::primitives;
use subcraft_paint::{BrushMaterial, PaintTarget};

const RES: u32 = 256;

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}

/// The issue's brush: Size 5 %, Flow/Opacity 100 %, Hardness 50 %, Spacing 0.1.
fn issue_brush(s: &mut Session) {
    s.brush.settings.size = 5.0;
    s.brush.settings.flow = 1.0;
    s.brush.settings.hardness = 0.5;
    s.brush.settings.spacing = 0.1;
}

fn stroke(s: &mut Session) {
    let cam = camera();
    let screen = |x: f32| -> Vec2 { cam.project(Vec3::new(x, 0.0, 0.0)).unwrap().truncate() };
    s.begin_stroke().unwrap();
    for i in 0..=24 {
        s.stroke_to(&cam, screen(-0.8 + 1.6 * i as f32 / 24.0), false).unwrap();
    }
    s.end_stroke();
}

/// Texels above `threshold` and the widest run of them in any column (the stroke runs along x).
fn coverage(get: impl Fn(u32, u32) -> f32, threshold: f32) -> (u32, u32) {
    let mut covered = 0;
    let mut widest = 0;
    for x in 0..RES {
        let column = (0..RES).filter(|&y| get(x, y) > threshold).count() as u32;
        covered += column;
        widest = widest.max(column);
    }
    (covered, widest)
}

#[test]
fn a_3d_mask_stroke_covers_the_brush_footprint_like_a_content_stroke() {
    // Content stroke on a paint layer: the reference footprint.
    let mut content = Session::new(primitives::plane(2.0, 1), RES);
    issue_brush(&mut content);
    content.brush.material = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    stroke(&mut content);
    let LayerContent::Paint(paint) = &content.selected_layer().unwrap().content else { panic!("a paint layer") };
    let image = &paint.channels[&ChannelKind::BaseColor];
    let (content_covered, content_width) = coverage(|x, y| image.get(x, y)[3], 0.5);
    assert!(content_width >= 4, "the reference stroke is several texels wide ({content_width})");

    // The issue's setup: a fill layer with a black mask, painting the mask with value 1.
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.6, 0.3, 0.1))) })
        .unwrap();
    s.execute(Command::AddMask { layer, white: false }).unwrap();
    s.execute(Command::SelectLayerTarget { layer, target: LayerTarget::Mask }).unwrap();
    assert_eq!(s.brush.target, PaintTarget::MaskBase);
    issue_brush(&mut s);
    s.brush.material.mask_value = 1.0;
    stroke(&mut s);
    let mask = &s.selected_layer().unwrap().mask.as_ref().unwrap().base;
    let (mask_covered, mask_width) = coverage(|x, y| mask.get(x, y)[0], 0.5);

    assert!(mask_width + 1 >= content_width, "the mask stroke is as wide as the content stroke ({mask_width} vs {content_width} texels)");
    assert!(mask_covered * 10 >= content_covered * 9, "the mask stroke covers as much as the content stroke ({mask_covered} vs {content_covered})");
}

/// Paint across the front of the sample crate (the app's start-up model) in the 3D view and count
/// covered texels of `read`, with `setup` choosing the layer and target.
fn crate_stroke(setup: impl Fn(&mut Session), read: impl Fn(&Session) -> u32) -> u32 {
    let mut s = Session::new(primitives::sample_crate(), 1024);
    setup(&mut s);
    issue_brush(&mut s);
    let b = s.scene().mesh.bounds;
    let center = b.center();
    let eye = center + Vec3::new(0.0, 0.2, 2.5) * b.diagonal();
    let cam = Camera::perspective(eye, center, Vec3::Y, 40.0, 0.01, 1000.0, [1280, 800]);
    let y = center.y;
    s.begin_stroke().unwrap();
    for i in 0..=40 {
        let x = b.min.x + (b.max.x - b.min.x) * (0.2 + 0.6 * i as f32 / 40.0);
        if let Some(p) = cam.project(Vec3::new(x, y, b.max.z)) {
            s.stroke_to(&cam, p.truncate(), false).unwrap();
        }
    }
    s.end_stroke();
    read(&s)
}

#[test]
fn on_the_sample_crate_a_mask_stroke_covers_as_much_as_a_paint_stroke() {
    let content = crate_stroke(
        |s| s.brush.material = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]),
        |s| {
            let LayerContent::Paint(paint) = &s.selected_layer().unwrap().content else { panic!() };
            let img = &paint.channels[&ChannelKind::BaseColor];
            (0..img.height()).map(|y| (0..img.width()).filter(|&x| img.get(x, y)[3] > 0.5).count() as u32).sum()
        },
    );
    let mask = crate_stroke(
        |s| {
            s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
            let layer = s.selection().layer.unwrap();
            s.execute(Command::AddMask { layer, white: false }).unwrap();
            s.execute(Command::SelectLayerTarget { layer, target: LayerTarget::Mask }).unwrap();
            s.brush.material.mask_value = 1.0;
        },
        |s| {
            let m = &s.selected_layer().unwrap().mask.as_ref().unwrap().base;
            (0..m.height()).map(|y| (0..m.width()).filter(|&x| m.get(x, y)[0] > 0.5).count() as u32).sum()
        },
    );
    println!("crate: content stroke {content} texels, mask stroke {mask} texels");
    assert!(content > 2000, "the paint stroke lands on the crate ({content})");
    assert!(mask * 10 >= content * 9, "the mask stroke covers as much ({mask} vs {content})");
}
