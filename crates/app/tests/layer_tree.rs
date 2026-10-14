//! Visual drop operations: hierarchy, stable identity, history, composites and persistence.
use subcraft_app::{Command, NewLayer, Session, layer_tree::{DropTarget, drop_destination}};
use subcraft_core::{LayerId, ObjectId, TextureSetId, ChannelKind, ChannelValue};
use subcraft_doc::{Layer, LayerContent, LayerLocation, TextureSet, FillChannel, FillSource};

fn stack() -> (TextureSet, [LayerId; 3]) {
    let mut ts = TextureSet::new("Body", 0, 64);
    let mut ids = [LayerId::default(); 3];
    for (i, name) in ["Bottom", "Middle", "Top"].iter().enumerate() {
        ids[i] = ts.insert_above(None, Layer::new_paint(*name, &ts.clone()));
    }
    (ts, ids)
}

#[test]
fn visual_positions_adjust_after_removal_and_skip_unchanged_moves() {
    let (ts, [bottom, middle, top]) = stack();
    for (source, target, index) in [(bottom, DropTarget::Above(top), 2), (top, DropTarget::Below(bottom), 0), (middle, DropTarget::Above(top), 2), (middle, DropTarget::Below(bottom), 0)] {
        assert_eq!(drop_destination(&ts, source, target), Some(LayerLocation { parent: None, index }));
    }
    for (source, target) in [(bottom, DropTarget::Below(middle)), (top, DropTarget::Above(middle)), (middle, DropTarget::Above(middle)), (top, DropTarget::RootTop), (bottom, DropTarget::RootBottom)] {
        assert_eq!(drop_destination(&ts, source, target), None);
    }
    assert_eq!(drop_destination(&ts, LayerId::new(), DropTarget::RootTop), None);
    assert_eq!(drop_destination(&ts, bottom, DropTarget::Inside(top)), None);
    assert_eq!(drop_destination(&ts, bottom, DropTarget::Above(LayerId::new())), None);
}

#[test]
fn nested_moves_reject_cycles_and_leave_folders_at_both_root_ends() {
    let (mut ts, [bottom, _, top]) = stack();
    let folder = ts.insert_above(None, Layer::new_folder("Folder", &ts.clone()));
    let inner = ts.insert(Some(folder), 0, Layer::new_folder("Nested", &ts.clone())).unwrap();
    let destination = drop_destination(&ts, top, DropTarget::Inside(inner)).unwrap();
    ts.move_to(top, destination.parent, destination.index).unwrap();
    assert_eq!(ts.location(top), Some(LayerLocation { parent: Some(inner), index: 0 }));
    for target in [DropTarget::Inside(inner), DropTarget::Above(top), DropTarget::Below(top), DropTarget::Inside(folder)] {
        assert_eq!(drop_destination(&ts, folder, target), None, "reject moving ancestor alongside or into descendant");
    }
    assert_eq!(drop_destination(&ts, top, DropTarget::RootBottom), Some(LayerLocation { parent: None, index: 0 }));
    assert_eq!(drop_destination(&ts, top, DropTarget::RootTop), Some(LayerLocation { parent: None, index: ts.layers.len() }));
    assert_eq!(drop_destination(&ts, top, DropTarget::Above(bottom)), Some(LayerLocation { parent: None, index: 1 }));
}

fn drop(s: &mut Session, layer: LayerId, target: DropTarget) {
    s.execute(Command::DropLayer { texture_set: s.selection().texture_set, layer, target }).unwrap();
}

#[test]
fn dropping_into_collapsed_folder_is_one_undo_step_and_preserves_children_on_move() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let paint = s.selection().layer.unwrap();
    s.execute(Command::AddLayer(NewLayer::Folder)).unwrap(); let folder = s.selection().layer.unwrap();
    s.execute(Command::SetFolderExpanded { layer: folder, expanded: false }).unwrap();
    let before = s.document().clone(); let history = s.history().entries().len();
    drop(&mut s, paint, DropTarget::Inside(folder));
    assert_eq!(s.selection().layer, Some(paint));
    assert_eq!(s.history().entries().len(), history + 1);
    assert!(matches!(&s.selected_texture_set().find(folder).unwrap().content, LayerContent::Group(g) if g.expanded && g.children[0].id == paint));
    let after = s.document().clone();
    s.execute(Command::Undo).unwrap(); assert_eq!(s.document(), &before);
    s.execute(Command::Redo).unwrap(); assert_eq!(s.document(), &after);
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap(); let extra = s.selection().layer.unwrap();
    drop(&mut s, extra, DropTarget::RootBottom);
    drop(&mut s, folder, DropTarget::Below(extra));
    assert_eq!(s.selected_texture_set().layers[0].id, folder);
    assert!(s.selected_texture_set().find(folder).unwrap().children().iter().any(|l| l.id == paint));
}

#[test]
fn stale_texture_set_and_invalid_drops_never_change_history_or_document() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let layer = s.selection().layer.unwrap(); let before = s.document().clone(); let revision = s.revision();
    assert!(s.execute(Command::DropLayer { texture_set: TextureSetId::new(), layer, target: DropTarget::RootTop }).is_err());
    assert!(s.execute(Command::DropLayer { texture_set: s.selection().texture_set, layer: LayerId::new(), target: DropTarget::RootTop }).is_err());
    for target in [DropTarget::RootTop, DropTarget::RootBottom, DropTarget::Inside(layer), DropTarget::Above(LayerId::new())] { drop(&mut s, layer, target); }
    assert_eq!(s.document(), &before); assert_eq!(s.revision(), revision); assert!(!s.history().can_undo());
    assert!(s.execute(Command::SetFolderExpanded { layer, expanded: false }).is_err());
    assert_eq!(s.document(), &before);
}

#[test]
fn reordered_fills_change_actual_texels_and_undo_restores_them() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let mut add = |value| {
        s.execute(Command::AddLayer(NewLayer::Fill)).unwrap(); let id = s.selection().layer.unwrap();
        s.execute(Command::SetFillChannel { layer: id, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(value)) }).unwrap(); id
    };
    let red = add(ChannelValue::rgb(1.0, 0.0, 0.0)); let blue = add(ChannelValue::rgb(0.0, 0.0, 1.0));
    let ts = s.selection().texture_set;
    s.update_composites(); let before = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    drop(&mut s, red, DropTarget::Above(blue));
    s.update_composites(); let after = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    assert_eq!(&before.get(32, 32)[..3], &[0.0, 0.0, 1.0]);
    assert_eq!(&after.get(32, 32)[..3], &[1.0, 0.0, 0.0]);
    s.execute(Command::Undo).unwrap(); s.update_composites();
    assert_eq!(s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap(), &before);
}

#[test]
fn nested_order_rename_and_collapsed_state_roundtrip_without_schema_change() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
    let paint = s.selection().layer.unwrap();
    s.execute(Command::GroupLayer(paint)).unwrap(); let folder = s.selected_texture_set().location(paint).unwrap().parent.unwrap();
    s.execute(Command::Rename { layer: paint, name: "Painted details".into() }).unwrap();
    s.execute(Command::SetFolderExpanded { layer: folder, expanded: false }).unwrap();
    let path = std::env::temp_dir().join(format!("layer-tree-{}.subcraft", ObjectId::new()));
    s.save(Some(&path)).unwrap(); let reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.document(), s.document());
    assert_eq!(reopened.selected_texture_set().find(paint).unwrap().name, "Painted details");
    std::fs::remove_file(path).unwrap();
}
