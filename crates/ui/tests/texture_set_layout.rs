//! The Texture Set list (names, resolution row) and Texture Set Settings (channels, mesh maps,
//! Bake button) at the sizes their panels get.
use egui::{Event, Pos2, Rect};
use subcraft_app::Session;
use subcraft_ui::{panels::{channels, texture_sets}, state::UiState, theme};

fn frame(ctx: &egui::Context, state: &mut UiState, width: f32, height: f32, settings: bool, events: Vec<Event>) -> (egui::FullOutput, Rect) {
    let mut bounds = Rect::NOTHING;
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, height))), events, ..Default::default() }, |ui| {
        if settings { channels::show(state, ui) } else { texture_sets::show(state, ui) }
        bounds = ui.min_rect();
    });
    out.textures_delta.clear();
    (out, bounds)
}
fn text(out: &egui::FullOutput, label: &str) -> Rect {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == label => Some(Rect::from_min_size(t.pos, t.galley.size())), _ => None
    }).unwrap_or_else(|| panic!("missing {label}"))
}
fn setup() -> (UiState, egui::Context) {
    let ctx = egui::Context::default(); theme::apply(&ctx);
    (UiState::new(Session::new(subcraft_mesh::primitives::sample_crate(), 128), None), ctx)
}

#[test]
fn texture_set_list_fits_its_panel_with_the_resolution_row_below() {
    for (width, height) in [(300., 140.), (400., 180.), (900., 220.)] {
        let (mut state, ctx) = setup();
        let before = state.session.document().clone();
        let mut result = frame(&ctx, &mut state, width, height, false, vec![]);
        for _ in 0..2 { result = frame(&ctx, &mut state, width, height, false, vec![]); }
        let (out, bounds) = result;
        assert!(bounds.right() <= width + 0.5, "horizontal overflow: {bounds:?}");
        assert!(bounds.bottom() <= height + 0.5, "the list scrolls, the panel does not grow: {bounds:?}");
        let (body, trim, size) = (text(&out, "Body"), text(&out, "Trim"), text(&out, "Size"));
        assert!(trim.top() > body.top() && size.top() > trim.bottom(), "sets, then the resolution row");
        assert_eq!(state.session.document(), &before);
    }
}

#[test]
fn selecting_a_set_and_the_bake_button_dispatch() {
    let (mut state, ctx) = setup();
    for _ in 0..3 { frame(&ctx, &mut state, 360., 180., false, vec![]); }
    let trim = text(&frame(&ctx, &mut state, 360., 180., false, vec![]).0, "Trim").center();
    let click = |state: &mut UiState, pos, settings| for pressed in [true, false] {
        frame(&ctx, state, 360., 1400., settings, vec![Event::PointerMoved(pos), Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]);
    };
    for pressed in [true, false] {
        frame(&ctx, &mut state, 360., 180., false, vec![Event::PointerMoved(trim), Event::PointerButton { pos: trim, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]);
    }
    assert_eq!(state.session.selected_texture_set().name, "Trim");
    for _ in 0..2 { frame(&ctx, &mut state, 360., 1400., true, vec![]); }
    let out = frame(&ctx, &mut state, 360., 1400., true, vec![]).0;
    // Every mesh map has a row, not baked yet.
    for kind in subcraft_core::MeshMapKind::ALL {
        text(&out, kind.display_name());
    }
    let bake = text(&out, &format!("{}  Bake Mesh Maps", subcraft_ui::icons::MAGIC_WAND)).center();
    click(&mut state, bake, true);
    assert!(state.bake.open);
}
