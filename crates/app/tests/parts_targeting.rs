//! Restricting paint and fills to a part, a UV island or the whole object, as in Painter:
//! polygon fill on a fill layer / folder fills its mask, geometry masks edited by box and in UV,
//! Alt+H hides excluded geometry, and the quick mask (U / Y / I).

use glam::Vec2;
use subcraft_app::{Action, Command, LayerTarget, NewLayer, PartKey, Session};
use subcraft_core::{ChannelKind, PartKind};
use subcraft_doc::{LayerContent, PartMask};
use subcraft_mesh::primitives;
use subcraft_paint::{PaintTarget, PolygonFillMode, StrokeMode};

/// A cube: each face is its own UV island.
fn session() -> Session {
    Session::new(primitives::cube(2.0), 64)
}

fn island_uv(s: &Session, id: u32) -> Vec2 {
    s.target_for_part(PartKey { kind: PartKind::UvIsland, id }).unwrap().uv
}

fn mask_at(s: &Session, uv: Vec2) -> f32 {
    let mask = s.selected_layer().unwrap().mask.as_ref().expect("a mask");
    let p = (uv * mask.base.width() as f32).as_uvec2();
    mask.base.get(p.x, p.y)[0]
}

fn paint_alpha(s: &Session, uv: Vec2) -> f32 {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("paint layer") };
    let Some(img) = p.channels.get(&ChannelKind::BaseColor) else { return 0.0 };
    let at = (uv * img.width() as f32).as_uvec2();
    img.get(at.x, at.y)[img.comps() - 1]
}

/// Fill the UV island under `uv` (on the layer, or on the quick mask while it is edited).
fn fill(s: &mut Session, uv: Vec2) {
    s.brush.mode = StrokeMode::Paint;
    s.polygon_fill_uv(uv, PolygonFillMode::UvIsland).unwrap();
}

/// A small 2D dab at `uv`.
fn dab_uv(s: &mut Session, uv: Vec2) {
    s.brush.mode = StrokeMode::Paint;
    s.brush.settings.size = 2.0;
    s.brush.settings.flow = 1.0;
    s.brush.settings.hardness = 1.0;
    s.brush.settings.stroke_opacity = 1.0;
    s.begin_uv_stroke(None).unwrap();
    s.stroke_to_uv(uv, false, 1.0).unwrap();
    s.end_stroke();
}

#[test]
fn polygon_fill_on_a_fill_layer_fills_its_new_black_mask_in_one_undo_step() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    assert!(s.selected_layer().unwrap().mask.is_none());
    assert_eq!(s.polygon_fill_destination(), Some(LayerTarget::Mask));
    let entries = s.history().entries().len();
    let (a, b) = (island_uv(&s, 0), island_uv(&s, 1));
    s.brush.mode = StrokeMode::Paint;
    assert!(!s.polygon_fill_uv(a, PolygonFillMode::UvIsland).unwrap().is_empty());
    assert_eq!(s.history().entries().len(), entries + 1, "mask and fill are one step");
    assert_eq!(s.brush.target, PaintTarget::MaskBase, "the mask stays selected, as in Painter");
    assert_eq!(mask_at(&s, a), 1.0, "the clicked island shows the fill");
    assert_eq!(mask_at(&s, b), 0.0, "every other island is masked out");
    s.perform(Action::Undo).unwrap();
    assert!(s.selected_layer().unwrap().mask.is_none(), "undo removes the added mask too");
}

#[test]
fn a_polygon_fill_that_fills_nothing_leaves_no_mask_and_no_history() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let entries = s.history().entries().len();
    let doc = s.document().clone();
    s.brush.mode = StrokeMode::Paint;
    let tiles = s.polygon_fill_uv(Vec2::new(-0.5, -0.5), PolygonFillMode::UvIsland).unwrap();
    assert!(tiles.is_empty());
    assert_eq!(s.document(), &doc);
    assert_eq!(s.history().entries().len(), entries);
    assert_eq!(s.brush.target, PaintTarget::Content);
}

#[test]
fn polygon_fill_on_a_folder_fills_the_folder_mask() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Folder)).unwrap();
    let a = island_uv(&s, 2);
    s.brush.mode = StrokeMode::Paint;
    assert!(!s.polygon_fill_uv(a, PolygonFillMode::UvIsland).unwrap().is_empty());
    assert_eq!(mask_at(&s, a), 1.0);
    assert_eq!(mask_at(&s, island_uv(&s, 3)), 0.0);
}

#[test]
fn polygon_fill_on_a_paint_layer_still_paints_its_content() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    assert_eq!(s.polygon_fill_destination(), Some(LayerTarget::Content));
    let a = island_uv(&s, 0);
    s.brush.mode = StrokeMode::Paint;
    s.polygon_fill_uv(a, PolygonFillMode::UvIsland).unwrap();
    assert!(s.selected_layer().unwrap().mask.is_none());
    assert!(paint_alpha(&s, a) > 0.99);
}

#[test]
fn geometry_mask_edits_by_uv_click_and_box() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetParts { layer, parts: Some(PartMask::for_kind(PartKind::UvIsland, [])) }).unwrap();
    let a = island_uv(&s, 0);
    let part = s.part_at_uv(a).expect("an island under its own centre");
    assert_eq!(part, 0);
    assert!(s.part_at_uv(Vec2::new(-1.0, 0.5)).is_none());
    // A box over the whole texture touches every island of the texture set.
    let all = s.parts_in_uv_box(Vec2::ZERO, Vec2::ONE);
    assert_eq!(all.len(), 6, "{all:?}");
    let entries = s.history().entries().len();
    s.set_layer_parts(layer, &all, true).unwrap();
    assert_eq!(s.history().entries().len(), entries + 1);
    assert_eq!(s.selected_layer().unwrap().parts.as_ref().unwrap().parts, all);
    s.set_layer_parts(layer, &[part].into(), false).unwrap();
    let parts = &s.selected_layer().unwrap().parts.as_ref().unwrap().parts;
    assert_eq!(parts.len(), 5);
    assert!(!parts.contains(&part));
}

#[test]
fn excluding_from_a_layer_without_a_geometry_mask_starts_from_every_part() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    // A mask that is switched off counts as none: excluding starts from every island again.
    s.execute(Command::SetParts { layer, parts: Some(PartMask { enabled: false, ..PartMask::for_kind(PartKind::UvIsland, [3]) }) }).unwrap();
    s.set_layer_parts(layer, &[0].into(), false).unwrap();
    let p = s.selected_layer().unwrap().parts.clone().unwrap();
    assert!(p.enabled && !p.parts.contains(&0));
    assert_eq!(p.parts.len(), 5, "the other islands stay covered: {:?}", p.parts);
}

#[test]
fn alt_h_hides_what_the_geometry_mask_leaves_out_and_follows_the_mask() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let layer = s.selection().layer.unwrap();
    assert!(!s.has_excluded_geometry());
    s.perform(Action::HideExcludedGeometry).unwrap();
    assert!(s.hide_excluded());
    assert_eq!(s.viewport_scene().mesh.indices, s.scene().mesh.indices, "nothing excluded yet: nothing hidden");
    s.execute(Command::SetParts { layer, parts: Some(PartMask::for_kind(PartKind::UvIsland, [0])) }).unwrap();
    s.sync_excluded_view();
    let view = &s.viewport_scene().mesh;
    let mesh = &s.scene().mesh;
    let hidden = (0..mesh.triangle_count()).filter(|&t| view.indices[t * 3..t * 3 + 3] != mesh.indices[t * 3..t * 3 + 3]).count();
    let kept = s.parts(PartKind::UvIsland).iter().find(|p| p.id == 0).unwrap().triangles;
    assert_eq!(hidden, mesh.triangle_count() - kept, "only island 0 stays");
    s.perform(Action::HideExcludedGeometry).unwrap();
    s.sync_excluded_view();
    assert_eq!(s.viewport_scene().mesh.indices, s.scene().mesh.indices);
}

#[test]
fn quick_mask_edits_without_touching_the_document_then_restricts_strokes() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let (a, b) = (island_uv(&s, 0), island_uv(&s, 1));
    s.perform(Action::QuickMaskToggle).unwrap();
    assert!(s.quick_mask_editing());
    let doc = s.document().clone();
    let entries = s.history().entries().len();
    fill(&mut s, a);
    assert_eq!(s.document(), &doc, "a quick mask stroke leaves the layers alone");
    assert_eq!(s.history().entries().len(), entries, "and adds no history step");
    let q = s.quick_mask().unwrap();
    let at = |uv: Vec2| { let p = (uv * q.image.width() as f32).as_uvec2(); q.image.get(p.x, p.y)[0] };
    assert!(at(a) > 0.99 && at(b) == 0.0);
    // Applied: paint lands only where the quick mask was painted.
    s.perform(Action::QuickMaskToggle).unwrap();
    assert!(!s.quick_mask_editing() && s.applied_quick_mask().is_some());
    dab_uv(&mut s, a);
    fill(&mut s, b);
    assert!(paint_alpha(&s, a) > 0.99);
    assert_eq!(paint_alpha(&s, b), 0.0, "outside the quick mask nothing is painted");
    // Inverted: the other way round.
    assert!(s.perform(Action::QuickMaskInvert).unwrap());
    s.perform(Action::Undo).unwrap();
    dab_uv(&mut s, b);
    fill(&mut s, a);
    assert!(paint_alpha(&s, b) > 0.99);
    assert_eq!(paint_alpha(&s, a), 0.0);
    // Cleared: everywhere again.
    s.perform(Action::QuickMaskClear).unwrap();
    assert!(s.quick_mask().is_none());
}

/// QA 2026-11: the quick mask kept its old size after a resolution change: applied, strokes
/// ignored it (protected areas were painted); edited, every stroke failed.
#[test]
fn the_quick_mask_follows_the_resolution_of_its_texture_set() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let (a, b) = (island_uv(&s, 0), island_uv(&s, 1));
    s.perform(Action::QuickMaskToggle).unwrap();
    fill(&mut s, a);
    s.perform(Action::QuickMaskToggle).unwrap();
    let ts = s.selection().texture_set;
    s.execute(Command::SetResolution { texture_set: Some(ts), resolution: 128 }).unwrap();
    assert_eq!(s.quick_mask().unwrap().image.width(), 128, "resampled with its set");
    dab_uv(&mut s, b);
    assert_eq!(paint_alpha(&s, b), 0.0, "the protected area stays protected at the new size");
    dab_uv(&mut s, a);
    assert!(paint_alpha(&s, a) > 0.99);
    // Back across the resolution change: the mask follows again, and editing it still paints.
    for _ in 0..8 {
        if s.document().texture_set(ts).unwrap().resolution == 64 { break; }
        s.execute(Command::Undo).unwrap();
    }
    assert_eq!(s.document().texture_set(ts).unwrap().resolution, 64);
    assert_eq!(s.quick_mask().unwrap().image.width(), 64);
    s.perform(Action::QuickMaskToggle).unwrap();
    assert!(s.quick_mask_editing());
    fill(&mut s, b);
}

#[test]
fn leaving_an_empty_quick_mask_removes_it_and_polygon_fill_paints_the_quick_mask_while_editing() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    s.perform(Action::QuickMaskToggle).unwrap();
    s.perform(Action::QuickMaskToggle).unwrap();
    assert!(s.quick_mask().is_none(), "nothing painted: no restriction left behind");
    assert!(!s.perform(Action::QuickMaskInvert).unwrap(), "nothing to invert");
    s.perform(Action::QuickMaskToggle).unwrap();
    let doc = s.document().clone();
    s.brush.mode = StrokeMode::Paint;
    assert!(!s.polygon_fill_uv(island_uv(&s, 0), PolygonFillMode::UvIsland).unwrap().is_empty());
    assert_eq!(s.document(), &doc, "the fill layer gets no mask while the quick mask is edited");
    assert!(s.quick_mask().unwrap().image.allocated_tiles() > 0);
}
