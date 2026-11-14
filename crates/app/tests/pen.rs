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

/// QA 2026-11: a resolution change resampled the raster of a pen path (text layers were drawn
/// again), so the path stayed blurred until its next edit.
#[test]
fn a_resolution_change_draws_the_path_again_instead_of_resampling_it() {
    let mut s = session(); let mut editor = PenEditor::default();
    point(&mut editor, &mut s, Vec2::new(0.2, 0.2)); point(&mut editor, &mut s, Vec2::new(0.8, 0.7));
    let path = pen(&s).clone();
    let steps = s.history().entries().len();
    s.execute(Command::SetResolution { texture_set: None, resolution: 512 }).unwrap();
    assert_eq!(s.history().entries().len(), steps + 1);
    assert_eq!(pen(&s), &path, "the path itself is unchanged");
    let mut raster = subcraft_paint::pen::PenRasterizer::new(&path, 512).unwrap();
    raster.run();
    let expected = raster.finish().unwrap();
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    assert!(!expected.is_empty());
    assert!(p.channels == expected, "the raster is the path drawn at 512");
}

/// QA 2026-11: a path layer pasted into a set of another resolution (and Paste layer content)
/// was resampled instead of drawn again, so it stayed blurred until its next edit.
#[test]
fn a_path_pasted_at_another_resolution_is_drawn_again() {
    let mut s = session(); let mut editor = PenEditor::default();
    point(&mut editor, &mut s, Vec2::new(0.2, 0.2)); point(&mut editor, &mut s, Vec2::new(0.8, 0.7));
    let path = pen(&s).clone();
    let source = s.selection().layer.unwrap();
    let clip = std::sync::Arc::new(s.copy_layers(&[source]).unwrap());
    s.execute(Command::SetResolution { texture_set: None, resolution: 512 }).unwrap();
    let mut raster = subcraft_paint::pen::PenRasterizer::new(&path, 512).unwrap();
    raster.run();
    let expected = raster.finish().unwrap();
    assert!(!expected.is_empty());
    s.execute(Command::PasteLayers(clip.clone())).unwrap();
    assert_ne!(s.selection().layer, Some(source));
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    assert!(p.channels == expected, "the pasted raster is the path drawn at 512");
    // Paste layer content onto a plain paint layer does the same.
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let target = s.selection().layer.unwrap();
    s.execute(Command::PasteLayerContent { layer: target, clip }).unwrap();
    let LayerContent::Paint(p) = &s.selected_texture_set().find(target).unwrap().content else { panic!() };
    assert!(p.channels == expected, "pasted content is the path drawn at 512");
}

#[test]
fn insert_on_the_curve_close_on_the_first_point_and_toggle_corner_smooth() {
    let mut s = session(); let mut editor = PenEditor::default();
    for p in [Vec2::new(0.2, 0.2), Vec2::new(0.8, 0.2), Vec2::new(0.8, 0.8)] { point(&mut editor, &mut s, p); }
    let before = pen(&s).polyline(0.001).unwrap();
    // A click on the middle of the first segment inserts a point there without changing the curve.
    editor.insert_point(&s, 0, 0.5).unwrap(); finish(&mut editor, &mut s);
    assert_eq!(pen(&s).anchors.len(), 4);
    assert!((pen(&s).anchors[1].position - Vec2::new(0.5, 0.2)).length() < 1e-5);
    let after = pen(&s).polyline(0.001).unwrap();
    assert!(after.iter().all(|p| before.windows(2).any(|w| {
        let d = w[1] - w[0]; let t = if d.length_squared() > 0.0 { ((*p - w[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        p.distance(w[0] + d * t) < 1e-3
    })), "same curve");
    // With the last point selected, a click on the first point closes the path.
    let (first, last) = (pen(&s).anchors[0].id, pen(&s).anchors[3].id);
    assert!(!editor.closes_on(&s, first), "the inserted point is selected, not the last one");
    editor.toggle_point(&s, last).unwrap(); finish(&mut editor, &mut s);
    assert!(pen(&s).is_smooth(last), "a corner becomes smooth");
    assert!(editor.closes_on(&s, first));
    editor.close_path(&s).unwrap(); finish(&mut editor, &mut s);
    assert!(pen(&s).closed);
    editor.toggle_point(&s, last).unwrap(); finish(&mut editor, &mut s);
    assert!(!pen(&s).is_smooth(last), "and back to a corner");
}

#[test]
fn ctrl_scales_both_tangents_and_a_shift_click_point_stays_straight() {
    let mut s = session(); let mut editor = PenEditor::default();
    // Drag on creation: mirrored tangents of length 0.1.
    editor.begin(&s, Vec2::splat(0.5), None).unwrap(); editor.drag_to(Vec2::new(0.6, 0.5), false); editor.release(&s).unwrap(); finish(&mut editor, &mut s);
    let id = pen(&s).anchors[0].id;
    assert!((pen(&s).anchors[0].outgoing - Vec2::new(0.1, 0.0)).length() < 1e-6 && (pen(&s).anchors[0].incoming + pen(&s).anchors[0].outgoing).length() < 1e-6);
    // Ctrl on the outgoing handle: drag it up and twice as long; the incoming one doubles too
    // and keeps pointing left.
    editor.begin(&s, Vec2::new(0.6, 0.5), Some(PenHit { anchor: id, handle: Handle::Outgoing })).unwrap();
    editor.drag_to_scaled(Vec2::new(0.5, 0.7), false, true); editor.release(&s).unwrap(); finish(&mut editor, &mut s);
    let a = &pen(&s).anchors[0];
    assert!((a.outgoing - Vec2::new(0.0, 0.2)).length() < 1e-6);
    assert!((a.incoming - Vec2::new(-0.2, 0.0)).length() < 1e-6, "{:?}", a.incoming);
    // A Shift click: the new point gets no tangents even when the pointer is dragged.
    editor.begin(&s, Vec2::new(0.8, 0.5), None).unwrap(); editor.keep_new_point_straight();
    editor.drag_to(Vec2::new(0.9, 0.6), false); editor.release(&s).unwrap(); finish(&mut editor, &mut s);
    let b = pen(&s).anchors.last().unwrap();
    assert_eq!((b.position, b.incoming, b.outgoing), (Vec2::new(0.8, 0.5), Vec2::ZERO, Vec2::ZERO));
    // Grabbing an existing handle is never made straight.
    editor.begin(&s, Vec2::new(0.5, 0.7), Some(PenHit { anchor: id, handle: Handle::Outgoing })).unwrap(); editor.keep_new_point_straight();
    editor.drag_to(Vec2::new(0.5, 0.6), false); editor.release(&s).unwrap(); finish(&mut editor, &mut s);
    assert!((pen(&s).anchors[0].outgoing - Vec2::new(0.0, 0.1)).length() < 1e-6);
}

