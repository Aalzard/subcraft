//! Painting in the 2D view through real egui input: brush drags, channel switches, panning.
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use glam::Vec2;
use subcraft_app::Session;
use subcraft_core::ChannelKind;
use subcraft_doc::LayerContent;
use subcraft_ui::{panels::uv_view, state::{Tool, UiState}, theme};

fn setup() -> (UiState, egui::Context) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 128), None);
    state.tool = Tool::Brush;
    state.uv_wireframe = false;
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (state, ctx)
}

fn frame(state: &mut UiState, ctx: &egui::Context, mut events: Vec<Event>, mods: Modifiers) -> egui::FullOutput {
    events.insert(0, Event::ModifiersChanged(mods));
    let mut output = ctx.run_ui(
        egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() },
        |ui| uv_view::show(state, ui),
    );
    output.textures_delta.clear();
    output
}

fn settle(state: &mut UiState, ctx: &egui::Context) -> egui::FullOutput {
    for _ in 0..3 {
        frame(state, ctx, vec![], Modifiers::NONE);
    }
    frame(state, ctx, vec![], Modifiers::NONE)
}

fn image(output: &egui::FullOutput) -> Rect {
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::epaint::Shape::Rect(r) if r.stroke.color == theme::BORDER_STRONG && r.rect.width() > 100.0 => Some(r.rect),
            _ => None,
        })
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("UV image border")
}

fn text(output: &egui::FullOutput, label: &str) -> Option<Pos2> {
    output.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == label => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    })
}

fn button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers }
}

fn click(state: &mut UiState, ctx: &egui::Context, pos: Pos2, mods: Modifiers) {
    frame(state, ctx, vec![Event::PointerMoved(pos)], mods);
    frame(state, ctx, vec![button(pos, true, mods)], mods);
    frame(state, ctx, vec![button(pos, false, mods)], mods);
}

/// Texture UV at the centroid of the first triangle of the selected texture set.
fn island_uv(state: &UiState) -> Vec2 {
    let mesh = &state.session.scene().mesh;
    let t = mesh.material_triangles(state.session.selected_texture_set().material_slot).next().unwrap();
    let (q, _) = subcraft_mesh::uv_raster::wrap_to_tile(mesh.tri_uvs(t));
    (q[0] + q[1] + q[2]) / 3.0
}

fn painted(state: &UiState, kind: ChannelKind) -> usize {
    let LayerContent::Paint(p) = &state.session.selected_layer().unwrap().content else { panic!() };
    p.channels.get(&kind).map_or(0, |i| i.allocated_tiles())
}

fn drag(state: &mut UiState, ctx: &egui::Context, from: Pos2, to: Pos2, mods: Modifiers) {
    frame(state, ctx, vec![Event::PointerMoved(from), button(from, true, mods)], mods);
    for k in 1..=4 {
        frame(state, ctx, vec![Event::PointerMoved(from.lerp(to, k as f32 / 4.0))], mods);
    }
    frame(state, ctx, vec![button(to, false, mods)], mods);
    frame(state, ctx, vec![], Modifiers::NONE);
}

#[test]
fn brush_drag_in_the_2d_view_paints_the_layer_as_one_undo_step_without_panning() {
    let (mut state, ctx) = setup();
    let rect = image(&settle(&mut state, &ctx));
    let uv = island_uv(&state);
    let start = rect.min + egui::vec2(uv.x, uv.y) * rect.width();
    let entries = state.session.history().entries().len();
    drag(&mut state, &ctx, start, start + egui::vec2(6.0, 3.0), Modifiers::NONE);
    assert!(!state.uv.painting && !state.session.is_stroking(), "stroke ends on release");
    assert_eq!(state.session.history().entries().len(), entries + 1);
    assert_eq!(state.session.history().undo_label(), Some("Paint Stroke"));
    assert!(painted(&state, ChannelKind::BaseColor) > 0);
    assert_eq!(state.uv.offset, egui::Vec2::ZERO, "painting must not pan the view");

    // Alt+drag pans instead of painting.
    drag(&mut state, &ctx, start, start + egui::vec2(40.0, 0.0), Modifiers::ALT);
    assert_eq!(state.session.history().entries().len(), entries + 1);
    assert!(state.uv.offset.x > 10.0, "{:?}", state.uv.offset);
}

#[test]
fn shift_clicking_a_channel_switch_paints_only_that_channel() {
    let (mut state, ctx) = setup();
    let output = settle(&mut state, &ctx);
    let rough = text(&output, "Rough").expect("channel switches in the 2D toolbar");
    click(&mut state, &ctx, rough, Modifiers::SHIFT);
    let brush = &state.session.brush.material;
    for kind in state.session.channels() {
        assert_eq!(brush.is_enabled(kind), kind == ChannelKind::Roughness, "{kind:?}");
    }

    let rect = image(&settle(&mut state, &ctx));
    let uv = island_uv(&state);
    let start = rect.min + egui::vec2(uv.x, uv.y) * rect.width();
    drag(&mut state, &ctx, start, start + egui::vec2(4.0, 0.0), Modifiers::NONE);
    assert!(painted(&state, ChannelKind::Roughness) > 0);
    for kind in [ChannelKind::BaseColor, ChannelKind::Metallic, ChannelKind::Height, ChannelKind::Normal] {
        assert_eq!(painted(&state, kind), 0, "{kind:?} must stay untouched");
    }

    // Shift+click again restores every channel; a plain click switches one off.
    let output = settle(&mut state, &ctx);
    click(&mut state, &ctx, text(&output, "Rough").unwrap(), Modifiers::SHIFT);
    assert!(state.session.channels().into_iter().all(|k| state.session.brush.material.is_enabled(k)));
    let output = settle(&mut state, &ctx);
    click(&mut state, &ctx, text(&output, "Metal").unwrap(), Modifiers::NONE);
    assert!(!state.session.brush.material.is_enabled(ChannelKind::Metallic));
    assert!(state.session.brush.material.is_enabled(ChannelKind::BaseColor));
}

#[test]
fn polygon_fill_in_the_2d_view_fills_the_clicked_island() {
    let (mut state, ctx) = setup();
    state.tool = Tool::PolygonFill;
    let rect = image(&settle(&mut state, &ctx));
    let uv = island_uv(&state);
    click(&mut state, &ctx, rect.min + egui::vec2(uv.x, uv.y) * rect.width(), Modifiers::NONE);
    assert_eq!(state.session.history().undo_label(), Some("Polygon Fill"));
    assert!(painted(&state, ChannelKind::BaseColor) > 0);
}
