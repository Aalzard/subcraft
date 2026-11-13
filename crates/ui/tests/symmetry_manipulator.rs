//! Symmetry manipulator through real egui input: dragging a mirror plane's handle moves the
//! plane (Painter's axis position) and paints nothing; a drag away from the handles still paints.
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use glam::Vec3;
use subcraft_app::{Command, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::LayerContent;
use subcraft_paint::StrokeOptions;
use subcraft_ui::{state::{Tool, UiState}, theme, viewport};

const SCREEN: egui::Vec2 = egui::vec2(1200.0, 800.0);

fn setup() -> (UiState, egui::Context) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 128), None);
    state.tool = Tool::Brush;
    state.session.execute(Command::SetStrokeOptions(StrokeOptions { symmetry: [true, false, false], ..StrokeOptions::default() })).unwrap();
    state.recovery.prefs.viewport.symmetry_display.manipulator = true;
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (state, ctx)
}

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<Event>) {
    let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| viewport::show(state, ui));
    output.textures_delta.clear();
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn drag(state: &mut UiState, ctx: &egui::Context, from: Pos2, to: Pos2) {
    frame(state, ctx, vec![Event::PointerMoved(from)]);
    frame(state, ctx, vec![button(from, true)]);
    for k in 1..=12 {
        frame(state, ctx, vec![Event::PointerMoved(from.lerp(to, k as f32 / 12.0))]);
    }
    frame(state, ctx, vec![button(to, false)]);
    frame(state, ctx, vec![]);
}

/// Screen position (points) of an object-space point in the 3D view.
fn screen_of(state: &UiState, p: Vec3) -> Pos2 {
    let (rect, size) = (state.last_view_rect, state.last_view_size);
    let s = state.camera.paint_camera(size).project(p).expect("in front of the camera");
    rect.min + egui::vec2(s.x * rect.width() / size[0] as f32, s.y * rect.height() / size[1] as f32)
}

fn painted_tiles(state: &UiState) -> usize {
    let LayerContent::Paint(p) = &state.session.selected_layer().unwrap().content else { panic!("a paint layer") };
    p.channels.get(&ChannelKind::BaseColor).map_or(0, |i| i.allocated_tiles())
}

#[test]
fn dragging_the_mirror_handle_moves_the_plane_without_painting() {
    let (mut state, ctx) = setup();
    for _ in 0..3 {
        frame(&mut state, &ctx, vec![]);
    }
    let entries = state.session.history().entries().len();
    // The X handle sits at the plane's center; drag it towards the screen position of x = +0.5.
    let (from, to) = (screen_of(&state, Vec3::ZERO), screen_of(&state, Vec3::new(0.5, 0.0, 0.0)));
    assert!((to - from).length() > 20.0, "the X axis is visible on screen: {from:?} -> {to:?}");
    drag(&mut state, &ctx, from, to);
    let offset = state.session.brush.options.mirror_offset;
    assert!((offset[0] - 0.5).abs() < 0.05, "the plane followed the handle: {offset:?}");
    assert_eq!(offset[1..], [0.0, 0.0]);
    assert_eq!(state.session.history().entries().len(), entries, "moving the plane is not a document edit");
    assert_eq!(painted_tiles(&state), 0, "the press on the handle did not paint");

    // With the manipulator off the same press paints (and the plane stays where it is).
    state.recovery.prefs.viewport.symmetry_display.manipulator = false;
    let from = screen_of(&state, Vec3::new(0.5, 0.0, 0.0));
    drag(&mut state, &ctx, from, from + egui::vec2(40.0, 10.0));
    assert!(painted_tiles(&state) > 0);
    assert!((state.session.brush.options.mirror_offset[0] - offset[0]).abs() < 1e-6);
}

/// QA 2026-11: with the Pen tool the handles are hidden, but a press on one still moved the
/// mirror plane instead of reaching the pen.
#[test]
fn the_pen_ignores_the_hidden_symmetry_handles() {
    let (mut state, ctx) = setup();
    state.tool = Tool::Pen;
    for _ in 0..3 {
        frame(&mut state, &ctx, vec![]);
    }
    let (from, to) = (screen_of(&state, Vec3::ZERO), screen_of(&state, Vec3::new(0.5, 0.0, 0.0)));
    drag(&mut state, &ctx, from, to);
    assert_eq!(state.session.brush.options.mirror_offset, [0.0; 3], "the hidden handle did not move the plane");
}
