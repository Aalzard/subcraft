//! Layer / effect clipboard, multi-select operations and texture-set commands.
use std::sync::Arc;

use subcraft_app::{Command, NewLayer, Session, LayerTarget};
use subcraft_app::clipboard::{LayerClip, top_level_in_order};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{ContentEffectKind, Levels, Layer, LayerContent, TextureSet};
use subcraft_mesh::primitives;

fn session(res: u32) -> Session {
    Session::new(primitives::cube(2.0), res)
}

fn top(s: &Session) -> Vec<LayerId> {
    s.selected_texture_set().layers.iter().map(|l| l.id).collect()
}

fn add(s: &mut Session, kind: NewLayer, name: &str) -> LayerId {
    s.execute(Command::AddLayer(kind)).unwrap();
    let id = s.selection().layer.unwrap();
    s.execute(Command::Rename { layer: id, name: name.into() }).unwrap();
    id
}

fn names(s: &Session) -> Vec<String> {
    s.selected_texture_set().layers.iter().map(|l| l.name.clone()).collect()
}

#[test]
fn copy_paste_creates_new_ids_keeps_the_original_and_undoes_in_one_step() {
    let mut s = session(64);
    let a = add(&mut s, NewLayer::Fill, "A");
    let b = add(&mut s, NewLayer::Paint, "B");
    let before = s.history().entries().len();
    let clip = Arc::new(s.copy_layers(&[a, b]).unwrap());
    s.execute(Command::PasteLayers(clip)).unwrap();
    // Pasted duplicates are renamed (Painter); a third paste counts up.
    assert_eq!(names(&s), ["Layer 1", "A", "B", "A copy", "B copy"].map(String::from));
    {
        let ts = s.selected_texture_set();
        assert_eq!(subcraft_app::clipboard::pasted_name(ts, "A"), "A copy 2");
        assert_eq!(subcraft_app::clipboard::pasted_name(ts, "New"), "New");
    }
    assert_eq!(s.history().entries().len(), before + 1);
    let ids = top(&s);
    assert_eq!(ids.len(), 5);
    assert!(ids[3] != a && ids[4] != b, "pasted layers get new UUIDs");
    assert_eq!(s.selection().layer, Some(ids[4]), "the last pasted layer is selected");
    s.execute(Command::Undo).unwrap();
    assert_eq!(top(&s).len(), 3);
    assert!(top(&s).ends_with(&[a, b]));
}

#[test]
fn cut_is_copy_plus_delete_and_paste_restores_the_layer() {
    let mut s = session(64);
    let a = add(&mut s, NewLayer::Fill, "A");
    let b = add(&mut s, NewLayer::Fill, "B");
    let clip = Arc::new(s.copy_layers(&[b]).unwrap());
    s.execute(Command::DeleteLayers(vec![b])).unwrap();
    assert_eq!(names(&s), ["Layer 1", "A"].map(String::from));
    let _ = a;
    s.execute(Command::PasteLayers(clip)).unwrap();
    assert_eq!(names(&s), ["Layer 1", "A", "B"].map(String::from));
}

#[test]
fn multi_delete_duplicate_and_visibility_are_single_undo_steps_and_folders_cover_children() {
    let mut s = session(64);
    let a = add(&mut s, NewLayer::Fill, "A");
    let f = add(&mut s, NewLayer::Folder, "F");
    let c = add(&mut s, NewLayer::Fill, "C");
    s.execute(Command::MoveLayer { layer: c, parent: Some(f), index: 0 }).unwrap();
    let n = s.history().entries().len();
    // Folder and its child both selected: the child is not processed twice.
    s.execute(Command::DuplicateLayers(vec![a, f, c])).unwrap();
    assert_eq!(s.selected_texture_set().layer_count(), 7, "Layer 1, A, F with C, plus copies of A and of F with its child");
    assert_eq!(s.history().entries().len(), n + 1);
    s.execute(Command::SetLayersVisible { layers: vec![a, f], visible: false }).unwrap();
    assert!(!s.selected_texture_set().find(a).unwrap().visible && !s.selected_texture_set().find(f).unwrap().visible);
    s.execute(Command::DeleteLayers(vec![a, f, c])).unwrap();
    assert_eq!(s.selected_texture_set().layer_count(), 4, "Layer 1 and the copies remain");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), 5);
}

#[test]
fn layers_in_different_texture_sets_cannot_be_mixed() {
    let mut s = Session::sample();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| t.id).collect();
    if sets.len() < 2 { return; }
    let a = add(&mut s, NewLayer::Fill, "A");
    s.execute(Command::SelectTextureSet(sets[1])).unwrap();
    let b = add(&mut s, NewLayer::Fill, "B");
    assert!(s.execute(Command::DeleteLayers(vec![a, b])).is_err());
}

#[test]
fn paste_across_projects_resamples_rasters_and_leaves_the_clip_reusable() {
    let mut src = session(128);
    let mut layer = Layer::new_paint("Painted", src.selected_texture_set());
    if let LayerContent::Paint(pc) = &mut layer.content {
        pc.channel_mut(ChannelKind::BaseColor, 128);
    }
    let original = layer.id;
    src.execute(Command::InsertLayer { layer: Box::new(layer), label: "Painted".into() }).unwrap();
    let clip = Arc::new(src.copy_layers(&[original]).unwrap());
    assert_eq!(clip.resolution, 128);

    let mut dst = session(64);
    dst.execute(Command::PasteLayers(clip.clone())).unwrap();
    let pasted = dst.selection().layer.unwrap();
    assert_ne!(pasted, original, "new UUID, not the source's");
    let LayerContent::Paint(pc) = &dst.selected_layer().unwrap().content else { panic!("paint layer") };
    let img = &pc.channels[&ChannelKind::BaseColor];
    assert_eq!((img.width(), img.height()), (64, 64), "rasters follow the target resolution");
    dst.execute(Command::PasteLayers(clip)).unwrap();
    assert_eq!(dst.selected_texture_set().layers.len(), 3);
}

#[test]
fn paste_adds_channels_the_layers_need() {
    let ao = ChannelKind::AmbientOcclusion;
    let mut src = session(64);
    src.execute(Command::AddChannel { texture_set: src.selection().texture_set, kind: ao }).unwrap();
    let f = add(&mut src, NewLayer::Fill, "F");
    let clip = Arc::new(src.copy_layers(&[f]).unwrap());
    let mut dst = session(64);
    assert!(!dst.selected_texture_set().has_channel(ao));
    assert_eq!(clip.missing_channels(dst.selected_texture_set()), vec![ao]);
    dst.execute(Command::PasteLayers(clip)).unwrap();
    assert!(dst.selected_texture_set().has_channel(ao));
}

#[test]
fn part_masks_survive_only_a_paste_into_the_same_mesh() {
    let mut a = session(64);
    let l = add(&mut a, NewLayer::Fill, "Parted");
    a.execute(Command::SetParts { layer: l, parts: Some(subcraft_doc::PartMask::new([0])) }).unwrap();
    let clip = Arc::new(a.copy_layers(&[l]).unwrap());
    a.execute(Command::PasteLayers(clip.clone())).unwrap();
    assert!(a.selected_layer().unwrap().parts.is_some(), "same mesh keeps part masks");
    let mut b = Session::new(primitives::uv_sphere(1.0, 16, 8), 64);
    b.execute(Command::PasteLayers(clip)).unwrap();
    assert!(b.selected_layer().unwrap().parts.is_none(), "other mesh: part ids would be meaningless");
}

#[test]
fn effects_copy_paste_between_layers_with_fresh_ids() {
    let mut s = session(64);
    let a = add(&mut s, NewLayer::Fill, "A");
    let b = add(&mut s, NewLayer::Fill, "B");
    s.execute(Command::AddContentEffect { layer: a, kind: ContentEffectKind::Levels { channel: ChannelKind::BaseColor, levels: Levels::default() } }).unwrap();
    let effect = s.selected_texture_set().find(a).unwrap().content_effects[0].id;
    let clip = Arc::new(s.copy_effects(a, LayerTarget::Content, &[effect]).unwrap());
    s.execute(Command::PasteEffects { layer: b, clip }).unwrap();
    let pasted = &s.selected_texture_set().find(b).unwrap().content_effects;
    assert_eq!(pasted.len(), 1);
    assert_ne!(pasted[0].id, effect);
    s.execute(Command::Undo).unwrap();
    assert!(s.selected_texture_set().find(b).unwrap().content_effects.is_empty());
    assert!(s.copy_effects(a, LayerTarget::Content, &[]).is_err(), "nothing selected, nothing copied");
}

#[test]
fn pasting_onto_a_folder_follows_the_adjustment_rule() {
    let mut s = session(64);
    let a = add(&mut s, NewLayer::Fill, "A");
    let f = add(&mut s, NewLayer::Folder, "F");
    s.execute(Command::AddContentEffect { layer: a, kind: ContentEffectKind::Levels { channel: ChannelKind::BaseColor, levels: Levels::default() } }).unwrap();
    let effect = &s.selected_texture_set().find(a).unwrap().content_effects[0];
    let adjustment = effect.kind.is_adjustment();
    let id = effect.id;
    let clip = Arc::new(s.copy_effects(a, LayerTarget::Content, &[id]).unwrap());
    assert_eq!(s.execute(Command::PasteEffects { layer: f, clip }).is_ok(), adjustment);
}

#[test]
fn texture_set_rename_is_undoable_and_hidden_state_is_view_only() {
    let mut s = session(64);
    let ts = s.selection().texture_set;
    let n = s.history().entries().len();
    s.execute(Command::RenameTextureSet { texture_set: ts, name: "  Hull ".into() }).unwrap();
    assert_eq!(s.selected_texture_set().name, "Hull");
    assert!(s.execute(Command::RenameTextureSet { texture_set: ts, name: "   ".into() }).is_err());
    s.execute(Command::SetTextureSetHidden { texture_set: ts, hidden: true }).unwrap();
    assert!(s.is_set_hidden(ts));
    assert_eq!(s.history().entries().len(), n + 1, "hiding adds no undo step");
    s.execute(Command::Undo).unwrap();
    assert_ne!(s.selected_texture_set().name, "Hull");
    assert!(s.is_set_hidden(ts), "undo does not change view state");
}

#[test]
fn top_level_in_order_drops_descendants_and_unknown_ids() {
    let mut ts = TextureSet::new("S", 0, 64);
    let f = ts.insert_above(None, Layer::new_folder("F", &ts.clone()));
    let c = ts.insert(Some(f), 0, Layer::new_paint("C", &ts.clone())).unwrap();
    let o = ts.insert_above(None, Layer::new_paint("O", &ts.clone()));
    assert_eq!(top_level_in_order(&ts, &[o, c, f, LayerId::new()]), vec![f, o]);
    let clip = LayerClip::capture(&ts, &[c, f], &Default::default(), (0, 0)).unwrap();
    assert_eq!(clip.layers.len(), 1, "a folder carries its child; the child is not copied twice");
}

/// QA 2026-11: a channel the texture set gained through a paste got no blend entry on the layers
/// already there. A missing entry means "off", so a layer pasted into an existing folder had its
/// new channel switched off by that folder.
#[test]
fn a_channel_added_by_paste_gets_blend_entries_on_the_existing_layers() {
    let ao = ChannelKind::AmbientOcclusion;
    let mut src = session(64);
    src.execute(Command::AddChannel { texture_set: src.selection().texture_set, kind: ao }).unwrap();
    let f = add(&mut src, NewLayer::Fill, "F");
    let clip = Arc::new(src.copy_layers(&[f]).unwrap());
    let mut dst = session(64);
    let folder = add(&mut dst, NewLayer::Folder, "Folder");
    let paint = add(&mut dst, NewLayer::Paint, "Paint");
    dst.execute(Command::PasteLayers(clip)).unwrap();
    let ts = dst.selected_texture_set();
    assert_eq!(ts.find(folder).unwrap().blend.get(&ao).map(|b| b.mode), Some(subcraft_doc::BlendMode::Passthrough), "the folder lets the new channel through");
    assert_eq!(ts.find(paint).unwrap().blend.get(&ao).map(|b| b.mode), Some(subcraft_doc::BlendMode::Normal));
}

/// QA 2026-11: a copied instance pasted into another texture set or project kept its link. In
/// another texture set it took its source's rasters at that set's resolution (the saved project
/// no longer opened); in another project it was a layer that could never be painted.
#[test]
fn an_instance_pasted_away_from_its_source_becomes_an_ordinary_layer() {
    let mut src = session(128);
    let f = add(&mut src, NewLayer::Fill, "F");
    src.execute(Command::AddMask { layer: f, white: false }).unwrap();
    src.execute(Command::PasteAsInstance(vec![f])).unwrap();
    let instance = src.selection().layer.unwrap();
    let clip = Arc::new(src.copy_layers(&[instance]).unwrap());

    let mut dst = session(64);
    dst.execute(Command::PasteLayers(clip.clone())).unwrap();
    let pasted = dst.selected_layer().unwrap().clone();
    assert_eq!(pasted.instance_of, None, "no source in this project");
    assert_eq!(pasted.mask.as_ref().unwrap().base.width(), 64);
    assert!(!dst.instance_locked(pasted.id));

    // Next to its source it stays an instance.
    src.execute(Command::PasteLayers(clip)).unwrap();
    assert_eq!(src.selected_layer().unwrap().instance_of, Some(f));

    // Another texture set of the same project.
    let mut s = Session::sample();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| t.id).collect();
    if sets.len() < 2 { return; }
    let a = add(&mut s, NewLayer::Fill, "A");
    s.execute(Command::PasteAsInstance(vec![a])).unwrap();
    let instance = s.selection().layer.unwrap();
    let clip = Arc::new(s.copy_layers(&[instance]).unwrap());
    let other = *sets.iter().find(|&&t| t != s.selection().texture_set).unwrap();
    s.execute(Command::SelectTextureSet(other)).unwrap();
    s.execute(Command::PasteLayers(clip)).unwrap();
    assert_eq!(s.selected_layer().unwrap().instance_of, None, "the source is in another texture set");
}
