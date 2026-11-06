//! The Channels panel lists the selected texture set's channels; removing one, changing a format
//! and adding a channel are undoable steps, and the last channel cannot be removed.
use subcraft_app::{Command, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::ChannelFormat;
use subcraft_ui::{panels::channels, state::UiState, theme};

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520.0, 700.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| channels::show(state, ui));
    out.textures_delta.clear();
    out
}

/// Centre of the last shape whose text is exactly `text`.
fn find(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().rev().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    })
}

fn click(state: &mut UiState, ctx: &egui::Context, at: egui::Pos2) {
    let b = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(state, ctx, vec![egui::Event::PointerMoved(at)]);
    frame(state, ctx, vec![b(true)]);
    frame(state, ctx, vec![b(false)]);
}

#[test]
fn channels_are_listed_removed_and_restored() {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut out = frame(&mut state, &ctx, vec![]);
    for _ in 0..2 {
        out = frame(&mut state, &ctx, vec![]);
    }
    let before: Vec<ChannelKind> = state.session.selected_texture_set().channels.iter().map(|c| c.kind).collect();
    for kind in &before {
        assert!(find(&out, kind.display_name()).is_some(), "{} is listed", kind.display_name());
    }
    // The trash button on the Roughness row (same row height, far right).
    let row = find(&out, ChannelKind::Roughness.display_name()).unwrap();
    let trash = out.shapes.iter().filter_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == subcraft_ui::icons::TRASH && (t.pos.y + t.galley.size().y * 0.5 - row.y).abs() < 8.0 => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    }).next().expect("Roughness trash button");
    click(&mut state, &ctx, trash);
    assert!(!state.session.selected_texture_set().has_channel(ChannelKind::Roughness), "Roughness removed");
    state.session.execute(Command::Undo).unwrap();
    assert!(state.session.selected_texture_set().has_channel(ChannelKind::Roughness), "undo restores it");

    // Formats and the last-channel rule through the commands the panel sends.
    let ts = state.session.selected_texture_set().id;
    state.session.execute(Command::SetChannelFormat { texture_set: ts, kind: ChannelKind::BaseColor, format: ChannelFormat::Bit16 }).unwrap();
    assert_eq!(state.session.selected_texture_set().channels.iter().find(|c| c.kind == ChannelKind::BaseColor).unwrap().format, ChannelFormat::Bit16);
    let all: Vec<ChannelKind> = state.session.selected_texture_set().channels.iter().map(|c| c.kind).collect();
    for kind in &all[1..] {
        state.session.execute(Command::RemoveChannel { texture_set: ts, kind: *kind }).unwrap();
    }
    assert!(state.session.execute(Command::RemoveChannel { texture_set: ts, kind: all[0] }).is_err(), "the last channel stays");
}
