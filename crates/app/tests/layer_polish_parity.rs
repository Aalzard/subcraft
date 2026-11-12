//! Painter parity for layer rows: blending applied to all channels, copied blending options,
//! colour tags, drops of several layers, Ctrl-drag copies and un-hiding inside hidden folders.
use std::collections::BTreeMap;

use subcraft_app::layer_tree::DropTarget;
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{BlendMode, ChannelBlend, LayerColor};

fn session() -> Session {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let set = s.selection().texture_set;
    for kind in [ChannelKind::Height, ChannelKind::Normal] {
        s.execute(Command::AddChannel { texture_set: set, kind }).unwrap();
    }
    s
}

fn add(s: &mut Session, kind: NewLayer) -> LayerId {
    s.execute(Command::AddLayer(kind)).unwrap();
    s.selection().layer.unwrap()
}

/// Root layer ids, bottom to top.
fn root(s: &Session) -> Vec<LayerId> {
    s.selected_texture_set().layers.iter().map(|l| l.id).collect()
}

fn children(s: &Session, folder: LayerId) -> Vec<LayerId> {
    s.selected_texture_set().find(folder).unwrap().children().iter().map(|l| l.id).collect()
}

fn steps(s: &Session) -> usize {
    s.history().entries().len()
}

fn blend(s: &Session, layer: LayerId, channel: ChannelKind) -> ChannelBlend {
    s.selected_texture_set().find(layer).unwrap().blend_for(channel)
}

#[test]
fn a_blend_mode_or_opacity_is_applied_to_all_channels_of_the_layer() {
    let mut s = session();
    let l = s.selection().layer.unwrap();
    let other = add(&mut s, NewLayer::Paint);
    s.execute(Command::SetBlendMode { layer: l, channel: ChannelKind::BaseColor, mode: BlendMode::Multiply }).unwrap();
    s.execute(Command::SetBlendOpacity { layer: l, channel: ChannelKind::BaseColor, opacity: 0.4 }).unwrap();
    s.execute(Command::Seal).unwrap();
    let untouched = s.selected_texture_set().find(other).unwrap().blend.clone();

    // The mode alone: opacities stay.
    let before = steps(&s);
    s.execute(Command::SetBlendAllChannels { layer: l, from: ChannelKind::BaseColor, mode: true, opacity: false }).unwrap();
    assert_eq!(steps(&s), before + 1);
    for kind in s.selected_texture_set().channel_kinds().collect::<Vec<_>>() {
        assert_eq!(blend(&s, l, kind).mode, BlendMode::Multiply, "{kind:?}");
        assert_eq!(blend(&s, l, kind).opacity, if kind == ChannelKind::BaseColor { 0.4 } else { 1.0 }, "{kind:?}");
    }
    // The opacity alone.
    s.execute(Command::SetBlendMode { layer: l, channel: ChannelKind::Roughness, mode: BlendMode::Screen }).unwrap();
    s.execute(Command::SetBlendAllChannels { layer: l, from: ChannelKind::BaseColor, mode: false, opacity: true }).unwrap();
    assert_eq!(blend(&s, l, ChannelKind::Roughness), ChannelBlend { mode: BlendMode::Screen, opacity: 0.4 });
    assert_eq!(blend(&s, l, ChannelKind::Height).opacity, 0.4);

    // A normal-map mode only exists on the Normal channel: the others keep theirs.
    s.execute(Command::SetBlendMode { layer: l, channel: ChannelKind::Normal, mode: BlendMode::NormalMapCombine }).unwrap();
    s.execute(Command::SetBlendAllChannels { layer: l, from: ChannelKind::Normal, mode: true, opacity: false }).unwrap();
    assert_eq!(blend(&s, l, ChannelKind::Normal).mode, BlendMode::NormalMapCombine);
    assert_eq!(blend(&s, l, ChannelKind::BaseColor).mode, BlendMode::Multiply);
    assert_eq!(blend(&s, l, ChannelKind::Roughness).mode, BlendMode::Screen);

    assert_eq!(s.selected_texture_set().find(other).unwrap().blend, untouched, "other layers are not touched");
    assert!(s.execute(Command::SetBlendAllChannels { layer: l, from: ChannelKind::Emissive, mode: true, opacity: true }).is_err(), "a channel the layer does not have");
}

#[test]
fn blending_options_paste_onto_several_layers_and_skip_what_does_not_fit() {
    let mut s = session();
    let a = s.selection().layer.unwrap();
    let b = add(&mut s, NewLayer::Fill);
    let folder = add(&mut s, NewLayer::Folder);
    s.execute(Command::SetBlendMode { layer: a, channel: ChannelKind::BaseColor, mode: BlendMode::Overlay }).unwrap();
    s.execute(Command::SetBlendOpacity { layer: a, channel: ChannelKind::Height, opacity: 0.5 }).unwrap();
    s.execute(Command::Seal).unwrap();
    let copied = s.selected_texture_set().find(a).unwrap().blend.clone();

    let before = steps(&s);
    s.execute(Command::PasteBlending { layers: vec![b, folder], blend: copied.clone() }).unwrap();
    assert_eq!(steps(&s), before + 1, "one undo step for all layers");
    assert_eq!(s.selected_texture_set().find(b).unwrap().blend, copied);
    assert_eq!(s.selected_texture_set().find(folder).unwrap().blend, copied, "a folder takes ordinary modes too");

    // Passthrough copied from a folder is not a mode of a paint layer: its mode stays, the opacity is pasted.
    s.execute(Command::SetBlendMode { layer: folder, channel: ChannelKind::BaseColor, mode: BlendMode::Passthrough }).unwrap();
    s.execute(Command::SetBlendOpacity { layer: folder, channel: ChannelKind::BaseColor, opacity: 0.3 }).unwrap();
    s.execute(Command::Seal).unwrap();
    let from_folder = s.selected_texture_set().find(folder).unwrap().blend.clone();
    s.execute(Command::PasteBlending { layers: vec![b], blend: from_folder }).unwrap();
    assert_eq!(blend(&s, b, ChannelKind::BaseColor), ChannelBlend { mode: BlendMode::Overlay, opacity: 0.3 });

    // Channels missing from the copy are left alone; pasting the same again adds no undo step.
    let only_height: BTreeMap<_, _> = [(ChannelKind::Height, ChannelBlend { mode: BlendMode::Subtract, opacity: 1.0 })].into();
    s.execute(Command::PasteBlending { layers: vec![b], blend: only_height.clone() }).unwrap();
    assert_eq!(blend(&s, b, ChannelKind::Height).mode, BlendMode::Subtract);
    assert_eq!(blend(&s, b, ChannelKind::BaseColor).mode, BlendMode::Overlay);
    let before = steps(&s);
    s.execute(Command::PasteBlending { layers: vec![b], blend: only_height }).unwrap();
    assert_eq!(steps(&s), before);
    assert!(s.execute(Command::PasteBlending { layers: vec![LayerId::new()], blend: copied }).is_err());
}

#[test]
fn colour_tags_are_set_on_several_layers_survive_duplication_and_undo() {
    let mut s = session();
    let a = s.selection().layer.unwrap();
    let b = add(&mut s, NewLayer::Folder);
    let color = |s: &Session, id| s.selected_texture_set().find(id).unwrap().color;
    assert_eq!((color(&s, a), color(&s, b)), (None, None), "no tag by default");

    let before = steps(&s);
    s.execute(Command::SetLayerColor { layers: vec![a, b], color: Some(LayerColor::Teal) }).unwrap();
    assert_eq!((color(&s, a), color(&s, b)), (Some(LayerColor::Teal), Some(LayerColor::Teal)));
    assert_eq!(steps(&s), before + 1);
    // Setting the same tag again is not an undo step.
    s.execute(Command::SetLayerColor { layers: vec![a], color: Some(LayerColor::Teal) }).unwrap();
    assert_eq!(steps(&s), before + 1);

    s.execute(Command::DuplicateLayer(a)).unwrap();
    let copy = root(&s).into_iter().find(|id| ![a, b].contains(id)).expect("the copy");
    assert_eq!(color(&s, copy), Some(LayerColor::Teal), "a copy keeps the tag");

    s.execute(Command::SetLayerColor { layers: vec![a], color: None }).unwrap();
    assert_eq!((color(&s, a), color(&s, b)), (None, Some(LayerColor::Teal)));
    s.execute(Command::Undo).unwrap();
    assert_eq!(color(&s, a), Some(LayerColor::Teal));
    // The tag never changes what is composited.
    assert_eq!(LayerColor::ALL.len(), 9);
    assert!(LayerColor::ALL.iter().all(|c| !c.label().is_empty()));
}

#[test]
fn several_layers_drop_together_and_keep_their_stacking_order() {
    let mut s = session();
    let a = s.selection().layer.unwrap();
    let (b, c, d) = (add(&mut s, NewLayer::Paint), add(&mut s, NewLayer::Paint), add(&mut s, NewLayer::Paint));
    s.execute(Command::SelectLayer(None)).unwrap();
    let folder = add(&mut s, NewLayer::Folder);
    assert_eq!(root(&s), [a, b, c, d, folder]);
    let set = s.selection().texture_set;
    let drop = |s: &mut Session, layers: Vec<LayerId>, target, copy| s.execute(Command::DropLayers { texture_set: set, layers, target, copy });

    // a and c (given top first) go above d: a below c, as they were.
    let before = steps(&s);
    drop(&mut s, vec![c, a], DropTarget::Above(d), false).unwrap();
    assert_eq!(root(&s), [b, d, a, c, folder]);
    assert_eq!(steps(&s), before + 1, "one undo step");
    s.execute(Command::Undo).unwrap();
    assert_eq!(root(&s), [a, b, c, d, folder]);

    drop(&mut s, vec![b, d], DropTarget::Below(a), false).unwrap();
    assert_eq!(root(&s), [b, d, a, c, folder]);
    s.execute(Command::Undo).unwrap();
    drop(&mut s, vec![a, d], DropTarget::RootTop, false).unwrap();
    assert_eq!(root(&s), [b, c, folder, a, d]);
    s.execute(Command::Undo).unwrap();
    drop(&mut s, vec![c, d], DropTarget::RootBottom, false).unwrap();
    assert_eq!(root(&s), [c, d, a, b, folder]);
    s.execute(Command::Undo).unwrap();

    // Into a folder: it opens, and the layers arrive in order.
    drop(&mut s, vec![d, b], DropTarget::Inside(folder), false).unwrap();
    assert_eq!((root(&s), children(&s, folder)), (vec![a, c, folder], vec![b, d]));
    // A folder dragged together with one of its own children moves once, child inside.
    drop(&mut s, vec![folder, b, a], DropTarget::Above(c), false).unwrap();
    assert_eq!((root(&s), children(&s, folder)), (vec![c, a, folder], vec![b, d]));

    // Refused or unchanged drops change nothing and add no undo step.
    let doc = s.document().clone();
    let before = steps(&s);
    drop(&mut s, vec![folder, c], DropTarget::Inside(folder), false).unwrap();
    drop(&mut s, vec![folder, c], DropTarget::Above(b), false).unwrap();
    drop(&mut s, vec![a, c], DropTarget::Above(a), false).unwrap();
    drop(&mut s, vec![a, c], DropTarget::Inside(a), false).unwrap();
    drop(&mut s, vec![c, a], DropTarget::RootBottom, false).unwrap();
    assert!(drop(&mut s, vec![], DropTarget::RootTop, false).is_err());
    assert!(drop(&mut s, vec![a], DropTarget::Above(LayerId::new()), false).is_err());
    assert_eq!((s.document(), steps(&s)), (&doc, before));
}

#[test]
fn a_ctrl_drag_drops_copies_and_leaves_the_originals() {
    let mut s = session();
    let a = s.selection().layer.unwrap();
    let b = add(&mut s, NewLayer::Fill);
    let c = add(&mut s, NewLayer::Paint);
    let set = s.selection().texture_set;
    s.execute(Command::Rename { layer: a, name: "A".into() }).unwrap();
    s.execute(Command::Rename { layer: b, name: "B".into() }).unwrap();
    let names = |s: &Session| s.selected_texture_set().layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>();

    let before = steps(&s);
    s.execute(Command::DropLayers { texture_set: set, layers: vec![b, a], target: DropTarget::RootTop, copy: true }).unwrap();
    assert_eq!(steps(&s), before + 1);
    let now = root(&s);
    assert_eq!(now.len(), 5);
    assert_eq!(now[..3], [a, b, c], "the originals stay where they were");
    assert_eq!(names(&s)[3..], ["A copy", "B copy"]);
    assert_eq!(s.selection().layer, Some(now[4]), "the top copy is selected");
    assert_ne!(s.selected_texture_set().find(now[3]).unwrap().kind(), s.selected_texture_set().find(now[4]).unwrap().kind());
    s.execute(Command::Undo).unwrap();
    assert_eq!(root(&s), [a, b, c]);

    // A copy may land right on its original (above or below it).
    s.execute(Command::DropLayers { texture_set: set, layers: vec![a], target: DropTarget::Below(a), copy: true }).unwrap();
    assert_eq!(names(&s)[..2], ["A copy", "A"]);
    assert_eq!(root(&s)[1..], [a, b, c]);
}

#[test]
fn showing_a_layer_inside_hidden_folders_shows_the_folders_too() {
    let mut s = session();
    let outer = add(&mut s, NewLayer::Folder);
    let inner = add(&mut s, NewLayer::Folder);
    let leaf = add(&mut s, NewLayer::Paint);
    let ts = s.selected_texture_set();
    assert_eq!(ts.location(inner).unwrap().parent, Some(outer));
    assert_eq!(ts.location(leaf).unwrap().parent, Some(inner));
    let visible = |s: &Session, id| s.selected_texture_set().find(id).unwrap().visible;
    for id in [leaf, inner, outer] {
        s.execute(Command::SetVisible { layer: id, visible: false }).unwrap();
    }

    let before = steps(&s);
    s.execute(Command::SetVisible { layer: leaf, visible: true }).unwrap();
    assert!(visible(&s, leaf) && visible(&s, inner) && visible(&s, outer));
    assert_eq!(steps(&s), before + 1, "one undo step shows them all");
    s.execute(Command::Undo).unwrap();
    assert!(!visible(&s, leaf) && !visible(&s, inner) && !visible(&s, outer));

    // Hiding never touches the folders around; the multi-layer command shows them as well.
    s.execute(Command::SetVisible { layer: outer, visible: true }).unwrap();
    s.execute(Command::SetVisible { layer: leaf, visible: false }).unwrap();
    assert!(visible(&s, outer) && !visible(&s, inner));
    s.execute(Command::SetLayersVisible { layers: vec![leaf], visible: true }).unwrap();
    assert!(visible(&s, leaf) && visible(&s, inner) && visible(&s, outer));
}
