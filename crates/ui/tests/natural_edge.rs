//! Old generators opt in through the real Properties UI, through one undoable app command.
use egui::{Event, Pos2, Rect};
use subcraft_app::{Command, LayerTarget, Session};
use subcraft_doc::{Generator, GeneratorKind, MaskEffectKind};
use subcraft_ui::{panels::properties, state::UiState, theme};

fn draw(ctx: &egui::Context, state: &mut UiState, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(420.0, 1100.0))),
        events, ..Default::default() }, |ui| properties::show(state, ui));
    out.textures_delta.clear(); out
}

fn label(out: &egui::FullOutput, name: &str) -> Option<Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos + t.galley.size() * 0.5), _ => None,
    })
}

#[test]
fn legacy_effect_can_enable_spread_from_properties_and_undo_without_losing_settings() {
    let mut session = Session::new(subcraft_mesh::primitives::sample_crate(), 64);
    let layer = session.selection().layer.unwrap();
    session.execute(Command::AddMask { layer, white: false }).unwrap();
    let mut g = Generator::new(GeneratorKind::Edges); g.edge_spread = None; g.noise.seed = 91;
    session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).unwrap();
    let id = session.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].id;
    session.execute(Command::SelectEffect { layer, target: LayerTarget::Mask, effect: Some(id) }).unwrap();
    session.execute(Command::Seal).unwrap(); let before = session.document().clone();
    let mut state = UiState::new(session, None); let ctx = egui::Context::default(); theme::apply(&ctx);
    draw(&ctx, &mut state, vec![]); let out = draw(&ctx, &mut state, vec![]);
    assert!(label(&out, "Spread").is_none());
    let pos = label(&out, "Natural edge wear").unwrap();
    let button = |pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    draw(&ctx, &mut state, vec![Event::PointerMoved(pos), button(true)]);
    draw(&ctx, &mut state, vec![button(false)]);
    let out = draw(&ctx, &mut state, vec![]);
    assert!(label(&out, "Spread").is_some() && label(&out, "Variation").is_some());
    let MaskEffectKind::Generator(edited) = state.session.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].kind else { panic!() };
    assert!(edited.edge_spread.is_some()); assert_eq!(edited.noise.seed, 91);
    state.session.execute(Command::Undo).unwrap(); assert_eq!(state.session.document(), &before);
}
