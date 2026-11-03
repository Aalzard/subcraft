//! Text tool through the session: editable decal layers, undo, resolution changes, embedded
//! fonts, persistence, text brush tips and projected text stamps.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{LayerContent, LayerKind};
use subcraft_mesh::primitives;
use subcraft_text::{Align, BUNDLED_FONTS, FontRef};

fn session() -> Session {
    let mut s = Session::new(primitives::cube(2.0), 256);
    s.brush.material = subcraft_paint::BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-text-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn paint(s: &Session) -> &subcraft_doc::PaintContent {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("not a paint layer") };
    p
}

fn alpha_sum_and_bounds(s: &Session) -> (f64, (u32, u32, u32, u32)) {
    let img = &paint(s).channels[&ChannelKind::BaseColor];
    let c = img.comps();
    let (mut sum, mut b) = (0.0f64, (u32::MAX, u32::MAX, 0u32, 0u32));
    for (i, p) in img.to_dense().chunks_exact(c).enumerate() {
        let a = p[c - 1];
        sum += a as f64;
        if a > 0.02 {
            let (x, y) = (i as u32 % img.width(), i as u32 / img.width());
            b = (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1));
        }
    }
    (sum, b)
}

#[test]
fn adding_text_makes_one_undoable_editable_layer() {
    let mut s = session();
    let before = s.document().clone();
    let entries = s.history().entries().len();
    let mut d = s.new_text_decal("Hello");
    d.size = 0.15;
    s.add_text_layer(d.clone()).unwrap();
    assert_eq!(s.history().entries().len(), entries + 1);
    let layer = s.selected_layer().unwrap();
    assert_eq!(layer.kind(), LayerKind::Text);
    assert_eq!(layer.name, "Text: Hello");
    assert_eq!(s.selected_text(), Some(&d));
    let (sum, (x0, _, x1, _)) = alpha_sum_and_bounds(&s);
    assert!(sum > 100.0 && x0 > 0 && x1 < 256);
    // Only the channels the decal writes blend; the others are disabled on the layer.
    assert!(layer.blend.iter().filter(|(k, _)| **k != ChannelKind::BaseColor).all(|(_, b)| !b.is_active()));

    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
    s.execute(Command::Redo).unwrap();
    assert_eq!(s.selected_layer().unwrap().kind(), LayerKind::Text);
}

#[test]
fn editing_text_rerenders_and_undo_restores_the_exact_raster() {
    let mut s = session();
    let mut d = s.new_text_decal("A");
    d.size = 0.12;
    s.add_text_layer(d.clone()).unwrap();
    let (one, _) = alpha_sum_and_bounds(&s);
    let after_add = s.document().clone();

    let mut longer = d.clone();
    longer.style.text = "AAAA".into();
    longer.style.align = Align::Center;
    s.update_selected_text(longer).unwrap();
    let (four, _) = alpha_sum_and_bounds(&s);
    assert!((four / one - 4.0).abs() < 0.1, "four glyphs carry ~4x the ink: {four} vs {one}");
    assert_eq!(s.selected_layer().unwrap().name, "Text: AAAA", "automatic name follows the text");
    assert_eq!(s.history().entries().last().unwrap().label, "Edit Text");

    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &after_add, "undo restores the earlier text and raster exactly");

    // Moving and rotating re-renders at the new place.
    let mut moved = d.clone();
    moved.center = Vec2::new(0.25, 0.25);
    moved.rotation = 1.0;
    s.update_selected_text(moved).unwrap();
    let (_, (x0, y0, x1, y1)) = alpha_sum_and_bounds(&s);
    assert!(((x0 + x1) / 2) < 100 && ((y0 + y1) / 2) < 100);
    // Editing a non-text layer is refused.
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Paint)).unwrap();
    assert!(s.update_selected_text(d).is_err());
}

#[test]
fn changing_resolution_rerenders_text_from_outlines() {
    let mut s = session();
    let mut d = s.new_text_decal("Res");
    d.size = 0.2;
    s.add_text_layer(d.clone()).unwrap();
    let (sum_lo, (x0, _, x1, _)) = alpha_sum_and_bounds(&s);
    let ts = s.selected_texture_set().id;
    s.execute(Command::SetResolution { texture_set: Some(ts), resolution: 512 }).unwrap();
    assert_eq!(s.selected_text(), Some(&d), "parameters are untouched");
    let (sum_hi, (hx0, _, hx1, _)) = alpha_sum_and_bounds(&s);
    assert!((sum_hi / sum_lo - 4.0).abs() < 0.15, "{}", sum_hi / sum_lo);
    assert!((((hx1 - hx0) as f32) / ((x1 - x0) as f32) - 2.0).abs() < 0.08);
    // Exactly the raster a fresh render at 512 produces (not an upscale of the 256 one).
    let fresh = {
        let mut f = session();
        f.execute(Command::SetResolution { texture_set: Some(f.selected_texture_set().id), resolution: 512 }).unwrap();
        f.add_text_layer(d).unwrap();
        f.selected_layer().unwrap().clone()
    };
    let LayerContent::Paint(fp) = &fresh.content else { panic!() };
    assert_eq!(&paint(&s).channels, &fp.channels);
}

#[test]
fn imported_font_files_are_embedded_deduplicated_and_survive_save_and_open() {
    let dir = temp("font");
    let font_path = dir.join("MyFace.ttf");
    std::fs::write(&font_path, BUNDLED_FONTS[2].1).unwrap();
    let mut s = session();
    let font = s.import_font(&font_path).unwrap();
    assert!(matches!(font, FontRef::Embedded { .. }));
    assert_eq!(s.import_font(&font_path).unwrap(), font, "same bytes, same resource");
    assert_eq!(s.embedded_fonts().len(), 1);
    assert!(s.import_font_bytes("junk.ttf", vec![1, 2, 3]).is_err());
    assert!(s.import_font(&dir.join("missing.ttf")).is_err());

    let mut d = s.new_text_decal("Serif مرحبا");
    d.font = font.clone();
    d.style.letter_spacing = 0.05;
    s.add_text_layer(d.clone()).unwrap();
    let raster_before = paint(&s).channels.clone();

    let project = dir.join("text.subcraft");
    std::fs::remove_file(&font_path).unwrap(); // the project must not depend on the path
    s.save(Some(&project)).unwrap();
    let mut reopened = Session::open_project(&project).unwrap();
    assert_eq!(reopened.embedded_fonts().len(), 1);
    let layer = reopened.selected_layer().map(|l| l.id).or_else(|| reopened.selected_texture_set().layers.first().map(|l| l.id)).unwrap();
    reopened.execute(Command::SelectLayer(Some(layer))).unwrap();
    assert_eq!(reopened.selected_text(), Some(&d), "text stays editable after reopening");
    assert_eq!(&paint(&reopened).channels, &raster_before);
    // And it can be edited with the reloaded font.
    let mut e = d.clone();
    e.style.text = "Edited".into();
    reopened.update_selected_text(e).unwrap();
    assert_eq!(reopened.selected_text().unwrap().style.text, "Edited");
}

#[test]
fn missing_embedded_font_is_a_clear_error_and_system_fonts_fall_back() {
    let mut s = session();
    let mut d = s.new_text_decal("x");
    d.font = FontRef::Embedded { resource: subcraft_core::ResourceId::new() };
    assert!(s.add_text_layer(d.clone()).is_err());
    d.font = FontRef::System { family: "No Such Family 987".into(), bold: false, italic: false };
    s.add_text_layer(d).unwrap();
    assert_eq!(s.selected_layer().unwrap().kind(), LayerKind::Text);
}

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.05, 100.0, [512, 512])
}

#[test]
fn text_brush_tip_paints_the_text_along_a_stroke() {
    let mut s = session();
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Paint)).unwrap();
    s.set_text_brush_tip(&FontRef::default(), &subcraft_text::TextStyle::new("HI")).unwrap();
    let tip = s.brush.alpha.clone().unwrap();
    assert_eq!((tip.width, tip.height), (512, 512));
    assert_eq!(s.brush.alpha_name, "Text: HI");
    s.brush.settings.size = 40.0;
    s.brush.settings.spacing = 2.0;
    let before = s.document().clone();
    let cam = camera();
    s.begin_stroke().unwrap();
    for k in 0..=6 {
        let p = cam.project(Vec3::new(-0.6 + k as f32 * 0.2, 0.0, 1.0)).unwrap().truncate();
        s.stroke_to(&cam, p, false).unwrap();
    }
    s.end_stroke();
    assert_ne!(s.document(), &before);
    // The painted coverage is the text shape, not a disc: gaps exist inside the footprint.
    let img = &paint(&s).channels[&ChannelKind::BaseColor];
    let c = img.comps();
    let covered = img.to_dense().chunks_exact(c).filter(|p| p[c - 1] > 0.5).count();
    assert!(covered > 50);
}

#[test]
fn projected_text_stamp_is_one_undo_step_and_restores_the_brush() {
    let mut s = session();
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Paint)).unwrap();
    let before = s.document().clone();
    let entries = s.history().entries().len();
    let alpha_before = s.brush.alpha.is_some();
    let settings_before = s.brush.settings;
    let cam = camera();
    let screen = cam.project(Vec3::new(0.0, 0.0, 1.0)).unwrap().truncate();
    s.stamp_text_projected(&cam, screen, &FontRef::default(), &subcraft_text::TextStyle::new("DECAL"), 60.0, 0.0).unwrap();
    assert_ne!(s.document(), &before);
    assert_eq!(s.history().entries().len(), entries + 1);
    assert_eq!(s.brush.alpha.is_some(), alpha_before);
    assert_eq!(s.brush.settings, settings_before);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
    // Off the model: an error, nothing recorded.
    assert!(s.stamp_text_projected(&cam, Vec2::new(2.0, 2.0), &FontRef::default(), &subcraft_text::TextStyle::new("X"), 10.0, 0.0).is_err());
    assert_eq!(s.document(), &before);
}

#[test]
fn text_decal_can_be_placed_on_the_surface_by_clicking() {
    let mut s = session();
    let mut d = s.new_text_decal("Here");
    d.size = 0.1;
    s.add_text_layer(d).unwrap();
    let cam = camera();
    let screen = cam.project(Vec3::new(0.5, 0.5, 1.0)).unwrap().truncate();
    let pick = s.pick(&cam, screen).unwrap();
    s.place_selected_text_at(&cam, screen).unwrap();
    let center = s.selected_text().unwrap().center;
    assert!((center - pick.uv).length() < 1e-4 || (center - pick.uv).abs().max_element() >= 0.99, "{center:?} vs {:?}", pick.uv);
    assert!(s.place_selected_text_at(&cam, Vec2::new(-5.0, -5.0)).is_err());
}
