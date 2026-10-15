//! Real egui gestures without any desktop input. Render jobs run through the same frame hook.
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use glam::Vec2;
use subcraft_app::Session;
use subcraft_doc::{LayerContent, LayerKind};
use subcraft_ui::{panels::uv_view, pen, state::{Tool, UiState}, theme};

fn setup() -> (UiState, egui::Context) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 128), None);
    state.tool = Tool::Pen; state.uv_wireframe = false;
    let ctx = egui::Context::default(); theme::apply(&ctx); (state, ctx)
}

fn frame(state: &mut UiState, ctx: &egui::Context, mut events: Vec<Event>, mods: Modifiers, view3d: bool) -> egui::FullOutput {
    events.insert(0, Event::ModifiersChanged(mods));
    let mut output = ctx.run_ui(egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() }, |ui| {
        let now = ui.input(|i| i.time); pen::keyboard(state, ui.ctx(), now);
        if view3d { subcraft_ui::viewport::show(state, ui); } else { uv_view::show(state, ui); }
        pen::finish_frame(state, ui.ctx(), now);
    }); output.textures_delta.clear(); output
}

fn output(state: &mut UiState, ctx: &egui::Context) -> egui::FullOutput {
    for _ in 0..3 { frame(state, ctx, vec![], Modifiers::NONE, false); }
    frame(state, ctx, vec![], Modifiers::NONE, false)
}

fn image(output: &egui::FullOutput) -> Rect {
    output.shapes.iter().filter_map(|s| match &s.shape {
        egui::epaint::Shape::Rect(r) if r.stroke.color == theme::BORDER_STRONG && r.rect.width() > 100.0 => Some(r.rect), _ => None,
    }).max_by(|a,b| a.area().total_cmp(&b.area())).expect("UV image border")
}

fn button(pos: Pos2, pressed: bool) -> Event { Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE } }
fn key(key: egui::Key) -> Event { Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE } }
fn at(image: Rect, uv: Vec2) -> Pos2 { image.min + egui::vec2(uv.x, uv.y) * image.width() }

fn finish(state: &mut UiState, ctx: &egui::Context) {
    for _ in 0..1000 { if !state.pen.is_pending() { return; } frame(state, ctx, vec![], Modifiers::NONE, false); }
    panic!("job did not finish");
}

fn gesture(state: &mut UiState, ctx: &egui::Context, from: Vec2, to: Vec2) {
    let rect = image(&output(state, ctx)); let start = at(rect, from); let end = at(rect, to);
    frame(state, ctx, vec![Event::PointerMoved(start), button(start, true)], Modifiers::NONE, false);
    frame(state, ctx, vec![Event::PointerMoved(end)], Modifiers::NONE, false);
    frame(state, ctx, vec![button(end, false)], Modifiers::NONE, false); finish(state, ctx);
}

fn path(state: &UiState) -> &subcraft_doc::PenPath {
    let LayerContent::Paint(p) = &state.session.selected_layer().unwrap().content else { panic!() }; p.path.as_ref().unwrap()
}

#[test]
fn uv_click_drag_creates_handles_and_moving_anchor_changes_raster_without_panning() {
    let (mut state, ctx) = setup();
    gesture(&mut state, &ctx, Vec2::new(0.2, 0.4), Vec2::new(0.2, 0.4));
    gesture(&mut state, &ctx, Vec2::new(0.8, 0.4), Vec2::new(0.8, 0.1));
    assert_eq!(path(&state).anchors.len(), 2); assert!((path(&state).anchors[1].outgoing.y + 0.3).abs() < 0.001);
    let handles = (path(&state).anchors[1].incoming, path(&state).anchors[1].outgoing);
    let history = state.session.history().entries().len();
    gesture(&mut state, &ctx, Vec2::new(0.8, 0.4), Vec2::new(0.7, 0.6));
    assert!(path(&state).anchors[1].position.distance(Vec2::new(0.7, 0.6)) < 0.001);
    assert_eq!((path(&state).anchors[1].incoming, path(&state).anchors[1].outgoing), handles);
    assert_eq!(state.uv.offset, egui::Vec2::ZERO); assert_eq!(state.session.history().entries().len(), history + 1);
    let LayerContent::Paint(p) = &state.session.selected_layer().unwrap().content else { panic!() };
    assert_eq!(p.channels[&subcraft_core::ChannelKind::BaseColor].get(102, 51)[3], 0.0, "old endpoint is cleared");
}

#[test]
fn escape_cancels_a_drag_and_delete_removes_the_selected_anchor() {
    let (mut state, ctx) = setup(); gesture(&mut state, &ctx, Vec2::splat(0.4), Vec2::splat(0.4));
    let before = state.session.document().clone(); let rect = image(&output(&mut state, &ctx)); let start = at(rect, Vec2::splat(0.4)); let end = at(rect, Vec2::splat(0.6));
    frame(&mut state, &ctx, vec![Event::PointerMoved(start), button(start, true)], Modifiers::NONE, false);
    frame(&mut state, &ctx, vec![Event::PointerMoved(end)], Modifiers::NONE, false);
    frame(&mut state, &ctx, vec![key(egui::Key::Escape)], Modifiers::NONE, false);
    frame(&mut state, &ctx, vec![button(end, false)], Modifiers::NONE, false);
    assert_eq!(state.session.document(), &before);
    gesture(&mut state, &ctx, Vec2::splat(0.4), Vec2::splat(0.4));
    frame(&mut state, &ctx, vec![key(egui::Key::Delete)], Modifiers::NONE, false); finish(&mut state, &ctx);
    assert!(path(&state).anchors.is_empty());
}

#[test]
fn hit_testing_selects_movable_handles_and_is_independent_of_zoom() {
    let (mut state, ctx) = setup(); gesture(&mut state, &ctx, Vec2::splat(0.5), Vec2::new(0.7, 0.5));
    state.uv.zoom = 1.5; let rect = image(&output(&mut state, &ctx)); let p = path(&state); let a = &p.anchors[0];
    let hit = pen::hit(p, Some(a.id), at(rect, a.position + a.outgoing), |uv| Some(at(rect, uv))).unwrap();
    assert_eq!(hit.handle, subcraft_app::pen::Handle::Outgoing);
}

#[test]
fn pen_clicks_on_3d_surface_create_a_path_without_a_brush_stroke() {
    let (mut state, ctx) = setup();
    for _ in 0..3 { frame(&mut state, &ctx, vec![], Modifiers::NONE, true); }
    let camera = state.camera.paint_camera([800, 600]);
    let screen = camera.project(glam::Vec3::new(0.2, 0.2, 1.0)).unwrap().truncate(); let pos = egui::pos2(screen.x, screen.y);
    frame(&mut state, &ctx, vec![Event::PointerMoved(pos), button(pos, true)], Modifiers::NONE, true);
    frame(&mut state, &ctx, vec![button(pos, false)], Modifiers::NONE, true); finish(&mut state, &ctx);
    assert_eq!(state.session.selected_layer().unwrap().kind(), LayerKind::Path); assert_eq!(path(&state).anchors.len(), 1);
    assert!(!state.painting); assert!(!state.session.is_stroking());
}

#[test]
fn update_restart_waits_for_inflight_pen_work() {
    let (mut state, _) = setup(); assert!(subcraft_ui::updates::can_restart(&state));
    state.pen.begin(&state.session, Vec2::splat(0.5), None).unwrap(); assert!(!subcraft_ui::updates::can_restart(&state));
    state.pen.release(&state.session).unwrap(); assert!(!subcraft_ui::updates::can_restart(&state));
    while state.pen.poll(&mut state.session, 16384).unwrap() {}
    assert!(subcraft_ui::updates::can_restart(&state));
}

#[test]
fn shift_drag_breaks_a_handle_and_alt_drag_pans_without_editing_the_path() {
    let (mut state, ctx) = setup(); gesture(&mut state, &ctx, Vec2::splat(0.5), Vec2::new(0.7, 0.5));
    let outgoing = path(&state).anchors[0].outgoing; let rect = image(&output(&mut state, &ctx));
    let start = at(rect, Vec2::new(0.3, 0.5)); let end = at(rect, Vec2::new(0.3, 0.7));
    frame(&mut state, &ctx, vec![Event::PointerMoved(start), button(start, true)], Modifiers::SHIFT, false);
    frame(&mut state, &ctx, vec![Event::PointerMoved(end)], Modifiers::SHIFT, false);
    frame(&mut state, &ctx, vec![button(end, false)], Modifiers::SHIFT, false); finish(&mut state, &ctx);
    assert_eq!(path(&state).anchors[0].outgoing, outgoing); assert!(path(&state).anchors[0].incoming.y > 0.19);
    let before = state.session.document().clone(); let start = at(rect, Vec2::splat(0.4)); let end = start + egui::vec2(50.0, 30.0);
    frame(&mut state, &ctx, vec![Event::PointerMoved(start), button(start, true)], Modifiers::ALT, false);
    frame(&mut state, &ctx, vec![Event::PointerMoved(end)], Modifiers::ALT, false);
    frame(&mut state, &ctx, vec![button(end, false)], Modifiers::ALT, false);
    assert_ne!(state.uv.offset, egui::Vec2::ZERO); assert_eq!(state.session.document(), &before);
}
