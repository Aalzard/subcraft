//! Feed real egui pointer/key events directly, without touching the user's desktop.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::LayerId;
use subcraft_ui::{panels::{layers, shelf::ShelfDrag}, state::UiState};

fn setup() -> (UiState, egui::Context, [LayerId; 3]) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    let mut ids = [LayerId::default(); 3];
    for (i, name) in ["Bottom", "Middle", "Top"].iter().enumerate() {
        if i > 0 { state.session.execute(Command::AddLayer(NewLayer::Paint)).unwrap(); }
        ids[i] = state.session.selection().layer.unwrap();
        state.session.execute(Command::Rename { layer: ids[i], name: name.to_string() }).unwrap();
    }
    let ctx = egui::Context::default(); subcraft_ui::theme::apply(&ctx);
    (state, ctx, ids)
}

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(360.0, 500.0))), ..Default::default() }, |ui| layers::show(state, ui));
    output.textures_delta.clear(); output
}

fn text_position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    output.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5), _ => None,
    }).unwrap_or_else(|| panic!("Missing visible text: {text}"))
}

fn layout(state: &mut UiState, ctx: &egui::Context) -> egui::FullOutput {
    for _ in 0..3 { frame(state, ctx, vec![]); } frame(state, ctx, vec![])
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

fn drag(state: &mut UiState, ctx: &egui::Context, source: &str, target: &str, offset: f32) {
    let output = layout(state, ctx); let start = text_position(&output, source);
    let end = text_position(&output, target) + egui::vec2(0.0, offset);
    frame(state, ctx, vec![egui::Event::PointerMoved(start), button(start, true)]);
    frame(state, ctx, vec![egui::Event::PointerMoved(start + egui::vec2(8.0, 0.0))]);
    frame(state, ctx, vec![egui::Event::PointerMoved(end)]);
    frame(state, ctx, vec![button(end, false)]);
}

#[test]
fn dragging_layer_names_above_and_below_reorders_with_one_undo_entry() {
    let (mut state, ctx, [bottom, middle, top]) = setup(); let count = state.session.history().entries().len();
    drag(&mut state, &ctx, "Bottom", "Top", -12.0);
    assert_eq!(state.session.selected_texture_set().layers.iter().map(|l| l.id).collect::<Vec<_>>(), [middle, top, bottom]);
    assert_eq!(state.session.history().entries().len(), count + 1);
    state.session.execute(Command::Undo).unwrap();
    drag(&mut state, &ctx, "Top", "Bottom", 12.0);
    assert_eq!(state.session.selected_texture_set().layers.iter().map(|l| l.id).collect::<Vec<_>>(), [top, bottom, middle]);
}

#[test]
fn dragging_into_collapsed_folder_expands_it_and_root_zones_move_out() {
    let (mut state, ctx, [bottom, _, top]) = setup();
    state.session.execute(Command::AddLayer(NewLayer::Folder)).unwrap(); let folder = state.session.selection().layer.unwrap();
    state.session.execute(Command::Rename { layer: folder, name: "Folder".into() }).unwrap();
    state.session.execute(Command::SetFolderExpanded { layer: folder, expanded: false }).unwrap();
    drag(&mut state, &ctx, "Top", "Folder", 0.0);
    assert_eq!(state.session.selected_texture_set().location(top).unwrap().parent, Some(folder));
    assert!(matches!(&state.session.selected_texture_set().find(folder).unwrap().content, subcraft_doc::LayerContent::Group(g) if g.expanded));
    drag(&mut state, &ctx, "Top", "Top level — bottom", 0.0);
    assert_eq!(state.session.selected_texture_set().layers[0].id, top);
    drag(&mut state, &ctx, "Bottom", "Folder", 0.0);
    drag(&mut state, &ctx, "Bottom", "Top level — top", 0.0);
    assert_eq!(state.session.selected_texture_set().layers.last().unwrap().id, bottom);
}

#[test]
fn folder_disclosure_is_clickable_and_does_not_start_a_drag() {
    let (mut state, ctx, [_, _, top]) = setup();
    state.session.execute(Command::GroupLayer(top)).unwrap();
    let folder = state.session.selected_texture_set().location(top).unwrap().parent.unwrap();
    let output = layout(&mut state, &ctx); let pos = text_position(&output, subcraft_ui::icons::CARET_DOWN);
    frame(&mut state, &ctx, vec![egui::Event::PointerMoved(pos), button(pos, true)]);
    frame(&mut state, &ctx, vec![button(pos, false)]);
    assert!(matches!(&state.session.selected_texture_set().find(folder).unwrap().content, subcraft_doc::LayerContent::Group(g) if !g.expanded));
    assert!(!egui::DragAndDrop::has_any_payload(&ctx));
    let output = layout(&mut state, &ctx);
    assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.text() == "Top")));
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

#[test]
fn f2_renames_inline_enter_commits_and_escape_or_blank_preserve_original() {
    let (mut state, ctx, [_, _, top]) = setup(); layout(&mut state, &ctx);
    frame(&mut state, &ctx, vec![key(egui::Key::F2)]); frame(&mut state, &ctx, vec![]);
    // Ctrl+A targets the focused text editor, followed by replacement text and Enter.
    let select_all = egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers { ctrl: true, command: true, ..Default::default() } };
    frame(&mut state, &ctx, vec![select_all.clone(), egui::Event::Text("  Details  ".into())]);
    frame(&mut state, &ctx, vec![key(egui::Key::Enter)]);
    assert_eq!(state.session.selected_texture_set().find(top).unwrap().name, "Details");
    layout(&mut state, &ctx); frame(&mut state, &ctx, vec![key(egui::Key::F2)]); frame(&mut state, &ctx, vec![]);
    frame(&mut state, &ctx, vec![select_all.clone(), egui::Event::Text("Cancelled".into())]);
    frame(&mut state, &ctx, vec![key(egui::Key::Escape)]);
    assert_eq!(state.session.selected_texture_set().find(top).unwrap().name, "Details");
    layout(&mut state, &ctx); frame(&mut state, &ctx, vec![key(egui::Key::F2)]); frame(&mut state, &ctx, vec![]);
    frame(&mut state, &ctx, vec![select_all, egui::Event::Text("   ".into())]);
    frame(&mut state, &ctx, vec![key(egui::Key::Enter)]);
    assert_eq!(state.session.selected_texture_set().find(top).unwrap().name, "Details");
}

#[test]
fn layer_drop_handler_preserves_shelf_material_payloads() {
    let (mut state, ctx, _) = setup(); let output = layout(&mut state, &ctx); let pos = text_position(&output, "Middle");
    let count = state.session.selected_texture_set().layers.len();
    frame(&mut state, &ctx, vec![egui::Event::PointerMoved(pos), button(pos, true)]);
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Material(0));
    frame(&mut state, &ctx, vec![button(pos, false)]);
    assert_eq!(state.session.selected_texture_set().layers.len(), count + 1);
    assert_eq!(state.session.selected_layer().unwrap().name, state.library.materials[0].name);
}

#[test]
fn held_drag_scrolls_a_long_list_and_can_reach_the_root_bottom_target() {
    let (mut state, ctx, _) = setup();
    for i in 0..12 {
        state.session.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
        let layer = state.session.selection().layer.unwrap();
        state.session.execute(Command::Rename { layer, name: format!("Extra {i}") }).unwrap();
    }
    let source = state.session.selection().layer.unwrap();
    let output = layout(&mut state, &ctx); let start = text_position(&output, "Extra 11");
    let clip = output.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == "Extra 11" => Some(s.clip_rect), _ => None,
    }).unwrap();
    frame(&mut state, &ctx, vec![egui::Event::PointerMoved(start), button(start, true)]);
    frame(&mut state, &ctx, vec![egui::Event::PointerMoved(start + egui::vec2(8.0, 0.0))]);
    let edge = egui::pos2(180.0, clip.bottom() - 4.0);
    for _ in 0..120 { frame(&mut state, &ctx, vec![egui::Event::PointerMoved(edge)]); }
    let output = frame(&mut state, &ctx, vec![]);
    let end = text_position(&output, "Top level — bottom");
    assert!(clip.contains(end), "holding the drag at the edge reveals the bottom drop target");
    frame(&mut state, &ctx, vec![egui::Event::PointerMoved(end)]);
    frame(&mut state, &ctx, vec![button(end, false)]);
    assert_eq!(state.session.selected_texture_set().layers[0].id, source);
}
