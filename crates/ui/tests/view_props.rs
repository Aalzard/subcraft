//! Right-click Properties popup and polygon fill click / box, through real egui input in the 3D
//! and 2D views (the popup is drawn before the views each frame, as the app does).
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use subcraft_app::Session;
use subcraft_core::ChannelKind;
use subcraft_doc::LayerContent;
use subcraft_ui::view_props::View;
use subcraft_ui::{panels::uv_view, state::{Tool, UiState}, theme, view_props, viewport};

const SCREEN: egui::Vec2 = egui::vec2(1200.0, 800.0);

fn setup() -> (UiState, egui::Context) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 128), None);
    state.tool = Tool::Brush;
    state.uv_wireframe = false;
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (state, ctx)
}

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<Event>, three_d: bool) -> egui::FullOutput {
    let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| {
        state.view_props_swallow = view_props::show(ui.ctx(), state);
        if three_d { viewport::show(state, ui) } else { uv_view::show(state, ui) }
    });
    output.textures_delta.clear();
    output
}

fn settle(state: &mut UiState, ctx: &egui::Context, three_d: bool) -> egui::FullOutput {
    for _ in 0..3 {
        frame(state, ctx, vec![], three_d);
    }
    frame(state, ctx, vec![], three_d)
}

fn texts(output: &egui::FullOutput) -> Vec<String> {
    output.shapes.iter().filter_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
        _ => None,
    }).collect()
}

fn press(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE }
}

fn click(state: &mut UiState, ctx: &egui::Context, pos: Pos2, button: PointerButton, three_d: bool) {
    frame(state, ctx, vec![Event::PointerMoved(pos)], three_d);
    frame(state, ctx, vec![press(pos, button, true)], three_d);
    frame(state, ctx, vec![press(pos, button, false)], three_d);
}

fn drag(state: &mut UiState, ctx: &egui::Context, from: Pos2, to: Pos2, button: PointerButton, three_d: bool) {
    frame(state, ctx, vec![Event::PointerMoved(from), press(from, button, true)], three_d);
    for k in 1..=6 {
        frame(state, ctx, vec![Event::PointerMoved(from.lerp(to, k as f32 / 6.0))], three_d);
    }
    frame(state, ctx, vec![press(to, button, false)], three_d);
}

fn painted(state: &UiState) -> usize {
    let LayerContent::Paint(p) = &state.session.selected_layer().unwrap().content else { panic!("paint layer") };
    p.channels.get(&ChannelKind::BaseColor).map_or(0, |i| i.allocated_tiles())
}

#[test]
fn right_click_in_the_3d_view_opens_properties_at_the_cursor_and_a_right_drag_still_orbits() {
    let (mut state, ctx) = setup();
    settle(&mut state, &ctx, true);
    let center = Pos2::new(SCREEN.x * 0.5, SCREEN.y * 0.55);
    click(&mut state, &ctx, center, PointerButton::Secondary, true);
    let props = state.view_props.expect("a right-click opens the popup");
    assert_eq!(props.view, View::View3d);
    assert!((props.pos - center).length() < 1.0);
    assert!(state.context_part.is_some(), "the part under the cursor is ready for the header menu");
    // The popup shows the Properties content: brush settings for the paint layer.
    let out = settle(&mut state, &ctx, true);
    let t = texts(&out);
    assert!(t.iter().any(|s| s == "Properties"), "{t:?}");
    assert!(t.iter().any(|s| s.to_lowercase().contains("paint target")), "brush settings while painting: {t:?}");
    // Esc closes it.
    frame(&mut state, &ctx, vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], true);
    assert!(state.view_props.is_none());

    // A right-button drag orbits (Painter scheme) and opens nothing.
    let before = state.camera;
    drag(&mut state, &ctx, center, center + egui::vec2(120.0, 30.0), PointerButton::Secondary, true);
    settle(&mut state, &ctx, true);
    assert!(state.view_props.is_none(), "a drag is navigation, not a click");
    assert_ne!(state.camera, before, "the drag orbited");

    // Click-away closes it without painting.
    click(&mut state, &ctx, center, PointerButton::Secondary, true);
    assert!(state.view_props.is_some());
    settle(&mut state, &ctx, true);
    let entries = state.session.history().entries().len();
    click(&mut state, &ctx, Pos2::new(SCREEN.x * 0.2, SCREEN.y * 0.5), PointerButton::Primary, true);
    assert!(state.view_props.is_none(), "a click outside closes it");
    assert_eq!(state.session.history().entries().len(), entries, "and does not paint");
    assert_eq!(painted(&state), 0);
}

#[test]
fn right_click_in_the_2d_view_opens_properties_and_shows_the_selected_effect_or_brush() {
    let (mut state, ctx) = setup();
    settle(&mut state, &ctx, false);
    let p = Pos2::new(SCREEN.x * 0.5, SCREEN.y * 0.5);
    click(&mut state, &ctx, p, PointerButton::Secondary, false);
    assert_eq!(state.view_props.map(|v| v.view), Some(View::View2d));
    let t = texts(&settle(&mut state, &ctx, false));
    assert!(t.iter().any(|s| s.to_lowercase().contains("paint target")), "{t:?}");
    // Alt / Ctrl right-clicks are gestures, never the popup.
    state.view_props = None;
    let mods = Modifiers { alt: true, ..Modifiers::NONE };
    frame(&mut state, &ctx, vec![Event::ModifiersChanged(mods), Event::PointerMoved(p)], false);
    frame(&mut state, &ctx, vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: true, modifiers: mods }], false);
    frame(&mut state, &ctx, vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: false, modifiers: mods }], false);
    frame(&mut state, &ctx, vec![Event::ModifiersChanged(Modifiers::NONE)], false);
    assert!(state.view_props.is_none());
}

#[test]
fn polygon_fill_box_and_click_in_the_2d_view_are_one_undo_step_each() {
    let (mut state, ctx) = setup();
    state.tool = Tool::PolygonFill;
    state.fill_mode = subcraft_paint::PolygonFillMode::Triangle;
    let out = settle(&mut state, &ctx, false);
    let image = out.shapes.iter().filter_map(|s| match &s.shape {
        egui::epaint::Shape::Rect(r) if r.stroke.color == theme::BORDER_STRONG() && r.rect.width() > 100.0 => Some(r.rect),
        _ => None,
    }).max_by(|a, b| a.area().total_cmp(&b.area())).expect("UV image");
    let entries = state.session.history().entries().len();
    // A box over the top-left face cell of the cube's UV atlas.
    let at = |u: f32, v: f32| image.min + egui::vec2(u * image.width(), v * image.height());
    drag(&mut state, &ctx, at(0.02, 0.02), at(0.3, 0.45), PointerButton::Primary, false);
    assert!(painted(&state) > 0, "the box filled");
    assert_eq!(state.session.history().entries().len(), entries + 1);
    // A click (no drag) is a single-element fill: another history entry.
    click(&mut state, &ctx, at(0.5, 0.25), PointerButton::Primary, false);
    assert_eq!(state.session.history().entries().len(), entries + 2);
}

#[test]
fn polygon_fill_box_in_the_3d_view_fills_and_escape_cancels_a_box() {
    let (mut state, ctx) = setup();
    state.tool = Tool::PolygonFill;
    settle(&mut state, &ctx, true);
    let entries = state.session.history().entries().len();
    let c = Pos2::new(SCREEN.x * 0.5, SCREEN.y * 0.55);
    drag(&mut state, &ctx, c - egui::vec2(60.0, 60.0), c + egui::vec2(60.0, 60.0), PointerButton::Primary, true);
    assert!(painted(&state) > 0);
    assert_eq!(state.session.history().entries().len(), entries + 1, "one box = one undo step");
    // Esc during a box drag cancels it.
    frame(&mut state, &ctx, vec![Event::PointerMoved(c), press(c, PointerButton::Primary, true)], true);
    frame(&mut state, &ctx, vec![Event::PointerMoved(c + egui::vec2(80.0, 40.0))], true);
    frame(&mut state, &ctx, vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], true);
    frame(&mut state, &ctx, vec![press(c + egui::vec2(80.0, 40.0), PointerButton::Primary, false)], true);
    assert_eq!(state.session.history().entries().len(), entries + 1, "a cancelled box fills nothing");
}
