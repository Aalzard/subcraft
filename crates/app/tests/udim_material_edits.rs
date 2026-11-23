//! Issue #581 follow-up: with "Paint across UDIM tiles" on, a material dropped on the model is a
//! copy on every tile, and adjusting it (fill channels, projection, mask and content effects)
//! used to change only the selected tile's copy. Every edit now reaches the matching layer of
//! each tile, as one undo step, so one material controls the whole object.
use glam::{Vec2, Vec3};
use subcraft_app::new_project::NewProjectSettings;
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::{ContentEffectKind, FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Levels, MaskEffectKind};
use subcraft_mesh::{Corner, Mesh, MeshBuilder};

/// Two quads, the left on UDIM tile 1001, the right on 1002 (two texture sets of one group).
fn mesh() -> Mesh {
    let mut b = MeshBuilder::new("Strip");
    let m = b.material("Body");
    for (i, shift) in [Vec2::ZERO, Vec2::new(1.0, 0.0)].into_iter().enumerate() {
        let origin = Vec3::new(i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(shift + Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    b.build().unwrap()
}

/// A session with the option on and a fill layer on both tiles, selected on tile 1001.
fn session(across: bool) -> (Session, subcraft_core::LayerId) {
    let mut s = Session::from_mesh_with(mesh(), &NewProjectSettings { resolution: 128, ..Default::default() }).unwrap();
    assert_eq!(s.document().texture_sets.len(), 2, "two UDIM tile sets");
    if across {
        let first = s.document().texture_sets[0].id;
        s.execute(Command::SetPaintAcrossUdims { texture_set: first, on: true }).unwrap();
    }
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let top = s.selection().layer.unwrap();
    s.execute(Command::Seal).unwrap();
    (s, top)
}

fn top(s: &Session, set: usize) -> &Layer {
    s.document().texture_sets[set].layers.last().unwrap()
}

fn base_color(l: &Layer) -> Option<FillChannel> {
    let LayerContent::Fill(f) = &l.content else { panic!("fill layer") };
    f.channels.get(&K::BaseColor).cloned()
}

fn red(v: f32) -> FillChannel {
    FillChannel::new(FillSource::uniform(ChannelValue::rgb(v, 0.0, 0.0)))
}

fn mask_kinds(l: &Layer) -> Vec<String> {
    l.mask.as_ref().map_or_else(Vec::new, |m| m.effects.iter().map(|e| e.kind.label()).collect())
}

#[test]
fn a_fill_color_drag_changes_every_tile_in_one_undo_step() {
    let (mut s, layer) = session(true);
    let original = base_color(top(&s, 1));
    let before = s.history().entries().len();
    for i in 1..=10 {
        s.execute(Command::SetFillChannel { layer, channel: K::BaseColor, fill: red(i as f32 / 10.0) }).unwrap();
    }
    assert_eq!(s.history().entries().len(), before + 1, "the drag is one history entry");
    for set in 0..2 {
        assert_eq!(base_color(top(&s, set)), Some(red(1.0)), "tile {set} follows");
    }
    assert_eq!(s.selection().layer, Some(layer), "the selection stays on the first tile");
    s.execute(Command::Undo).unwrap();
    assert_eq!(base_color(top(&s, 1)), original, "undo reverts the other tile too");
}

#[test]
fn without_the_option_each_tile_keeps_its_own_material() {
    let (mut s, layer) = session(false);
    let other = s.document().texture_sets[1].clone();
    s.execute(Command::SetFillChannel { layer, channel: K::BaseColor, fill: red(0.5) }).unwrap();
    assert_eq!(base_color(top(&s, 0)), Some(red(0.5)));
    assert_eq!(s.document().texture_sets[1], other, "unchanged default: tiles are independent");
}

#[test]
fn mask_effects_are_added_edited_moved_and_removed_on_every_tile() {
    let (mut s, layer) = session(true);
    s.execute(Command::AddMask { layer, white: false }).unwrap();
    assert!(top(&s, 1).mask.is_some(), "masks already crossed tiles");
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)) }).unwrap();
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Levels(Levels::default()) }).unwrap();
    assert_eq!(mask_kinds(top(&s, 1)), mask_kinds(top(&s, 0)));
    assert_eq!(mask_kinds(top(&s, 1)).len(), 2);

    // Add above a selected effect in the middle: the other tile inserts at the same place.
    let generator = top(&s, 0).mask.as_ref().unwrap().effects[0].id;
    s.execute(Command::SelectEffect { layer, target: LayerTarget::Mask, effect: Some(generator) }).unwrap();
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Invert }).unwrap();
    assert_eq!(mask_kinds(top(&s, 0)), vec!["Generator (Edges)", "Invert", "Levels"]);
    assert_eq!(mask_kinds(top(&s, 1)), mask_kinds(top(&s, 0)), "same order on both tiles");

    // Edit the generator's level: the partner's copy (its own id) follows.
    let mut edited = (*top(&s, 0).mask.as_ref().unwrap().effects[0]).clone();
    let MaskEffectKind::Generator(g) = &mut edited.kind else { panic!() };
    g.level = 0.8;
    s.execute(Command::SetMaskEffect { layer, effect: edited }).unwrap();
    let partner = &top(&s, 1).mask.as_ref().unwrap().effects[0];
    assert!(matches!(&partner.kind, MaskEffectKind::Generator(g) if g.level == 0.8), "the other tile's generator follows");
    assert_ne!(partner.id, top(&s, 0).mask.as_ref().unwrap().effects[0].id, "each tile keeps its own ids");

    let invert = top(&s, 0).mask.as_ref().unwrap().effects[1].id;
    s.execute(Command::RemoveMaskEffect { layer, effect: invert }).unwrap();
    assert_eq!(mask_kinds(top(&s, 1)), vec!["Generator (Edges)", "Levels"]);
    s.execute(Command::Undo).unwrap();
    assert_eq!(mask_kinds(top(&s, 1)), vec!["Generator (Edges)", "Invert", "Levels"], "one undo step per edit");
}

#[test]
fn a_painted_mask_effect_keeps_each_tiles_own_strokes() {
    let (mut s, layer) = session(true);
    s.execute(Command::AddMask { layer, white: true }).unwrap();
    let image = subcraft_image::TiledImage::new(128, 128, 2, [0.0; 4]);
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Paint { image } }).unwrap();
    assert_eq!(mask_kinds(top(&s, 1)), vec!["Paint"]);
    // Paint the first tile's raster directly, then rename / reblend the effect.
    let mut edited = (*top(&s, 0).mask.as_ref().unwrap().effects[0]).clone();
    let MaskEffectKind::Paint { image } = &mut edited.kind else { panic!() };
    *image = subcraft_image::TiledImage::new(128, 128, 2, [1.0, 1.0, 0.0, 0.0]);
    edited.opacity = 0.5;
    s.execute(Command::SetMaskEffect { layer, effect: edited }).unwrap();
    let partner = &top(&s, 1).mask.as_ref().unwrap().effects[0];
    assert_eq!(partner.opacity, 0.5, "settings follow");
    let MaskEffectKind::Paint { image } = &partner.kind else { panic!() };
    assert_eq!(image.fill()[0], 0.0, "but not the other tile's painted texels");
}

#[test]
fn content_effects_and_projection_follow_too() {
    let (mut s, layer) = session(true);
    s.execute(Command::AddContentEffect { layer, kind: ContentEffectKind::Levels { channel: K::Roughness, levels: Levels::default() } }).unwrap();
    assert_eq!(top(&s, 1).content_effects.len(), 1);
    let mut e = (*top(&s, 0).content_effects[0]).clone();
    let ContentEffectKind::Levels { levels, .. } = &mut e.kind else { panic!() };
    levels.gamma = 2.0;
    s.execute(Command::SetContentEffect { layer, effect: e }).unwrap();
    assert!(matches!(&top(&s, 1).content_effects[0].kind, ContentEffectKind::Levels { levels, .. } if levels.gamma == 2.0));

    let LayerContent::Fill(f) = &top(&s, 0).content else { panic!() };
    let mut projection = f.projection.clone();
    projection.rotation_deg = 45.0;
    s.execute(Command::SetFillProjection { layer, projection: projection.clone() }).unwrap();
    let LayerContent::Fill(f1) = &top(&s, 1).content else { panic!() };
    assert_eq!(f1.projection, projection);
}

#[test]
fn a_material_dropped_onto_the_fill_layer_replaces_it_on_every_tile() {
    let (mut s, layer) = session(true);
    let preset = subcraft_assets::presets::materials().remove(0);
    let original = top(&s, 1).content.clone();
    let before = s.history().entries().len();
    s.apply_material_to_layer(layer, &preset).unwrap();
    assert_eq!(s.history().entries().len(), before + 1, "one undo step");
    assert_eq!(top(&s, 1).content, top(&s, 0).content, "the other tile's fill takes the same material");
    assert_eq!(top(&s, 1).name, top(&s, 0).name, "names stay paired");
    s.execute(Command::Undo).unwrap();
    assert_eq!(top(&s, 1).content, original, "undo reverts both tiles");
}

#[test]
fn procedural_material_sliders_drive_every_tile() {
    let (mut s, _) = session(true);
    let lib = subcraft_assets::Library::builtin();
    let material = lib.smart_materials.iter().find(|m| m.name == "Procedural Painted Metal").expect("on the shelf");
    s.add_smart_material(material).unwrap();
    let root = s.selection().layer.unwrap();
    let params = |s: &Session, set: usize| top(s, set).material.as_deref().expect("material on every tile").params.clone();
    let contents = |s: &Session, set: usize| top(s, set).children().iter().map(|c| c.content.clone()).collect::<Vec<_>>();
    let param = params(&s, 0)[0].clone();
    let value = match param.value {
        subcraft_doc::ParamValue::Number(v) => subcraft_doc::ParamValue::Number(if v > 0.5 { v - 0.3 } else { v + 0.3 }),
        subcraft_doc::ParamValue::Toggle(b) => subcraft_doc::ParamValue::Toggle(!b),
        subcraft_doc::ParamValue::Color(_) => subcraft_doc::ParamValue::Color([0.8, 0.1, 0.1]),
    };
    let before = s.history().entries().len();
    let untouched = contents(&s, 1);
    s.execute(Command::SetMaterialParam { layer: root, param: param.id.clone(), value }).unwrap();
    assert_eq!(s.history().entries().len(), before + 1, "one undo step");
    assert_eq!(params(&s, 1), params(&s, 0), "the other tile's material follows the slider");
    assert_eq!(contents(&s, 1), contents(&s, 0), "and so do the layers it drives");
    assert_ne!(contents(&s, 1), untouched, "the slider changed something");
    s.execute(Command::ResetMaterialParams(root)).unwrap();
    assert_eq!(params(&s, 1), params(&s, 0), "reset reaches the other tile too");
    assert_eq!(contents(&s, 1), untouched);
}
