//! Content effect stacks through the session: commands, typed effect selection, strokes routed
//! into Paint effects (content and mask), undo/redo and atomic rejection.

use glam::{Vec2, Vec3};
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue, EffectId, LayerId};
use subcraft_doc::{ContentEffectKind, FillChannel, FillSource, Levels, MaskEffectKind};
use subcraft_paint::{Camera, PaintTarget};

const RES: u32 = 64;

/// A fill layer with uniform red base color on a cube.
fn setup() -> (Session, LayerId) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let id = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: id, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 0.0, 0.0))) })
        .unwrap();
    (s, id)
}

fn camera() -> Camera {
    Camera::orthographic(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y, 1.3, 0.01, 20.0, [512, 512])
}

/// One stroke across the cube's front face with the current brush.
fn stroke(s: &mut Session) {
    s.brush.settings.size = 8.0;
    s.brush.settings.hardness = 1.0;
    s.begin_stroke().unwrap();
    for i in 0..24 {
        s.stroke_to(&camera(), Vec2::new(150.0 + 9.0 * i as f32, 256.0), false).unwrap();
    }
    s.end_stroke();
}

fn base_color(s: &mut Session) -> subcraft_image::TiledImage {
    s.update_composites();
    s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap().clone()
}

fn count(img: &subcraft_image::TiledImage, f: impl Fn([f32; 4]) -> bool) -> usize {
    (0..RES).flat_map(|y| (0..RES).map(move |x| (x, y))).filter(|&(x, y)| f(img.get(x, y))).count()
}

fn green(p: [f32; 4]) -> bool {
    p[1] > 0.9 && p[0] < 0.1
}

fn effects(s: &Session, layer: LayerId) -> Vec<EffectId> {
    s.document().texture_sets[0].find(layer).unwrap().content_effects.iter().map(|e| e.id).collect()
}

#[test]
fn painting_a_selected_paint_effect_on_a_fill_layer_is_one_undo_step() {
    let (mut s, id) = setup();
    let fill_before = s.selected_layer().unwrap().content.clone();
    s.execute(Command::AddContentEffect { layer: id, kind: ContentEffectKind::Paint { channels: Default::default() } }).unwrap();
    let effect = effects(&s, id)[0];
    // Adding selects the effect and routes strokes into it; selection itself is not history.
    assert_eq!(s.selected_effect(), Some(effect));
    assert_eq!(s.brush.target, PaintTarget::ContentEffect(effect));
    assert_eq!(s.layer_target(), LayerTarget::Content);

    let mut material = s.brush.material.clone();
    material.channels.get_mut(&ChannelKind::BaseColor).unwrap().value = ChannelValue::rgb(0.0, 1.0, 0.0);
    s.execute(Command::SetBrushMaterial(material)).unwrap();
    assert_eq!(count(&base_color(&mut s), green), 0);
    let history = s.history().entries().len();
    stroke(&mut s);
    assert_eq!(s.history().entries().len(), history + 1, "one entry per stroke");
    assert_eq!(s.history().entries().last().unwrap().label, "Paint Effect Stroke");
    let painted = count(&base_color(&mut s), green);
    assert!(painted > 20, "the stroke shows over the fill: {painted} green texels");
    // The fill itself is untouched; the paint lives in the effect.
    assert_eq!(s.selected_layer().unwrap().content, fill_before);

    s.execute(Command::Undo).unwrap();
    assert_eq!(count(&base_color(&mut s), green), 0);
    s.execute(Command::Redo).unwrap();
    assert_eq!(count(&base_color(&mut s), green), painted);
    // Disabling the effect hides its paint without losing it.
    let mut e = (*s.selected_layer().unwrap().content_effects[0]).clone();
    e.visible = false;
    s.execute(Command::SetContentEffect { layer: id, effect: e }).unwrap();
    assert_eq!(count(&base_color(&mut s), green), 0);
    s.execute(Command::Undo).unwrap();
    assert_eq!(count(&base_color(&mut s), green), painted);
}

#[test]
fn effect_lifecycle_reorder_duplicate_remove_and_selection_follow_undo() {
    let (mut s, id) = setup();
    let levels = ContentEffectKind::Levels { channel: ChannelKind::Roughness, levels: Levels { out_low: 0.5, ..Levels::default() } };
    s.execute(Command::AddContentEffect { layer: id, kind: ContentEffectKind::Paint { channels: Default::default() } }).unwrap();
    s.execute(Command::AddContentEffect { layer: id, kind: levels.clone() }).unwrap();
    let [paint, lv] = effects(&s, id)[..] else { panic!() };
    // A non-paint effect is selected for Properties; strokes go to the layer's own content.
    assert_eq!(s.selected_effect(), Some(lv));
    assert_eq!(s.brush.target, PaintTarget::Content);

    s.execute(Command::MoveContentEffect { layer: id, effect: lv, index: 0 }).unwrap();
    assert_eq!(effects(&s, id), vec![lv, paint]);
    s.execute(Command::DuplicateContentEffect { layer: id, effect: lv }).unwrap();
    let after = effects(&s, id);
    assert_eq!(after.len(), 3);
    assert!(after[1] != lv && after[0] == lv && after[2] == paint);
    let copy = s.selected_layer().unwrap().content_effects[1].clone();
    assert_eq!(copy.kind, levels);
    assert_eq!(copy.name, "Levels (Roughness) copy");

    // Invalid edits are rejected atomically: no history entry, no change.
    let history = s.history().entries().len();
    let doc = s.document().clone();
    assert!(s.execute(Command::MoveContentEffect { layer: id, effect: lv, index: 9 }).is_err());
    assert!(s.execute(Command::RemoveContentEffect { layer: id, effect: EffectId::new() }).is_err());
    assert!(s.execute(Command::AddContentEffect { layer: id, kind: ContentEffectKind::Levels { channel: ChannelKind::Normal, levels: Levels::default() } }).is_err());
    assert!(s.execute(Command::SelectEffect { layer: id, target: LayerTarget::Content, effect: Some(EffectId::new()) }).is_err());
    assert_eq!(s.document(), &doc);
    assert_eq!(s.history().entries().len(), history);

    // Selecting the Paint effect routes strokes; removing it (or undoing its creation) falls
    // back to the layer content.
    s.execute(Command::SelectEffect { layer: id, target: LayerTarget::Content, effect: Some(paint) }).unwrap();
    assert_eq!(s.brush.target, PaintTarget::ContentEffect(paint));
    assert_eq!(s.history().entries().len(), history, "selection is not an undo step");
    s.execute(Command::RemoveContentEffect { layer: id, effect: paint }).unwrap();
    assert_eq!(s.selected_effect(), None);
    assert_eq!(s.brush.target, PaintTarget::Content);
    s.execute(Command::Undo).unwrap();
    s.execute(Command::SelectEffect { layer: id, target: LayerTarget::Content, effect: Some(paint) }).unwrap();
    for _ in 0..4 {
        s.execute(Command::Undo).unwrap();
    }
    assert!(effects(&s, id).is_empty());
    assert_eq!(s.selected_effect(), None);
    assert_eq!(s.brush.target, PaintTarget::Content);

    // Folders are rejected for now.
    s.execute(Command::AddLayer(NewLayer::Folder)).unwrap();
    let folder = s.selection().layer.unwrap();
    assert!(s.execute(Command::AddContentEffect { layer: folder, kind: levels }).is_err());
}

#[test]
fn mask_paint_effect_receives_strokes_separately_from_the_mask_base() {
    let (mut s, id) = setup();
    s.execute(Command::AddMask { layer: id, white: false }).unwrap();
    s.execute(Command::AddMaskEffect { layer: id, kind: MaskEffectKind::Paint { image: subcraft_image::TiledImage::new(RES, RES, 2, [0.0; 4]) } })
        .unwrap();
    let effect = s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].id;
    assert_eq!(s.layer_target(), LayerTarget::Mask);
    assert_eq!(s.selected_effect(), Some(effect));
    assert_eq!(s.brush.target, PaintTarget::MaskEffect(effect));
    let base = s.selected_layer().unwrap().mask.as_ref().unwrap().base.clone();
    let red = |p: [f32; 4]| p[0] > 0.9 && p[1] < 0.1;
    assert_eq!(count(&base_color(&mut s), red), 0, "black mask hides the fill");
    stroke(&mut s);
    assert_eq!(s.history().entries().last().unwrap().label, "Mask Stroke");
    let revealed = count(&base_color(&mut s), red);
    assert!(revealed > 20, "white paint in the effect reveals the fill: {revealed}");
    assert_eq!(s.selected_layer().unwrap().mask.as_ref().unwrap().base, base, "mask base untouched");
    s.execute(Command::Undo).unwrap();
    assert_eq!(count(&base_color(&mut s), red), 0);
    // Choosing the mask itself paints the base again.
    s.execute(Command::SelectEffect { layer: id, target: LayerTarget::Mask, effect: None }).unwrap();
    assert_eq!(s.brush.target, PaintTarget::MaskBase);
    assert_eq!(s.selected_effect(), None);
}
