//! Brush colour picker (issue #141): a hue ring around a saturation/value square, RGB and HSV
//! sliders, a hex field, old/new swatches and a row of recent colours.
//!
//! The colour being edited stays linear (as everywhere in SubCraft). HSV, the 0-255 RGB numbers
//! and hex are the sRGB-encoded display values, like in any paint program, and are converted with
//! `subcraft_core::color` on the way in and out. The widget only edits the `[f32; 3]` it is given;
//! the caller dispatches the command that applies it.

use std::f32::consts::TAU;

use egui::{Color32, CornerRadius, Id, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};
use subcraft_core::color::{linear3_to_srgb, srgb3_to_linear};

use crate::theme::{self, BORDER_STRONG, CONTROL_H, RADIUS, TEXT, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{self, Edit};

/// Recent colours kept in the preferences.
pub const MAX_RECENT: usize = 12;

/// Parse `#RRGGBB`, `RRGGBB`, `#RGB` or `RGB` (surrounding whitespace allowed) into 8-bit sRGB.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    if !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize, n: usize| u8::from_str_radix(&t[i..i + n], 16).ok();
    match t.len() {
        6 => Some([digit(0, 2)?, digit(2, 2)?, digit(4, 2)?]),
        3 => Some([digit(0, 1)? * 17, digit(1, 1)? * 17, digit(2, 1)? * 17]),
        _ => None,
    }
}

/// `#RRGGBB` in upper case.
pub fn format_hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Encoded 0..1 values to 8-bit, clamped and rounded.
pub fn to_u8(srgb: [f32; 3]) -> [u8; 3] {
    srgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub fn from_u8(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| v as f32 / 255.0)
}

/// RGB (0..1) to HSV, every component 0..1 (hue 1 = 360 degrees). Grey gives hue 0.
pub fn rgb_to_hsv(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|v| v.clamp(0.0, 1.0));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let s = if max > 0.0 { d / max } else { 0.0 };
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h.rem_euclid(1.0), s, max]
}

/// HSV (all 0..1, hue wraps) to RGB 0..1.
pub fn hsv_to_rgb(hsv: [f32; 3]) -> [f32; 3] {
    let h = hsv[0].rem_euclid(1.0) * 6.0;
    let s = hsv[1].clamp(0.0, 1.0);
    let v = hsv[2].clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

/// HSV of `rgb`, keeping `hue` (and saturation) where the colour does not define them, so dragging
/// to black or white and back does not lose the hue.
fn hsv_keeping(rgb: [f32; 3], keep: [f32; 3]) -> [f32; 3] {
    let mut hsv = rgb_to_hsv(rgb);
    if hsv[2] <= 1e-4 {
        hsv[0] = keep[0];
        hsv[1] = keep[1];
    } else if hsv[1] <= 1e-4 {
        hsv[0] = keep[0];
    }
    hsv
}

/// Put `c` first in the recent list, dropping an equal entry and anything past [`MAX_RECENT`].
pub fn push_recent(recent: &mut Vec<[f32; 3]>, c: [f32; 3]) {
    recent.retain(|r| !same(*r, c));
    recent.insert(0, c);
    recent.truncate(MAX_RECENT);
}

fn same(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-5)
}

/// Swatch fill for a linear colour.
fn swatch_color(linear: [f32; 3]) -> Color32 {
    let [r, g, b] = linear.map(|v| v.max(0.0));
    Color32::from(egui::Rgba::from_rgb(r, g, b))
}

fn display_color(srgb: [f32; 3]) -> Color32 {
    let [r, g, b] = to_u8(srgb);
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Zone {
    Ring,
    Square,
}

/// Per-picker state kept in egui memory while the popup is open.
#[derive(Clone)]
struct PickerState {
    open: bool,
    /// The colour when the popup opened (linear), for the "old" swatch.
    old: [f32; 3],
    /// Display HSV; kept so hue survives grey / black.
    hsv: [f32; 3],
    /// The linear value `hsv` belongs to; anything else means the colour changed elsewhere.
    synced: [f32; 3],
    drag: Option<Zone>,
    hex_text: String,
    hex_focused: bool,
}

impl PickerState {
    fn new(linear: [f32; 3]) -> Self {
        Self { open: false, old: linear, hsv: rgb_to_hsv(linear3_to_srgb(linear)), synced: linear, drag: None, hex_text: String::new(), hex_focused: false }
    }
}

/// The colour being edited plus its display HSV.
struct Target<'a> {
    value: &'a mut [f32; 3],
    st: &'a mut PickerState,
    changed: bool,
}

impl Target<'_> {
    fn srgb(&self) -> [f32; 3] {
        hsv_to_rgb(self.st.hsv)
    }
    fn set_hsv(&mut self, hsv: [f32; 3]) {
        self.st.hsv = [hsv[0].rem_euclid(1.0), hsv[1].clamp(0.0, 1.0), hsv[2].clamp(0.0, 1.0)];
        self.apply(srgb3_to_linear(hsv_to_rgb(self.st.hsv)));
    }
    fn set_srgb(&mut self, srgb: [f32; 3]) {
        self.st.hsv = hsv_keeping(srgb, self.st.hsv);
        self.apply(srgb3_to_linear(srgb.map(|v| v.clamp(0.0, 1.0))));
    }
    fn set_linear(&mut self, linear: [f32; 3]) {
        self.st.hsv = hsv_keeping(linear3_to_srgb(linear), self.st.hsv);
        self.apply(linear);
    }
    fn apply(&mut self, linear: [f32; 3]) {
        if !same(*self.value, linear) {
            *self.value = linear;
            self.changed = true;
        }
        self.st.synced = *self.value;
    }
}

/// Colour swatch button; a click opens the picker popup. `rgb` is linear. Finished picks go to the
/// front of `recent` when the popup closes.
pub fn color_button(ui: &mut Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, rgb: &mut [f32; 3], recent: &mut Vec<[f32; 3]>) -> Edit {
    let id = ui.make_persistent_id(("color_picker", id_salt));
    let width = ui.available_width().clamp(44.0, 120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, CONTROL_H), Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    paint_swatch(ui, rect, *rgb, response.hovered());
    let response = response.on_hover_text(format!("{} · click to pick a colour", format_hex(to_u8(linear3_to_srgb(*rgb))))).on_hover_cursor(egui::CursorIcon::PointingHand);

    let state_id = id.with("state");
    let mut st = ui.data_mut(|d| d.get_temp::<PickerState>(state_id)).unwrap_or_else(|| PickerState::new(*rgb));
    let mut edit = Edit::default();
    let shown = egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            if !st.open {
                st = PickerState::new(*rgb);
                st.open = true;
            }
            edit = picker_body(ui, id, rgb, &mut st, recent);
        });
    if shown.is_none() && st.open {
        // Closed: remember the colour if it was changed.
        st.open = false;
        if !same(st.old, *rgb) {
            push_recent(recent, *rgb);
        }
        edit.done = true;
    }
    ui.data_mut(|d| d.insert_temp(state_id, st));
    edit
}

fn paint_swatch(ui: &Ui, rect: Rect, linear: [f32; 3], hovered: bool) {
    let stroke = if hovered { Stroke::new(1.0, TEXT) } else { Stroke::new(1.0, BORDER_STRONG) };
    ui.painter().rect(rect, CornerRadius::same(RADIUS), swatch_color(linear), stroke, egui::StrokeKind::Inside);
}

/// The whole picker. `rgb` is linear.
fn picker_body(ui: &mut Ui, id: Id, rgb: &mut [f32; 3], st: &mut PickerState, recent: &mut [[f32; 3]]) -> Edit {
    ui.set_width(236.0);
    ui.spacing_mut().item_spacing.y = theme::SP_S;
    // Changed elsewhere (undo, eyedropper) while open: follow it.
    if !same(*rgb, st.synced) {
        st.hsv = hsv_keeping(linear3_to_srgb(*rgb), st.hsv);
        st.synced = *rgb;
    }
    let mut t = Target { value: rgb, st, changed: false };
    let mut done = false;

    done |= wheel(ui, &mut t);
    old_new(ui, &mut t);
    done |= hex_field(ui, id, &mut t);

    ui.scope(|ui| {
        ui.spacing_mut().slider_width = 150.0;
        ui.spacing_mut().interact_size.x = 44.0;
        let srgb = t.srgb();
        let mut rgb8 = to_u8(srgb).map(f32::from);
        let mut rgb_changed = false;
        for (i, name) in ["R", "G", "B"].into_iter().enumerate() {
            let r = channel_slider(ui, name, &mut rgb8[i], 0.0..=255.0, "");
            rgb_changed |= r.changed();
            done |= r.drag_stopped() || r.lost_focus();
        }
        if rgb_changed {
            t.set_srgb(rgb8.map(|v| v / 255.0));
        }
        ui.add_space(2.0);
        let mut hsv = [t.st.hsv[0] * 360.0, t.st.hsv[1] * 100.0, t.st.hsv[2] * 100.0];
        let mut hsv_changed = false;
        for (i, (name, max, suffix)) in [("H", 360.0, "°"), ("S", 100.0, "%"), ("V", 100.0, "%")].into_iter().enumerate() {
            let r = channel_slider(ui, name, &mut hsv[i], 0.0..=max, suffix);
            hsv_changed |= r.changed();
            done |= r.drag_stopped() || r.lost_focus();
        }
        if hsv_changed {
            t.set_hsv([hsv[0] / 360.0, hsv[1] / 100.0, hsv[2] / 100.0]);
        }
    });

    if !recent.is_empty() {
        ui.label(RichText::new("Recent").font(theme::font(11.0)).color(TEXT_DIM));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
            for &c in recent.iter() {
                let (rect, r) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
                paint_swatch(ui, rect, c, r.hovered());
                if r.on_hover_text(format_hex(to_u8(linear3_to_srgb(c)))).clicked() {
                    t.set_linear(c);
                    done = true;
                }
            }
        });
    }
    Edit { changed: t.changed, done }
}

// No `integer()`: its step would round the HSV value of the shown colour on every frame and count as an edit.
fn channel_slider(ui: &mut Ui, name: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_sized([12.0, 18.0], egui::Label::new(RichText::new(name).font(theme::font(11.5)).color(TEXT_DIM)));
        ui.add(widgets::typed_slider(egui::Slider::new(v, range).fixed_decimals(0).suffix(suffix)))
    })
    .inner
}

/// Hue ring around a saturation (x) / value (y) square. Returns true when a drag ended.
fn wheel(ui: &mut Ui, t: &mut Target<'_>) -> bool {
    let size = ui.available_width().min(200.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    let center = rect.center();
    let outer = size * 0.5 - 1.0;
    let inner = outer - 16.0;
    let half = (inner - 6.0) / std::f32::consts::SQRT_2;
    let square = Rect::from_center_size(center, Vec2::splat(half * 2.0));

    if response.is_pointer_button_down_on()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let d = pos - center;
        if t.st.drag.is_none() {
            t.st.drag = if square.expand(4.0).contains(pos) {
                Some(Zone::Square)
            } else if d.length() >= inner - 6.0 && d.length() <= outer + 6.0 {
                Some(Zone::Ring)
            } else {
                None
            };
        }
        match t.st.drag {
            Some(Zone::Ring) => {
                let hue = (d.y.atan2(d.x) / TAU).rem_euclid(1.0);
                t.set_hsv([hue, t.st.hsv[1], t.st.hsv[2]]);
            }
            Some(Zone::Square) => {
                let s = ((pos.x - square.left()) / square.width()).clamp(0.0, 1.0);
                let v = 1.0 - ((pos.y - square.top()) / square.height()).clamp(0.0, 1.0);
                t.set_hsv([t.st.hsv[0], s, v]);
            }
            None => {}
        }
    } else {
        t.st.drag = None;
    }

    let painter = ui.painter_at(rect.expand(2.0));
    // Hue ring.
    let mut mesh = egui::Mesh::default();
    const SEGMENTS: u32 = 96;
    for k in 0..=SEGMENTS {
        let hue = k as f32 / SEGMENTS as f32;
        let dir = Vec2::angled(hue * TAU);
        let c = display_color(hsv_to_rgb([hue, 1.0, 1.0]));
        mesh.colored_vertex(center + dir * inner, c);
        mesh.colored_vertex(center + dir * outer, c);
        if k > 0 {
            let i = 2 * k;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    // Saturation / value square for the current hue.
    const N: u32 = 16;
    let base = mesh.vertices.len() as u32;
    for y in 0..=N {
        for x in 0..=N {
            let (s, v) = (x as f32 / N as f32, 1.0 - y as f32 / N as f32);
            let pos = Pos2::new(square.left() + square.width() * x as f32 / N as f32, square.top() + square.height() * y as f32 / N as f32);
            mesh.colored_vertex(pos, display_color(hsv_to_rgb([t.st.hsv[0], s, v])));
            if x > 0 && y > 0 {
                let i = base + y * (N + 1) + x;
                let (a, b, c, d) = (i - N - 2, i - N - 1, i - 1, i);
                mesh.add_triangle(a, b, d);
                mesh.add_triangle(a, d, c);
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.rect_stroke(square, CornerRadius::ZERO, Stroke::new(1.0, BORDER_STRONG), egui::StrokeKind::Outside);

    // Markers.
    let ring_pos = center + Vec2::angled(t.st.hsv[0] * TAU) * ((inner + outer) * 0.5);
    painter.circle_stroke(ring_pos, 6.0, Stroke::new(2.0, Color32::WHITE));
    painter.circle_stroke(ring_pos, 7.5, Stroke::new(1.0, Color32::BLACK));
    let sq_pos = Pos2::new(square.left() + square.width() * t.st.hsv[1], square.top() + square.height() * (1.0 - t.st.hsv[2]));
    let mark = if t.st.hsv[2] > 0.55 && t.st.hsv[1] < 0.45 { Color32::BLACK } else { Color32::WHITE };
    painter.circle_stroke(sq_pos, 5.0, Stroke::new(1.5, mark));

    response.drag_stopped() || response.clicked()
}

/// Old (click to restore) and new swatches side by side.
fn old_new(ui: &mut Ui, t: &mut Target<'_>) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 26.0), Sense::hover());
    let (left, right) = rect.split_left_right_at_fraction(0.5);
    let old = ui.interact(left, ui.id().with("old_swatch"), Sense::click());
    let painter = ui.painter();
    let r = RADIUS;
    painter.rect_filled(left, CornerRadius { nw: r, sw: r, ne: 0, se: 0 }, swatch_color(t.st.old));
    painter.rect_filled(right, CornerRadius { nw: 0, sw: 0, ne: r, se: r }, swatch_color(*t.value));
    painter.rect_stroke(rect, CornerRadius::same(r), Stroke::new(1.0, BORDER_STRONG), egui::StrokeKind::Inside);
    for (half, text) in [(left, "Old"), (right, "New")] {
        let lum = rgb_to_hsv(linear3_to_srgb(if text == "Old" { t.st.old } else { *t.value }))[2];
        let fg = if lum > 0.6 { Color32::from_black_alpha(170) } else { Color32::from_white_alpha(200) };
        painter.text(half.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, text, theme::font(10.5), fg);
    }
    if old.on_hover_text("Click to go back to this colour").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        let c = t.st.old;
        t.set_linear(c);
    }
}

/// Hex entry: the text stays as typed while focused; a complete 6-digit code applies at once (so a
/// paste works), and on Enter / focus loss `#RGB` is accepted too. Escape reverts.
fn hex_field(ui: &mut Ui, id: Id, t: &mut Target<'_>) -> bool {
    let current = format_hex(to_u8(t.srgb()));
    ui.horizontal(|ui| {
        ui.label(RichText::new("Hex").font(theme::font(11.5)).color(TEXT_DIM));
        let mut text = if t.st.hex_focused { std::mem::take(&mut t.st.hex_text) } else { current.clone() };
        let r = ui.add(egui::TextEdit::singleline(&mut text).id(id.with("hex")).desired_width(84.0).char_limit(16).font(egui::TextStyle::Monospace));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut done = false;
        if r.changed() {
            let digits = text.trim().trim_start_matches('#');
            if digits.len() == 6
                && let Some(c) = parse_hex(&text)
            {
                t.set_srgb(from_u8(c));
            }
        }
        if r.lost_focus() && !escape {
            if let Some(c) = parse_hex(&text) {
                t.set_srgb(from_u8(c));
            }
            done = true;
        }
        t.st.hex_focused = r.has_focus();
        t.st.hex_text = text;
        ui.label(RichText::new("#RGB or #RRGGBB").small().color(TEXT_FAINT));
        done
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#FF8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex("ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex("  #0a0B0c "), Some([10, 11, 12]));
        assert_eq!(parse_hex("#F80"), Some([255, 136, 0]));
        assert_eq!(parse_hex("abc"), Some([0xAA, 0xBB, 0xCC]));
        for bad in ["", "#", "#12", "#1234", "#12345", "#1234567", "#GG0000", "12 345", "##123456", "+12345"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn hex_formatting_round_trips() {
        assert_eq!(format_hex([255, 128, 0]), "#FF8000");
        assert_eq!(format_hex([0, 0, 0]), "#000000");
        for c in [[0, 0, 0], [255, 255, 255], [1, 2, 3], [200, 17, 99]] {
            assert_eq!(parse_hex(&format_hex(c)), Some(c));
        }
    }

    #[test]
    fn hsv_known_values() {
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        assert!(near(rgb_to_hsv([1.0, 0.0, 0.0]), [0.0, 1.0, 1.0]));
        assert!(near(rgb_to_hsv([0.0, 1.0, 0.0]), [1.0 / 3.0, 1.0, 1.0]));
        assert!(near(rgb_to_hsv([0.0, 0.0, 1.0]), [2.0 / 3.0, 1.0, 1.0]));
        assert!(near(rgb_to_hsv([0.5, 0.5, 0.5]), [0.0, 0.0, 0.5]));
        assert!(near(hsv_to_rgb([1.0 / 6.0, 1.0, 1.0]), [1.0, 1.0, 0.0]));
        assert!(near(hsv_to_rgb([0.5, 0.5, 1.0]), [0.5, 1.0, 1.0]));
        assert!(near(hsv_to_rgb([1.0, 1.0, 1.0]), [1.0, 0.0, 0.0]), "hue wraps");
    }

    #[test]
    fn hsv_rgb_round_trip() {
        for r in 0..=10 {
            for g in 0..=10 {
                for b in 0..=10 {
                    let rgb = [r as f32 / 10.0, g as f32 / 10.0, b as f32 / 10.0];
                    let back = hsv_to_rgb(rgb_to_hsv(rgb));
                    assert!(rgb.iter().zip(back).all(|(x, y)| (x - y).abs() < 1e-5), "{rgb:?} -> {back:?}");
                }
            }
        }
        // Every 8-bit hex survives hex -> HSV -> hex.
        for c in [[12, 200, 77], [255, 255, 0], [3, 3, 3], [128, 0, 255]] {
            assert_eq!(to_u8(hsv_to_rgb(rgb_to_hsv(from_u8(c)))), c);
        }
    }

    #[test]
    fn linear_hex_round_trip() {
        for c in [[0u8, 0, 0], [255, 255, 255], [10, 128, 240], [1, 2, 254]] {
            let linear = srgb3_to_linear(from_u8(c));
            assert_eq!(to_u8(linear3_to_srgb(linear)), c, "linear storage keeps the 8-bit colour");
        }
    }

    #[test]
    fn grey_keeps_hue() {
        let hsv = hsv_keeping([0.4, 0.4, 0.4], [0.3, 0.8, 0.9]);
        assert!((hsv[0] - 0.3).abs() < 1e-6 && hsv[1] == 0.0);
        let hsv = hsv_keeping([0.0, 0.0, 0.0], [0.7, 0.6, 0.9]);
        assert!((hsv[0] - 0.7).abs() < 1e-6 && (hsv[1] - 0.6).abs() < 1e-6 && hsv[2] == 0.0);
    }

    #[test]
    fn recent_colours_dedupe_and_cap() {
        let mut recent = Vec::new();
        for i in 0..20 {
            push_recent(&mut recent, [i as f32 / 20.0, 0.0, 0.0]);
        }
        assert_eq!(recent.len(), MAX_RECENT);
        assert_eq!(recent[0], [19.0 / 20.0, 0.0, 0.0]);
        push_recent(&mut recent, [10.0 / 20.0, 0.0, 0.0]);
        assert_eq!(recent.len(), MAX_RECENT);
        assert_eq!(recent[0], [0.5, 0.0, 0.0]);
        assert_eq!(recent.iter().filter(|c| **c == [0.5, 0.0, 0.0]).count(), 1);
    }

    /// The picker renders headless and edits only the colour it is given.
    #[test]
    fn picker_renders_and_follows_external_changes() {
        let ctx = egui::Context::default();
        let mut rgb = [0.2, 0.4, 0.6];
        let mut st = PickerState::new(rgb);
        st.open = true;
        let mut recent = vec![[1.0, 0.0, 0.0]];
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let e = picker_body(ui, Id::new("t"), &mut rgb, &mut st, &mut recent);
                assert!(!e.changed, "no input, no change");
            });
        }
        assert_eq!(rgb, [0.2, 0.4, 0.6]);
        rgb = [0.0, 0.0, 0.0];
        let hue = st.hsv[0];
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            picker_body(ui, Id::new("t"), &mut rgb, &mut st, &mut recent);
        });
        assert_eq!(st.synced, [0.0, 0.0, 0.0]);
        assert_eq!(st.hsv[2], 0.0);
        assert!((st.hsv[0] - hue).abs() < 1e-6, "black keeps the previous hue");
    }
}
