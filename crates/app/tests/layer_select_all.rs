//! Select All + Delete removes every layer of the texture set at once, in one undo step.
use subcraft_app::multi_select::MultiSelect;
use subcraft_app::{Command, NewLayer, Session};

#[test]
fn select_all_then_delete_removes_them_all_in_one_step() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    for kind in [NewLayer::Fill, NewLayer::Paint, NewLayer::Folder, NewLayer::Fill] {
        s.execute(Command::AddLayer(kind)).unwrap();
    }
    let mut rows = Vec::new();
    s.selected_texture_set().walk(|l, _| rows.push(l.id));
    assert_eq!(rows.len(), 5, "the start layer and four more");
    let mut multi = MultiSelect::default();
    multi.select_all(&rows);
    assert!(multi.is_multi());
    let undo_depth = s.history().entries().len();
    s.execute(Command::DeleteLayers(multi.ids.clone())).unwrap();
    assert!(s.selected_texture_set().layers.is_empty(), "every layer is gone");
    assert_eq!(s.history().entries().len(), undo_depth + 1, "one undo step");
    s.execute(Command::Undo).unwrap();
    let mut back = 0;
    s.selected_texture_set().walk(|_, _| back += 1);
    assert_eq!(back, 5, "undo brings them all back");
}
