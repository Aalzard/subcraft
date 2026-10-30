//! Real egui events: the Brush panel's Pen Pressure section edits the brush settings, and touch
//! events with a force reach the stylus state the 3D view paints with.
use subcraft_app::Session;
use subcraft_ui::{panels::brush, state::UiState};

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    state.stylus.update(&events);
    let mut output = ctx.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 3000.0))),
            ..Default::default()
        },
        |ui| brush::show(state, ui),
    );
    output.textures_delta.clear();
    output
}

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos + t.galley.size() * 0.5)),
            _ => None,
        })
        .collect()
}

fn position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    texts(output)
        .into_iter()
        .find(|(t, _)| t == text || t.ends_with(&format!(" {text}")))
        .map(|(_, p)| p)
        .unwrap_or_else(|| panic!("no text: {text}"))
}

fn click(state: &mut UiState, ctx: &egui::Context, text: &str) {
    let out = frame(state, ctx, vec![]);
    let p = position(&out, text);
    let button = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    frame(state, ctx, vec![egui::Event::PointerMoved(p), button(true)]);
    frame(state, ctx, vec![button(false)]);
}

#[test]
fn pressure_section_toggles_curves_and_shows_the_pen_reading() {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    assert!(!state.session.brush.settings.pressure.any_enabled());
    click(&mut state, &ctx, "PEN PRESSURE");
    let out = frame(&mut state, &ctx, vec![]);
    assert!(texts(&out).iter().any(|(t, _)| t.starts_with("No pen pressure")));
    assert!(!texts(&out).iter().any(|(t, _)| t == "Minimum"), "curve sliders stay hidden while off");

    click(&mut state, &ctx, "Opacity");
    let p = state.session.brush.settings.pressure;
    assert!(p.opacity.enabled && !p.size.enabled && !p.flow.enabled && !p.hardness.enabled);
    let out = frame(&mut state, &ctx, vec![]);
    assert_eq!(texts(&out).iter().filter(|(t, _)| t == "Minimum").count(), 1);

    let touch = |phase, force| egui::Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(1),
        phase,
        pos: egui::pos2(5.0, 5.0),
        force,
    };
    frame(&mut state, &ctx, vec![touch(egui::TouchPhase::Start, Some(0.25)), touch(egui::TouchPhase::Move, Some(0.6))]);
    assert_eq!(state.stylus.pressure(), 0.6);
    let out = frame(&mut state, &ctx, vec![]);
    assert!(texts(&out).iter().any(|(t, _)| t == "Pen pressure: 60%"), "{:?}", texts(&out));
    frame(&mut state, &ctx, vec![touch(egui::TouchPhase::End, None)]);
    assert_eq!(state.stylus.pressure(), 1.0);
}
