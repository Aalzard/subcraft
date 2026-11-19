//! In-place shelf application (Alt readings) and the New Text Layer action.

use std::sync::Arc;
use subcraft_app::actions::{Action, Key, Shortcut, action_for, KeymapPreset};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_assets::BrushAlpha;
use subcraft_core::AssetId;
use subcraft_doc::{LayerContent, MaskEffectKind};
use subcraft_image::Bitmap;
use subcraft_mesh::primitives;

fn session() -> Session {
    Session::new(primitives::cube(2.0), 64)
}

#[test]
fn new_text_layer_action_adds_one_text_layer_and_has_a_shortcut() {
    let mut s = session();
    let n = s.selected_texture_set().layers.len();
    s.perform(Action::NewTextLayer).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), n + 1);
    assert!(s.selected_text().is_some(), "the new layer is a text layer");
    assert_eq!(action_for(KeymapPreset::SubCraft, Shortcut::ctrl_shift(Key::T)), Some(Action::NewTextLayer));
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), n);
}

#[test]
fn material_replaces_a_fill_layer_in_place_and_refuses_a_paint_layer() {
    let mut s = session();
    let preset = subcraft_assets::presets::materials().remove(0);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::Rename { layer, name: "Keep me".into() }).unwrap();
    let count = s.selected_texture_set().layers.len();
    s.apply_material_to_layer(layer, &preset).unwrap();
    let l = s.selected_layer().unwrap();
    assert_eq!(l.id, layer);
    assert_eq!(l.name, "Keep me", "name is kept");
    assert_eq!(s.selected_texture_set().layers.len(), count, "no layer created");
    assert!(matches!(l.content, LayerContent::Fill(_)));

    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let paint = s.selection().layer.unwrap();
    assert!(s.apply_material_to_layer(paint, &preset).is_err());
}

#[test]
fn alpha_dropped_on_a_mask_becomes_an_image_effect_and_the_round_tip_is_refused() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    let bitmap = Bitmap::new_f32(4, 4, 1, (0..16).map(|i| i as f32 / 15.0).collect());
    let alpha = BrushAlpha { id: AssetId::from_name("test-alpha"), name: "Test alpha".into(), bitmap: Some(Arc::new(bitmap)), user_file: None };
    assert!(s.selected_layer().unwrap().mask.is_none());
    s.apply_alpha_to_mask(layer, &alpha).unwrap();
    let mask = s.selected_layer().unwrap().mask.as_ref().expect("mask added");
    assert!(mask.effects.iter().any(|e| matches!(e.kind, MaskEffectKind::Image { .. })));
    s.execute(Command::Undo).unwrap();
    assert!(s.selected_layer().unwrap().mask.is_none(), "one undo step");

    let round = BrushAlpha { bitmap: None, ..alpha };
    assert!(s.apply_alpha_to_mask(layer, &round).is_err());
}

#[test]
fn alpha_dropped_on_a_channel_row_becomes_that_channels_image() {
    use subcraft_core::ChannelKind;
    use subcraft_doc::{FillChannel, FillSource};
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    // A strength set beforehand is kept, as for a texture dropped on the row.
    let before = FillChannel { strength: 0.4, ..FillChannel::new(FillSource::uniform(subcraft_core::ChannelValue::scalar(0.5))) };
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Roughness, fill: before.clone() }).unwrap();
    // (Another edit in between: consecutive edits of one channel share an undo step.)
    s.execute(Command::Rename { layer, name: "Alpha target".into() }).unwrap();
    // 2x1 picture: black on the left, white on the right.
    let alpha = BrushAlpha { id: AssetId::from_name("test-alpha"), name: "Test alpha".into(), bitmap: Some(Arc::new(Bitmap::new_f32(2, 1, 1, vec![0.0, 1.0]))), user_file: None };
    let channel = |s: &Session, k: ChannelKind| match &s.selected_layer().unwrap().content {
        LayerContent::Fill(f) => f.channels[&k].clone(),
        _ => panic!("fill layer"),
    };

    s.set_fill_channel_alpha(layer, ChannelKind::Roughness, &alpha).unwrap();
    let after = channel(&s, ChannelKind::Roughness);
    let FillSource::Bitmap { resource } = after.source else { panic!("image source, got {:?}", after.source) };
    assert_eq!(after.strength, 0.4, "strength is kept");
    assert!(after.enabled);
    let stored = s.resources().bitmap(resource).expect("the alpha is stored with the project");
    assert_eq!((stored.width, stored.height), (2, 1));
    // The picture reaches the composite: dark on the left of UV space, bright on the right.
    s.update_composites();
    let ts = s.selected_texture_set().id;
    let img = s.composite(ts).unwrap().channel(ChannelKind::Roughness).unwrap();
    let (w, h) = (img.width(), img.height());
    let (dark, bright) = (img.get(w / 8, h / 2)[0], img.get(7 * w / 8, h / 2)[0]);
    // (Two texels wide, so the halves blend into each other; 0.4 strength caps the bright side.)
    assert!(bright > dark + 0.15 && bright <= 0.4 + 1e-3, "left {dark}, right {bright}");

    // One undo step back to the uniform value.
    s.execute(Command::Undo).unwrap();
    assert_eq!(channel(&s, ChannelKind::Roughness), before);

    // Refused: a normal channel, the round tip (no picture), and a layer that is not a fill.
    assert!(s.set_fill_channel_alpha(layer, ChannelKind::Normal, &alpha).is_err());
    assert!(s.set_fill_channel_alpha(layer, ChannelKind::Roughness, &BrushAlpha { bitmap: None, ..alpha.clone() }).is_err());
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let paint = s.selection().layer.unwrap();
    assert!(s.set_fill_channel_alpha(paint, ChannelKind::Roughness, &alpha).is_err());
}
