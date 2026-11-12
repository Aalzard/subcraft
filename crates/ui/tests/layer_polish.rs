//! Layer rows (Painter parity): sliding over eyes, stepping the blend dropdown, Ctrl + drag
//! duplicates, several rows dragged together and colour tags.
//! Real egui events, without moving the owner's desktop pointer.
use egui::{Event, Modifiers, Pos2, Rect};
use subcraft_app::{Command, NewLayer, Session, multi_select::ClickMods};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{BlendMode, LayerColor};
use subcraft_ui::{icons, panels::layers, state::UiState, theme};

/// Three paint layers "One" (bottom), "Two", "Three" (top, selected).
fn fixture() -> (UiState, egui::Context, [LayerId; 3]) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let one = s.selection().layer.unwrap();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let two = s.selection().layer.unwrap();
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let three = s.selection().layer.unwrap();
    for (layer, name) in [(one, "One"), (two, "Two"), (three, "Three")] {
        s.execute(Command::Rename { layer, name: name.into() }).unwrap();
    }
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (UiState::new(s, None), ctx, [one, two, three])
}

fn frame(state: &mut UiState, ctx: &egui::Context, modifiers: Modifiers, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(380.0, 600.0))), events: std::iter::once(Event::ModifiersChanged(modifiers)).chain(events).collect(), ..Default::default() },
        |ui| layers::show(state, ui),
    );
    out.textures_delta.clear();
    out
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

/// The text `name` drawn on the row of the layer called `row`.
fn on_row(out: &egui::FullOutput, row: &str, name: &str) -> Rect {
    let y = label(out, row).center().y;
    texts(out, name).into_iter().min_by(|a, b| (a.center().y - y).abs().total_cmp(&(b.center().y - y).abs())).unwrap_or_else(|| panic!("missing {name}"))
}

fn button(pos: Pos2, pressed: bool, modifiers: Modifiers) -> Event {
    Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers }
}

fn key(key: egui::Key) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn names(state: &UiState) -> Vec<String> {
    state.session.selected_texture_set().layers.iter().map(|l| l.name.clone()).collect()
}

fn visible(state: &UiState, id: LayerId) -> bool {
    state.session.selected_texture_set().find(id).unwrap().visible
}

#[test]
fn pressing_an_eye_and_sliding_over_others_gives_them_the_same_state() {
    let (mut state, ctx, [one, two, three]) = fixture();
    // "Two" starts hidden: the slide hides the others and leaves it as it is.
    state.session.execute(Command::SetVisible { layer: two, visible: false }).unwrap();
    let out = layout(&mut state, &ctx);
    let eye = |row: &str, icon: &str| on_row(&out, row, icon).center();
    let (top, middle, bottom) = (eye("Three", icons::EYE), eye("Two", icons::EYE_SLASH), eye("One", icons::EYE));
    let history = state.session.history().entries().len();

    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(top), button(top, true, Modifiers::NONE)]);
    assert!(!visible(&state, three), "the pressed eye toggles at once");
    // Held still, it does not flip back.
    frame(&mut state, &ctx, Modifiers::NONE, vec![]);
    assert!(!visible(&state, three));
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(middle)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(bottom)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![button(bottom, false, Modifiers::NONE)]);
    assert!(!visible(&state, three) && !visible(&state, two) && !visible(&state, one));
    assert_eq!(state.session.history().entries().len(), history + 2, "only the two eyes that changed");

    // After the release, moving over an eye does nothing.
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(top)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(middle)]);
    assert_eq!(state.session.history().entries().len(), history + 2);

    // Sliding back up from a hidden eye shows them all again.
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(bottom), button(bottom, true, Modifiers::NONE)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(middle)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(top)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![button(top, false, Modifiers::NONE)]);
    assert!(visible(&state, three) && visible(&state, two) && visible(&state, one));
    assert_eq!(names(&state), ["One", "Two", "Three"], "no row was dragged");
}

#[test]
fn a_clicked_blend_dropdown_steps_with_the_arrow_keys_until_escape() {
    let (mut state, ctx, [_, _, three]) = fixture();
    let mode = |state: &UiState| state.session.selected_texture_set().find(three).unwrap().blend_for(ChannelKind::BaseColor).mode;
    let modes: Vec<BlendMode> = BlendMode::available(false, false).collect();
    assert_eq!(mode(&state), modes[0]);
    let out = layout(&mut state, &ctx);

    // Arrows do nothing before the dropdown was clicked.
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::ArrowDown)]);
    assert_eq!(mode(&state), modes[0]);

    // Click it open and closed again: it keeps the focus.
    let combo = on_row(&out, "Three", modes[0].short_label()).center();
    for _ in 0..2 {
        frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(combo), button(combo, true, Modifiers::NONE)]);
        frame(&mut state, &ctx, Modifiers::NONE, vec![button(combo, false, Modifiers::NONE)]);
        frame(&mut state, &ctx, Modifiers::NONE, vec![]);
    }
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::ArrowDown)]);
    assert_eq!(mode(&state), modes[1]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::ArrowDown)]);
    assert_eq!(mode(&state), modes[2]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::ArrowUp)]);
    assert_eq!(mode(&state), modes[1]);
    // The wheel over the dropdown steps too (up = previous mode).
    let wheel = |y: f32| Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, y), phase: egui::TouchPhase::Move, modifiers: Modifiers::NONE };
    frame(&mut state, &ctx, Modifiers::NONE, vec![Event::PointerMoved(combo), wheel(-1.0)]);
    assert_eq!(mode(&state), modes[2]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![wheel(1.0)]);
    assert_eq!(mode(&state), modes[1]);
    // The first mode is the end of the list.
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::ArrowUp)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::ArrowUp)]);
    assert_eq!(mode(&state), modes[0]);

    // Escape gives the keys back.
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::Escape)]);
    frame(&mut state, &ctx, Modifiers::NONE, vec![key(egui::Key::ArrowDown)]);
    assert_eq!(mode(&state), modes[0]);
}

#[test]
fn ctrl_drag_duplicates_a_layer_and_selected_rows_drag_together() {
    let (mut state, ctx, [one, two, three]) = fixture();
    let out = layout(&mut state, &ctx);
    let drag = |state: &mut UiState, from: Pos2, to: Pos2, modifiers: Modifiers| {
        frame(state, &ctx, modifiers, vec![Event::PointerMoved(from), button(from, true, modifiers)]);
        frame(state, &ctx, modifiers, vec![Event::PointerMoved(from + egui::vec2(0.0, 8.0))]);
        frame(state, &ctx, modifiers, vec![Event::PointerMoved(to)]);
        frame(state, &ctx, modifiers, vec![button(to, false, modifiers)]);
        frame(state, &ctx, Modifiers::NONE, vec![]);
    };
    // Ctrl + drag "One" onto the upper half of "Three": a copy lands on top, "One" stays.
    let from = label(&out, "One").center();
    let to = egui::pos2(from.x, label(&out, "Three").top() - 2.0);
    let history = state.session.history().entries().len();
    drag(&mut state, from, to, Modifiers::COMMAND);
    assert_eq!(names(&state), ["One", "Two", "Three", "One copy"]);
    assert_eq!(state.session.history().entries().len(), history + 1);
    let copy = state.session.selected_texture_set().layers[3].id;
    assert_eq!(state.session.selection().layer, Some(copy), "the copy is selected");
    state.session.execute(Command::Undo).unwrap();

    // "One" and "Two" selected: dragging "Two" above "Three" takes both, in order.
    state.session.execute(Command::SelectLayer(Some(two))).unwrap();
    let rows = [three, two, one];
    state.multi.click(&rows, two, ClickMods::default());
    state.multi.click(&rows, one, ClickMods { toggle: true, range: false });
    let out = layout(&mut state, &ctx);
    let from = label(&out, "Two").center();
    let to = egui::pos2(from.x, label(&out, "Three").top() - 2.0);
    drag(&mut state, from, to, Modifiers::NONE);
    assert_eq!(names(&state), ["Three", "One", "Two"]);
    assert_eq!(state.session.history().entries().len(), history + 1, "one undo step for both rows");
}

#[test]
fn a_colour_tag_draws_a_stripe_on_its_row_and_a_dimmed_one_inside_a_tagged_folder() {
    let (mut state, ctx, [one, _, three]) = fixture();
    let [r, g, b] = LayerColor::Teal.rgb();
    let teal = egui::Color32::from_rgb(r, g, b);
    let stripes = |out: &egui::FullOutput, color: egui::Color32| {
        out.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(rect) if rect.fill == color && rect.rect.width() <= 4.5)).count()
    };
    let out = layout(&mut state, &ctx);
    assert_eq!(stripes(&out, teal), 0);

    state.session.execute(Command::SetLayerColor { layers: vec![one], color: Some(LayerColor::Teal) }).unwrap();
    let out = layout(&mut state, &ctx);
    assert_eq!(stripes(&out, teal), 1);

    // A tagged folder marks the rows inside it with a dimmed stripe.
    state.session.execute(Command::GroupLayers(vec![three])).unwrap();
    let folder = state.session.selection().layer.unwrap();
    state.session.execute(Command::SetLayerColor { layers: vec![folder], color: Some(LayerColor::Teal) }).unwrap();
    let out = layout(&mut state, &ctx);
    assert_eq!(stripes(&out, teal), 2, "the folder and the tagged layer");
    assert_eq!(stripes(&out, teal.gamma_multiply(0.4)), 1, "the layer inside the folder");
}
