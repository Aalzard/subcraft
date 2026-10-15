use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, NewLayer, Session, pen::{Handle, PenEditor, PenHit}};
use subcraft_core::{ChannelKind, ChannelValue, ObjectId};
use subcraft_doc::{LayerContent, LayerKind};

fn session() -> Session {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
    s.brush.material = subcraft_paint::BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]); s
}

fn finish(editor: &mut PenEditor, s: &mut Session) {
    for _ in 0..10000 { if !editor.poll(s, 16384).unwrap() { return; } }
    panic!("pen raster job did not finish");
}

fn point(editor: &mut PenEditor, s: &mut Session, p: Vec2) {
    editor.begin(s, p, None).unwrap(); editor.release(s).unwrap(); finish(editor, s);
}

fn pen(s: &Session) -> &subcraft_doc::PenPath {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() }; p.path.as_ref().unwrap()
}

#[test]
fn click_drag_creates_editable_curve_as_one_history_entry_per_gesture() {
    let mut s = session(); let mut editor = PenEditor::default(); let before = s.document().clone();
    editor.begin(&s, Vec2::new(0.2, 0.5), None).unwrap(); editor.drag_to(Vec2::new(0.2, 0.2), false); editor.release(&s).unwrap();
    assert_eq!(s.document(), &before); assert!(editor.is_pending());
    finish(&mut editor, &mut s); let layer = s.selection().layer.unwrap();
    assert_eq!(s.selected_layer().unwrap().kind(), LayerKind::Path); assert_eq!(s.history().entries().len(), 2);
    assert_eq!(pen(&s).anchors[0].incoming, Vec2::new(0.0, 0.3));
    point(&mut editor, &mut s, Vec2::new(0.8, 0.5)); let after = s.document().clone();
    assert_eq!(s.selection().layer, Some(layer)); assert_eq!(pen(&s).anchors.len(), 2);
    s.execute(Command::Undo).unwrap(); assert_eq!(pen(&s).anchors.len(), 1);
    s.execute(Command::Redo).unwrap(); assert_eq!(s.document(), &after);
}

#[test]
fn moving_points_preserves_tangents_shift_separates_handles_and_cancel_is_lossless() {
    let mut s = session(); let mut editor = PenEditor::default(); point(&mut editor, &mut s, Vec2::splat(0.4));
    let id = pen(&s).anchors[0].id;
    editor.begin(&s, Vec2::splat(0.4), Some(PenHit { anchor: id, handle: Handle::Outgoing })).unwrap();
    editor.drag_to(Vec2::new(0.6, 0.4), false); editor.release(&s).unwrap(); finish(&mut editor, &mut s);
    let tangents = (pen(&s).anchors[0].incoming, pen(&s).anchors[0].outgoing);
    editor.begin(&s, Vec2::splat(0.4), Some(PenHit { anchor: id, handle: Handle::Anchor })).unwrap();
    editor.drag_to(Vec2::splat(0.5), false); editor.release(&s).unwrap(); finish(&mut editor, &mut s);
    assert_eq!((pen(&s).anchors[0].incoming, pen(&s).anchors[0].outgoing), tangents);
    editor.begin(&s, Vec2::splat(0.5), Some(PenHit { anchor: id, handle: Handle::Incoming })).unwrap();
    editor.drag_to(Vec2::new(0.3, 0.7), true); editor.release(&s).unwrap(); finish(&mut editor, &mut s);
    assert_eq!(pen(&s).anchors[0].outgoing, tangents.1);
    let before = s.document().clone(); let count = s.history().entries().len();
    editor.begin(&s, Vec2::splat(0.5), Some(PenHit { anchor: id, handle: Handle::Anchor })).unwrap(); editor.drag_to(Vec2::ZERO, false); editor.cancel();
    assert!(!editor.poll(&mut s, 100).unwrap()); assert_eq!(s.document(), &before); assert_eq!(s.history().entries().len(), count);
}

#[test]
fn pending_jobs_survive_selection_changes_but_cannot_overwrite_document_or_scene_changes() {
    let mut s = session(); let mut editor = PenEditor::default(); let base = s.selection().layer;
    editor.begin(&s, Vec2::splat(0.5), None).unwrap(); editor.release(&s).unwrap();
    s.execute(Command::SelectLayer(None)).unwrap(); finish(&mut editor, &mut s);
    assert_eq!(s.selection().layer, None); assert!(s.selected_texture_set().layers.iter().any(|l| l.kind() == LayerKind::Path));
    s.execute(Command::SelectLayer(base)).unwrap(); editor.new_path();
    editor.begin(&s, Vec2::splat(0.6), None).unwrap(); editor.release(&s).unwrap();
    s.execute(Command::DeleteLayer(base.unwrap())).unwrap(); let before = s.document().clone();
    assert!(editor.poll(&mut s, usize::MAX).is_err()); assert_eq!(s.document(), &before);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    editor.begin(&s, Vec2::splat(0.3), None).unwrap(); editor.release(&s).unwrap();
    let mut other = Session::from_parts(s.scene().mesh.as_ref().clone(), s.document().clone(), s.resources().clone(), "reloaded");
    assert!(editor.poll(&mut other, usize::MAX).is_err());
}

#[test]
fn finish_keeps_pending_work_and_new_path_uses_a_separate_layer() {
    let mut s = session(); let mut editor = PenEditor::default();
    editor.begin(&s, Vec2::splat(0.4), None).unwrap(); editor.release(&s).unwrap(); editor.finish_path();
    finish(&mut editor, &mut s); let first = s.selection().layer.unwrap();
    point(&mut editor, &mut s, Vec2::splat(0.6)); assert_ne!(s.selection().layer, Some(first));
}

#[test]
fn delete_smooth_and_corner_are_undoable_and_width_style_rerenders() {
    let mut s = session(); let mut editor = PenEditor::default();
    for p in [Vec2::new(0.2, 0.4), Vec2::new(0.5, 0.6), Vec2::new(0.8, 0.4)] { point(&mut editor, &mut s, p); }
    let last = pen(&s).anchors[2].id;
    editor.shape_point(&s, true).unwrap(); finish(&mut editor, &mut s); assert_ne!(pen(&s).anchors[2].incoming, Vec2::ZERO);
    editor.shape_point(&s, false).unwrap(); finish(&mut editor, &mut s); assert_eq!(pen(&s).anchors[2].incoming, Vec2::ZERO);
    editor.delete_anchor(&s).unwrap(); finish(&mut editor, &mut s); assert!(!pen(&s).anchors.iter().any(|a| a.id == last));
    s.execute(Command::Undo).unwrap(); assert!(pen(&s).anchors.iter().any(|a| a.id == last));
    let mut path = pen(&s).clone(); path.closed = true; path.radius = 0.1; path.opacity = 0.25;
    editor.set_path(&s, path.clone()).unwrap(); editor.release(&s).unwrap(); finish(&mut editor, &mut s); assert_eq!(pen(&s), &path);
}

#[test]
fn surface_placement_uses_uv_tile_wrapping_and_matches_surface_geometry() {
    let camera = Camera::perspective(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y, 40.0, 0.05, 100.0, [800, 600]);
    let point = Vec3::new(0.25, 0.2, 1.0); let screen = camera.project(point).unwrap().truncate();
    let mut mesh = subcraft_mesh::primitives::cube(2.0); for uv in &mut mesh.uvs { *uv += Vec2::new(3.0, 2.0); }
    let mut s = Session::new(mesh, 128); let mut editor = PenEditor::default();
    editor.begin_surface(&mut s, &camera, screen, None).unwrap(); editor.release(&s).unwrap(); finish(&mut editor, &mut s);
    let uv = pen(&s).anchors[0].position; assert!(uv.min_element() >= 0.0 && uv.max_element() <= 1.0);
    assert!(s.pen_uv_position(uv).unwrap().distance(point) < 0.1);
    assert_eq!(s.begin_stroke(), Err(subcraft_paint::PaintError::EditablePath));
}

#[test]
fn project_and_update_snapshot_keep_editable_points_and_channel_cache() {
    let mut s = session(); let mut editor = PenEditor::default(); point(&mut editor, &mut s, Vec2::new(0.2, 0.3)); point(&mut editor, &mut s, Vec2::new(0.8, 0.5));
    let layer = s.selection().layer.unwrap(); let path = pen(&s).clone();
    let file = std::env::temp_dir().join(format!("subcraft-pen-{}.subcraft", ObjectId::new()));
    s.snapshot().save(&file).unwrap(); let reopened = Session::open_project(&file).unwrap();
    assert_eq!(reopened.document(), s.document()); assert_eq!(pen(&reopened), &path);
    assert!(reopened.selected_texture_set().find(layer).is_some()); std::fs::remove_file(file).unwrap();
}

#[test]
fn rapid_points_keep_individual_undo_steps_and_undo_pending_only_removes_latest() {
    let mut s = session(); let mut editor = PenEditor::default();
    for x in [0.2, 0.5, 0.8] { editor.begin(&s, Vec2::new(x, 0.5), None).unwrap(); editor.release(&s).unwrap(); }
    assert_eq!(editor.preview(&s).unwrap().anchors.len(), 3); assert_eq!(s.history().entries().len(), 1);
    editor.undo_pending(&s); assert_eq!(editor.preview(&s).unwrap().anchors.len(), 2);
    finish(&mut editor, &mut s); assert_eq!(pen(&s).anchors.len(), 2); assert_eq!(s.history().entries().len(), 3);
    s.execute(Command::Undo).unwrap(); assert_eq!(pen(&s).anchors.len(), 1);
    s.execute(Command::Redo).unwrap(); assert_eq!(pen(&s).anchors.len(), 2);
}

#[test]
fn job_commit_preserves_renames_masks_and_folder_moves_made_while_it_runs() {
    let mut s = session(); let mut editor = PenEditor::default(); point(&mut editor, &mut s, Vec2::splat(0.4));
    let layer = s.selection().layer.unwrap(); let anchor = pen(&s).anchors[0].id;
    editor.begin(&s, Vec2::splat(0.4), Some(PenHit { anchor, handle: Handle::Anchor })).unwrap();
    editor.drag_to(Vec2::splat(0.6), false); editor.release(&s).unwrap();
    s.execute(Command::Rename { layer, name: "Detail curve".into() }).unwrap();
    s.execute(Command::AddMask { layer, white: true }).unwrap(); s.execute(Command::GroupLayer(layer)).unwrap();
    finish(&mut editor, &mut s);
    let saved = s.selected_texture_set().find(layer).unwrap(); assert_eq!(saved.name, "Detail curve"); assert!(saved.mask.is_some());
    assert!(s.selected_texture_set().location(layer).unwrap().parent.is_some());
    assert_eq!(pen(&s).anchors[0].position, Vec2::splat(0.6));
}
