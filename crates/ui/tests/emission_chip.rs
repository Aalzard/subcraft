//! Properties > Channels: a "+ emiss" chip adds Emission to a texture set that lacks it.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::ChannelKind;
use subcraft_ui::state::UiState;

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 1400.0))), ..Default::default() },
        |ui| subcraft_ui::panels::properties::show(state, ui),
    );
    out.textures_delta.clear();
    out
}
fn texts(out: &egui::FullOutput) -> Vec<String> {
    out.shapes.iter().filter_map(|s| if let egui::epaint::Shape::Text(t) = &s.shape { Some(t.galley.text().to_string()) } else { None }).collect()
}
fn position(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    })
}
fn click(state: &mut UiState, ctx: &egui::Context, text: &str) {
    let out = frame(state, ctx, vec![]);
    let p = position(&out, text).unwrap_or_else(|| panic!("no {text:?}"));
    let b = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(state, ctx, vec![egui::Event::PointerMoved(p), b(true)]);
    frame(state, ctx, vec![b(false)]);
}

#[test]
fn the_plus_chip_adds_emission_to_the_set_and_the_layer_then_shows_its_controls() {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 {
        frame(&mut state, &ctx, vec![]);
    }
    let out = frame(&mut state, &ctx, vec![]);
    assert!(position(&out, "+ emiss").is_some(), "chip offered when the set lacks Emission: {:?}", texts(&out));
    assert!(position(&out, "+ ao").is_some());
    assert!(position(&out, "emiss").is_none());
    let before = state.session.document().clone();

    click(&mut state, &ctx, "+ emiss");
    assert!(state.session.selected_texture_set().has_channel(ChannelKind::Emissive));
    let out = frame(&mut state, &ctx, vec![]);
    assert!(position(&out, "+ emiss").is_none(), "offered only once");
    assert!(position(&out, "emiss").is_some(), "a normal chip replaces it");
    assert!(position(&out, "Emissive").is_some(), "the Emissive controls are shown: {:?}", texts(&out));
    state.session.execute(Command::Undo).unwrap();
    assert_eq!(state.session.document(), &before, "one undo removes the channel again");
}
