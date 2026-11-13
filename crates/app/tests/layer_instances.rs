//! Painter's layer instancing, part a: Paste as instance inside one texture set.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{InstanceState, MaskEffectKind};

/// A session with a fill layer selected, and that layer's id.
fn session() -> (Session, LayerId) {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 2), 128);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let fill = s.selection().layer.unwrap();
    (s, fill)
}

fn effects(s: &Session, layer: LayerId) -> usize {
    s.selected_texture_set().find(layer).unwrap().mask.as_ref().map_or(0, |m| m.effects.len())
}

#[test]
fn a_pasted_instance_follows_its_source_within_the_same_undo_step() {
    let (mut s, source) = session();
    let layers = s.selected_texture_set().layers.len();
    s.execute(Command::PasteAsInstance(vec![source])).unwrap();
    let instance = s.selection().layer.unwrap();
    assert_ne!(instance, source);
    let ts = s.selected_texture_set();
    assert_eq!(ts.layers.len(), layers + 1);
    assert_eq!(ts.location(instance).unwrap().index, ts.location(source).unwrap().index + 1, "above the selection");
    assert_eq!(ts.find(instance).unwrap().instance_of, Some(source));
    assert_eq!(ts.find(instance).unwrap().name, format!("{} instance", ts.find(source).unwrap().name));
    assert_eq!(s.instance_state(instance), Some(InstanceState::Linked));
    assert_eq!(s.instance_state(source), None);

    // An edit of the source reaches the instance, as one history step.
    let steps = s.history().entries().len();
    s.execute(Command::AddMaskEffect { layer: source, kind: MaskEffectKind::Invert }).unwrap_err();
    s.execute(Command::AddMask { layer: source, white: false }).unwrap();
    s.execute(Command::AddMaskEffect { layer: source, kind: MaskEffectKind::Invert }).unwrap();
    assert_eq!(s.history().entries().len(), steps + 2);
    assert_eq!(effects(&s, instance), 1);
    let ts = s.selected_texture_set();
    assert_eq!(ts.find(instance).unwrap().content, ts.find(source).unwrap().content);
    s.execute(Command::Undo).unwrap();
    assert_eq!(effects(&s, instance), 0, "undo takes the source's edit back from the instance too");
    s.execute(Command::Redo).unwrap();
    assert_eq!(effects(&s, instance), 1);
}

#[test]
fn an_instance_keeps_its_own_visibility_and_blending_and_refuses_content_edits() {
    let (mut s, source) = session();
    s.execute(Command::PasteAsInstance(vec![source])).unwrap();
    let instance = s.selection().layer.unwrap();
    assert!(s.instance_locked(instance));
    assert!(!s.instance_locked(source));
    // Its own settings.
    s.execute(Command::SetVisible { layer: instance, visible: false }).unwrap();
    s.execute(Command::SetBlendOpacity { layer: instance, channel: ChannelKind::BaseColor, opacity: 0.5 }).unwrap();
    s.execute(Command::Rename { layer: instance, name: "Copy of rust".into() }).unwrap();
    // The source moves on; the instance keeps them.
    s.execute(Command::AddMask { layer: source, white: true }).unwrap();
    let l = s.selected_texture_set().find(instance).unwrap().clone();
    assert!(!l.visible);
    assert_eq!(l.blend[&ChannelKind::BaseColor].opacity, 0.5);
    assert_eq!(l.name, "Copy of rust");
    assert!(l.mask.is_some());
    // Content edits are refused and leave no history step.
    let steps = s.history().entries().len();
    let err = s.execute(Command::RemoveMask(instance)).unwrap_err();
    assert!(err.to_string().contains("instance"), "{err}");
    assert!(s.selected_texture_set().find(instance).unwrap().mask.is_some());
    assert_eq!(s.history().entries().len(), steps);
    // No strokes either.
    s.execute(Command::SelectLayer(Some(instance))).unwrap();
    assert_eq!(s.begin_stroke(), Err(subcraft_paint::PaintError::Instance));
}

#[test]
fn deleting_the_source_deletes_its_instances_in_one_step() {
    let (mut s, source) = session();
    s.execute(Command::PasteAsInstance(vec![source])).unwrap();
    let instance = s.selection().layer.unwrap();
    // An instance of the instance follows the same source.
    s.execute(Command::PasteAsInstance(vec![instance])).unwrap();
    let second = s.selection().layer.unwrap();
    assert_eq!(s.selected_texture_set().find(second).unwrap().instance_of, Some(source));
    let layers = s.selected_texture_set().layers.len();
    // Deleting an instance leaves the source and the other instance.
    s.execute(Command::DeleteLayer(second)).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), layers - 1);
    s.execute(Command::DeleteLayer(source)).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), layers - 3);
    assert!(s.selected_texture_set().find(instance).is_none());
    s.execute(Command::Undo).unwrap();
    assert!(s.selected_texture_set().find(instance).is_some());
    assert!(s.selected_texture_set().find(source).is_some());
}

#[test]
fn a_folder_instance_shows_the_layers_of_its_source_and_cannot_go_inside_it() {
    let (mut s, fill) = session();
    s.execute(Command::GroupLayers(vec![fill])).unwrap();
    let folder = s.selection().layer.unwrap();
    // With a layer of the folder selected, the instance would land inside its own source.
    s.execute(Command::SelectLayer(Some(fill))).unwrap();
    assert!(s.execute(Command::PasteAsInstance(vec![folder])).is_err());
    s.execute(Command::SelectLayer(Some(folder))).unwrap();
    s.execute(Command::PasteAsInstance(vec![folder])).unwrap();
    let instance = s.selection().layer.unwrap();
    let child = s.selected_texture_set().find(instance).unwrap().children()[0].clone();
    assert_ne!(child.id, fill, "copied layers have their own ids");
    assert!(s.instance_locked(child.id));
    // A layer added to the source shows up in the instance.
    s.execute(Command::SelectLayer(Some(fill))).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    assert_eq!(s.selected_texture_set().find(folder).unwrap().children().len(), 2);
    assert_eq!(s.selected_texture_set().find(instance).unwrap().children().len(), 2);
    // The copies inside the instance cannot be removed on their own.
    assert!(s.execute(Command::DeleteLayer(child.id)).is_err());
    assert_eq!(s.selected_texture_set().find(instance).unwrap().children().len(), 2);
}

#[test]
fn an_instance_survives_save_and_load() {
    let (mut s, source) = session();
    s.execute(Command::PasteAsInstance(vec![source])).unwrap();
    let instance = s.selection().layer.unwrap();
    let path = std::env::temp_dir().join(format!("instances-{}.subcraft", subcraft_core::ObjectId::new()));
    s.save(Some(&path)).unwrap();
    let mut back = Session::open_project(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(back.selected_texture_set().find(instance).unwrap().instance_of, Some(source));
    assert_eq!(back.instance_state(instance), Some(InstanceState::Linked));
    // Still linked after loading.
    back.execute(Command::AddMask { layer: source, white: true }).unwrap();
    assert!(back.selected_texture_set().find(instance).unwrap().mask.is_some());
}

/// QA 2026-11: duplicating a folder that holds a layer and its instance left the copied instance
/// following the original layer, so deleting the original folder took a layer out of the copy.
#[test]
fn an_instance_duplicated_with_its_source_follows_the_copied_source() {
    let (mut s, fill) = session();
    s.execute(Command::PasteAsInstance(vec![fill])).unwrap();
    let instance = s.selection().layer.unwrap();
    s.execute(Command::GroupLayers(vec![fill, instance])).unwrap();
    let folder = s.selection().layer.unwrap();
    s.execute(Command::DuplicateLayer(folder)).unwrap();
    let copy = s.selection().layer.unwrap();
    assert_ne!(copy, folder);
    {
        let children = s.selected_texture_set().find(copy).unwrap().children();
        assert_eq!(children.len(), 2);
        let source = children.iter().find(|l| l.instance_of.is_none()).unwrap();
        let inst = children.iter().find(|l| l.instance_of.is_some()).unwrap();
        assert_ne!(source.id, fill);
        assert_eq!(inst.instance_of, Some(source.id));
    }
    s.execute(Command::DeleteLayer(folder)).unwrap();
    assert_eq!(s.selected_texture_set().find(copy).unwrap().children().len(), 2, "the copy does not depend on the original");
}
