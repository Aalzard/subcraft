//! Effect rows in the Layers panel (Painter parity): multi-selection, Alt+Up / Alt+Down,
//! rename by double-click, opacity on the row, drops on other layers and Alt-drag copies.
//! Real egui events, without moving the owner's desktop pointer.
use egui::{Event, Modifiers, Pos2, Rect};
use subcraft_app::{Command, LayerTarget, NewLayer, Session, effect_stack::EffectAddress};
use subcraft_core::{ChannelKind, EffectId, LayerId};
use subcraft_doc::{ContentEffectKind, Levels};
use subcraft_ui::{panels::layers, state::UiState, theme};

/// A base layer "Base" and, above it, the selected fill layer "Top" with `count` content effects
/// named "Step 0" (bottom) … Returns (state, context, base layer, top layer, effects bottom to top).
fn fixture(count: usize) -> (UiState, egui::Context, LayerId, LayerId, Vec<EffectId>) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let base = s.selection().layer.unwrap();
    s.execute(Command::Rename { layer: base, name: "Base".into() }).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let top = s.selection().layer.unwrap();
    s.execute(Command::Rename { layer: top, name: "Top".into() }).unwrap();
    let mut ids = Vec::new();
    for i in 0..count {
        s.execute(Command::AddContentEffect { layer: top, kind: ContentEffectKind::Levels { channel: ChannelKind::BaseColor, levels: Levels::default() } }).unwrap();
        let id = s.selected_effect().unwrap();
        s.execute(Command::RenameEffect { layer: top, target: LayerTarget::Content, effect: id, name: format!("Step {i}") }).unwrap();
        ids.push(id);
    }
    s.execute(Command::SelectLayerTarget { layer: top, target: LayerTarget::Content }).unwrap();
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (UiState::new(s, None), ctx, base, top, ids)
}

fn frame_at(state: &mut UiState, ctx: &egui::Context, width: f32, modifiers: Modifiers, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 500.0))), events: std::iter::once(Event::ModifiersChanged(modifiers)).chain(events).collect(), ..Default::default() },
        |ui| layers::show(state, ui),
    );
    out.textures_delta.clear();
    out
}

fn frame(state: &mut UiState, ctx: &egui::Context, modifiers: Modifiers, events: Vec<Event>) -> egui::FullOutput {
    frame_at(state, ctx, 380.0, modifiers, events)
}

fn layout(state: &mut UiState, ctx: &egui::Context) -> egui::FullOutput {
    for _ in 0..3 {
        frame(state, ctx, Modifiers::NONE, vec![]);
    }
    frame(state, ctx, Modifiers::NONE, vec![])
}

fn texts(out: &egui::FullOutput, name: &str) -> Vec<Rect> {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(Rect::from_min_size(t.pos, t.galley.size())),
            _ => None,
        })
        .collect()
}

fn label(out: &egui::FullOutput, name: &str) -> Rect {
    *texts(out, name).first().unwrap_or_else(|| panic!("missing {name}"))
}

fn button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers }
}

fn click(state: &mut UiState, ctx: &egui::Context, pos: Pos2, modifiers: Modifiers) {
    frame(state, ctx, modifiers, vec![Event::PointerMoved(pos), button(pos, true, modifiers)]);
    frame(state, ctx, modifiers, vec![button(pos, false, modifiers)]);
    frame(state, ctx, modifiers, vec![]);
}

fn key(key: egui::Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn content(state: &UiState, layer: LayerId) -> Vec<EffectId> {
    state.session.selected_texture_set().find(layer).unwrap().content_effects.iter().map(|e| e.id).collect()
}

fn name_of(state: &UiState, layer: LayerId, effect: EffectId) -> String {
    state.session.selected_texture_set().find(layer).unwrap().content_effects.iter().find(|e| e.id == effect).unwrap().name.clone()
}

#[test]
fn ctrl_and_shift_clicks_select_several_effects_and_delete_or_duplicate_takes_them_all() {
    let (mut state, ctx, _, top, ids) = fixture(4);
    let out = layout(&mut state, &ctx);
    // Plain click selects one; Ctrl adds another and makes it the one Properties shows.
    click(&mut state, &ctx, label(&out, "Step 0").center(), Modifiers::NONE);
    assert_eq!(state.session.selected_effect(), Some(ids[0]));
    click(&mut state, &ctx, label(&out, "Step 2").center(), Modifiers::COMMAND);
    assert_eq!(state.session.selected_effect(), Some(ids[2]));
    let stack = (top, LayerTarget::Content);
    assert_eq!(state.effect_multi.targets(stack, state.session.selected_effect()), [ids[0], ids[2]]);

    // Ctrl+D (the Duplicate action) copies both in one undo step; the layer is not duplicated.
    let history = state.session.history().entries().len();
    assert!(subcraft_ui::panels::effects::selection_action(&mut state, false, 0.0));
    assert_eq!(content(&state, top).len(), 6);
    assert_eq!(state.session.selected_texture_set().layers.len(), 2);
    assert_eq!(state.session.history().entries().len(), history + 1);
    state.session.execute(Command::Undo).unwrap();

    // Shift extends from the selected effect; Delete removes the whole span.
    let out = layout(&mut state, &ctx);
    click(&mut state, &ctx, label(&out, "Step 1").center(), Modifiers::NONE);
    click(&mut state, &ctx, label(&out, "Step 3").center(), Modifiers::SHIFT);
    assert_eq!(state.session.selected_effect(), Some(ids[1]), "the first one stays in Properties");
    assert!(subcraft_ui::panels::effects::selection_action(&mut state, true, 0.0));
    assert_eq!(content(&state, top), [ids[0]]);
    assert!(state.session.selected_texture_set().find(top).is_some());
    state.session.execute(Command::Undo).unwrap();
    assert_eq!(content(&state, top), ids);

    // Nothing selected in the stack: the keys are left to the layer.
    state.session.execute(Command::SelectEffect { layer: top, target: LayerTarget::Content, effect: None }).unwrap();
    assert!(!subcraft_ui::panels::effects::selection_action(&mut state, true, 0.0));
    assert_eq!(content(&state, top), ids);
}

#[test]
fn alt_arrows_move_the_selected_effects_one_step() {
    let (mut state, ctx, _, top, ids) = fixture(3);
    let out = layout(&mut state, &ctx);
    click(&mut state, &ctx, label(&out, "Step 0").center(), Modifiers::NONE);
    let history = state.session.history().entries().len();
    frame(&mut state, &ctx, Modifiers::ALT, vec![key(egui::Key::ArrowUp, Modifiers::ALT)]);
    assert_eq!(content(&state, top), [ids[1], ids[0], ids[2]]);
    assert_eq!(state.session.selected_effect(), Some(ids[0]));
    frame(&mut state, &ctx, Modifiers::ALT, vec![key(egui::Key::ArrowUp, Modifiers::ALT)]);
    assert_eq!(content(&state, top), [ids[1], ids[2], ids[0]]);
    // Already on top: nothing happens and no undo step is added.
    frame(&mut state, &ctx, Modifiers::ALT, vec![key(egui::Key::ArrowUp, Modifiers::ALT)]);
    assert_eq!(content(&state, top), [ids[1], ids[2], ids[0]]);
    assert_eq!(state.session.history().entries().len(), history + 2);
    frame(&mut state, &ctx, Modifiers::ALT, vec![key(egui::Key::ArrowDown, Modifiers::ALT)]);
    assert_eq!(content(&state, top), [ids[1], ids[0], ids[2]]);
    // A plain arrow does not move anything.
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::ArrowDown, Modifiers::NONE)]);
    assert_eq!(content(&state, top), [ids[1], ids[0], ids[2]]);
}

#[test]
fn double_click_renames_an_effect_and_escape_cancels() {
    let (mut state, ctx, _, top, ids) = fixture(2);
    let double = |state: &mut UiState, ctx: &egui::Context| {
        let out = layout(state, ctx);
        let pos = label(&out, &name_of(state, top, ids[1])).center();
        for _ in 0..2 {
            frame(state, ctx, Modifiers::NONE, vec![Event::PointerMoved(pos), button(pos, true, Modifiers::NONE)]);
            frame(state, ctx, Modifiers::NONE, vec![button(pos, false, Modifiers::NONE)]);
        }
        // The text box takes focus on its first frame.
        frame(state, ctx, Modifiers::NONE, vec![]);
        frame(state, ctx, Modifiers::NONE, vec![]);
    };
    double(&mut state, &ctx);
    assert_eq!(state.session.selected_effect(), Some(ids[1]), "the renamed row is selected");
    let history = state.session.history().entries().len();
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::Text(" worn".into())]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::Enter, Modifiers::NONE)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![]);
    let renamed = name_of(&state, top, ids[1]);
    assert!(renamed.contains("worn") && renamed.contains("Step 1"), "{renamed}");
    assert_eq!(state.session.history().entries().len(), history + 1);
    assert_eq!(name_of(&state, top, ids[0]), "Step 0");

    // Escape leaves the name alone.
    double(&mut state, &ctx);
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::Text("zzz".into())]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::Escape, Modifiers::NONE)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![]);
    assert_eq!(name_of(&state, top, ids[1]), renamed);
    assert_eq!(state.session.history().entries().len(), history + 1);
    // Typing while renaming never reaches the stack (no Delete, no reorder).
    assert_eq!(content(&state, top), ids);
}

#[test]
fn the_row_shows_blend_and_opacity_and_dragging_the_opacity_edits_the_effect() {
    let (mut state, ctx, _, top, ids) = fixture(1);
    let out = layout(&mut state, &ctx);
    let row = label(&out, "Step 0");
    // The effect row has its own blend mode and opacity, next to the layer rows' ones.
    let on_row = |name: &str| texts(&out, name).into_iter().find(|r| (r.center().y - row.center().y).abs() < 8.0);
    assert!(on_row("Norm").is_some(), "blend mode on the effect row");
    let opacity = on_row("100").expect("opacity on the effect row");
    let history = state.session.history().entries().len();
    let start = opacity.center();
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(start), button(start, true, Modifiers::NONE)]);
    for step in 1..=6 {
        frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(start - egui::vec2(10.0 * step as f32, 0.0))]);
    }
    frame(&mut state, &ctx, Modifiers::NONE, vec![button(start - egui::vec2(60.0, 0.0), false, Modifiers::NONE)]);
    let layer = state.session.selected_texture_set().find(top).unwrap();
    let blend = layer.content_effects[0].blend_for(ChannelKind::BaseColor);
    assert!(blend.opacity < 0.95 && blend.opacity >= 0.0, "opacity {}", blend.opacity);
    assert_eq!(blend.mode, subcraft_doc::BlendMode::Normal);
    assert!(state.session.history().entries().len() > history);
    assert_eq!(content(&state, top), ids, "dragging the value does not drag the row");

    // On another channel the Levels effect (Base Color only) shows no blend controls.
    state.active_channel = ChannelKind::Roughness;
    let out = layout(&mut state, &ctx);
    let row = label(&out, "Step 0");
    assert!(!texts(&out, "100").into_iter().any(|r| (r.center().y - row.center().y).abs() < 8.0));

    // At the narrowest supported panel width (280, see layer_hierarchy) the controls give way to the name.
    state.active_channel = ChannelKind::BaseColor;
    for _ in 0..3 {
        let out = frame_at(&mut state, &ctx, 280.0, Modifiers::NONE, vec![]);
        for shape in &out.shapes {
            if let egui::Shape::Text(t) = &shape.shape {
                assert!(t.pos.x + t.galley.size().x <= 281.0, "text overflow: {:?}", t.galley.text());
            }
        }
    }
}

#[test]
fn effects_drop_on_another_layer_row_and_alt_drops_a_copy() {
    let (mut state, ctx, base, top, ids) = fixture(3);
    let out = layout(&mut state, &ctx);
    // Drag "Step 1" with "Step 0" also selected onto the Base layer row.
    click(&mut state, &ctx, label(&out, "Step 0").center(), Modifiers::NONE);
    click(&mut state, &ctx, label(&out, "Step 1").center(), Modifiers::COMMAND);
    let out = layout(&mut state, &ctx);
    let start = label(&out, "Step 1").center();
    let end = label(&out, "Base").center();
    let history = state.session.history().entries().len();
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(start), button(start, true, Modifiers::NONE)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(start + egui::vec2(8.0, 0.0))]);
    assert!(egui::DragAndDrop::has_payload_of_type::<EffectAddress>(&ctx));
    let hover = frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(end)]);
    assert!(!texts(&hover, "Move to Base").is_empty(), "the target layer says what the drop does");
    assert_eq!(content(&state, top), ids, "nothing moves before the release");
    frame(&mut state, &ctx, Modifiers::NONE, vec![button(end, false, Modifiers::NONE)]);
    assert_eq!(content(&state, top), [ids[2]]);
    assert_eq!(content(&state, base), [ids[0], ids[1]]);
    assert_eq!(state.session.selection().layer, Some(base));
    assert_eq!(state.session.selected_effect(), Some(ids[1]));
    assert_eq!(state.session.history().entries().len(), history + 1);

    // Alt-drag inside the stack: a copy lands at the drop position, the original stays.
    let out = layout(&mut state, &ctx);
    let start = label(&out, "Step 0").center();
    let end = label(&out, "Step 1").center() - egui::vec2(0.0, 6.0);
    frame(&mut state, &ctx, Modifiers::ALT, vec![Event::PointerMoved(start), button(start, true, Modifiers::ALT)]);
    frame(&mut state, &ctx, Modifiers::ALT, vec![Event::PointerMoved(start + egui::vec2(8.0, 0.0))]);
    frame(&mut state, &ctx, Modifiers::ALT, vec![Event::PointerMoved(end)]);
    frame(&mut state, &ctx, Modifiers::ALT, vec![button(end, false, Modifiers::ALT)]);
    let now = content(&state, base);
    assert_eq!((now.len(), now[0], now[1]), (3, ids[0], ids[1]));
    assert_eq!(state.session.selected_effect(), Some(now[2]));
    assert_eq!(name_of(&state, base, now[2]), "Step 0 copy");
    assert_eq!(state.session.history().entries().len(), history + 2);
}
