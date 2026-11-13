//! Text entry points and the Alt readings of shelf clicks / drops (headless).

use subcraft_app::{Command, NewLayer, Session};
use subcraft_doc::LayerContent;
use subcraft_ui::panels::shelf::{ShelfDrag, apply_shelf_item_with, drop_label};
use subcraft_ui::state::UiState;
use subcraft_ui::text_panel::Window;

fn state() -> UiState {
    UiState::new(Session::new(subcraft_mesh::primitives::sample_crate(), 128), None)
}

fn frame(state: &mut UiState, ctx: &egui::Context) {
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() }, |ui| {
        let now = ui.input(|i| i.time);
        subcraft_ui::text_panel::window(state, ui.ctx(), now);
    });
    out.textures_delta.clear();
}

#[test]
fn new_text_window_opens_empty_and_adds_nothing_until_confirmed() {
    let mut s = state();
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    s.text.open_new();
    assert_eq!(s.text.window, Some(Window::New));
    for _ in 0..3 { frame(&mut s, &ctx); }
    assert!(s.session.selected_text().is_none(), "nothing is added until the user confirms");
    assert_eq!(s.text.window, Some(Window::New), "window stays open");
}

#[test]
fn new_text_window_keeps_a_fixed_width_on_a_wide_screen() {
    let mut s = state();
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    s.text.open_new();
    for _ in 0..4 {
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2400.0, 1200.0))), ..Default::default() }, |ui| {
            let now = ui.input(|i| i.time);
            subcraft_ui::text_panel::window(&mut s, ui.ctx(), now);
        });
        out.textures_delta.clear();
    }
    let rect = ctx.memory(|m| m.area_rect(egui::Id::new("text_window"))).expect("window laid out");
    assert!(rect.width() <= subcraft_ui::text_panel::WINDOW_WIDTH + 20.0, "window width {} must not stretch", rect.width());
    assert!(rect.width() >= 300.0, "window width {} too narrow", rect.width());
}

#[test]
fn edit_window_closes_when_no_text_layer_is_selected() {
    let mut s = state();
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    s.text.open_edit();
    frame(&mut s, &ctx);
    assert_eq!(s.text.window, None);
    s.session.perform(subcraft_app::actions::Action::NewTextLayer).unwrap();
    s.text.open_edit();
    for _ in 0..2 { frame(&mut s, &ctx); }
    assert_eq!(s.text.window, Some(Window::Edit));
}

#[test]
fn alt_click_material_replaces_the_selected_fill_layer_without_creating_a_layer() {
    let mut s = state();
    s.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.session.selection().layer.unwrap();
    let count = s.session.selected_texture_set().layers.len();
    apply_shelf_item_with(&mut s, ShelfDrag::Material(0), true, 0.0);
    assert_eq!(s.session.selected_texture_set().layers.len(), count);
    assert_eq!(s.session.selection().layer, Some(layer));
    apply_shelf_item_with(&mut s, ShelfDrag::Material(0), false, 1.0);
    assert_eq!(s.session.selected_texture_set().layers.len(), count + 1, "plain click still adds a layer");
}

#[test]
fn alt_on_unsupported_targets_says_why_instead_of_doing_nothing() {
    let mut s = state();
    s.session.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let doc = s.session.document().clone();
    apply_shelf_item_with(&mut s, ShelfDrag::Material(0), true, 0.0);
    assert_eq!(s.session.document(), &doc);
    assert!(toast(&s).contains("fill layer"), "toast explains: {}", toast(&s));
    apply_shelf_item_with(&mut s, ShelfDrag::SmartMaterial(0), true, 0.0);
    assert!(toast(&s).contains("folder"), "{}", toast(&s));
}

#[test]
fn alpha_alt_goes_to_the_mask_and_plain_sets_the_brush_tip() {
    let mut s = state();
    let i = s.library.alphas.iter().position(|a| a.bitmap.is_some()).expect("an image alpha");
    s.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    apply_shelf_item_with(&mut s, ShelfDrag::Alpha(i), false, 0.0);
    assert_eq!(s.session.brush.alpha_name, s.library.alphas[i].name);
    assert!(s.session.selected_layer().unwrap().mask.is_none());
    apply_shelf_item_with(&mut s, ShelfDrag::Alpha(i), true, 1.0);
    let l = s.session.selected_layer().unwrap();
    assert!(l.mask.is_some());
    assert!(matches!(l.content, LayerContent::Fill(_)));
}

#[test]
fn drop_labels_say_what_will_happen() {
    assert_eq!(drop_label(ShelfDrag::Material(0), false, Some("Rust"), false), "Add material layer above \"Rust\" (Ctrl: with a black mask)");
    assert_eq!(drop_label(ShelfDrag::Material(0), true, Some("Rust"), false), "Replace the material of \"Rust\"");
    assert_eq!(drop_label(ShelfDrag::SmartMask(0), false, Some("Rust"), false), "Apply mask to \"Rust\"");
    assert_eq!(drop_label(ShelfDrag::Alpha(0), false, Some("Rust"), true), "Use this alpha in the mask of \"Rust\"");
    assert_eq!(drop_label(ShelfDrag::Alpha(0), false, None, false), "Set as brush tip");
}

fn toast(s: &UiState) -> String {
    s.status.as_ref().map(|(m, _)| m.clone()).unwrap_or_default()
}
