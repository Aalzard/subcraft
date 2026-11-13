//! Warp projection grid through real egui input: Transform warp drags the whole grid,
//! Edit vertices moves the dragged point only (Ctrl: in the view plane, otherwise onto the
//! surface), a rectangle selects points, and none of it paints.
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};
use glam::Vec3;
use subcraft_app::{Command, NewLayer, Session};
use subcraft_doc::{LayerContent, Projection, ProjectionMode, WarpGrid};
use subcraft_ui::{state::{Tool, UiState}, theme, viewport, warp_view::CubeFrame};

const SCREEN: egui::Vec2 = egui::vec2(1200.0, 800.0);

fn setup() -> (UiState, egui::Context) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 128), None);
    state.tool = Tool::Brush;
    state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = state.session.selection().layer.unwrap();
    let projection = Projection { mode: ProjectionMode::Warp, planar_axis: 2, ..Projection::default() };
    state.session.execute(Command::SetFillProjection { layer, projection }).unwrap();
    state.session.execute(Command::Seal).unwrap();
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (state, ctx)
}

fn frame(state: &mut UiState, ctx: &egui::Context, mut events: Vec<Event>, modifiers: Modifiers) {
    // This egui takes the held modifiers from events.
    events.insert(0, Event::ModifiersChanged(modifiers));
    let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| viewport::show(state, ui));
    output.textures_delta.clear();
}

fn button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers }
}

fn drag(state: &mut UiState, ctx: &egui::Context, from: Pos2, to: Pos2, m: Modifiers) {
    frame(state, ctx, vec![Event::PointerMoved(from)], m);
    frame(state, ctx, vec![button(from, true, m)], m);
    for k in 1..=10 {
        frame(state, ctx, vec![Event::PointerMoved(from.lerp(to, k as f32 / 10.0))], m);
    }
    frame(state, ctx, vec![button(to, false, m)], m);
    frame(state, ctx, vec![], Modifiers::NONE);
}

fn screen_of(state: &UiState, p: Vec3) -> Pos2 {
    let (rect, size) = (state.last_view_rect, state.last_view_size);
    let s = state.camera.paint_camera(size).project(p).expect("in front of the camera");
    rect.min + egui::vec2(s.x * rect.width() / size[0] as f32, s.y * rect.height() / size[1] as f32)
}

fn grid(state: &UiState) -> WarpGrid {
    let LayerContent::Fill(f) = &state.session.selected_layer().unwrap().content else { panic!("a fill layer") };
    f.projection.warp_grid()
}

fn warm_up(state: &mut UiState, ctx: &egui::Context) {
    for _ in 0..3 {
        frame(state, ctx, vec![], Modifiers::NONE);
    }
}

/// QA 2026-11: Shift+V and Esc reached the warp tool while a text field had the keyboard.
#[test]
fn shift_v_while_typing_leaves_the_warp_mode_alone() {
    let (mut state, ctx) = setup();
    warm_up(&mut state, &ctx);
    let centre = state.last_view_rect.center();
    frame(&mut state, &ctx, vec![Event::PointerMoved(centre)], Modifiers::NONE);
    let field = egui::Id::new("some text field");
    let shift_v = Event::Key { key: Key::V, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT };
    let input = egui::RawInput { events: vec![Event::ModifiersChanged(Modifiers::SHIFT), shift_v.clone()], screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| {
        ui.memory_mut(|m| m.request_focus(field));
        viewport::show(&mut state, ui);
    });
    out.textures_delta.clear();
    assert!(!state.warp.edit_vertices, "typed into the field, not a warp shortcut");
    // The field is gone (not drawn): focus drops within a frame or two.
    warm_up(&mut state, &ctx);
    frame(&mut state, &ctx, vec![shift_v], Modifiers::SHIFT);
    assert!(state.warp.edit_vertices, "without a focused field Shift+V still switches");
}

#[test]
fn transform_warp_drags_the_whole_grid_in_the_view_plane_as_one_undo_step() {
    let (mut state, ctx) = setup();
    warm_up(&mut state, &ctx);
    let (layers, entries) = (state.session.selected_texture_set().layer_count(), state.session.history().entries().len());
    let before = grid(&state);
    let cube = CubeFrame::of(&state);
    let p = cube.to_object(before.points()[0]);
    let right = state.camera.paint_camera(state.last_view_size).right();
    let (from, to) = (screen_of(&state, p), screen_of(&state, p + right * 0.3));
    drag(&mut state, &ctx, from, to, Modifiers::NONE);
    let after = grid(&state);
    let expected = right * 0.3 / cube.extent;
    for (a, b) in before.points().iter().zip(after.points()) {
        let d = Vec3::from_array(*b) - Vec3::from_array(*a);
        assert!(d.distance(expected) < 0.01, "every point moved by the same view-plane offset: {d} vs {expected}");
    }
    assert_eq!(state.session.history().entries().len(), entries + 1, "one drag, one undo step");
    assert_eq!(state.session.selected_texture_set().layer_count(), layers, "the press did not start a paint stroke");
}

#[test]
fn edit_vertices_moves_one_point_in_the_view_plane_or_onto_the_surface() {
    let (mut state, ctx) = setup();
    warm_up(&mut state, &ctx);
    // Shift+V over the view switches to Edit vertices.
    let centre = state.last_view_rect.center();
    frame(&mut state, &ctx, vec![Event::PointerMoved(centre)], Modifiers::NONE);
    frame(&mut state, &ctx, vec![Event::Key { key: Key::V, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT }], Modifiers::SHIFT);
    assert!(state.warp.edit_vertices);
    let before = grid(&state);
    let cube = CubeFrame::of(&state);
    let i = before.index(1, 1);
    let p = cube.to_object(before.points()[i]);
    let up = state.camera.paint_camera(state.last_view_size).right().cross(state.camera.paint_camera(state.last_view_size).forward());
    // Ctrl: in the view plane through the point.
    let (from, to) = (screen_of(&state, p), screen_of(&state, p + up * 0.25));
    drag(&mut state, &ctx, from, to, Modifiers::CTRL);
    let after = grid(&state);
    let moved = cube.to_object(after.points()[i]);
    assert!(moved.distance(p + up * 0.25) < 0.02, "{moved} vs {}", p + up * 0.25);
    for (k, (a, b)) in before.points().iter().zip(after.points()).enumerate() {
        if k != i {
            assert_eq!(a, b, "point {k} stayed");
        }
    }
    // Without Ctrl: onto the surface under the cursor.
    let target_screen = state.last_view_rect.center();
    let size = state.last_view_size;
    let rect = state.last_view_rect;
    let to_paint = glam::Vec2::new((target_screen.x - rect.min.x) * size[0] as f32 / rect.width(), (target_screen.y - rect.min.y) * size[1] as f32 / rect.height());
    let hit = state.session.pick(&state.camera.paint_camera(size), to_paint).expect("the cube under the view centre");
    let from = screen_of(&state, moved);
    drag(&mut state, &ctx, from, target_screen, Modifiers::NONE);
    let on_surface = cube.to_object(grid(&state).points()[i]);
    assert!(on_surface.distance(hit.position) < 0.02, "{on_surface} vs {}", hit.position);
}

#[test]
fn a_rectangle_selects_points_and_escape_clears_the_selection() {
    let (mut state, ctx) = setup();
    warm_up(&mut state, &ctx);
    state.warp.edit_vertices = true;
    let g = grid(&state);
    let cube = CubeFrame::of(&state);
    let screens: Vec<Pos2> = g.points().iter().map(|p| screen_of(&state, cube.to_object(*p))).collect();
    // A box around the first two points of the first row only.
    let (a, b) = (screens[0], screens[1]);
    let r = Rect::from_two_pos(a, b).expand(4.0);
    let others: Vec<usize> = (0..screens.len()).filter(|&k| r.contains(screens[k])).collect();
    drag(&mut state, &ctx, r.min - egui::vec2(6.0, 6.0), r.max + egui::vec2(6.0, 6.0), Modifiers::NONE);
    let mut selected = state.warp.selection.clone();
    selected.sort();
    assert!(selected.contains(&0) && selected.contains(&1), "{selected:?}");
    assert!(selected.len() <= others.len() + 2, "{selected:?}");
    frame(&mut state, &ctx, vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE);
    assert!(state.warp.selection.is_empty());
    assert_eq!(grid(&state), g, "selecting moves nothing");
}
