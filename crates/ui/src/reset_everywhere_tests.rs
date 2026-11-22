//! Ctrl+click (and the change dot) on the settings rows outside Display Settings: the brush
//! panel, the filter editor and the shared row helpers, driven with real egui input (#609).

use crate::state::UiState;
use subcraft_app::Session;
use subcraft_doc::Filter;

const CTRL: egui::Modifiers = egui::Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

/// Runs `show` on a tall window and sends it input.
struct Harness<S> {
    ctx: egui::Context,
    s: S,
    show: fn(&mut S, &mut egui::Ui),
}

impl<S> Harness<S> {
    fn new(s: S, show: fn(&mut S, &mut egui::Ui)) -> Self {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut h = Harness { ctx, s, show };
        h.frame(vec![], egui::Modifiers::NONE);
        h.frame(vec![], egui::Modifiers::NONE);
        h
    }
    fn frame(&mut self, mut events: Vec<egui::Event>, modifiers: egui::Modifiers) -> egui::FullOutput {
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 8000.0))), events, ..Default::default() };
        let (s, show) = (&mut self.s, self.show);
        let mut out = self.ctx.run_ui(input, |ui| { egui::CentralPanel::default().show(ui, |ui| show(s, ui)); });
        out.textures_delta.clear();
        out
    }
    /// The rect of the first text `text` on screen.
    fn text_rect(&mut self, text: &str) -> egui::Rect {
        let out = self.frame(vec![], egui::Modifiers::NONE);
        out.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text == text => Some(t.visual_bounding_rect()),
            _ => None,
        }).unwrap_or_else(|| panic!("{text:?} is drawn"))
    }
    /// Ctrl goes down a frame before the click, as when a hand goes to the key.
    fn click(&mut self, at: egui::Pos2, modifiers: egui::Modifiers) {
        self.frame(vec![egui::Event::PointerMoved(at)], modifiers);
        self.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers }], modifiers);
        self.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers }], modifiers);
        self.frame(vec![], egui::Modifiers::NONE);
    }
    /// Whether the accent change dot is drawn just left of the label `text`.
    fn dot_before(&mut self, text: &str) -> bool {
        let label = self.text_rect(text);
        let out = self.frame(vec![], egui::Modifiers::NONE);
        out.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Circle(c)
            if c.fill == crate::theme::ACCENT() && (c.center.y - label.center().y).abs() < 4.0 && c.center.x < label.left() && c.center.x > label.left() - 20.0))
    }
    /// A point on the slider rail under the row label `label`, left of the middle.
    fn rail_under(&mut self, label: &str) -> egui::Pos2 {
        let l = self.text_rect(label);
        egui::pos2(l.left() + 30.0, l.bottom() + 12.0)
    }
}

fn state() -> UiState {
    UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None)
}

fn brush_panel() -> Harness<UiState> {
    Harness::new(state(), crate::panels::brush::show)
}

#[test]
fn ctrl_click_on_a_brush_slider_puts_the_default_back_and_a_plain_click_still_moves_it() {
    let d = subcraft_paint::BrushSettings::default();
    let mut h = brush_panel();
    h.s.session.brush.settings.spacing = 1.5;
    assert!(h.dot_before("Spacing"), "a changed setting shows the dot");
    let rail = h.rail_under("Spacing");
    h.click(rail, CTRL);
    assert_eq!(h.s.session.brush.settings.spacing, d.spacing, "Ctrl+click on the rail resets instead of moving the slider");
    assert!(!h.dot_before("Spacing"), "no dot at the default");
    h.click(rail, CTRL);
    assert_eq!(h.s.session.brush.settings.spacing, d.spacing, "Ctrl+click at the default leaves it");
    h.click(rail, egui::Modifiers::NONE);
    assert_ne!(h.s.session.brush.settings.spacing, d.spacing, "a plain click still sets the slider");
}

#[test]
fn ctrl_click_resets_brush_checkboxes_and_dropdowns_without_toggling_or_opening_them() {
    let d = subcraft_paint::BrushSettings::default();
    let mut h = brush_panel();
    h.s.session.brush.settings.follow_path = !d.follow_path;
    let check = h.text_rect("Follow Path").center();
    h.click(check, CTRL);
    assert_eq!(h.s.session.brush.settings.follow_path, d.follow_path, "Ctrl+click resets the checkbox");
    h.click(check, CTRL);
    assert_eq!(h.s.session.brush.settings.follow_path, d.follow_path, "a second Ctrl+click does not toggle it");

    h.s.session.brush.settings.alignment = subcraft_paint::Alignment::Uv;
    assert!(h.dot_before("Alignment"));
    let combo = h.text_rect("UV").center();
    h.click(combo, CTRL);
    assert_eq!(h.s.session.brush.settings.alignment, d.alignment, "Ctrl+click on the dropdown resets it");
    assert!(!egui::Popup::is_any_open(&h.ctx), "and does not open its list");
    assert!(!h.dot_before("Alignment"));

    // A label alone takes the gesture too (the Scatter and Smudge sections are closed by
    // default; Roundness is in the open Alpha section).
    h.s.session.brush.settings.roundness = 0.3;
    let label = h.text_rect("Roundness").center();
    h.click(label, CTRL);
    assert_eq!(h.s.session.brush.settings.roundness, d.roundness);
}

#[test]
fn ctrl_click_on_a_filter_slider_resets_it_and_keeps_the_filter_variant() {
    fn show(f: &mut (Filter, bool), ui: &mut egui::Ui) {
        f.1 |= crate::panels::filter::editor(ui, &mut f.0, None);
    }
    let mut h = Harness::new((Filter::BlurWrap { radius: 20.0 }, false), show);
    assert!(h.dot_before("Radius"));
    let rail = h.rail_under("Radius");
    h.click(rail, CTRL);
    assert_eq!(h.s.0, Filter::BlurWrap { radius: 4.0 }, "the Blur default radius, still wrapping");
    assert!(h.s.1, "the editor reports the change, so the command and undo step are made");
}

#[test]
fn a_reset_is_a_finished_edit_for_panels_that_commit_on_done() {
    struct S {
        value: f32,
        done: bool,
    }
    fn show(s: &mut S, ui: &mut egui::Ui) {
        let r = crate::widgets::slider_row_reset(ui, "Amount", &mut s.value, 0.25, |v| egui::Slider::new(v, 0.0..=1.0));
        s.done |= crate::widgets::Edit::from(&r).done;
    }
    let mut h = Harness::new(S { value: 0.9, done: false }, show);
    let label = h.text_rect("Amount").center();
    h.click(label, CTRL);
    assert_eq!(h.s.value, 0.25);
    assert!(h.s.done, "the text dialog and other Edit users commit the reset at once");
}

#[test]
fn ctrl_reset_in_toolbar_popups_resets_without_a_dot_or_menu() {
    fn show(v: &mut f32, ui: &mut egui::Ui) {
        let r = ui.add(egui::Slider::new(v, 0.0..=100.0).text("lazy"));
        crate::widgets::ctrl_reset(ui, r, v, 0.0);
    }
    let mut h = Harness::new(60.0_f32, show);
    let text = h.text_rect("lazy");
    // On the rail, left of the slider's value and text.
    let at = egui::pos2(text.left() - 120.0, text.center().y);
    h.click(at, CTRL);
    assert_eq!(h.s, 0.0);
    h.click(at, egui::Modifiers::NONE);
    assert!(h.s > 0.0, "a plain click still moves the slider, got {}", h.s);
}
