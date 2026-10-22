//! Render quality window: real egui events change the saved viewport look, and the export
//! settings keep the view's aspect ratio.
use subcraft_app::Session;
use subcraft_render::{GlareMode, PostParams, ToneMap};
use subcraft_ui::render_export::{self, ExportSettings};
use subcraft_ui::state::UiState;
use subcraft_ui::viewport_bar::ViewportPrefs;

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 1100.0))),
            ..Default::default()
        },
        |ui| render_export::window(state, ui.ctx()),
    );
    out.textures_delta.clear();
    out
}
fn position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text {text:?}"))
}
fn click(state: &mut UiState, ctx: &egui::Context, text: &str) {
    let out = frame(state, ctx, vec![]);
    let p = position(&out, text);
    let button = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(state, ctx, vec![egui::Event::PointerMoved(p), button(true)]);
    frame(state, ctx, vec![button(false)]);
}
fn setup() -> (UiState, egui::Context) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    state.render_window = true;
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 {
        frame(&mut state, &ctx, vec![]);
    }
    (state, ctx)
}

#[test]
fn the_window_changes_the_tone_map_and_reset_restores_the_default_look() {
    let (mut state, ctx) = setup();
    assert_eq!(state.recovery.prefs.viewport.tone_map(), ToneMap::Aces);
    click(&mut state, &ctx, "ACES");
    click(&mut state, &ctx, "Filmic");
    assert_eq!(state.recovery.prefs.viewport.tone_map(), ToneMap::Filmic);
    state.recovery.prefs.viewport.bloom = 1.2;
    state.recovery.prefs.viewport.ssao = 0.7;
    state.recovery.prefs.viewport.glare = 1.0;
    state.recovery.prefs.viewport.scale_percent = 50; // not part of the look: must survive a reset
    click(&mut state, &ctx, "Reset look");
    let p = &state.recovery.prefs.viewport;
    assert_eq!((p.tone_map(), p.bloom, p.ssao, p.glare, p.exposure), (ToneMap::Aces, 0.0, 0.0, 0.0, 1.0));
    assert_eq!(p.emission_glow, 0.35, "reset restores the subtle default glow");
    assert_eq!(p.scale_percent, 50, "reset only touches the look");
    // Closing the window with its X leaves the settings alone.
    state.render_window = false;
    frame(&mut state, &ctx, vec![]);
    assert_eq!(state.recovery.prefs.viewport.scale_percent, 50);
}

#[test]
fn prefs_map_to_render_settings_and_old_files_get_defaults() {
    let defaults = ViewportPrefs::default().post_params(2.0);
    assert_eq!(defaults, PostParams { ssao_radius: defaults.ssao_radius, ..PostParams::default() });
    assert!((defaults.ssao_radius - 0.03 * 2.0).abs() < 1e-6, "3% of a model with diagonal 2");
    let old: ViewportPrefs = serde_json::from_str(r#"{"grid":false}"#).unwrap();
    assert_eq!((old.tone_map(), old.bloom, old.ssao, old.exposure, old.emission_glow), (ToneMap::Aces, 0.0, 0.0, 1.0, 0.35));
    let wild = ViewportPrefs { tone_map: 99, bloom: -3.0, ssao: 9.0, glare_mode: 99, glare_length: 7.0, ssao_radius: 1e6, ..Default::default() };
    let p = wild.post_params(1.0);
    assert_eq!((p.tone_map, p.bloom, p.ssao, p.glare_mode, p.glare_length), (ToneMap::Aces, 0.0, 1.0, GlareMode::Horizontal, 1.0), "out-of-range values are clamped");
    assert!(p.ssao_radius <= 0.5);
    let round: ViewportPrefs = serde_json::from_str(&serde_json::to_string(&ViewportPrefs { bloom: 0.8, glare_mode: 3, ..Default::default() }).unwrap()).unwrap();
    assert_eq!((round.bloom, round.glare_mode), (0.8, 3));
}

#[test]
fn export_pictures_keep_the_view_aspect_with_the_chosen_long_side() {
    let s = ExportSettings { size: 2048, ..Default::default() };
    assert_eq!(s.picture([1600, 900]), [2048, 1152]);
    assert_eq!(s.picture([900, 1600]), [1152, 2048]);
    assert_eq!(s.picture([1000, 1000]), [2048, 2048]);
    assert_eq!(ExportSettings { size: 1024, ..s.clone() }.picture([1, 1]), [1024, 1024]);
    assert!(s.picture([0, 0])[0] >= 1);
    assert!(s.supersample && render_export::FRAME_COUNTS.contains(&s.frames));
}
