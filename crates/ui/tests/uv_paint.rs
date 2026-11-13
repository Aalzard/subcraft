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
            egui::epaint::Shape::Rect(r) if r.stroke.color == theme::BORDER_STRONG() && r.rect.width() > 100.0 => Some(r.rect),
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

    // Alt+drag turns the view instead of painting (Painter's 2D view); the texture point under
    // the cursor at the start of the drag stays there.
    drag(&mut state, &ctx, start, start + egui::vec2(40.0, 0.0), Modifiers::ALT);
    assert_eq!(state.session.history().entries().len(), entries + 1);
    assert!(state.uv.rotation > 0.1, "{}", state.uv.rotation);
    // Shift snaps the turn to quarter turns: back to upright from here.
    drag(&mut state, &ctx, start, start - egui::vec2(20.0, 0.0), Modifiers::ALT | Modifiers::SHIFT);
    assert_eq!(state.uv.rotation, 0.0, "snapped back upright");
    // Ctrl+Alt+drag pans.
    let before = state.uv.offset;
    drag(&mut state, &ctx, start, start + egui::vec2(40.0, 0.0), Modifiers::ALT | Modifiers::COMMAND);
    assert_eq!(state.session.history().entries().len(), entries + 1);
    assert!(state.uv.offset.x > before.x + 10.0, "{:?} from {before:?}", state.uv.offset);
    assert_eq!(state.uv.rotation, 0.0, "panning does not turn the view");
}

#[test]
fn a_solo_brush_channel_paints_only_that_channel_in_the_2d_view() {
    // The Brush panel's channel toggles send this (Alt+click "rough").
    let (mut state, ctx) = setup();
    state.session.execute(subcraft_app::Command::ToggleBrushChannel { channel: ChannelKind::Roughness, solo: true }).unwrap();
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
    let output = settle(&mut state, &ctx);
    assert!(text(&output, "Rough").is_none(), "channel switches live in the Brush panel, not the 2D toolbar");
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

/// Largest base colour alpha painted on the selected layer.
fn max_alpha(state: &UiState) -> f32 {
    let LayerContent::Paint(p) = &state.session.selected_layer().unwrap().content else { panic!() };
    let Some(img) = p.channels.get(&ChannelKind::BaseColor) else { return 0.0 };
    let mut max = 0.0f32;
    for y in 0..img.height() {
        for x in 0..img.width() {
            max = max.max(img.get(x, y)[img.comps() - 1]);
        }
    }
    max
}

/// QA 2026-11: the Clone and Smudge tools painted a plain brush-colour stroke in the 2D view.
/// They now work there like in the 3D view: neither deposits the brush colour on an empty layer,
/// and Clone needs a source first.
#[test]
fn clone_and_smudge_do_not_paint_plain_strokes_in_the_2d_view() {
    for tool in [Tool::Clone, Tool::Smudge] {
        let (mut state, ctx) = setup();
        state.tool = tool;
        let rect = image(&settle(&mut state, &ctx));
        let uv = island_uv(&state);
        let start = rect.min + egui::vec2(uv.x, uv.y) * rect.width();
        let doc = state.session.document().clone();
        drag(&mut state, &ctx, start, start + egui::vec2(6.0, 3.0), Modifiers::NONE);
        assert!(!state.uv.painting && !state.session.is_stroking(), "{tool:?}");
        assert_eq!(max_alpha(&state), 0.0, "{tool:?} painted the brush colour");
        if tool == Tool::Clone {
            assert_eq!(state.session.document(), &doc, "Clone without a source changes nothing");
        }
    }
}

/// Ctrl+click sets the clone source in the 2D view; the next drag copies the texture from there.
#[test]
fn ctrl_click_sets_the_2d_clone_source_and_a_drag_copies_from_it() {
    let (mut state, ctx) = setup();
    let rect = image(&settle(&mut state, &ctx));
    let uv = island_uv(&state);
    let source = rect.min + egui::vec2(uv.x, uv.y) * rect.width();
    // Something to copy: a brush dab at the source.
    drag(&mut state, &ctx, source, source + egui::vec2(2.0, 0.0), Modifiers::NONE);
    assert!(max_alpha(&state) > 0.0);
    state.tool = Tool::Clone;
    settle(&mut state, &ctx);
    click(&mut state, &ctx, source, Modifiers::COMMAND);
    let set = state.session.clone_source_uv().expect("Ctrl+click sets the source");
    assert!((set - uv).length() < 0.01, "{set} vs {uv}");
    assert!(!state.session.is_stroking() && !state.uv.painting, "setting the source paints nothing");
    let entries = state.session.history().entries().len();
    let target = source + egui::vec2(0.0, 30.0);
    drag(&mut state, &ctx, target, target + egui::vec2(3.0, 0.0), Modifiers::NONE);
    assert_eq!(state.session.history().undo_label(), Some("Clone Stroke"));
    assert_eq!(state.session.history().entries().len(), entries + 1);
}
