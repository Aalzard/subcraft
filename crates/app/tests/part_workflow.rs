//! Whole workflow regressions: geometry domains, unlit picking, actual paint writes and persistence.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, PartAction, PartKey, Session};
use subcraft_core::{ChannelKind, ChannelValue, PartKind};
use subcraft_doc::{GeneratorKind, LayerContent, PartMask};
use subcraft_mesh::{Corner, Mesh, MeshBuilder, primitives};

fn pieces() -> Mesh {
    let mut b = MeshBuilder::new("Axe");
    let material = b.material("Steel");
    for (name, left, right, u0, u1) in [("Blade", -1.0, -0.2, 0.05, 0.45), ("Handle", 0.2, 1.0, 0.55, 0.95)] {
        b.object(name);
        let c = |x, z, u, v| Corner { position: Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(Vec2::new(u, v)) };
        let q = [c(left, -1.0, u0, 0.05), c(right, -1.0, u1, 0.05), c(right, 1.0, u1, 0.95), c(left, 1.0, u0, 0.95)];
        b.push_triangle(material, [q[0], q[2], q[1]]); b.push_triangle(material, [q[0], q[3], q[2]]);
    }
    b.build().unwrap()
}
fn camera() -> Camera { Camera::perspective(Vec3::new(0.0, 4.0, 0.01), Vec3::ZERO, Vec3::Y, 40.0, 0.05, 100.0, [800, 600]) }
fn screen(cam: &Camera, x: f32) -> Vec2 { cam.project(Vec3::new(x, 0.0, 0.0)).unwrap().truncate() }
fn session() -> Session {
    let mut s = Session::new(pieces(), 128);
    s.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0)); s
}

#[test]
fn catalogue_has_names_material_ids_and_correct_domains() {
    let s = session();
    for kind in [PartKind::Connected, PartKind::UvIsland, PartKind::Object] {
        let parts = s.parts(kind);
        assert_eq!(parts.len(), 2); assert!(parts.iter().all(|p| p.triangles == 2 && p.materials == [0]));
        assert!(parts[0].name.contains("Blade")); assert!(parts[1].name.contains("Handle"));
    }
    assert_eq!(s.parts(PartKind::Material)[0].name, "Steel");
    let s = Session::new(primitives::cube(2.0), 64);
    assert_eq!(s.parts(PartKind::Connected).len(), 1); assert_eq!(s.parts(PartKind::UvIsland).len(), 6);
}

#[test]
fn fill_mask_and_generator_are_one_undo_step_and_save_object_identity() {
    let mut s = session(); let cam = camera();
    let target = s.surface_target(&cam, screen(&cam, -0.6), PartKind::Object).unwrap();
    let before = s.document().clone(); let count = s.history().entries().len();
    s.apply_part_action(target, PartAction::Fill).unwrap();
    assert_eq!(s.history().entries().len(), count + 1);
    let ts = s.selection().texture_set;
    let left = s.sample_uv(ts, Vec2::new(0.25, 0.5), ChannelKind::BaseColor).unwrap();
    let right = s.sample_uv(ts, Vec2::new(0.75, 0.5), ChannelKind::BaseColor).unwrap();
    assert!(left.0[0] > 0.99 && left.0[1] < 0.001); assert!(right.0[1] > 0.1);
    s.execute(Command::Undo).unwrap(); assert_eq!(s.document(), &before);
    s.execute(Command::Redo).unwrap();
    let before_generator = s.document().clone(); let count = s.history().entries().len();
    s.apply_part_action(target, PartAction::Generator(GeneratorKind::Directional)).unwrap();
    assert_eq!(s.history().entries().len(), count + 1);
    assert_eq!(s.selected_layer().unwrap().mask.as_ref().unwrap().effects.len(), 1);
    s.execute(Command::Undo).unwrap(); assert_eq!(s.document(), &before_generator);
    let path = std::env::temp_dir().join(format!("subcraft-parts-workflow-{}.subcraft", subcraft_core::ObjectId::new()));
    s.save(Some(&path)).unwrap();
    let mut reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.scene().mesh.objects, s.scene().mesh.objects);
    assert_eq!(reopened.document(), s.document());
    assert_eq!(reopened.sample_uv(ts, Vec2::new(0.25, 0.5), ChannelKind::BaseColor), Some(left));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn part_mask_blocks_real_stroke_writes_and_polygon_fills() {
    let mut s = session(); let cam = camera();
    let target = s.surface_target(&cam, screen(&cam, -0.6), PartKind::Object).unwrap();
    s.apply_part_action(target, PartAction::PaintOnly).unwrap();
    let layer = s.selection().layer.unwrap();
    s.brush.settings.alignment = subcraft_paint::Alignment::Uv;
    s.brush.settings.size = 200.0; s.brush.settings.flow = 1.0; s.brush.settings.hardness = 1.0;
    s.begin_stroke().unwrap(); s.stroke_to(&cam, screen(&cam, -0.6), false).unwrap(); s.end_stroke();
    s.polygon_fill(&cam, screen(&cam, 0.6), subcraft_paint::PolygonFillMode::UvIsland).unwrap();
    // Removing the mask must not reveal paint that was written on the excluded piece.
    s.execute(Command::SetParts { layer, parts: None }).unwrap();
    let LayerContent::Paint(paint) = &s.selected_layer().unwrap().content else { panic!("paint layer") };
    let image = &paint.channels[&ChannelKind::BaseColor];
    assert!(image.get(image.width() / 4, image.height() / 2)[3] > 0.99); assert_eq!(image.get(image.width() * 3 / 4, image.height() / 2)[3], 0.0);
}

#[test]
fn uv_island_masks_do_not_cover_the_other_cube_faces() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    s.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0));
    let target = s.target_for_part(PartKey { kind: PartKind::UvIsland, id: 0 }).unwrap();
    s.apply_part_action(target, PartAction::Fill).unwrap(); let ts = s.selection().texture_set;
    assert!(s.sample_uv(ts, target.uv, ChannelKind::BaseColor).unwrap().0[1] < 0.001);
    let other = s.target_for_part(PartKey { kind: PartKind::UvIsland, id: 1 }).unwrap();
    assert!(s.sample_uv(ts, other.uv, ChannelKind::BaseColor).unwrap().0[1] > 0.1);
}

#[test]
fn hidden_and_isolated_parts_stop_picking_without_editing_or_saving_visibility() {
    let mut s = session(); let cam = camera(); s.update_composites();
    let before = s.document().clone(); let target = s.surface_target(&cam, screen(&cam, -0.6), PartKind::Connected).unwrap();
    s.apply_part_action(target, PartAction::Hide).unwrap();
    assert!(s.pick(&cam, screen(&cam, -0.6)).is_none()); assert!(s.pick(&cam, screen(&cam, 0.6)).is_some());
    assert_eq!(s.document(), &before); assert!(!s.is_modified());
    s.apply_part_action(target, PartAction::Hide).unwrap(); assert!(s.pick(&cam, screen(&cam, -0.6)).is_some());
    s.apply_part_action(target, PartAction::Isolate).unwrap(); assert!(s.pick(&cam, screen(&cam, 0.6)).is_none());
    s.show_all_parts(); assert!(s.pick(&cam, screen(&cam, 0.6)).is_some());
    assert_eq!(s.scene().mesh.triangle_count(), 4);
}

#[test]
fn eyedropper_reads_linear_unlit_values_and_layer_menu_creates_masks() {
    let mut s = session(); let cam = camera();
    let target = s.surface_target(&cam, screen(&cam, -0.6), PartKind::Connected).unwrap();
    s.apply_part_action(target, PartAction::Fill).unwrap();
    s.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.0, 1.0, 0.0));
    assert!(s.sample_surface(&cam, screen(&cam, -0.6), true));
    assert_eq!(s.brush.material.channels[&ChannelKind::BaseColor].value, ChannelValue::rgb(1.0, 0.0, 0.0));
    let ts = s.selection().texture_set;
    assert_eq!(s.sample_uv(ts, Vec2::new(-0.75, 1.5), ChannelKind::BaseColor), s.sample_uv(ts, Vec2::new(0.25, 0.5), ChannelKind::BaseColor));
    assert!(s.sample_uv(ts, Vec2::splat(f32::NAN), ChannelKind::BaseColor).is_none());
    let layer = s.selection().layer.unwrap();
    for action in [subcraft_app::part_workflow::LayerAction::Generator(GeneratorKind::Cavity), subcraft_app::part_workflow::LayerAction::MaskFill, subcraft_app::part_workflow::LayerAction::MaskPaint] {
        let count = s.history().entries().len(); s.layer_action(layer, action).unwrap(); assert_eq!(s.history().entries().len(), count + 1);
    }
    assert_eq!(s.selected_layer().unwrap().mask.as_ref().unwrap().effects.len(), 3);
    assert!(matches!(s.selected_layer().unwrap().parts, Some(PartMask { kind: PartKind::Connected, .. })));
}

#[test]
fn masking_uses_the_selected_layer_even_when_it_is_not_the_top_layer() {
    let mut s = session(); let selected = s.selection().layer.unwrap();
    s.execute(Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
    let top = s.selection().layer.unwrap();
    s.execute(Command::SelectLayer(Some(selected))).unwrap();
    let target = s.target_for_part(PartKey { kind: PartKind::Object, id: 0 }).unwrap();
    s.apply_part_action(target, PartAction::MaskLayer).unwrap();
    assert!(s.selected_texture_set().find(selected).unwrap().parts.is_some());
    assert!(s.selected_texture_set().find(top).unwrap().parts.is_none());
}

#[test]
fn one_part_crossing_material_ids_gets_layers_in_both_sets_in_one_undo_step() {
    let mut b = MeshBuilder::new("Two materials"); let a = b.material("Steel"); let z = b.material("Paint"); b.object("Blade");
    let c = |x, y, u, v| Corner { position: Vec3::new(x, 0.0, y), normal: Some(Vec3::Y), uv: Some(Vec2::new(u, v)) };
    let q = [c(-1.0, -1.0, 0.0, 0.0), c(1.0, -1.0, 1.0, 0.0), c(1.0, 1.0, 1.0, 1.0), c(-1.0, 1.0, 0.0, 1.0)];
    b.push_triangle(a, [q[0], q[2], q[1]]); b.push_triangle(z, [q[0], q[3], q[2]]);
    let mut s = Session::new(b.build().unwrap(), 128);
    s.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0));
    let target = s.target_for_part(PartKey { kind: PartKind::Object, id: 0 }).unwrap();
    let before = s.document().clone(); let count = s.history().entries().len();
    s.apply_part_action(target, PartAction::Fill).unwrap(); assert_eq!(s.history().entries().len(), count + 1);
    for t in 0..2 {
        let ts = s.document().texture_set_for_slot(s.scene().mesh.tri_material[t]).unwrap().id;
        let uv = s.scene().mesh.interpolate(t, 1.0 / 3.0, 1.0 / 3.0).uv;
        assert!(s.sample_uv(ts, uv, ChannelKind::BaseColor).unwrap().0[1] < 0.001);
    }
    s.execute(Command::Undo).unwrap(); assert_eq!(s.document(), &before);
    s.apply_part_action(target, PartAction::PaintOnly).unwrap();
    let cam = camera();
    let p = s.scene().mesh.tri_positions(1).into_iter().sum::<Vec3>() / 3.0;
    s.prepare_surface_paint(&cam, cam.project(p).unwrap().truncate()).unwrap();
    assert_eq!(s.selected_texture_set().material_slot, 1);
    assert!(s.selected_layer().unwrap().parts.is_some());
}
