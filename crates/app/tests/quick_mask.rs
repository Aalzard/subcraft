//! Quick mask (Painter's U / Y / I) on a flat plane: edition paints the mask and leaves the
//! document alone; once closed, the mask protects what it covers from strokes and fills.
use glam::{Vec2, Vec3};
use subcraft_app::actions::Action;
use subcraft_app::drop_target::DropRegion;
use subcraft_app::{Camera, Session};
use subcraft_core::{ChannelKind as K, ChannelValue, PartKind};
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_paint::{BrushMaterial, StrokeMode};

const RES: u32 = 256;

fn session() -> Session {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s.brush.settings.size = 18.0;
    s.brush.settings.hardness = 1.0;
    s
}
fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}
fn screen(cam: &Camera, x: f32, z: f32) -> Vec2 { cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate() }
fn stroke(s: &mut Session, cam: &Camera, points: &[(f32, f32)]) {
    s.begin_stroke().unwrap();
    for &(x, z) in points { s.stroke_to(cam, screen(cam, x, z), false).unwrap(); }
    s.end_stroke();
}
/// Base color alpha of the selected layer at world `(x, z)` (0 where nothing was painted).
fn painted(s: &Session, x: f32, z: f32) -> f32 {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("a paint layer") };
    p.channels.get(&K::BaseColor).map_or(0.0, |img| at(img, x, z)[3])
}
fn at(img: &TiledImage, x: f32, z: f32) -> [f32; 4] {
    img.get(((x / 2.0 + 0.5) * RES as f32) as u32, ((z / 2.0 + 0.5) * RES as f32) as u32)
}
fn mask_at(s: &Session, x: f32, z: f32) -> f32 {
    s.quick_mask(s.selection().texture_set).map_or(0.0, |m| at(m, x, z)[0])
}

#[test]
fn edition_paints_the_mask_only_and_the_closed_mask_protects_what_it_covers() {
    let mut s = session();
    let cam = camera();
    let (doc, entries) = (s.document().clone(), s.history().entries().len());
    s.toggle_quick_mask();
    assert!(s.quick_mask_editing());
    stroke(&mut s, &cam, &[(-0.6, 0.0), (-0.4, 0.0), (-0.2, 0.0)]);
    assert_eq!(s.document(), &doc, "a quick mask stroke does not touch the document");
    assert_eq!(s.history().entries().len(), entries, "nor its undo history");
    assert!(mask_at(&s, -0.4, 0.0) > 0.99 && mask_at(&s, 0.6, 0.0) == 0.0, "the mask holds the stroke");

    s.toggle_quick_mask();
    assert!(!s.quick_mask_editing());
    stroke(&mut s, &cam, &[(-0.8, 0.0), (-0.4, 0.0), (0.0, 0.0), (0.4, 0.0), (0.8, 0.0)]);
    assert!(painted(&s, -0.4, 0.0) < 1e-3, "masked texels stay clean: {}", painted(&s, -0.4, 0.0));
    assert!(painted(&s, 0.6, 0.0) > 0.99, "free texels take the paint");
    assert_eq!(s.history().entries().len(), entries + 1);
}

#[test]
fn erase_in_edition_frees_and_polygon_fill_respects_the_mask() {
    let mut s = session();
    let cam = camera();
    s.toggle_quick_mask();
    // The brush reaches less than 0.5 world units (see the first test), so the far end of this
    // mask stroke stays out of reach of an eraser dab at its other end.
    stroke(&mut s, &cam, &[(-0.9, 0.0), (-0.6, 0.0), (-0.3, 0.0), (0.0, 0.0)]);
    s.brush.mode = StrokeMode::Erase;
    stroke(&mut s, &cam, &[(-0.1, 0.0)]);
    assert!(mask_at(&s, -0.1, 0.0) < 1e-3, "erase frees the texel: {}", mask_at(&s, -0.1, 0.0));
    assert!(mask_at(&s, -0.85, 0.0) > 0.99, "the rest of the mask stays: {}", mask_at(&s, -0.85, 0.0));
    s.brush.mode = StrokeMode::Paint;
    s.toggle_quick_mask();
    // A polygon fill over the whole plane skips the masked texels too.
    s.polygon_fill(&cam, screen(&cam, 0.5, 0.5), subcraft_paint::PolygonFillMode::MeshElement).unwrap();
    let (masked, freed, free) = (painted(&s, -0.85, 0.0), painted(&s, -0.1, 0.0), painted(&s, 0.6, 0.6));
    assert!(masked < 1e-3 && freed > 0.99 && free > 0.99, "{masked} {freed} {free}");
}

#[test]
fn invert_and_clear_act_only_while_editing_through_the_actions() {
    let mut s = session();
    let cam = camera();
    assert_eq!(s.perform(Action::QuickMaskInvert), Ok(false), "outside edition I is the part isolate key");
    assert!(s.quick_mask(s.selection().texture_set).is_none());
    s.perform(Action::QuickMaskToggle).unwrap();
    assert_eq!(s.perform(Action::QuickMaskInvert), Ok(true));
    assert_eq!(mask_at(&s, 0.3, 0.3), 1.0, "inverting no mask protects everything");
    s.perform(Action::QuickMaskToggle).unwrap();
    let entries = s.history().entries().len();
    stroke(&mut s, &cam, &[(0.0, 0.0), (0.3, 0.0)]);
    assert_eq!(painted(&s, 0.0, 0.0), 0.0);
    assert_eq!(s.history().entries().len(), entries, "a fully masked stroke changes nothing");
    s.perform(Action::QuickMaskToggle).unwrap();
    assert_eq!(s.perform(Action::QuickMaskClear), Ok(true));
    assert!(s.quick_mask(s.selection().texture_set).is_none());
    s.perform(Action::QuickMaskToggle).unwrap();
    stroke(&mut s, &cam, &[(0.0, 0.0), (0.3, 0.0)]);
    assert!(painted(&s, 0.0, 0.0) > 0.99, "a cleared mask protects nothing");
}

#[test]
fn ctrl_drop_region_is_the_part_and_its_outline_borders_the_plane() {
    let mut s = session();
    let cam = camera();
    let center = screen(&cam, 0.1, 0.1);
    let ts = s.selection().texture_set;
    assert_eq!(s.drop_region_at(&cam, center, None), Some(DropRegion::TextureSet(ts)));
    let Some(region @ DropRegion::Part(_, key)) = s.drop_region_at(&cam, center, Some(PartKind::Connected)) else { panic!("a part under the cursor") };
    assert!(s.drop_region_at(&cam, Vec2::new(2.0, 2.0), None).is_none(), "off the model");
    // The plane is one square: four border edges, all on its rim.
    let outline = s.region_outline(DropRegion::TextureSet(ts));
    assert_eq!(outline.len(), 4, "{outline:?}");
    assert!(outline.iter().flatten().all(|p| (p.x.abs() - 1.0).abs() < 1e-5 || (p.z.abs() - 1.0).abs() < 1e-5));
    assert_eq!(s.region_outline(region).len(), 4);
    // Restricting a new layer to the part: a part mask showing only that part.
    let layer = s.selection().layer.unwrap();
    s.restrict_drop_to_part(layer, region).unwrap();
    let parts = s.selected_layer().unwrap().parts.clone().expect("a part mask");
    assert!(parts.enabled && parts.kind == key.kind && parts.parts.iter().copied().eq([key.id]));
}
