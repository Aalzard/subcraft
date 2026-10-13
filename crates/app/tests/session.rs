//! Session behaviour: commands + history, stroke lifecycle, composites, actions.

use glam::{Vec2, Vec3};
use subcraft_app::{Action, Camera, Command, CommandError, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{BlendMode, FillChannel, FillSource, LayerContent, LayerKind};
use subcraft_mesh::primitives;

fn session() -> Session {
    Session::new(primitives::sample_crate(), 256)
}

fn camera() -> Camera {
    Camera::perspective(Vec3::new(1.5, 1.2, 3.0), Vec3::ZERO, Vec3::Y, 40.0, 0.05, 100.0, [800, 600])
}

#[test]
fn new_session_has_a_paint_layer_per_texture_set() {
    let s = session();
    let doc = s.document();
    assert_eq!(doc.texture_sets.len(), 2);
    assert_eq!(doc.texture_sets[0].name, "Body");
    for ts in &doc.texture_sets {
        assert_eq!(ts.layers.len(), 1);
        assert_eq!(ts.layers[0].kind(), LayerKind::Paint);
    }
    assert_eq!(s.selected_layer().unwrap().name, "Layer 1");
    assert!(!s.history().can_undo());
}

#[test]
fn add_layer_undo_redo_and_selection() {
    let mut s = session();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let fill = s.selection().layer.unwrap();
    assert_eq!(s.selected_layer().unwrap().kind(), LayerKind::Fill);
    assert_eq!(s.selected_texture_set().layers.len(), 2);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), 1);
    assert_ne!(s.selection().layer, Some(fill), "selection falls back to an existing layer");
    s.execute(Command::Redo).unwrap();
    assert!(s.selected_texture_set().find(fill).is_some());
    assert_eq!(s.history().undo_label(), Some("New Fill Layer"));
}

#[test]
fn opacity_drag_coalesces_until_sealed() {
    let mut s = session();
    let layer = s.selection().layer.unwrap();
    for o in [0.9, 0.7, 0.5] {
        s.execute(Command::SetBlendOpacity { layer, channel: ChannelKind::BaseColor, opacity: o }).unwrap();
    }
    assert_eq!(s.history().entries().len(), 2);
    s.execute(Command::Seal).unwrap();
    s.execute(Command::SetBlendOpacity { layer, channel: ChannelKind::BaseColor, opacity: 0.2 }).unwrap();
    assert_eq!(s.history().entries().len(), 3);
    assert_eq!(s.selected_layer().unwrap().blend_for(ChannelKind::BaseColor).opacity, 0.2);
}

#[test]
fn layer_edits_and_errors() {
    let mut s = session();
    let base = s.selection().layer.unwrap();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let top = s.selection().layer.unwrap();
    s.execute(Command::MoveDown(top)).unwrap();
    assert_eq!(s.selected_texture_set().layers[0].id, top);
    assert!(s.execute(Command::MoveDown(top)).is_err(), "already at bottom");
    s.execute(Command::SetBlendMode { layer: base, channel: ChannelKind::BaseColor, mode: BlendMode::Multiply }).unwrap();
    s.execute(Command::SetVisible { layer: base, visible: false }).unwrap();
    s.execute(Command::Rename { layer: base, name: "Dirt".into() }).unwrap();
    let l = s.selected_texture_set().find(base).unwrap();
    assert_eq!((l.name.as_str(), l.visible, l.blend_for(ChannelKind::BaseColor).mode), ("Dirt", false, BlendMode::Multiply));
    assert_eq!(
        s.execute(Command::SetFillChannel { layer: base, channel: ChannelKind::Roughness, fill: FillChannel { enabled: true, source: FillSource::uniform(ChannelValue::scalar(0.1)) } }),
        Err(CommandError::NotAFillLayer)
    );
    s.execute(Command::AddMask { layer: base, white: false }).unwrap();
    assert!(s.selected_texture_set().find(base).unwrap().mask.is_some());
    s.execute(Command::DeleteLayer(top)).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), 1);
    assert_eq!(s.selection().layer, Some(base));
}

#[test]
fn stroke_is_one_history_entry_and_updates_the_composite() {
    let mut s = session();
    let first = s.update_composites();
    assert_eq!(first.len(), 2, "both texture sets evaluated initially");
    assert!(s.update_composites().is_empty());

    let cam = camera();
    let center = cam.project(Vec3::new(0.0, 0.0, 0.5)).unwrap().truncate();
    s.brush.settings.size = 10.0;
    s.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0));
    s.begin_stroke().unwrap();
    assert!(s.is_stroking());
    assert_eq!(s.execute(Command::AddLayer(NewLayer::Paint)), Err(CommandError::StrokeInProgress));
    let mut painted = 0;
    for k in 0..10 {
        painted += s.stroke_to(&cam, center + Vec2::new(k as f32 * 8.0, 0.0), false).unwrap().len();
    }
    assert!(painted > 0);
    s.end_stroke();
    assert_eq!(s.history().entries().len(), 2);
    assert_eq!(s.history().undo_label(), Some("Paint Stroke"));

    let updates = s.update_composites();
    assert_eq!(updates.len(), 1, "only the Body texture set changed");
    assert!(!updates[0].tiles.is_empty() && updates[0].tiles.len() < 16, "{:?}", updates[0].tiles);
    let body = s.document().texture_sets[0].id;
    let c = s.composite(body).unwrap().channel(ChannelKind::BaseColor).unwrap().clone();
    s.execute(Command::Undo).unwrap();
    s.update_composites();
    assert_ne!(&c, s.composite(body).unwrap().channel(ChannelKind::BaseColor).unwrap());
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    assert!(p.channels.is_empty(), "undo removes the stroke");
}

#[test]
fn empty_stroke_adds_no_history_and_cancel_restores() {
    let mut s = session();
    s.begin_stroke().unwrap();
    s.stroke_to(&camera(), Vec2::new(1.0, 1.0), false).unwrap(); // misses the mesh
    s.end_stroke();
    assert_eq!(s.history().entries().len(), 1);

    let cam = camera();
    let p = cam.project(Vec3::new(0.0, 0.0, 0.5)).unwrap().truncate();
    s.begin_stroke().unwrap();
    s.stroke_to(&cam, p, false).unwrap();
    assert_ne!(s.document(), s.history().current());
    s.cancel_stroke();
    assert_eq!(s.document(), s.history().current());
}

#[test]
fn actions_drive_the_session() {
    let mut s = session();
    let size = s.brush.settings.size;
    assert!(s.perform(Action::BrushLarger).unwrap());
    assert!(s.brush.settings.size > size);
    s.perform(Action::NewFillLayer).unwrap();
    assert_eq!(s.selected_layer().unwrap().kind(), LayerKind::Fill);
    s.perform(Action::Undo).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), 1);
    assert!(!s.perform(Action::ResetCamera).unwrap(), "camera actions belong to the front end");
    s.perform(Action::ToggleSymmetryX).unwrap();
    assert!(s.brush.options.symmetry[0]);
}

#[test]
fn pick_reports_material_slot() {
    let s = session();
    let cam = camera();
    let p = s.pick(&cam, cam.project(Vec3::new(0.0, 0.0, 0.5)).unwrap().truncate()).unwrap();
    assert_eq!(p.material_slot, 0);
    assert!((p.position.z - 0.5).abs() < 1e-3);
    assert!(s.pick(&cam, Vec2::new(1.0, 1.0)).is_none());
}

#[test]
fn save_and_reopen_project() {
    let dir = std::env::temp_dir().join(format!("subcraft-app-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.subcraft");
    let mut s = session();
    assert!(!s.is_modified());
    assert!(s.save(None).is_err(), "no path yet");
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let cam = camera();
    s.begin_stroke().unwrap();
    s.stroke_to(&cam, cam.project(Vec3::new(0.0, 0.0, 0.5)).unwrap().truncate(), false).unwrap();
    s.end_stroke();
    assert!(s.is_modified());
    s.save(Some(&path)).unwrap();
    assert!(!s.is_modified());
    assert_eq!(s.project_path(), Some(path.as_path()));

    let reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.document(), s.document());
    assert!(!reopened.is_modified());
    assert!(!reopened.history().can_undo(), "fresh history after open");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn export_writes_maps_for_every_texture_set() {
    let mut s = session();
    let settings = subcraft_app::ExportSettings::default();
    let plan = s.export_plan(&settings);
    // Body + Trim, each BaseColor/Roughness/Metallic/Normal/Height.
    assert_eq!(plan.len(), 10, "{plan:?}");
    let dir = std::env::temp_dir().join(format!("subcraft-app-export-{}", std::process::id()));
    let files = s.export_textures(&dir, &settings).unwrap();
    assert_eq!(files.len(), 10);
    assert!(files.iter().all(|f| f.exists()));
    assert!(files[0].file_name().unwrap().to_str().unwrap().starts_with("SampleCrate_Body_"));
    std::fs::remove_dir_all(&dir).unwrap();
}
