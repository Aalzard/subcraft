//! Shelf > Brushes: one click activates, a fading viewport notice acknowledges it, and a
//! double-click opens the Brush settings. Real egui pointer events; no desktop mouse.
use egui::{Event, Pos2, Rect};
use subcraft_app::brush_preset::ShelfBrush;
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_doc::MaskEffectKind;
use subcraft_image::TiledImage;
use subcraft_paint::PaintTarget;
use subcraft_ui::panels::shelf::{self, ShelfTab};
use subcraft_ui::state::{BrushNotice, Tool, UiState};

/// Paint layer with a mask Paint effect selected as the brush destination.
fn state() -> UiState {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::AddMask { layer, white: true }).unwrap();
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Paint { image: TiledImage::new(64, 64, 2, [0.0; 4]) } }).unwrap();
    let effect = s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].id;
    s.execute(Command::SelectEffect { layer, target: LayerTarget::Mask, effect: Some(effect) }).unwrap();
    assert_eq!(s.brush.target, PaintTarget::MaskEffect(effect));
    UiState::new(s, None)
}

fn tip(state: &UiState) -> subcraft_assets::BrushAlpha {
    state.library.alphas.iter().find(|a| a.bitmap.is_some() && !a.name.starts_with("Starter: ")).expect("a textured tip").clone()
}

#[test]
fn one_click_activates_from_any_tool_and_keeps_the_mask_paint_effect_destination() {
    for (tool, expected) in [(Tool::PolygonFill, Tool::Brush), (Tool::Eyedropper, Tool::Brush), (Tool::Pen, Tool::Brush),
        (Tool::Eraser, Tool::Eraser), (Tool::Brush, Tool::Brush)] {
        let mut s = state();
        let target = s.session.brush.target;
        let history = s.session.history().entries().len();
        let document = s.session.document().clone();
        s.tool = tool; s.picking_parts = true;
        let alpha = tip(&s);
        s.select_shelf_brush(ShelfBrush::Tip(&alpha), 3.0);
        assert_eq!(s.tool, expected, "from {tool:?}");
        assert!(!s.picking_parts, "the next viewport click must paint");
        assert_eq!(s.session.brush.alpha_name, alpha.name);
        assert_eq!(s.session.brush.target, target, "destination is tool state, not reset by a brush click");
        assert_eq!(s.session.history().entries().len(), history, "selection is not an undo step");
        assert_eq!(s.session.document(), &document);
        assert_eq!(s.brush_notice, Some(BrushNotice { name: alpha.name.clone(), tip: alpha.name.clone(), since: 3.0 }));
    }
}

#[test]
fn presets_acknowledge_the_brush_that_became_active_and_failures_change_nothing() {
    let mut s = state();
    let alpha = tip(&s);
    s.session.set_brush_alpha(&alpha);
    s.session.brush.settings.size = 17.0;
    let mut preset = s.session.brush_preset("Scratches");
    s.session.brush.settings.size = 3.0;
    s.select_shelf_brush(ShelfBrush::Preset(&preset), 1.0);
    assert_eq!(s.session.brush.settings.size, 17.0);
    assert_eq!(s.brush_notice.as_ref().map(|n| (n.name.as_str(), n.tip.as_str())), Some(("Scratches", alpha.name.as_str())));
    // Missing tip: applied with Round, notice names the preset, status explains the fallback.
    preset.tip = Some("No such tip".into());
    s.select_shelf_brush(ShelfBrush::Preset(&preset), 2.0);
    let round = s.library.alphas.iter().find(|a| a.bitmap.is_none()).unwrap().name.clone();
    assert_eq!(s.brush_notice.as_ref().unwrap().tip, round);
    assert!(s.status.as_ref().unwrap().0.contains("not found"));
    // Broken embedded image: brush unchanged, no acknowledgement of a brush that is not active.
    let before = (s.session.brush.settings, s.brush_notice.clone());
    preset.material.textures.insert(subcraft_core::ChannelKind::BaseColor, subcraft_core::ResourceId::new());
    preset.settings.size = 99.0;
    s.select_shelf_brush(ShelfBrush::Preset(&preset), 5.0);
    assert_eq!((s.session.brush.settings, s.brush_notice.clone()), before);
    assert!(s.status.as_ref().unwrap().0.contains("brush unchanged"));
}

#[test]
fn notice_holds_then_fades_and_a_new_selection_replaces_it() {
    let notice = BrushNotice { name: "A".into(), tip: "A".into(), since: 10.0 };
    assert_eq!(notice.opacity(10.0), Some(1.0));
    assert_eq!(notice.opacity(10.0 + BrushNotice::HOLD), Some(1.0));
    let mid = notice.opacity(10.0 + BrushNotice::HOLD + BrushNotice::FADE * 0.5).unwrap();
    assert!((mid - 0.5).abs() < 1e-3);
    assert_eq!(notice.opacity(10.0 + BrushNotice::HOLD + BrushNotice::FADE), None);
    let mut s = state();
    let alphas: Vec<_> = s.library.alphas.iter().filter(|a| !a.name.starts_with("Starter: ")).take(2).cloned().collect();
    s.select_shelf_brush(ShelfBrush::Tip(&alphas[0]), 1.0);
    s.select_shelf_brush(ShelfBrush::Tip(&alphas[1]), 1.2);
    assert_eq!(s.brush_notice.as_ref().unwrap().name, alphas[1].name);
    assert_eq!(s.brush_notice.as_ref().unwrap().since, 1.2);
}

fn frame(state: &mut UiState, ctx: &egui::Context, time: f64, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1200.0, 1000.0))), time: Some(time), events, ..Default::default() },
        |ui| shelf::show(state, ui),
    );
    out.textures_delta.clear();
    out
}

fn click(state: &mut UiState, ctx: &egui::Context, time: f64, pos: Pos2) {
    let button = |pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(state, ctx, time, vec![Event::PointerMoved(pos), button(true)]);
    frame(state, ctx, time + 0.02, vec![button(false)]);
}

#[test]
fn real_click_selects_and_double_click_opens_the_editor_without_a_second_notice() {
    let mut s = state();
    s.shelf.tab = ShelfTab::Brushes;
    s.tool = Tool::PolygonFill;
    let target = s.session.brush.target;
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    let mut out = frame(&mut s, &ctx, 0.0, vec![]);
    for k in 1..4 { out = frame(&mut s, &ctx, k as f64 * 0.01, vec![]); }
    let alpha = s.library.alphas.iter().filter(|a| !a.name.starts_with("Starter: ")).nth(1).unwrap().name.clone();
    let label = out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Text(t) if t.galley.text() == alpha => Some(Rect::from_min_size(t.pos, t.galley.size())),
        _ => None,
    }).unwrap_or_else(|| panic!("tile {alpha} not drawn"));
    let tile = label.center() - egui::vec2(0.0, 30.0);
    click(&mut s, &ctx, 1.0, tile);
    assert_eq!(s.session.brush.alpha_name, alpha);
    assert_eq!(s.tool, Tool::Brush);
    assert_eq!(s.session.brush.target, target);
    assert!(!s.open_brush_editor && !s.asset_preview.open, "a single click paints; no preview popup or editor");
    let since = s.brush_notice.as_ref().unwrap().since;
    click(&mut s, &ctx, 1.1, tile);
    assert!(s.open_brush_editor, "double-click opens Brush settings");
    assert_eq!(s.brush_notice.as_ref().unwrap().since, since, "no duplicate acknowledgement");
}
