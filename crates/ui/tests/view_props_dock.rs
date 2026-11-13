//! The right-click Properties popup and the docked Properties tab never draw the same widgets in
//! one frame (same ids: both copies took a click). Real egui frames; no desktop mouse.
use egui::{Pos2, Rect};
use subcraft_app::Session;
use subcraft_ui::panels::{brush_rows, properties};
use subcraft_ui::state::UiState;

fn frame(state: &mut UiState, ctx: &egui::Context) {
    let mut out = ctx.run_ui(
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(420.0, 2600.0))), ..Default::default() },
        |ui| properties::show_docked(state, ui),
    );
    out.textures_delta.clear();
}

/// QA 2026-11: a pressure toggle clicked in the popup was flipped back by the docked panel.
#[test]
fn the_docked_properties_step_aside_while_the_popup_shows_them() {
    let mut s = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    s.brush_pinned = true;
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..2 {
        frame(&mut s, &ctx);
    }
    assert!(ctx.read_response(brush_rows::pressure_toggle_id("size")).is_some(), "the docked tab shows the brush rows");
    s.view_props = Some(subcraft_ui::view_props::ViewProps { pos: Pos2::new(200.0, 200.0), view: subcraft_ui::view_props::View::View3d });
    for _ in 0..2 {
        frame(&mut s, &ctx);
    }
    assert!(ctx.read_response(brush_rows::pressure_toggle_id("size")).is_none(), "only the popup shows them");
    s.view_props = None;
    frame(&mut s, &ctx);
    assert!(ctx.read_response(brush_rows::pressure_toggle_id("size")).is_some(), "back once the popup closes");
}
