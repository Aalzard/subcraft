//! 2D-view tool routing in the session: clone source picked in the 2D view, 2D clone / smudge
//! strokes as one undo step each, and the eyedropper reading the texture under a UV.

use glam::Vec2;
use subcraft_app::{Command, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_mesh::primitives;
use subcraft_paint::{PaintError, StrokeMode};

fn session() -> Session {
    Session::new(primitives::sample_crate(), 256)
}

/// UV centroids of two different triangles of the selected texture set.
fn two_uvs(s: &Session) -> (Vec2, Vec2) {
    let slot = s.selected_texture_set().material_slot;
    let mesh = s.scene().mesh.clone();
    let mut tris = mesh.material_triangles(slot);
    let c = |t: usize| {
        let (q, _) = subcraft_mesh::uv_raster::wrap_to_tile(mesh.tri_uvs(t));
        (q[0] + q[1] + q[2]) / 3.0
    };
    let a = c(tris.next().unwrap());
    let b = tris.map(c).find(|b| (*b - a).length() > 0.05).unwrap();
    (a, b)
}

fn base_color(s: &Session, uv: Vec2) -> [f32; 4] {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("paint layer") };
    let img = &p.channels[&ChannelKind::BaseColor];
    let at = (uv * img.width() as f32).as_uvec2();
    let px = img.get(at.x, at.y);
    [px[0], px[1], px[2], px[img.comps() - 1]]
}

fn paint_dot(s: &mut Session, uv: Vec2, rgb: [f32; 3]) {
    let mut material = s.brush.material.clone();
    material.set(ChannelKind::BaseColor, ChannelValue::rgb(rgb[0], rgb[1], rgb[2]));
    s.execute(Command::SetBrushMaterial(material)).unwrap();
    let settings = subcraft_paint::BrushSettings { size: 3.0, hardness: 1.0, flow: 1.0, stroke_opacity: 1.0, ..s.brush.settings };
    s.execute(Command::SetBrushSettings(settings)).unwrap();
    s.brush.mode = StrokeMode::Paint;
    s.begin_uv_stroke(None).unwrap();
    s.stroke_to_uv(uv, false, 1.0).unwrap();
    s.end_stroke();
}

#[test]
fn a_2d_clone_needs_a_source_and_leaves_no_trace_when_refused() {
    let mut s = session();
    s.ensure_paint_layer();
    let before = s.document().clone();
    s.brush.mode = StrokeMode::Clone;
    assert!(matches!(s.begin_uv_stroke(None), Err(PaintError::NoCloneSource)));
    assert_eq!(s.document(), &before);
    assert!(!s.set_clone_source_uv(Vec2::new(-0.5, 2.0)), "outside the texture: no source");
    assert!(s.clone_source_uv().is_none());
}

#[test]
fn the_2d_clone_source_is_also_the_3d_source_and_a_2d_clone_copies_the_texture() {
    let mut s = session();
    s.ensure_paint_layer();
    let (source, target) = two_uvs(&s);
    paint_dot(&mut s, source, [0.0, 0.0, 1.0]);
    assert!(s.set_clone_source_uv(source));
    assert_eq!(s.clone_source_uv(), Some(source));
    assert!(s.clone_source().is_some(), "the 3D view clones from the same point");
    let entries = s.history().entries().len();
    let mut material = s.brush.material.clone();
    material.set(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0));
    s.execute(Command::SetBrushMaterial(material)).unwrap();
    s.brush.mode = StrokeMode::Clone;
    s.begin_uv_stroke(None).unwrap();
    s.stroke_to_uv(target, false, 1.0).unwrap();
    s.end_stroke();
    assert_eq!(s.history().entries().len(), entries + 1, "one undo step");
    assert!((s.clone_offset_uv().unwrap() - (source - target)).length() < 1e-4);
    let c = base_color(&s, target);
    assert!(c[3] > 0.5 && c[2] > 0.5 && c[0] < 0.1, "the blue texture is copied, not the red brush colour: {c:?}");
}

#[test]
fn a_2d_smudge_stroke_does_not_paint_the_brush_colour() {
    let mut s = session();
    s.ensure_paint_layer();
    let (a, _) = two_uvs(&s);
    paint_dot(&mut s, a, [1.0, 0.0, 0.0]);
    let mut material = s.brush.material.clone();
    material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.0, 1.0, 0.0));
    s.execute(Command::SetBrushMaterial(material)).unwrap();
    s.brush.mode = StrokeMode::Smudge;
    s.brush.settings.smudge_material = 0.0;
    s.begin_uv_stroke(None).unwrap();
    for k in 0..10 {
        s.stroke_to_uv(a + Vec2::new(k as f32 * 0.002, 0.0), false, 1.0).unwrap();
    }
    s.end_stroke();
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    let img = &p.channels[&ChannelKind::BaseColor];
    let mut green = 0.0f32;
    for y in 0..img.height() {
        for x in 0..img.width() {
            let px = img.get(x, y);
            green = green.max(px[1] * px[img.comps() - 1]);
        }
    }
    assert!(green < 0.05, "smudge must not deposit the green brush colour (max green {green})");
}

#[test]
fn the_eyedropper_reads_the_texture_under_a_2d_position() {
    let mut s = session();
    s.ensure_paint_layer();
    let (a, _) = two_uvs(&s);
    paint_dot(&mut s, a, [0.2, 0.6, 0.9]);
    let ts = s.selected_texture_set().id;
    let mut material = s.brush.material.clone();
    material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.0, 0.0, 0.0));
    s.execute(Command::SetBrushMaterial(material)).unwrap();
    s.sample_material_at(ts, a, false);
    let picked = s.brush.material.channels[&ChannelKind::BaseColor].value.0;
    assert!((picked[0] - 0.2).abs() < 0.05 && (picked[1] - 0.6).abs() < 0.05 && (picked[2] - 0.9).abs() < 0.05, "{picked:?}");
}
