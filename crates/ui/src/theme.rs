//! Dark theme with a warm accent.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(232, 132, 52);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(120, 70, 30);

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    let bg = Color32::from_rgb(30, 31, 34);
    let panel = Color32::from_rgb(37, 38, 42);
    let raised = Color32::from_rgb(48, 50, 55);
    v.panel_fill = panel;
    v.window_fill = panel;
    v.extreme_bg_color = Color32::from_rgb(22, 23, 25);
    v.faint_bg_color = Color32::from_rgb(42, 43, 47);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.widgets.noninteractive.bg_fill = bg;
    v.widgets.inactive.bg_fill = raised;
    v.widgets.inactive.weak_bg_fill = raised;
    v.widgets.hovered.bg_fill = Color32::from_rgb(62, 64, 70);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(62, 64, 70);
    v.widgets.active.bg_fill = ACCENT_DIM;
    v.widgets.active.weak_bg_fill = ACCENT_DIM;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(110, 112, 120));
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(3);
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
    });
}
