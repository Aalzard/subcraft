//! Regressions found by the QA sweep (2026-11): each test failed before its fix.

use std::path::PathBuf;

use subcraft_app::{Command, CommandError, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{ContentEffectKind, FillChannel, FillSource};
use subcraft_mesh::primitives;

fn session() -> Session {
    Session::new(primitives::sample_crate(), 128)
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-qa-{name}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An anchor point could be added to a folder and saved, but the loader refused the file
/// ("source effects on a folder"), so the project (and its recovery copies) never opened again.
#[test]
fn a_project_with_an_anchor_point_on_a_folder_opens_again() {
    let dir = temp_dir("folder-anchor");
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Folder)).unwrap();
    let folder = s.selection().layer.unwrap();
    s.execute(Command::AddContentEffect { layer: folder, kind: ContentEffectKind::Anchor }).unwrap();
    let path = dir.join("folder-anchor.subcraft");
    s.save(Some(&path)).unwrap();
    let reopened = Session::open_project(&path).expect("a saved project must open");
    assert_eq!(reopened.document(), s.document());

    // The anchor on a folder can also be edited (renamed), not only added.
    let mut effect = (*s.selected_texture_set().find(folder).unwrap().content_effects[0]).clone();
    effect.name = "Folder anchor".into();
    s.execute(Command::SetContentEffect { layer: folder, effect }).unwrap();
    assert_eq!(s.selected_texture_set().find(folder).unwrap().content_effects[0].name, "Folder anchor");
    std::fs::remove_dir_all(dir).unwrap();
}

/// `clamp` keeps NaN; a NaN in the document is written as JSON `null` and the file is rejected
/// on open ("invalid type: null, expected f32").
#[test]
fn values_that_are_not_numbers_are_refused_and_never_reach_the_file() {
    let dir = temp_dir("nan");
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    let before = s.document().clone();
    for opacity in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let r = s.execute(Command::SetBlendOpacity { layer, channel: ChannelKind::BaseColor, opacity });
        assert!(matches!(r, Err(CommandError::Invalid(_))), "opacity {opacity}: {r:?}");
    }
    let mut fill = FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.5)));
    fill.strength = f32::NAN;
    let r = s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Roughness, fill });
    assert!(matches!(r, Err(CommandError::Invalid(_))), "{r:?}");
    assert_eq!(s.document(), &before, "refused commands leave the document alone");

    // In range values are still clamped, not refused.
    s.execute(Command::SetBlendOpacity { layer, channel: ChannelKind::BaseColor, opacity: 2.5 }).unwrap();
    assert_eq!(s.selected_layer().unwrap().blend[&ChannelKind::BaseColor].opacity, 1.0);
    let path = dir.join("nan.subcraft");
    s.save(Some(&path)).unwrap();
    assert!(!s.is_modified());
    Session::open_project(&path).expect("a saved project must open");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Deleting a folder left the selection on a layer inside it, which no longer exists.
#[test]
fn deleting_a_folder_moves_the_selection_off_its_children() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let child = s.selection().layer.unwrap();
    s.execute(Command::GroupLayer(child)).unwrap();
    s.execute(Command::SelectLayer(Some(child))).unwrap();
    let folder = s.selected_texture_set().location(child).unwrap().parent.unwrap();
    s.execute(Command::DeleteLayer(folder)).unwrap();
    assert!(s.selected_texture_set().find(child).is_none());
    let selected = s.selection().layer;
    assert!(selected.is_none_or(|l| s.selected_texture_set().find(l).is_some()), "selection {selected:?} names a deleted layer");
    assert!(selected.is_some(), "another layer of the set is selected");
}

/// Duplicating a layer of another texture set selected the copy's id while the selected texture
/// set stayed put, so the selection named a layer the selected set does not have.
#[test]
fn duplicating_a_layer_of_another_texture_set_keeps_the_selection_valid() {
    let mut s = session();
    assert!(s.document().texture_sets.len() >= 2, "the sample crate has two texture sets");
    let selected = s.selection();
    let other = s.document().texture_sets.iter().find(|t| t.id != selected.texture_set).unwrap();
    let (other_id, other_layer, count) = (other.id, other.layers[0].id, other.layers.len());
    for cmd in [Command::DuplicateLayer(other_layer), Command::DuplicateLayers(vec![other_layer])] {
        s.execute(cmd).unwrap();
        let sel = s.selection();
        assert_eq!(sel.texture_set, selected.texture_set);
        assert!(sel.layer.is_none_or(|l| s.selected_texture_set().find(l).is_some()), "selection {:?} is not in the selected texture set", sel.layer);
    }
    assert_eq!(s.document().texture_sets.iter().find(|t| t.id == other_id).unwrap().layers.len(), count + 2, "both copies were made");
}

/// With the background compositor running (as in the app), changing a texture set's Base Color
/// filtering was not seen as a change, so the viewport and exports kept the old filtering until
/// some layer was edited.
#[test]
fn an_albedo_filter_change_reaches_the_background_compositor() {
    use std::sync::Arc;
    use subcraft_doc::{Layer, LayerContent, Resource, ResourceKind, ResourceStore};
    use subcraft_image::{Bitmap, TextureFilter};
    let mesh = primitives::plane(2., 1);
    let mut doc = Session::new(mesh.clone(), 128).document().clone();
    let ts = &mut doc.texture_sets[0];
    ts.layers.clear();
    let bitmap = Bitmap::new_u8(2, 1, 3, vec![0, 0, 0, 255, 255, 255]);
    let (bytes, _) = subcraft_image::encode_bitmap(&bitmap).unwrap();
    let mut resources = ResourceStore::default();
    let id = subcraft_core::ResourceId::new();
    resources.insert(Resource { id, name: "pixels.png".into(), kind: ResourceKind::Texture, bitmap: Arc::new(bitmap), encoded: Some(Arc::new(bytes)), source_path: None });
    let mut layer = Layer::new_fill("Pixels", ts);
    if let LayerContent::Fill(fill) = &mut layer.content {
        fill.channels.insert(ChannelKind::BaseColor, FillChannel::new(FillSource::Bitmap { resource: id }));
    }
    ts.layers.push(Arc::new(layer));
    let ts = ts.id;
    let mut s = Session::from_parts(mesh, doc, resources, "QA");
    s.update_composites_within(None); // starts the background compositor, as the app's frames do
    s.update_composites();
    let linear = s.composite(ts).unwrap().clone();
    assert!(linear.channel(ChannelKind::BaseColor).unwrap().get(63, 64)[0] > 0.3, "bilinear: the edge is blended");

    s.execute(Command::SetAlbedoFilter { texture_set: ts, filter: TextureFilter::Nearest }).unwrap();
    s.update_composites();
    let base = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    assert_eq!((base.get(63, 64)[0], base.get(64, 64)[0]), (0.0, 1.0), "nearest: a hard edge");
}
