//! Dark theme: layered charcoal surfaces, softened controls, generous spacing, one warm accent.
//! Inter for text, Phosphor for icons (both embedded, see `assets/fonts`).

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};
use egui_dock::Style as DockStyle;

// Surfaces, darkest to lightest.
pub const BG_0: Color32 = Color32::from_rgb(13, 14, 16); // gaps between panels, status bar
pub const BG_1: Color32 = Color32::from_rgb(22, 23, 26); // panels
pub const BG_2: Color32 = Color32::from_rgb(29, 31, 35); // header bar, tab bar, cards
pub const BG_3: Color32 = Color32::from_rgb(38, 40, 46); // controls
pub const BG_4: Color32 = Color32::from_rgb(48, 51, 58); // hovered controls
/// Quiet alternating bands, below the contrast of hover and selection.
pub const ROW_BANDS: [Color32; 2] = [BG_1, Color32::from_rgb(27, 29, 33)];
pub const SECTION_BANDS: [Color32; 2] = [Color32::from_rgb(27, 29, 33), Color32::from_rgb(31, 33, 38)];
pub const FIELD: Color32 = Color32::from_rgb(16, 17, 20); // text fields, slider rails

pub const BORDER: Color32 = Color32::from_rgb(40, 43, 49);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(62, 66, 75);

pub const TEXT: Color32 = Color32::from_rgb(202, 207, 214);
pub const TEXT_DIM: Color32 = Color32::from_rgb(152, 158, 168);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(102, 108, 118);

pub const ACCENT: Color32 = Color32::from_rgb(255, 128, 56);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(255, 150, 90);
/// Accent at low alpha over panels (selected rows, active toggles).
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(64, 40, 28);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(120, 64, 32);

pub const WARNING: Color32 = Color32::from_rgb(240, 196, 110);

/// Panel content padding.
pub const PAD: i8 = 12;
/// Height of rows in lists (layers, history, texture sets).
pub const ROW_HEIGHT: f32 = 34.0;
/// Width of the label column in property rows.
pub const LABEL_WIDTH: f32 = 84.0;

/// Font family for headings / emphasis.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn font_strong(size: f32) -> FontId {
    FontId::new(size, semibold())
}

fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut f, "inter", include_bytes!("../assets/fonts/Inter-Regular.ttf"));
    add(&mut f, "inter-semibold", include_bytes!("../assets/fonts/Inter-SemiBold.ttf"));
    let mut icons = FontData::from_static(include_bytes!("../assets/fonts/Phosphor.ttf"));
    icons.tweak.y_offset_factor = 0.08;
    icons.tweak.scale = 1.1;
    f.font_data.insert("phosphor".into(), Arc::new(icons));

    let fallback: Vec<String> = f.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = vec!["inter".to_string(), "phosphor".to_string()];
    prop.extend(fallback.iter().cloned());
    f.families.insert(FontFamily::Proportional, prop);
    let mut strong = vec!["inter-semibold".to_string(), "phosphor".to_string()];
    strong.extend(fallback);
    f.families.insert(semibold(), strong);
    if let Some(mono) = f.families.get_mut(&FontFamily::Monospace) {
        mono.push("phosphor".into());
    }
    f
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());

    let mut v = Visuals::dark();
    v.panel_fill = BG_1;
    v.window_fill = BG_2;
    v.window_stroke = Stroke::new(1.0, BORDER_STRONG);
    v.window_corner_radius = CornerRadius::ZERO;
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(90) };
    v.menu_corner_radius = CornerRadius::ZERO;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = BG_2;
    v.code_bg_color = FIELD;
    v.override_text_color = None;
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = WARNING;
    v.slider_trailing_fill = true;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BG_1;
    w.noninteractive.weak_bg_fill = BG_1;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);

    w.inactive.bg_fill = BG_3;
    w.inactive.weak_bg_fill = BG_3;
    w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(52, 55, 62));
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);

    w.hovered.bg_fill = BG_4;
    w.hovered.weak_bg_fill = BG_4;
    w.hovered.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.5, TEXT);
    w.hovered.expansion = 0.0;

    w.active.bg_fill = ACCENT_DIM;
    w.active.weak_bg_fill = ACCENT_DIM;
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.active.fg_stroke = Stroke::new(1.5, TEXT);
    w.active.expansion = 0.0;

    w.open.bg_fill = BG_3;
    w.open.weak_bg_fill = BG_3;
    w.open.bg_stroke = Stroke::new(1.0, BORDER_STRONG);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);

    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = CornerRadius::same(2);
    }
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, font(11.0)),
            (TextStyle::Body, font(13.0)),
            (TextStyle::Button, font(13.0)),
            (TextStyle::Heading, font_strong(17.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        let sp = &mut s.spacing;
        sp.item_spacing = egui::vec2(8.0, 7.0);
        sp.button_padding = egui::vec2(10.0, 5.0);
        sp.interact_size = egui::vec2(36.0, 26.0);
        sp.window_margin = Margin::same(16);
        sp.menu_margin = Margin::same(6);
        sp.indent = 16.0;
        sp.slider_width = 150.0;
        sp.slider_rail_height = 4.0;
        sp.combo_width = 150.0;
        sp.icon_width = 16.0;
        sp.icon_width_inner = 9.0;
        sp.icon_spacing = 8.0;
        sp.menu_spacing = 4.0;
        sp.tooltip_width = 320.0;
        sp.scroll.bar_width = 6.0;
        sp.scroll.floating = true;
        s.interaction.selectable_labels = false;
    });
}

/// Dock styling: dark tab strip, the active tab merges with its panel, thin gaps between panels.
pub fn dock_style(ui_style: &egui::Style) -> DockStyle {
    let mut d = DockStyle::from_egui(ui_style);
    d.dock_area_padding = None;
    d.main_surface_border_stroke = Stroke::NONE;
    d.main_surface_border_rounding = CornerRadius::ZERO;

    d.separator.width = 3.0;
    d.separator.extra_interact_width = 4.0;
    d.separator.color_idle = BG_0;
    d.separator.color_hovered = ACCENT_DIM;
    d.separator.color_dragged = ACCENT;

    d.tab_bar.bg_fill = BG_2;
    d.tab_bar.height = 32.0;
    d.tab_bar.corner_radius = CornerRadius::ZERO;
    d.tab_bar.hline_color = BORDER;
    d.tab_bar.inner_margin = Margin::ZERO;

    let tab = |bg: Color32, text: Color32| egui_dock::TabInteractionStyle { outline_color: Color32::TRANSPARENT, corner_radius: CornerRadius::ZERO, bg_fill: bg, text_color: text };
    d.tab.active = tab(BG_1, TEXT);
    d.tab.focused = tab(BG_1, TEXT);
    d.tab.active_with_kb_focus = tab(BG_1, TEXT);
    d.tab.focused_with_kb_focus = tab(BG_1, TEXT);
    d.tab.inactive = tab(BG_2, TEXT_FAINT);
    d.tab.inactive_with_kb_focus = tab(BG_2, TEXT_FAINT);
    d.tab.hovered = tab(BG_3, TEXT_DIM);
    d.tab.hline_below_active_tab_name = false;
    d.tab.minimum_width = Some(84.0);
    d.tab.spacing = 0.0;
    d.tab.tab_body.inner_margin = Margin::ZERO;
    d.tab.tab_body.stroke = Stroke::NONE;
    d.tab.tab_body.corner_radius = CornerRadius::ZERO;
    d.tab.tab_body.bg_fill = BG_1;

    d.overlay.selection_color = ACCENT.linear_multiply(0.35);
    d.overlay.button_color = BG_3;
    d.overlay.button_border_stroke = Stroke::new(1.0, BORDER_STRONG);
    d.buttons.collapse_tabs_color = TEXT_FAINT;
    d.buttons.collapse_tabs_active_color = TEXT;
    d
}

/// Window / taskbar icon: the app artwork (same as the .exe, installer and Linux icon).
pub fn app_icon() -> egui::IconData {
    let img = image::load_from_memory_with_format(include_bytes!("../assets/icon.png"), image::ImageFormat::Png)
        .expect("embedded icon is a valid PNG")
        .into_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData { rgba: img.into_raw(), width, height }
}
