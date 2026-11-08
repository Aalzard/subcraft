//! Clicking a material on the Shelf swaps the material of the selected fill layer instead of
//! stacking a new layer; with no fill layer selected it adds one.
use subcraft_app::{Command, Session};

#[test]
fn clicking_materials_swaps_the_selected_fill_and_keeps_its_mask() {
    let library = subcraft_assets::presets::materials();
    let (a, b) = (&library[0], &library[1]);
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let start = s.selected_texture_set().layers.len();
    assert!(!s.apply_material(a).unwrap(), "a paint layer is selected: the first click adds a fill layer");
    let fill = s.selection().layer.unwrap();
    s.execute(Command::AddMask { layer: fill, white: false }).unwrap();
    assert!(s.apply_material(b).unwrap(), "the second click swaps");
    assert_eq!(s.selected_texture_set().layers.len(), start + 1, "no new layer");
    let layer = s.selected_layer().unwrap();
    assert_eq!(layer.id, fill);
    assert_eq!(layer.name, b.name);
    assert!(layer.mask.is_some(), "the mask stays");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_layer().unwrap().name, a.name, "one undo step restores the first material");
}
