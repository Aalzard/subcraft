//! Colour themes: Dark (layered charcoal surfaces, one warm accent), Light, and a neutral
//! mid-grey Painter-style theme. Inter for text, Phosphor for icons (both embedded, see
//! `assets/fonts`).
//!
//! Every colour the interface draws its chrome with is a field of [`Palette`]. The functions
//! named like constants (`TEXT()`, `BG_1()`, ...) read the palette of the theme in use, so a
//! theme picked in Settings > General applies on the next frame, without a restart.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};
use egui_dock::Style as DockStyle;
use serde::{Deserialize, Serialize};

/// The interface themes (Settings > General > Theme).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeKind {
    #[default]
    Dark,
    Light,
    /// Neutral mid-dark greys and flatter panels.
    Painter,
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 3] = [ThemeKind::Dark, ThemeKind::Light, ThemeKind::Painter];

    pub fn palette(self) -> &'static Palette {
        match self {
            ThemeKind::Dark => &DARK,
            ThemeKind::Light => &LIGHT,
            ThemeKind::Painter => &PAINTER,
        }
    }
}

/// Every colour of the interface chrome, for one theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Whether egui's dark defaults fit (text selection, stock widgets).
    pub dark: bool,
    /// Gaps between panels, status bar.
    pub bg_0: Color32,
    /// Panels.
    pub bg_1: Color32,
    /// Header bar, tab bar, cards.
    pub bg_2: Color32,
    /// Controls.
    pub bg_3: Color32,
    /// Hovered controls.
    pub bg_4: Color32,
    /// Quiet alternating bands, below the contrast of hover and selection.
    pub row_bands: [Color32; 2],
    /// Hairline between list rows (the layer stack).
    pub row_separator: Color32,
    pub section_bands: [Color32; 2],
    /// Text fields, slider rails.
    pub field: Color32,
    /// Stacked blocks in Properties (one channel, one mask effect): set off from the panel.
    pub card: Color32,
    pub card_border: Color32,
    /// Slider rail / checkbox box: an inset track.
    pub rail: Color32,
    /// Soft separators and control outlines.
    pub border_soft: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    /// Accent mixed into the panel (selected rows, active toggles).
    pub accent_soft: Color32,
    pub accent_dim: Color32,
    /// Text and marks drawn on an accent fill (selected labels, primary buttons).
    pub on_accent: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub success: Color32,
    /// Layer tree: folder icons and badges, folder row band, hierarchy guide lines.
    pub folder: Color32,
    pub folder_band: Color32,
    pub tree_line: Color32,
    /// Opacity of window and popup shadows.
    pub shadow_alpha: [u8; 2],
    /// Corner radius of controls.
    pub radius: u8,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

// Surfaces, darkest to lightest, on Substance Painter's levels: panel bodies a little lighter than
// the title / tab strips, darker fields and gaps. The greys keep a faint warm tint (red leads
// blue by about 3 levels) so the charcoal sits with the orange accent.
pub const DARK: Palette = Palette {
    dark: true,
    bg_0: rgb(34, 33, 31),
    bg_1: rgb(48, 47, 45),
    bg_2: rgb(44, 43, 41),
    bg_3: rgb(59, 58, 56),
    bg_4: rgb(71, 70, 67),
    row_bands: [rgb(48, 47, 45), rgb(58, 57, 55)],
    row_separator: rgb(38, 37, 35),
    section_bands: [rgb(44, 43, 41), rgb(47, 46, 44)],
    field: rgb(37, 36, 35),
    card: rgb(61, 59, 57),
    card_border: rgb(88, 85, 82),
    rail: rgb(34, 34, 34),
    border_soft: rgb(56, 55, 53),
    border: rgb(39, 38, 36),
    border_strong: rgb(80, 77, 74),
    text: rgb(204, 203, 199),
    text_dim: rgb(172, 171, 167),
    text_faint: rgb(128, 127, 124),
    accent: rgb(218, 145, 56),
    accent_hover: rgb(232, 164, 79),
    accent_soft: rgb(70, 62, 52),
    accent_dim: rgb(113, 81, 40),
    on_accent: rgb(34, 33, 31),
    warning: rgb(240, 196, 110),
    error: rgb(235, 110, 100),
    success: rgb(124, 196, 124),
    folder: rgb(226, 184, 98),
    folder_band: rgb(46, 44, 41),
    tree_line: rgb(104, 90, 64),
    shadow_alpha: [110, 90],
    radius: 4,
};

// Light: warm off-white panels, darker header strips and gaps, white fields; a deeper orange so
// accent text stays readable on the light panels.
pub const LIGHT: Palette = Palette {
    dark: false,
    bg_0: rgb(205, 204, 201),
    bg_1: rgb(236, 235, 232),
    bg_2: rgb(226, 225, 222),
    bg_3: rgb(214, 213, 210),
    bg_4: rgb(200, 199, 196),
    row_bands: [rgb(236, 235, 232), rgb(228, 227, 224)],
    row_separator: rgb(214, 213, 210),
    section_bands: [rgb(226, 225, 222), rgb(231, 230, 227)],
    field: rgb(250, 250, 248),
    card: rgb(245, 244, 241),
    card_border: rgb(190, 188, 184),
    rail: rgb(196, 195, 192),
    border_soft: rgb(208, 207, 204),
    border: rgb(190, 189, 186),
    border_strong: rgb(158, 156, 152),
    text: rgb(32, 32, 30),
    text_dim: rgb(78, 77, 74),
    text_faint: rgb(118, 117, 114),
    accent: rgb(190, 104, 20),
    accent_hover: rgb(212, 124, 36),
    accent_soft: rgb(246, 222, 192),
    accent_dim: rgb(236, 184, 124),
    on_accent: rgb(24, 14, 8),
    warning: rgb(160, 100, 0),
    error: rgb(190, 40, 30),
    success: rgb(40, 128, 50),
    folder: rgb(180, 122, 20),
    folder_band: rgb(230, 226, 218),
    tree_line: rgb(190, 160, 110),
    shadow_alpha: [45, 35],
    radius: 4,
};

// Painter-style: neutral mid-dark greys without the warm tint, little contrast between strips
// and panels, square-ish controls.
pub const PAINTER: Palette = Palette {
    dark: true,
    bg_0: rgb(40, 40, 40),
    bg_1: rgb(56, 56, 56),
    bg_2: rgb(51, 51, 51),
    bg_3: rgb(68, 68, 68),
    bg_4: rgb(80, 80, 80),
    row_bands: [rgb(56, 56, 56), rgb(61, 61, 61)],
    row_separator: rgb(46, 46, 46),
    section_bands: [rgb(51, 51, 51), rgb(54, 54, 54)],
    field: rgb(42, 42, 42),
    card: rgb(62, 62, 62),
    card_border: rgb(80, 80, 80),
    rail: rgb(38, 38, 38),
    border_soft: rgb(64, 64, 64),
    border: rgb(45, 45, 45),
    border_strong: rgb(88, 88, 88),
    text: rgb(214, 214, 214),
    text_dim: rgb(178, 178, 178),
    text_faint: rgb(134, 134, 134),
    accent: rgb(222, 150, 60),
    accent_hover: rgb(236, 170, 84),
    accent_soft: rgb(82, 72, 60),
    accent_dim: rgb(120, 88, 48),
    on_accent: rgb(30, 30, 30),
    warning: rgb(240, 196, 110),
    error: rgb(235, 110, 100),
    success: rgb(124, 196, 124),
    folder: rgb(220, 180, 100),
    folder_band: rgb(52, 52, 52),
    tree_line: rgb(100, 92, 78),
    shadow_alpha: [80, 60],
    radius: 2,
};

/// Text and marks drawn over the 3D and 2D views, usually on a dark translucent plate. The
/// views do not follow the theme, so these do not either: with the Light theme's dark text they
/// were unreadable over the dark render (the Dark theme's values, so Dark looks as before).
pub const OVERLAY_TEXT: Color32 = DARK.text;
pub const OVERLAY_TEXT_DIM: Color32 = DARK.text_dim;
pub const OVERLAY_TEXT_FAINT: Color32 = DARK.text_faint;
/// Dark ring around a light mark (pen handles).
pub const OVERLAY_OUTLINE: Color32 = DARK.bg_0;

/// The theme in use (a `ThemeKind` as a number). The interface runs on one thread; an atomic
/// keeps the read free.
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The theme in use.
pub fn current() -> ThemeKind {
    ThemeKind::ALL.get(CURRENT.load(Ordering::Relaxed) as usize).copied().unwrap_or_default()
}

/// The palette of the theme in use.
pub fn palette() -> &'static Palette {
    current().palette()
}

macro_rules! colours {
    ($($name:ident => $field:ident),* $(,)?) => {
        $(
            #[allow(non_snake_case)]
            #[inline]
            pub fn $name() -> Color32 {
                palette().$field
            }
        )*
    };
}

colours! {
    BG_0 => bg_0, BG_1 => bg_1, BG_2 => bg_2, BG_3 => bg_3, BG_4 => bg_4,
    ROW_SEPARATOR => row_separator, FIELD => field, CARD => card, CARD_BORDER => card_border,
    RAIL => rail, BORDER_SOFT => border_soft, BORDER => border, BORDER_STRONG => border_strong,
    TEXT => text, TEXT_DIM => text_dim, TEXT_FAINT => text_faint,
    ACCENT => accent, ACCENT_HOVER => accent_hover, ACCENT_SOFT => accent_soft, ACCENT_DIM => accent_dim,
    ON_ACCENT => on_accent, WARNING => warning, ERROR => error, SUCCESS => success,
    FOLDER => folder, FOLDER_BAND => folder_band, TREE_LINE => tree_line,
}

#[allow(non_snake_case)]
pub fn ROW_BANDS() -> [Color32; 2] {
    palette().row_bands
}

#[allow(non_snake_case)]
pub fn SECTION_BANDS() -> [Color32; 2] {
    palette().section_bands
}

/// A colour chosen for dark panels (pastel labels, channel and axis colours), darkened when the
/// panels are light so it stays readable there. Unchanged on dark themes.
pub fn on_panel(c: Color32) -> Color32 {
    on_panel_in(&palette(), c)
}

/// [`on_panel`] for palette `p`.
pub fn on_panel_in(p: &Palette, c: Color32) -> Color32 {
    if p.dark {
        return c;
    }
    let f = |v: u8| (v as f32 * 0.55).round() as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// Colour of the left accent bar of a channel card (muted, so the set reads as a family).
pub fn channel_accent(kind: subcraft_core::ChannelKind) -> Color32 {
    on_panel(channel_accent_base(kind))
}

fn channel_accent_base(kind: subcraft_core::ChannelKind) -> Color32 {
    use subcraft_core::ChannelKind::*;
    match kind {
        BaseColor => Color32::from_rgb(214, 150, 70),
        Height => Color32::from_rgb(140, 150, 168),
        Roughness => Color32::from_rgb(120, 168, 196),
        Metallic => Color32::from_rgb(176, 180, 196),
        Normal => Color32::from_rgb(132, 120, 208),
        AmbientOcclusion => Color32::from_rgb(120, 168, 130),
        Emissive => Color32::from_rgb(236, 206, 90),
        _ => Color32::from_rgb(150, 150, 146),
    }
}

/// Panel content padding.
pub const PAD: i8 = 12;
/// Height of rows in lists (layers, history, texture sets).
pub const ROW_HEIGHT: f32 = 34.0;
/// Material/mask previews need more room than a nested adjustment.
pub const LAYER_ROW_HEIGHT: f32 = 48.0;
pub const LAYER_THUMB_SIZE: f32 = 36.0;
pub const EFFECT_ROW_HEIGHT: f32 = 26.0;
/// Width of the label column in property rows (the maximum; narrow panels shrink it).
pub const LABEL_WIDTH: f32 = 92.0;

// Spacing scale. Rows are one step apart, groups two, sections three.
pub const SP_XS: f32 = 4.0;
pub const SP_S: f32 = 8.0;
pub const SP_M: f32 = 12.0;
pub const SP_L: f32 = 16.0;
/// Height of every editable control in a property row (slider, combo, field, button).
pub const CONTROL_H: f32 = 24.0;
/// Width of the quiet numeric field to the right of a slider.
pub const VALUE_W: f32 = 54.0;
/// Corner radius of controls, chips and cards in the Dark theme (see [`radius`]).
pub const RADIUS: u8 = 4;

/// Corner radius of controls, chips and cards in the theme in use (flatter in Painter-style).
pub fn radius() -> u8 {
    palette().radius
}
/// Small dimmed row label.
pub const LABEL_SIZE: f32 = 12.0;

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

pub(crate) fn fonts() -> FontDefinitions {
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

/// Fonts, spacing and the visuals of the theme in use.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    // egui follows the system theme by default, and visuals set for one of its themes only: with
    // a light system theme the menu bar and every stock widget took egui's light colours on the
    // app's dark panels (issue #206). Both of egui's slots get SubCraft's visuals.
    ctx.set_theme(egui::ThemePreference::Dark);
    apply_visuals(ctx, current());

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
        sp.item_spacing = egui::vec2(SP_S, SP_S);
        sp.button_padding = egui::vec2(10.0, 5.0);
        // Sliders derive their circular handle radius from this height; padding and explicit
        // button sizes retain the larger hit targets used elsewhere.
        sp.interact_size = egui::vec2(36.0, 18.0);
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

/// Switch to `kind`: the next frame draws with it (Settings > General > Theme).
pub fn set(ctx: &egui::Context, kind: ThemeKind) {
    CURRENT.store(ThemeKind::ALL.iter().position(|&k| k == kind).unwrap_or(0) as u8, Ordering::Relaxed);
    apply_visuals(ctx, kind);
    ctx.request_repaint();
}

/// [`set`] when `kind` is not the theme in use (called every frame with the saved setting).
pub fn follow(ctx: &egui::Context, kind: ThemeKind) {
    if current() != kind {
        set(ctx, kind);
    }
}

/// Give both of egui's theme slots the visuals of `kind`.
pub fn apply_visuals(ctx: &egui::Context, kind: ThemeKind) {
    let v = visuals(kind.palette());
    ctx.set_visuals_of(egui::Theme::Light, v.clone());
    ctx.set_visuals_of(egui::Theme::Dark, v);
}

/// egui visuals drawn with palette `p`.
pub fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    v.dark_mode = p.dark;
    v.panel_fill = p.bg_1;
    v.window_fill = p.bg_2;
    v.window_stroke = Stroke::new(1.0, p.border_strong);
    v.window_corner_radius = CornerRadius::ZERO;
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(p.shadow_alpha[0]) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(p.shadow_alpha[1]) };
    v.menu_corner_radius = CornerRadius::ZERO;
    v.extreme_bg_color = p.field;
    v.faint_bg_color = p.bg_2;
    v.code_bg_color = p.field;
    v.override_text_color = None;
    v.selection.bg_fill = p.accent;
    // egui also uses this stroke as selected-label text, including channel toggles.
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.error;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg_1;
    w.noninteractive.weak_bg_fill = p.bg_1;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text_dim);

    // `bg_fill` paints slider rails, handles and checkbox boxes; `weak_bg_fill` buttons and value fields.
    w.inactive.bg_fill = p.rail;
    w.inactive.weak_bg_fill = p.bg_3;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border_soft);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);

    w.hovered.bg_fill = p.bg_4;
    w.hovered.weak_bg_fill = p.bg_4;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border);
    w.hovered.fg_stroke = Stroke::new(1.5, p.text);
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.accent_dim;
    w.active.weak_bg_fill = p.accent_dim;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.5, p.text);
    w.active.expansion = 0.0;

    w.open.bg_fill = p.bg_3;
    w.open.weak_bg_fill = p.bg_3;
    w.open.bg_stroke = Stroke::new(1.0, p.border_strong);
    w.open.fg_stroke = Stroke::new(1.0, p.text);

    for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        s.corner_radius = CornerRadius::same(p.radius);
    }
    v
}

/// Dock styling: tab strip, the active tab merges with its panel, thin gaps between panels.
pub fn dock_style(ui_style: &egui::Style) -> DockStyle {
    let p = palette();
    let mut d = DockStyle::from_egui(ui_style);
    d.dock_area_padding = None;
    d.main_surface_border_stroke = Stroke::NONE;
    d.main_surface_border_rounding = CornerRadius::ZERO;

    d.separator.width = 3.0;
    d.separator.extra_interact_width = 4.0;
    d.separator.color_idle = p.bg_0;
    d.separator.color_hovered = p.accent_dim;
    d.separator.color_dragged = p.accent;

    d.tab_bar.bg_fill = p.bg_2;
    d.tab_bar.height = 32.0;
    d.tab_bar.corner_radius = CornerRadius::ZERO;
    d.tab_bar.hline_color = p.border;
    d.tab_bar.inner_margin = Margin::ZERO;

    let tab = |bg: Color32, text: Color32| egui_dock::TabInteractionStyle { outline_color: Color32::TRANSPARENT, corner_radius: CornerRadius::ZERO, bg_fill: bg, text_color: text };
    d.tab.active = tab(p.bg_1, p.text);
    d.tab.focused = tab(p.bg_1, p.text);
    d.tab.active_with_kb_focus = tab(p.bg_1, p.text);
    d.tab.focused_with_kb_focus = tab(p.bg_1, p.text);
    d.tab.inactive = tab(p.bg_2, p.text_faint);
    d.tab.inactive_with_kb_focus = tab(p.bg_2, p.text_faint);
    d.tab.hovered = tab(p.bg_3, p.text_dim);
    d.tab.hline_below_active_tab_name = false;
    d.tab.minimum_width = Some(84.0);
    d.tab.spacing = 0.0;
    d.tab.tab_body.inner_margin = Margin::ZERO;
    d.tab.tab_body.stroke = Stroke::NONE;
    d.tab.tab_body.corner_radius = CornerRadius::ZERO;
    d.tab.tab_body.bg_fill = p.bg_1;

    d.overlay.selection_color = p.accent.linear_multiply(0.35);
    d.overlay.button_color = p.bg_3;
    d.overlay.button_border_stroke = Stroke::new(1.0, p.border_strong);
    d.buttons.collapse_tabs_color = p.text_faint;
    d.buttons.collapse_tabs_active_color = p.text;
    d
}

/// The header logo (orange square with the white ribbon-star mark), uploaded once per context.
pub fn brand_mark(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("subcraft-brand-mark");
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return handle;
    }
    let img = image::load_from_memory_with_format(include_bytes!("../assets/logo-mark.png"), image::ImageFormat::Png)
        .expect("embedded logo is a valid PNG")
        .into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let handle = ctx.load_texture("brand-mark", color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    handle
}

/// Window / taskbar icon: the app artwork (same as the .exe, installer and Linux icon).
pub fn app_icon() -> egui::IconData {
    let img = image::load_from_memory_with_format(include_bytes!("../assets/icon.png"), image::ImageFormat::Png)
        .expect("embedded icon is a valid PNG")
        .into_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData { rgba: img.into_raw(), width, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance of an sRGB colour.
    fn luminance(c: Color32) -> f32 {
        let lin = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    /// WCAG contrast ratio (1 to 21).
    fn contrast(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn differs(a: Color32, b: Color32, by: u8) -> bool {
        a.r().abs_diff(b.r()).max(a.g().abs_diff(b.g())).max(a.b().abs_diff(b.b())) >= by
    }

    /// `over` with black at `alpha` on top (the translucent plates behind text over the views).
    fn under_plate(over: Color32, alpha: u8) -> Color32 {
        let k = 1.0 - alpha as f32 / 255.0;
        let f = |v: u8| (v as f32 * k).round() as u8;
        Color32::from_rgb(f(over.r()), f(over.g()), f(over.b()))
    }

    /// QA 2026-11: pills, the brush notice, shortcut hints and pen anchors over the 3D view used
    /// the theme's text colours: in the Light theme, dark text on the dark render (about 1.2:1).
    #[test]
    fn text_over_the_views_reads_in_every_theme() {
        // Over the render (its default background) ...
        let render = Color32::from_rgb(36, 37, 40);
        assert!(contrast(OVERLAY_TEXT, under_plate(render, 150)) >= 4.5, "pill text");
        assert!(contrast(OVERLAY_TEXT_FAINT, under_plate(render, 150)) >= 2.5, "faint pill text");
        assert!(contrast(OVERLAY_TEXT_DIM, under_plate(render, 70)) >= 4.5, "shortcut keys");
        assert!(contrast(OVERLAY_TEXT_DIM, under_plate(render, 70)) > contrast(OVERLAY_TEXT_FAINT, under_plate(render, 70)), "keys stand out from their labels");
        assert!(contrast(OVERLAY_TEXT, render) >= 4.5, "pen anchors");
        // ... and the channel name pill over the 2D view's surround, in every theme.
        for kind in ThemeKind::ALL {
            let pill = under_plate(kind.palette().bg_0, 150);
            assert!(contrast(OVERLAY_TEXT, pill) >= 4.5, "{kind:?}: 2D view pill: {}", contrast(OVERLAY_TEXT, pill));
        }
        assert_eq!((OVERLAY_TEXT, OVERLAY_TEXT_DIM, OVERLAY_TEXT_FAINT), (DARK.text, DARK.text_dim, DARK.text_faint), "the Dark theme looks as before");
    }

    /// QA 2026-11: channel names, R/G/B slot labels, axis labels and red warnings kept the pastel
    /// colours tuned for dark panels: on the Light theme's panels Emissive was 1.3:1.
    #[test]
    fn coloured_labels_read_on_every_panel() {
        use subcraft_core::ChannelKind;
        let kinds = [ChannelKind::BaseColor, ChannelKind::Height, ChannelKind::Roughness, ChannelKind::Metallic, ChannelKind::Normal, ChannelKind::AmbientOcclusion, ChannelKind::Emissive, ChannelKind::Opacity];
        let fixed = [Color32::from_rgb(230, 110, 110), Color32::from_rgb(120, 200, 120), Color32::from_rgb(120, 150, 240), Color32::from_rgb(190, 108, 100), Color32::from_rgb(112, 172, 126), Color32::from_rgb(106, 144, 198), Color32::from_rgb(226, 92, 92), Color32::LIGHT_RED];
        for kind in ThemeKind::ALL {
            let p = kind.palette();
            let colours = kinds.iter().map(|&k| channel_accent_base(k)).chain(fixed);
            for c in colours {
                let shown = on_panel_in(&p, c);
                for bg in [p.bg_1, p.bg_2] {
                    assert!(contrast(shown, bg) >= 3.0, "{kind:?}: {c:?} shown as {shown:?} on {bg:?}: {}", contrast(shown, bg));
                }
                if p.dark {
                    assert_eq!(shown, c, "dark themes keep the colour");
                }
            }
        }
    }

    /// QA 2026-11: Painter-style's flatter corners only reached egui's own widgets; buttons and
    /// chips drawn by SubCraft kept the dark theme's radius.
    #[test]
    fn custom_widgets_take_the_theme_radius() {
        let source = include_str!("widgets.rs");
        assert!(!source.contains("corner_radius(RADIUS)") && !source.contains("CornerRadius::same(RADIUS)"), "widgets read theme::radius()");
        assert_eq!(DARK.radius, RADIUS);
        assert!(PAINTER.radius < RADIUS);
    }

    #[test]
    fn every_theme_is_readable_with_visible_borders_and_selection() {
        for kind in ThemeKind::ALL {
            let p = kind.palette();
            for (bg, name) in [(p.bg_1, "panel"), (p.bg_2, "header"), (p.card, "card"), (p.row_bands[1], "row band")] {
                assert!(contrast(p.text, bg) >= 4.5, "{kind:?}: text on the {name}: {}", contrast(p.text, bg));
                assert!(contrast(p.text_dim, bg) >= 4.5, "{kind:?}: dim text on the {name}: {}", contrast(p.text_dim, bg));
                assert!(contrast(p.text_faint, bg) >= 2.5, "{kind:?}: faint text on the {name}: {}", contrast(p.text_faint, bg));
            }
            assert!(contrast(p.text, p.field) >= 4.5, "{kind:?}: text in fields");
            assert!(contrast(p.text, p.bg_3) >= 4.5 && contrast(p.text, p.bg_4) >= 4.5, "{kind:?}: button labels");
            assert!(contrast(p.accent, p.bg_1) >= 3.0, "{kind:?}: accent on panels: {}", contrast(p.accent, p.bg_1));
            assert!(contrast(p.on_accent, p.accent) >= 4.5, "{kind:?}: text on the accent: {}", contrast(p.on_accent, p.accent));
            assert!(contrast(p.border_strong, p.bg_1) >= 1.4, "{kind:?}: strong borders show: {}", contrast(p.border_strong, p.bg_1));
            assert!(contrast(p.warning, p.bg_1) >= 3.0 && contrast(p.error, p.bg_1) >= 3.0 && contrast(p.success, p.bg_1) >= 3.0, "{kind:?}: status colours");
            assert!(contrast(p.folder, p.bg_1) >= 2.0, "{kind:?}: folder icons");
            // A selected row and a slider rail stand out from the panel.
            assert!(differs(p.accent_soft, p.bg_1, 20) && differs(p.accent_soft, p.row_bands[1], 10), "{kind:?}: selected rows");
            assert!(differs(p.rail, p.bg_1, 10), "{kind:?}: slider rails");
            assert_eq!(p.dark, luminance(p.bg_1) < 0.2, "{kind:?}: dark flag matches the panels");
        }
    }

    #[test]
    fn every_theme_gives_egui_its_own_visuals() {
        for kind in ThemeKind::ALL {
            let p = kind.palette();
            let v = visuals(p);
            assert_eq!(v.dark_mode, p.dark);
            assert_eq!((v.panel_fill, v.window_fill, v.extreme_bg_color), (p.bg_1, p.bg_2, p.field));
            assert_eq!(v.selection.bg_fill, p.accent);
            assert_eq!(v.widgets.inactive.fg_stroke.color, p.text);
            assert_eq!(v.widgets.noninteractive.fg_stroke.color, p.text_dim);
            assert_eq!(v.text_color(), p.text_dim, "plain labels use the dim text");
            // Applied to a context, both of egui's slots carry them (a light system theme must not
            // bring egui's own colours back, issue #206).
            let ctx = egui::Context::default();
            apply_visuals(&ctx, kind);
            for theme in [egui::Theme::Dark, egui::Theme::Light] {
                assert_eq!(ctx.style_of(theme).visuals.panel_fill, p.bg_1, "{kind:?} in egui's {theme:?} slot");
            }
        }
        let flat = visuals(&PAINTER).widgets.inactive.corner_radius;
        assert!(flat.nw < visuals(&DARK).widgets.inactive.corner_radius.nw, "Painter-style controls are flatter");
    }

    /// The dark theme is the look SubCraft had before themes.
    #[test]
    fn the_dark_theme_keeps_the_previous_colours() {
        assert_eq!(DARK.bg_1, Color32::from_rgb(48, 47, 45));
        assert_eq!(DARK.bg_0, Color32::from_rgb(34, 33, 31));
        assert_eq!(DARK.text, Color32::from_rgb(204, 203, 199));
        assert_eq!(DARK.accent, Color32::from_rgb(218, 145, 56));
        assert_eq!(DARK.accent_soft, Color32::from_rgb(70, 62, 52));
        assert_eq!(DARK.on_accent, DARK.bg_0);
        assert_eq!(DARK.radius, RADIUS);
        assert_eq!(ThemeKind::default(), ThemeKind::Dark);
    }

    #[test]
    fn theme_names_are_stable_in_settings_files() {
        for (kind, name) in [(ThemeKind::Dark, "\"dark\""), (ThemeKind::Light, "\"light\""), (ThemeKind::Painter, "\"painter\"")] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), name);
            assert_eq!(serde_json::from_str::<ThemeKind>(name).unwrap(), kind);
        }
    }
}
