//! Tool Properties, Brush and Alpha blocks: Painter's section order, Size / Flow / Stroke opacity
//! rows with pen-pressure toggles, jitters in 0..1, Size Space and the alpha slot.
//! Real egui pointer events; no desktop mouse.
use egui::{Event, Pos2, Rect};
use subcraft_app::Session;
use subcraft_paint::SizeSpace;
use subcraft_ui::panels::{brush, brush_rows, shelf::ShelfDrag};
use subcraft_ui::state::UiState;

const WIDTH: f32 = 420.0;

fn state() -> UiState {
    UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None)
}

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(WIDTH, 2600.0))), ..Default::default() },
        |ui| brush::show(state, ui),
    );
    output.textures_delta.clear();
    output
}

fn context(state: &mut UiState) -> egui::Context {
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 {
        frame(state, &ctx, vec![]);
    }
    ctx
}

fn text_y(output: &egui::FullOutput, text: &str) -> Option<f32> {
    output.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos.y),
        _ => None,
    })
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

fn click_at(state: &mut UiState, ctx: &egui::Context, pos: Pos2) {
    frame(state, ctx, vec![Event::PointerMoved(pos), button(pos, true)]);
    frame(state, ctx, vec![button(pos, false)]);
    frame(state, ctx, vec![]);
}

fn click_id(state: &mut UiState, ctx: &egui::Context, id: egui::Id) {
    frame(state, ctx, vec![]);
    let rect = ctx.read_response(id).unwrap_or_else(|| panic!("widget {id:?} is not shown")).rect;
    click_at(state, ctx, rect.center());
}

#[test]
fn sections_follow_painters_order_and_the_brush_rows_are_in_the_panel() {
    let mut s = state();
    let ctx = context(&mut s);
    let out = frame(&mut s, &ctx, vec![]);
    let y = |t: &str| text_y(&out, t).unwrap_or_else(|| panic!("no text: {t}"));
    let order = ["BRUSH", "ALPHA", "STENCIL", "MATERIAL"].map(y);
    // "BRUSH" is also the caption of the preview card at the very top: the section header is the last one.
    let brush_header = out.shapes.iter().filter_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == "BRUSH" => Some(t.pos.y),
        _ => None,
    }).fold(f32::MIN, f32::max);
    assert!(brush_header < order[1] && order[1] < order[2] && order[2] < order[3], "Brush, Alpha, Stencil, Material: {brush_header} {order:?}");
    // Rows of the Brush block, top to bottom as Painter lists them.
    let rows = ["Size", "Flow", "Stroke opacity", "Spacing", "Angle", "Follow Path", "Size Jitter", "Flow Jitter", "Angle Jitter", "Position Jitter",
        "Alignment", "Backface Culling", "Size Space"].map(y);
    assert!(rows.windows(2).all(|w| w[0] < w[1]), "{rows:?}");
    assert!(brush_header < rows[0] && rows[12] < order[1], "the rows sit inside the Brush block");
    // Flow does not follow pressure by default: no minimum row for it, one for Size.
    assert!(text_y(&out, "Minimum Size").is_some() && text_y(&out, "Minimum Flow").is_none());
    assert!(text_y(&out, "JITTER").is_none(), "jitters are rows of the Brush block, not a block of their own");
    assert!(y("Hardness") > order[1] && y("Hardness") < order[2], "Hardness belongs to the Alpha block");
}

#[test]
fn pressure_toggles_sit_on_their_rows_and_switch_pen_pressure() {
    let mut s = state();
    let ctx = context(&mut s);
    let history = s.session.history().entries().len();
    let out = frame(&mut s, &ctx, vec![]);
    for (key, label) in [("size", "Size"), ("flow", "Flow"), ("size_jitter", "Size Jitter"), ("flow_jitter", "Flow Jitter"),
        ("angle_jitter", "Angle Jitter"), ("position_jitter", "Position Jitter")] {
        let rect = ctx.read_response(brush_rows::pressure_toggle_id(key)).unwrap_or_else(|| panic!("no toggle: {key}")).rect;
        let label_y = text_y(&out, label).unwrap();
        assert!((rect.center().y - label_y).abs() < 14.0, "{key} toggle is on the {label} row: {rect:?} vs {label_y}");
        assert!(rect.right() <= WIDTH && rect.left() > WIDTH * 0.6, "{key} toggle is at the right of the row: {rect:?}");
    }
    assert!(ctx.read_response(brush_rows::pressure_toggle_id("stroke_opacity")).is_none(), "Stroke opacity has no pressure control");

    // Size: on by default; off removes the minimum row and pressure stops shrinking the brush.
    s.session.brush.settings.set_size_pressure(true, 0.3);
    frame(&mut s, &ctx, vec![]);
    click_id(&mut s, &ctx, brush_rows::pressure_toggle_id("size"));
    assert!(!s.session.brush.settings.size_pressure());
    assert_eq!(s.session.brush.settings.pressure_size_factor(0.1), 1.0);
    let out = frame(&mut s, &ctx, vec![]);
    assert!(text_y(&out, "Minimum Size").is_none());
    // Back on: the minimum it had is restored.
    click_id(&mut s, &ctx, brush_rows::pressure_toggle_id("size"));
    assert!(s.session.brush.settings.size_pressure());
    assert!((s.session.brush.settings.min_size() - 0.3).abs() < 1e-5, "{}", s.session.brush.settings.min_size());

    click_id(&mut s, &ctx, brush_rows::pressure_toggle_id("flow"));
    assert!(s.session.brush.settings.flow_pressure());
    let out = frame(&mut s, &ctx, vec![]);
    assert!(text_y(&out, "Minimum Flow").is_some());

    for (key, get) in [("size_jitter", (|b: &subcraft_paint::BrushSettings| b.size_jitter_pressure) as fn(&subcraft_paint::BrushSettings) -> bool),
        ("flow_jitter", |b| b.flow_jitter_pressure), ("angle_jitter", |b| b.angle_jitter_pressure), ("position_jitter", |b| b.position_jitter_pressure)] {
        assert!(!get(&s.session.brush.settings));
        click_id(&mut s, &ctx, brush_rows::pressure_toggle_id(key));
        assert!(get(&s.session.brush.settings), "{key}");
        click_id(&mut s, &ctx, brush_rows::pressure_toggle_id(key));
        assert!(!get(&s.session.brush.settings), "{key}");
    }
    assert_eq!(s.session.history().entries().len(), history, "brush settings are tool state, never undo steps");
}

#[test]
fn jitter_rows_show_zero_to_one_whatever_the_stored_unit() {
    let mut s = state();
    s.session.brush.settings.angle_jitter_deg = 90.0;
    s.session.brush.settings.position_jitter = 0.4;
    let ctx = context(&mut s);
    let out = frame(&mut s, &ctx, vec![]);
    let values: Vec<String> = out.shapes.iter().filter_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()),
        _ => None,
    }).collect();
    assert!(values.iter().any(|v| v == "0.5" || v == "0.50"), "Angle Jitter 90 degrees shows as 0.5: {values:?}");
    assert!(values.iter().any(|v| v == "0.4" || v == "0.40"), "Position Jitter: {values:?}");
    assert!(!values.iter().any(|v| v == "90" || v == "90.00"));
    // Showing the panel never rewrites the stored values.
    assert_eq!(s.session.brush.settings.angle_jitter_deg, 90.0);
    assert_eq!(s.session.brush.settings.position_jitter, 0.4);
}

#[test]
fn size_space_dropdown_lists_object_viewport_texture() {
    let mut s = state();
    let ctx = context(&mut s);
    assert_eq!(s.session.brush.settings.size_space, SizeSpace::Object);
    let out = frame(&mut s, &ctx, vec![]);
    let pos = out.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == "Object" => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    }).expect("the dropdown shows the current space");
    click_at(&mut s, &ctx, pos);
    let out = frame(&mut s, &ctx, vec![]);
    let option = |name: &str| out.shapes.iter().filter_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == name => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    }).last();
    assert!(option("Viewport").is_some() && option("Texture").is_some(), "the open list offers every space");
    let texture = option("Texture").unwrap();
    click_at(&mut s, &ctx, texture);
    assert_eq!(s.session.brush.settings.size_space, SizeSpace::Texture);
}

#[test]
fn alpha_slot_takes_a_shelf_drop_and_its_cross_removes_the_alpha() {
    let mut s = state();
    let ctx = context(&mut s);
    assert!(s.session.brush.alpha.is_none(), "a new session starts on the round tip");
    assert!(ctx.read_response(brush_rows::alpha_remove_id()).is_none(), "nothing to remove on the round tip");
    let (index, name) = s.library.alphas.iter().enumerate().find(|(_, a)| a.bitmap.is_some() && !a.name.starts_with("Starter: "))
        .map(|(i, a)| (i, a.name.clone())).expect("a bitmap tip");
    let slot = ctx.read_response(brush_rows::alpha_slot_id()).expect("alpha slot").rect.center();
    // Drag a tip from the Shelf and release it over the slot.
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Alpha(index));
    frame(&mut s, &ctx, vec![Event::PointerMoved(slot), button(slot, true)]);
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Alpha(index));
    frame(&mut s, &ctx, vec![Event::PointerMoved(slot), button(slot, false)]);
    assert_eq!(s.session.brush.alpha_name, name);
    assert!(s.session.brush.alpha.is_some());
    // A material dropped there is not an alpha: ignored.
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Material(0));
    frame(&mut s, &ctx, vec![Event::PointerMoved(slot), button(slot, true)]);
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Material(0));
    frame(&mut s, &ctx, vec![Event::PointerMoved(slot), button(slot, false)]);
    egui::DragAndDrop::clear_payload(&ctx);
    assert_eq!(s.session.brush.alpha_name, name);

    click_id(&mut s, &ctx, brush_rows::alpha_remove_id());
    assert!(s.session.brush.alpha.is_none(), "the cross returns to the round tip");
}

#[test]
fn clicking_the_alpha_slot_opens_a_browser_that_sets_the_tip() {
    let mut s = state();
    let ctx = context(&mut s);
    let name = s.library.alphas.iter().find(|a| a.bitmap.is_some() && !a.name.starts_with("Starter: ")).map(|a| a.name.clone()).expect("a bitmap tip");
    click_id(&mut s, &ctx, brush_rows::alpha_slot_id());
    let out = frame(&mut s, &ctx, vec![]);
    assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.text() == "Search alphas")), "the browser has a search field");
    // No thumbnails are rendered in a headless test, so each tile shows the tip's name.
    let tile = out.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == name => Some(t.pos + egui::vec2(10.0, 10.0)),
        _ => None,
    }).unwrap_or_else(|| panic!("no tile for {name}"));
    click_at(&mut s, &ctx, tile);
    assert_eq!(s.session.brush.alpha_name, name);
}
