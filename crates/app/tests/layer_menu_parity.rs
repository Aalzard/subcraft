//! Painter parity for the layer header and right-click menu: grouping the selection (Ctrl+G),
//! a new layer landing inside a selected empty folder, and the default blend of new layers.
use subcraft_app::{Action, Command, NewLayer, Session};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{BlendMode, LayerLocation};

fn session() -> Session {
    Session::new(subcraft_mesh::primitives::cube(2.0), 64)
}

fn add(s: &mut Session, kind: NewLayer) -> LayerId {
    s.execute(Command::AddLayer(kind)).unwrap();
    s.selection().layer.unwrap()
}

/// Root layer ids, bottom to top.
fn root(s: &Session) -> Vec<LayerId> {
    s.selected_texture_set().layers.iter().map(|l| l.id).collect()
}

#[test]
fn group_layers_puts_the_selection_into_one_folder_where_the_top_one_was() {
    let mut s = session();
    let a = s.selection().layer.unwrap();
    let (b, c, d) = (add(&mut s, NewLayer::Paint), add(&mut s, NewLayer::Paint), add(&mut s, NewLayer::Paint));
    assert_eq!(root(&s), [a, b, c, d]);
    let steps = s.history().entries().len();

    // The order given does not matter; the stack order is kept.
    s.execute(Command::GroupLayers(vec![c, a])).unwrap();
    assert_eq!(s.history().entries().len(), steps + 1, "one undo step");
    let ts = s.selected_texture_set();
    let folder = ts.location(c).unwrap().parent.expect("c is in a folder");
    assert!(ts.find(folder).unwrap().is_folder());
    assert_eq!(ts.location(a), Some(LayerLocation { parent: Some(folder), index: 0 }));
    assert_eq!(ts.location(c), Some(LayerLocation { parent: Some(folder), index: 1 }));
    assert_eq!(root(&s), [b, folder, d], "the folder sits where the topmost grouped layer was");
    assert_eq!(s.selection().layer, Some(folder), "the new folder is selected");

    s.execute(Command::Undo).unwrap();
    assert_eq!(root(&s), [a, b, c, d]);

    // A folder and one of its own children selected together: the child moves with its folder.
    s.execute(Command::GroupLayers(vec![a, b])).unwrap();
    let inner = s.selected_texture_set().location(a).unwrap().parent.unwrap();
    s.execute(Command::GroupLayers(vec![inner, a, d])).unwrap();
    let ts = s.selected_texture_set();
    let outer = ts.location(d).unwrap().parent.unwrap();
    assert_eq!(ts.location(inner).unwrap().parent, Some(outer));
    assert_eq!(ts.location(a).unwrap().parent, Some(inner), "a stays inside its own folder");
    assert_eq!(root(&s), [c, outer]);

    let before = s.document().clone();
    assert!(s.execute(Command::GroupLayers(vec![])).is_err());
    assert!(s.execute(Command::GroupLayers(vec![LayerId::default()])).is_err());
    assert_eq!(s.document(), &before, "a refused group changes nothing");
}

#[test]
fn the_ctrl_g_action_wraps_the_selected_layer_and_makes_a_folder_when_nothing_is_selected() {
    let mut s = session();
    let a = s.selection().layer.unwrap();
    assert!(s.perform(Action::NewFolder).unwrap());
    let folder = s.selected_texture_set().location(a).unwrap().parent.expect("grouped");
    assert_eq!(root(&s), [folder]);

    s.execute(Command::SelectLayer(None)).unwrap();
    assert!(s.perform(Action::NewFolder).unwrap());
    let ts = s.selected_texture_set();
    assert_eq!(ts.layers.len(), 2);
    let empty = ts.layers.last().unwrap();
    assert!(empty.is_folder() && empty.children().is_empty());
}

#[test]
fn a_new_layer_goes_inside_a_selected_empty_folder_and_above_anything_else() {
    let mut s = session();
    let base = s.selection().layer.unwrap();
    let folder = add(&mut s, NewLayer::Folder);
    assert_eq!(root(&s), [base, folder]);

    // Empty folder selected: the layer lands inside it.
    let first = add(&mut s, NewLayer::Fill);
    assert_eq!(s.selected_texture_set().location(first), Some(LayerLocation { parent: Some(folder), index: 0 }));
    assert_eq!(s.selection().layer, Some(first));
    // The folder is no longer empty: a new layer goes above it, as for any other layer.
    s.execute(Command::SelectLayer(Some(folder))).unwrap();
    let above = add(&mut s, NewLayer::Paint);
    assert_eq!(s.selected_texture_set().location(above), Some(LayerLocation { parent: None, index: 2 }));
    // A layer selected inside the folder: the new one is its sibling, right above it.
    s.execute(Command::SelectLayer(Some(first))).unwrap();
    let sibling = add(&mut s, NewLayer::Paint);
    assert_eq!(s.selected_texture_set().location(sibling), Some(LayerLocation { parent: Some(folder), index: 1 }));
    // An empty folder inside an empty folder nests.
    s.execute(Command::SelectLayer(None)).unwrap();
    let outer = add(&mut s, NewLayer::Folder);
    let nested = add(&mut s, NewLayer::Folder);
    assert_eq!(s.selected_texture_set().location(nested), Some(LayerLocation { parent: Some(outer), index: 0 }));

    // One undo step per layer.
    s.execute(Command::Undo).unwrap();
    assert!(s.selected_texture_set().find(nested).is_none());
    assert!(s.selected_texture_set().find(outer).is_some());
}

#[test]
fn new_layers_get_painters_default_blend_per_channel() {
    let mut s = session();
    let set = s.selection().texture_set;
    for kind in [ChannelKind::Height, ChannelKind::Normal, ChannelKind::AmbientOcclusion] {
        s.execute(Command::AddChannel { texture_set: set, kind }).unwrap();
    }
    for new in [NewLayer::Paint, NewLayer::Fill] {
        let id = add(&mut s, new);
        let l = s.selected_texture_set().find(id).unwrap();
        assert_eq!(l.blend_for(ChannelKind::Height).mode, BlendMode::LinearDodge, "{new:?}");
        assert_eq!(l.blend_for(ChannelKind::Normal).mode, BlendMode::NormalMapDetail, "{new:?}");
        assert_eq!(l.blend_for(ChannelKind::AmbientOcclusion).mode, BlendMode::Multiply, "{new:?}");
        assert_eq!(l.blend_for(ChannelKind::BaseColor).mode, BlendMode::Normal, "{new:?}");
        assert_eq!(l.blend_for(ChannelKind::Roughness).mode, BlendMode::Normal, "{new:?}");
        assert!(s.selected_texture_set().channel_kinds().all(|k| l.blend_for(k).opacity == 1.0));
    }
    s.execute(Command::SelectLayer(None)).unwrap();
    let folder = add(&mut s, NewLayer::Folder);
    let f = s.selected_texture_set().find(folder).unwrap();
    assert!(s.selected_texture_set().channel_kinds().all(|k| f.blend_for(k).mode == BlendMode::Passthrough));
}

/// A new fill layer must leave Height, Normal and AO of the stack below unchanged: its default
/// values (height 0 added, a flat normal as detail, AO 1 multiplied) are the identities of
/// Painter's default blend modes.
#[test]
fn a_default_fill_layer_does_not_change_height_normal_or_ao_below() {
    let mut s = session();
    let set = s.selection().texture_set;
    for kind in [ChannelKind::Height, ChannelKind::Normal, ChannelKind::AmbientOcclusion] {
        s.execute(Command::AddChannel { texture_set: set, kind }).unwrap();
    }
    // A base fill with distinctive values.
    let base = add(&mut s, NewLayer::Fill);
    for (channel, value) in [
        (ChannelKind::Height, subcraft_core::ChannelValue::scalar(0.25)),
        (ChannelKind::AmbientOcclusion, subcraft_core::ChannelValue::scalar(0.5)),
    ] {
        let fill = subcraft_doc::FillChannel::new(subcraft_doc::FillSource::uniform(value));
        s.execute(Command::SetFillChannel { layer: base, channel, fill }).unwrap();
    }
    s.update_composites();
    let sample = |s: &Session, k: ChannelKind| s.composite(set).unwrap().channel(k).unwrap().get(32, 32);
    let before: Vec<_> = [ChannelKind::Height, ChannelKind::Normal, ChannelKind::AmbientOcclusion].into_iter().map(|k| sample(&s, k)).collect();
    add(&mut s, NewLayer::Fill);
    s.update_composites();
    let after: Vec<_> = [ChannelKind::Height, ChannelKind::Normal, ChannelKind::AmbientOcclusion].into_iter().map(|k| sample(&s, k)).collect();
    for (b, a) in before.iter().zip(&after) {
        for i in 0..4 {
            assert!((b[i] - a[i]).abs() < 1e-3, "before {b:?} after {a:?}");
        }
    }
    assert!((before[0][0] - 0.25).abs() < 1e-3 && (before[2][0] - 0.5).abs() < 1e-3, "{before:?}");
}
