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

/// QA 2026-11: an instance copied into another texture set kept following its source there,
/// so the next sync copied in rasters at the other set's resolution. It pastes as a plain copy.
#[test]
fn an_instance_pasted_into_another_texture_set_becomes_a_plain_copy() {
    use glam::{Vec2, Vec3};
    use subcraft_mesh::{Corner, MeshBuilder};
    let mut b = MeshBuilder::new("Two");
    for (i, name) in ["A", "B"].into_iter().enumerate() {
        let m = b.material(name);
        let x = i as f32 * 3.0;
        let c = |dx: f32, z: f32| Corner { position: Vec3::new(x + dx, 0.0, z), normal: Some(Vec3::Y), uv: Some(Vec2::new(dx + 1.0, z + 1.0) * 0.45) };
        b.push_triangle(m, [c(-1.0, -1.0), c(-1.0, 1.0), c(1.0, 1.0)]);
    }
    let mut s = Session::new(b.build().unwrap(), 128);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let source = s.selection().layer.unwrap();
    s.execute(Command::PasteAsInstance(vec![source])).unwrap();
    let instance = s.selection().layer.unwrap();
    let clip = std::sync::Arc::new(s.copy_layers(&[instance]).unwrap());
    // Same set: still an instance of the source.
    s.execute(Command::PasteLayers(clip.clone())).unwrap();
    let same = s.selection().layer.unwrap();
    assert_eq!(s.selected_texture_set().find(same).unwrap().instance_of, Some(source));
    // Another set: a plain layer.
    let other = s.document().texture_sets[1].id;
    s.execute(Command::SelectTextureSet(other)).unwrap();
    s.execute(Command::PasteLayers(clip)).unwrap();
    let pasted = s.selection().layer.unwrap();
    assert_eq!(s.selected_texture_set().find(pasted).unwrap().instance_of, None);
    assert!(!s.instance_locked(pasted));
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

/// Two quads, one material each: a project with two texture sets.
fn two_sets() -> Session {
    use glam::{Vec2, Vec3};
    use subcraft_mesh::{Corner, MeshBuilder};
    let mut b = MeshBuilder::new("Parts");
    for (i, name) in ["Body", "Trim"].into_iter().enumerate() {
        let m = b.material(name);
        let origin = Vec3::new(2.0 * i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    Session::new(b.build().unwrap(), 64)
}

#[test]
fn instantiating_across_texture_sets_adds_a_linked_instance_on_top_of_the_other_set() {
    let mut s = two_sets();
    let (body, trim) = (s.document().texture_sets[0].id, s.document().texture_sets[1].id);
    s.execute(Command::SelectTextureSet(body)).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let source = s.selection().layer.unwrap();
    let before = s.document().texture_set(trim).unwrap().layers.len();
    let steps = s.history().entries().len();
    // The layer's own set is skipped; nothing ticked is refused.
    assert!(s.execute(Command::InstantiateAcross { layers: vec![source], texture_sets: vec![body] }).is_err());
    s.execute(Command::InstantiateAcross { layers: vec![source], texture_sets: vec![body, trim] }).unwrap();
    assert_eq!(s.history().entries().len(), steps + 1);
    let set = s.document().texture_set(trim).unwrap();
    assert_eq!(set.layers.len(), before + 1);
    let instance = set.layers.last().unwrap().clone();
    assert_eq!(instance.instance_of, Some(source));
    assert_eq!(s.instance_state(instance.id), Some(InstanceState::Linked));
    assert!(s.instance_locked(instance.id));
    // An edit of the source reaches the other texture set.
    s.execute(Command::AddMask { layer: source, white: false }).unwrap();
    assert!(s.document().texture_set(trim).unwrap().find(instance.id).unwrap().mask.is_some());
    // Deleting the source removes the instance there, and undo brings both back.
    s.execute(Command::DeleteLayer(source)).unwrap();
    assert!(s.document().texture_set(trim).unwrap().find(instance.id).is_none());
    s.execute(Command::Undo).unwrap();
    assert!(s.document().texture_set(trim).unwrap().find(instance.id).is_some());
}

#[test]
fn a_layer_copied_in_one_texture_set_pastes_as_an_instance_in_another() {
    let mut s = two_sets();
    let (body, trim) = (s.document().texture_sets[0].id, s.document().texture_sets[1].id);
    s.execute(Command::SelectTextureSet(body)).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let source = s.selection().layer.unwrap();
    assert!(s.instance_channel_gaps(&[source], trim).is_empty(), "both sets have the same channels");
    s.execute(Command::SelectTextureSet(trim)).unwrap();
    s.execute(Command::PasteAsInstance(vec![source])).unwrap();
    let instance = s.selection().layer.unwrap();
    let set = s.document().texture_set(trim).unwrap();
    assert_eq!(set.find(instance).unwrap().instance_of, Some(source));
    assert_eq!(set.find(instance).unwrap().content, s.document().texture_set(body).unwrap().find(source).unwrap().content);
    // A layer that is gone cannot be instantiated.
    assert!(s.execute(Command::PasteAsInstance(vec![LayerId::new()])).is_err());
}

/// QA 2026-11: multi-select commands acted on layers inside an instance (the sync undid them:
/// an empty undo step, or a duplicated layer id after grouping one out of the instance).
#[test]
fn multi_select_commands_refuse_layers_inside_an_instance() {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 2), 128);
    s.execute(Command::AddLayer(NewLayer::Folder)).unwrap();
    let folder = s.selection().layer.unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    // The fill went inside the empty folder; with the folder selected, its instance lands above it.
    s.execute(Command::SelectLayer(Some(folder))).unwrap();
    s.execute(Command::PasteAsInstance(vec![folder])).unwrap();
    let instance = s.selection().layer.unwrap();
    let child = s.selected_texture_set().find(instance).unwrap().children()[0].id;
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let plain = s.selection().layer.unwrap();
    let doc = s.document().clone();
    let steps = s.history().entries().len();
    for cmd in [
        Command::SetLayersVisible { layers: vec![plain, child], visible: false },
        Command::DeleteLayers(vec![plain, child]),
        Command::DuplicateLayers(vec![child]),
        Command::GroupLayers(vec![plain, child]),
    ] {
        let err = s.execute(cmd).unwrap_err();
        assert!(err.to_string().contains("instance"), "{err}");
    }
    assert_eq!(s.document(), &doc);
    assert_eq!(s.history().entries().len(), steps);
    // The instance itself is the user's: hiding it with another layer works.
    s.execute(Command::SetLayersVisible { layers: vec![plain, instance], visible: false }).unwrap();
    assert!(!s.selected_texture_set().find(instance).unwrap().visible);
}
