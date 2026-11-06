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
