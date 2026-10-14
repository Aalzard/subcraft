//! Shelf items applied by click or drop (same routing).

use subcraft_app::Session;
use subcraft_doc::LayerKind;
use subcraft_mesh::primitives;
use subcraft_ui::panels::shelf::{ShelfDrag, apply_shelf_item};
use subcraft_ui::state::UiState;

fn state() -> UiState {
    UiState::new(Session::new(primitives::sample_crate(), 128), None)
}

#[test]
fn material_and_smart_material_insert_above_the_selected_layer() {
    let mut s = state();
    let before = s.session.selected_texture_set().layers.len();
    apply_shelf_item(&mut s, ShelfDrag::Material(0), 0.0);
    let name = s.library.materials[0].name.clone();
    assert_eq!(s.session.selected_texture_set().layers.len(), before + 1);
    assert_eq!(s.session.selected_layer().unwrap().name, name);

    apply_shelf_item(&mut s, ShelfDrag::SmartMaterial(0), 0.0);
    assert_eq!(s.session.selected_layer().unwrap().kind(), LayerKind::Folder);
    assert_eq!(s.session.selected_texture_set().layers.len(), before + 2);
}

#[test]
fn smart_mask_goes_onto_the_selected_layer() {
    let mut s = state();
    assert!(s.session.selected_layer().unwrap().mask.is_none());
    apply_shelf_item(&mut s, ShelfDrag::SmartMask(0), 0.0);
    let mask = s.session.selected_layer().unwrap().mask.as_ref().expect("mask added");
    assert!(!mask.effects.is_empty());
}

#[test]
fn out_of_range_items_do_nothing() {
    let mut s = state();
    let doc = s.session.document().clone();
    apply_shelf_item(&mut s, ShelfDrag::Material(9999), 0.0);
    assert_eq!(s.session.document(), &doc);
}
