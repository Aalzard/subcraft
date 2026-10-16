//! Surface hover must not add geometry overlays or alter material colors.
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use subcraft_app::Session;
use subcraft_core::PartKind;
use subcraft_ui::{state::{Tool, UiState}, viewport};

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() }, |ui| viewport::show(state, ui));
    out.textures_delta.clear(); out
}
fn setup() -> (UiState, egui::Context, Pos2) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    state.tool = Tool::Eyedropper;
    let ctx = egui::Context::default(); subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 { frame(&mut state, &ctx, vec![]); }
    let pos = Pos2::new(400.0, 300.0);
    let camera = state.camera.paint_camera([800,600]);
    assert!(state.session.surface_target(&camera, glam::vec2(pos.x,pos.y), PartKind::Connected).is_some());
    (state, ctx, pos)
}
fn no_surface_overlay(output: &egui::FullOutput) {
    // With no GPU texture/stencil, viewport geometry meshes could only be a part tint.
    assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Mesh(_))), "hover added a surface overlay");
}
fn key(key: egui::Key) -> Event { Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE } }

#[test]
fn hovering_a_surface_is_untinted_and_hide_restore_shortcuts_still_target_it() {
    let (mut state, ctx, pos) = setup(); let before = state.session.document().clone();
    no_surface_overlay(&frame(&mut state, &ctx, vec![Event::PointerMoved(pos)]));
    assert_eq!(state.session.document(), &before);
    frame(&mut state, &ctx, vec![key(egui::Key::H)]); assert!(state.session.has_hidden_parts());
    frame(&mut state, &ctx, vec![key(egui::Key::Escape)]); assert!(!state.session.has_hidden_parts());
    assert_eq!(state.session.document(), &before);
}

#[test]
fn explicit_part_picking_stays_untinted_and_clicks_still_edit_the_part_mask() {
    let (mut state, ctx, pos) = setup(); state.picking_parts = true;
    let before = state.session.document().clone();
    no_surface_overlay(&frame(&mut state, &ctx, vec![Event::PointerMoved(pos)]));
    assert_eq!(state.session.document(), &before);
    for pressed in [true,false] { frame(&mut state, &ctx, vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]); }
    assert!(state.session.selected_layer().unwrap().parts.is_some());
    assert_ne!(state.session.document(), &before);
}
