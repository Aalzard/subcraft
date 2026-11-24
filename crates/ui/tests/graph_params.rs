//! Properties > Graph material: an imported `.sbsar` layer shows its parameters, and a change
//! made in the panel is rendered (once the pointer is released) and lands in the layer.
use egui::{Pos2, Rect};
use subcraft_app::Session;
use subcraft_doc::GraphEngine;
use subcraft_ui::{panels, state::UiState, theme};

const CARDBOARD: &[u8] = include_bytes!("../../sbsar/tests/data/CardBoard.sbsar");
const HEIGHT: f32 = 2400.0;

fn frame(ctx: &egui::Context, state: &mut UiState, width: f32, time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, HEIGHT))), time: Some(time), events, ..Default::default() },
        |ui| panels::properties::show(state, ui),
    );
    out.textures_delta.clear();
    out
}

fn texts(out: &egui::FullOutput) -> Vec<(String, Rect)> {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
            _ => None,
        })
        .collect()
}

fn imported() -> UiState {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::sample_crate(), 64), None);
    state.graph_layers.import_bytes(&mut state.session, GraphEngine::Sbsar, CARDBOARD.to_vec(), "CardBoard.sbsar").unwrap();
    state.graph_layers.finish(&mut state.session);
    assert!(state.session.selected_layer().is_some_and(|l| l.graph.is_some()), "the new layer is selected");
    state
}

#[test]
fn an_sbsar_layer_shows_its_parameters_and_they_fit_the_panel() {
    for width in [260.0_f32, 420.0] {
        let mut state = imported();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut out = frame(&ctx, &mut state, width, 0.0, vec![]);
        for _ in 0..2 {
            out = frame(&ctx, &mut state, width, 0.0, vec![]);
        }
        let drawn = texts(&out);
        for needle in ["GRAPH MATERIAL", "Resolution", "Random seed", "ADVANCED PARAMETERS", "Cardboard Color", "Tearing"] {
            assert!(drawn.iter().any(|(t, _)| t.contains(needle)), "'{needle}' drawn at width {width}: {:?}", drawn.iter().map(|d| &d.0).collect::<Vec<_>>());
        }
        for (t, r) in &drawn {
            assert!(r.right() <= width + 0.5 && r.left() >= -0.5, "text {t:?} spills out at width {width}: {r:?}");
        }
    }
}

#[test]
fn a_slider_change_renders_after_release_and_updates_the_layer() {
    let width = 420.0;
    let mut state = imported();
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut out = frame(&ctx, &mut state, width, 0.0, vec![]);
    out = frame(&ctx, &mut state, width, 0.0, vec![]);
    let label = texts(&out).into_iter().find(|(t, _)| t == "Tearing").expect("Tearing row").1;
    // The slider sits under its label, left of the value box ("0.250"): press near its right end.
    let value = texts(&out).into_iter().find(|(t, r)| t == "0.250" && r.top() > label.top()).expect("Tearing value box").1;
    let at = Pos2::new(value.left() - 60.0, value.center().y);
    let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(&ctx, &mut state, width, 0.1, vec![egui::Event::PointerMoved(at)]);
    frame(&ctx, &mut state, width, 0.2, vec![press(true)]);
    let layer = state.session.selected_layer().unwrap().id;
    assert!(!state.graph_layers.is_rendering(layer), "nothing renders while the button is held");
    frame(&ctx, &mut state, width, 0.3, vec![press(false)]);
    // After the settle time with no button held, the change is sent.
    frame(&ctx, &mut state, width, 1.0, vec![]);
    assert!(state.graph_layers.is_rendering(layer), "the change was sent");
    state.graph_layers.finish(&mut state.session);
    let g = state.session.selected_layer().unwrap().graph.clone().unwrap();
    let tearing = g.values.get("tearing").expect("the new value is stored")[0];
    assert!(tearing > 0.4, "pressed right of the default 0.25: {tearing}");
}
