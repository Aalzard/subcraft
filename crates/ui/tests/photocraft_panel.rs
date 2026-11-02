//! Edit in PhotoCraft in the Layers panel: banner while running, Stop ends the exchange.
use subcraft_app::Session;
use subcraft_app::photocraft::Exchange;
use subcraft_ui::{panels::layers, state::UiState};

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 500.0))), ..Default::default() }, |ui| layers::show(state, ui));
    output.textures_delta.clear();
    output
}

fn find(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    })
}

#[test]
fn banner_shows_while_editing_and_stop_ends_the_exchange() {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..2 { frame(&mut state, &ctx, vec![]); }
    assert!(find(&frame(&mut state, &ctx, vec![]), "Editing in PhotoCraft").is_none());

    let dir = std::env::temp_dir().join(format!("subcraft-photocraft-ui-{}", std::process::id()));
    state.photocraft = Some(Exchange::start(&state.session, &dir, |_| Ok(())).unwrap());
    for _ in 0..2 { frame(&mut state, &ctx, vec![]); }
    let output = frame(&mut state, &ctx, vec![]);
    assert!(find(&output, "Editing in PhotoCraft").is_some());
    let stop = find(&output, "Stop").expect("Stop button");
    frame(&mut state, &ctx, vec![egui::Event::PointerMoved(stop), egui::Event::PointerButton { pos: stop, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
    frame(&mut state, &ctx, vec![egui::Event::PointerButton { pos: stop, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    assert!(state.photocraft.is_none());
    assert!(find(&frame(&mut state, &ctx, vec![]), "Editing in PhotoCraft").is_none());
    std::fs::remove_dir_all(dir).unwrap();
}
