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

use crate::theme::{self, BORDER_STRONG, CONTROL_H, TEXT, TEXT_DIM, TEXT_FAINT};
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

/// Where a "pick from the model" request is kept: the id of the colour button waiting for a
/// colour. The 3D view answers the next click on the model with [`deliver_pick`].
pub fn pick_target_id() -> Id {
    Id::new("color_picker_pick_target")
}

/// The colour button waiting for a pick from the model, if any.
pub fn pick_target(ctx: &egui::Context) -> Option<Id> {
    ctx.data(|d| d.get_temp::<Id>(pick_target_id()))
}

/// Give the waiting colour button the colour picked on the model (linear), ending the request.
pub fn deliver_pick(ctx: &egui::Context, linear: [f32; 3]) {
    if let Some(target) = pick_target(ctx) {
        ctx.data_mut(|d| {
            d.remove::<Id>(pick_target_id());
            d.insert_temp(target.with("picked"), linear);
        });
    }
}

/// Stop waiting for a pick (Esc in the 3D view).
pub fn cancel_pick(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Id>(pick_target_id()));
}

/// Colour swatch button; a click opens the picker popup. `rgb` is linear. Finished picks go to the
/// front of `recent` when the popup closes.
pub fn color_button(ui: &mut Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, rgb: &mut [f32; 3], recent: &mut Vec<[f32; 3]>) -> Edit {
    color_button_response(ui, id_salt, rgb, recent).0
}

/// The recent colours shared by every swatch ([`swatch_linear`] and friends) this frame; the app
/// fills them from, and saves them to, the preferences ([`share_recent`], [`shared_recent`]).
fn recent_id() -> Id {
    Id::new("color_picker_shared_recent")
}

/// Hand the preferences' recent colours to the swatches for this frame.
pub fn share_recent(ctx: &egui::Context, recent: &[[f32; 3]]) {
    ctx.data_mut(|d| d.insert_temp(recent_id(), recent.to_vec()));
}

/// The swatches' recent colours after this frame (to save back when they changed).
pub fn shared_recent(ctx: &egui::Context) -> Option<Vec<[f32; 3]>> {
    ctx.data(|d| d.get_temp::<Vec<[f32; 3]>>(recent_id()))
}

/// SubCraft's colour swatch (wheel, sliders, hex, recent colours and the eyedropper) for a
/// linear colour, as an [`Edit`]; the recent colours are the shared ones.
pub fn swatch_edit(ui: &mut Ui, rgb: &mut [f32; 3]) -> Edit {
    let salt = ui.next_auto_id();
    let mut recent = shared_recent(ui.ctx()).unwrap_or_default();
    let (edit, _) = color_button_response(ui, salt, rgb, &mut recent);
    ui.ctx().data_mut(|d| d.insert_temp(recent_id(), recent));
    edit
}

/// [`swatch_edit`] as a response (changed when the colour changed), for a linear colour.
pub fn swatch_linear(ui: &mut Ui, rgb: &mut [f32; 3]) -> Response {
    let salt = ui.next_auto_id();
    let mut recent = shared_recent(ui.ctx()).unwrap_or_default();
    let (edit, mut response) = color_button_response(ui, salt, rgb, &mut recent);
    ui.ctx().data_mut(|d| d.insert_temp(recent_id(), recent));
    if edit.changed {
        response.mark_changed();
    }
    response
}

/// [`swatch_linear`] for a colour stored as sRGB values (display colours, the 0..1 numbers egui's
/// own colour button edits).
pub fn swatch_srgb(ui: &mut Ui, srgb: &mut [f32; 3]) -> Response {
    let mut linear = srgb3_to_linear(*srgb);
    let response = swatch_linear(ui, &mut linear);
    if response.changed() {
        *srgb = linear3_to_srgb(linear).map(|v| v.clamp(0.0, 1.0));
    }
    response
}

/// [`swatch_linear`] for an 8-bit colour; its alpha is kept.
pub fn swatch_color32(ui: &mut Ui, c: &mut Color32) -> Response {
    let mut srgb = [c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0);
    let response = swatch_srgb(ui, &mut srgb);
    if response.changed() {
        let [r, g, b] = srgb.map(|v| (v * 255.0).round() as u8);
        *c = Color32::from_rgba_unmultiplied(r, g, b, c.a());
    }
    response
}

/// The 3D view's rect this frame, set while a pick is waiting: it answers clicks on the model
/// itself (the material's colour), everywhere else [`screen_pick`] reads the window.
pub fn set_model_rect(ctx: &egui::Context, rect: Rect) {
    ctx.data_mut(|d| d.insert_temp(model_rect_id(), rect));
}

fn model_rect_id() -> Id {
    Id::new("color_picker_model_rect")
}

fn pending_id() -> Id {
    Id::new("color_picker_screen_pending")
}

/// The colour (linear) of `image`, a screenshot of the window, at `pos` (points).
pub fn screenshot_colour(image: &egui::ColorImage, pos: Pos2, pixels_per_point: f32) -> Option<[f32; 3]> {
    let (x, y) = ((pos.x * pixels_per_point).floor(), (pos.y * pixels_per_point).floor());
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let (x, y) = (x as usize, y as usize);
    let [w, h] = image.size;
    if x >= w || y >= h {
        return None;
    }
    let c = image.pixels[y * w + x];
    Some(srgb3_to_linear([c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0)))
}

/// The eyedropper outside the 3D view (Painter's eyedropper reads the screen): while a colour
/// button waits for a pick, a click anywhere else in the window (2D view, panels, swatches,
/// thumbnails, reference images) takes the colour shown under the cursor, read from a
/// screenshot of the window. Esc cancels. Call once per frame after the panels; returns a
/// status line when a colour was picked.
pub fn screen_pick(ctx: &egui::Context) -> Option<&'static str> {
    let model = ctx.data_mut(|d| {
        let rect = d.get_temp::<Rect>(model_rect_id());
        d.remove::<Rect>(model_rect_id());
        rect
    });
    if pick_target(ctx).is_none() {
        ctx.data_mut(|d| d.remove::<Pos2>(pending_id()));
        return None;
    }
    // A screenshot was asked for: take its pixel under the click.
    if let Some(pos) = ctx.data(|d| d.get_temp::<Pos2>(pending_id())) {
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            ctx.data_mut(|d| d.remove::<Pos2>(pending_id()));
            if let Some(c) = screenshot_colour(&image, pos, ctx.pixels_per_point()) {
                deliver_pick(ctx, c);
                return Some("Picked the colour on screen");
            }
        }
        ctx.request_repaint();
        return None;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel_pick(ctx);
        return None;
    }
    // Over the 3D view, the view answers with the material's colour.
    if ctx.pointer_hover_pos().is_some_and(|p| model.is_some_and(|r| r.contains(p))) {
        return None;
    }
    let screen = ctx.content_rect();
    let response = egui::Area::new(Id::new("color_picker_screen_overlay"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| ui.allocate_rect(screen, Sense::click()))
        .inner
        .on_hover_cursor(egui::CursorIcon::Crosshair);
    if response.clicked() && let Some(p) = response.interact_pointer_pos() {
        ctx.data_mut(|d| d.insert_temp(pending_id(), p));
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        ctx.request_repaint();
    }
    None
}

/// [`color_button`] with the swatch's response.
fn color_button_response(ui: &mut Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, rgb: &mut [f32; 3], recent: &mut Vec<[f32; 3]>) -> (Edit, Response) {
    let id = ui.make_persistent_id(("color_picker", id_salt));
    let width = ui.available_width().clamp(44.0, 120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, CONTROL_H), Sense::hover());
    let response = ui.interact(rect, id, Sense::click());
    paint_swatch(ui, rect, *rgb, response.hovered());
    let response = response.on_hover_text(format!("{} · click to pick a colour", format_hex(to_u8(linear3_to_srgb(*rgb))))).on_hover_cursor(egui::CursorIcon::PointingHand);

    let state_id = id.with("state");
    let mut st = ui.data_mut(|d| d.get_temp::<PickerState>(state_id)).unwrap_or_else(|| PickerState::new(*rgb));
    let mut edit = Edit::default();
    // A colour picked on the model for this button (the picker's eyedropper).
    if let Some(picked) = ui.data_mut(|d| d.remove_temp::<[f32; 3]>(id.with("picked"))) {
        if !same(*rgb, picked) {
            *rgb = picked;
            edit.changed = true;
            push_recent(recent, picked);
        }
        edit.done = true;
    }
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
    (edit, response)
}

/// The whole picker shown in place (no swatch, no popup), for panels that keep it open: the Pen
/// panel's colour wheel. `rgb` is linear. Finished picks go to the front of `recent`.
pub fn inline_picker(ui: &mut Ui, id_salt: impl std::hash::Hash + std::fmt::Debug, rgb: &mut [f32; 3], recent: &mut Vec<[f32; 3]>) -> Edit {
    let id = ui.make_persistent_id(("color_picker_inline", id_salt));
    let state_id = id.with("state");
    let mut st = ui.data_mut(|d| d.get_temp::<PickerState>(state_id)).unwrap_or_else(|| PickerState::new(*rgb));
    st.open = true;
    let mut edit = Edit::default();
    if let Some(picked) = ui.data_mut(|d| d.remove_temp::<[f32; 3]>(id.with("picked"))) {
        if !same(*rgb, picked) {
            *rgb = picked;
            edit.changed = true;
        }
        edit.done = true;
    }
    let body = picker_body(ui, id, rgb, &mut st, recent);
    edit.changed |= body.changed;
    edit.done |= body.done;
    if edit.done && !same(st.old, *rgb) {
        push_recent(recent, *rgb);
        st.old = *rgb;
    }
    ui.data_mut(|d| d.insert_temp(state_id, st));
    edit
}

fn paint_swatch(ui: &Ui, rect: Rect, linear: [f32; 3], hovered: bool) {
    let stroke = if hovered { Stroke::new(1.0, TEXT()) } else { Stroke::new(1.0, BORDER_STRONG()) };
    ui.painter().rect(rect, CornerRadius::same(theme::radius()), swatch_color(linear), stroke, egui::StrokeKind::Inside);
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
    // Painter's eyedropper beside the colour: the next click picks a colour.
    if ui.button(format!("{}  Pick a colour", crate::icons::CROSSHAIR)).on_hover_text("Then click anywhere in SubCraft: on the model, its material's base colour; anywhere else (2D view, panels, swatches, thumbnails, images), the colour shown there. Esc cancels").clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(pick_target_id(), id));
        ui.close();
    }
    done |= hex_field(ui, id, &mut t);

    ui.scope(|ui| {
        ui.spacing_mut().slider_width = 150.0;
        ui.spacing_mut().interact_size.x = 44.0;
        let srgb = t.srgb();
        // The sliders get whole numbers and only a value that differs from them counts as an edit:
        // egui rounds a slider's value to its decimals every frame and reports that as a change.
        let shown_rgb = to_u8(srgb).map(f32::from);
        let mut rgb8 = shown_rgb;
        for (i, name) in ["R", "G", "B"].into_iter().enumerate() {
            let r = channel_slider(ui, name, &mut rgb8[i], 0.0..=255.0, "");
            done |= r.drag_stopped() || r.lost_focus();
        }
        if rgb8 != shown_rgb {
            t.set_srgb(rgb8.map(|v| v / 255.0));
        }
        ui.add_space(2.0);
        const SCALE: [f32; 3] = [360.0, 100.0, 100.0];
        let shown_hsv: [f32; 3] = std::array::from_fn(|i| (t.st.hsv[i] * SCALE[i]).round());
        let mut hsv = shown_hsv;
        for (i, (name, suffix)) in [("H", "°"), ("S", "%"), ("V", "%")].into_iter().enumerate() {
            let r = channel_slider(ui, name, &mut hsv[i], 0.0..=SCALE[i], suffix);
            done |= r.drag_stopped() || r.lost_focus();
        }
        if hsv != shown_hsv {
            // Only the moved component changes; the others keep their exact value.
            let mut next = t.st.hsv;
            for i in 0..3 {
                if hsv[i] != shown_hsv[i] {
                    next[i] = hsv[i] / SCALE[i];
                }
            }
            t.set_hsv(next);
        }
    });

    if !recent.is_empty() {
        ui.label(RichText::new("Recent").font(theme::font(11.0)).color(TEXT_DIM()));
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
        ui.add_sized([12.0, 18.0], egui::Label::new(RichText::new(name).font(theme::font(11.5)).color(TEXT_DIM())));
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
    painter.rect_stroke(square, CornerRadius::ZERO, Stroke::new(1.0, BORDER_STRONG()), egui::StrokeKind::Outside);

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
    let r = theme::radius();
    painter.rect_filled(left, CornerRadius { nw: r, sw: r, ne: 0, se: 0 }, swatch_color(t.st.old));
    painter.rect_filled(right, CornerRadius { nw: 0, sw: 0, ne: r, se: r }, swatch_color(*t.value));
    painter.rect_stroke(rect, CornerRadius::same(r), Stroke::new(1.0, BORDER_STRONG()), egui::StrokeKind::Inside);
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
        ui.label(RichText::new("Hex").font(theme::font(11.5)).color(TEXT_DIM()));
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
        ui.label(RichText::new("#RGB or #RRGGBB").small().color(TEXT_FAINT()));
        done
    })
    .inner
}

#[cfg(test)]
mod screen_pick_tests {
    use super::*;

    #[test]
    fn the_screenshot_pixel_under_the_click_becomes_the_colour() {
        // A 4 x 2 pixel window at 2 pixels per point: point (1.2, 0.4) is pixel (2, 0).
        let mut image = egui::ColorImage::new([4, 2], vec![Color32::BLACK; 8]);
        image.pixels[2] = Color32::from_rgb(255, 128, 0);
        let c = screenshot_colour(&image, Pos2::new(1.2, 0.4), 2.0).unwrap();
        let want = srgb3_to_linear([1.0, 128.0 / 255.0, 0.0]);
        assert!(c.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-6), "{c:?}");
        assert_eq!(screenshot_colour(&image, Pos2::new(0.1, 0.1), 2.0), Some([0.0; 3]));
        assert_eq!(screenshot_colour(&image, Pos2::new(5.0, 0.0), 2.0), None, "outside the window");
        assert_eq!(screenshot_colour(&image, Pos2::new(-1.0, 0.0), 2.0), None);
    }

    #[test]
    fn a_pick_is_delivered_to_the_waiting_swatch_and_srgb_swatches_round_trip() {
        let ctx = egui::Context::default();
        let target = Id::new("some swatch");
        ctx.data_mut(|d| d.insert_temp(pick_target_id(), target));
        deliver_pick(&ctx, [0.25, 0.5, 0.75]);
        assert!(pick_target(&ctx).is_none(), "the pick ends");
        assert_eq!(ctx.data(|d| d.get_temp::<[f32; 3]>(target.with("picked"))), Some([0.25, 0.5, 0.75]));
        // Recent colours are shared through the context.
        share_recent(&ctx, &[[1.0, 0.0, 0.0]]);
        assert_eq!(shared_recent(&ctx), Some(vec![[1.0, 0.0, 0.0]]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_picked_on_the_model_reaches_the_waiting_button() {
        let ctx = egui::Context::default();
        let button = Id::new("some colour");
        assert_eq!(pick_target(&ctx), None);
        deliver_pick(&ctx, [1.0, 0.0, 0.0]);
        assert_eq!(ctx.data(|d| d.get_temp::<[f32; 3]>(button.with("picked"))), None, "no request, nothing delivered");
        ctx.data_mut(|d| d.insert_temp(pick_target_id(), button));
        assert_eq!(pick_target(&ctx), Some(button));
        deliver_pick(&ctx, [0.2, 0.4, 0.6]);
        assert_eq!(pick_target(&ctx), None, "the request ends");
        assert_eq!(ctx.data(|d| d.get_temp::<[f32; 3]>(button.with("picked"))), Some([0.2, 0.4, 0.6]));
        ctx.data_mut(|d| d.insert_temp(pick_target_id(), button));
        cancel_pick(&ctx);
        assert_eq!(pick_target(&ctx), None);
    }

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
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let e = picker_body(ui, Id::new("t"), &mut rgb, &mut st, &mut recent);
                assert!(!e.changed, "no input, no change");
            });
            // Debug builds assert that texture uploads are handled before the output is dropped.
            out.textures_delta.clear();
        }
        assert_eq!(rgb, [0.2, 0.4, 0.6]);
        rgb = [0.0, 0.0, 0.0];
        let hue = st.hsv[0];
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            picker_body(ui, Id::new("t"), &mut rgb, &mut st, &mut recent);
        });
        out.textures_delta.clear();
        assert_eq!(st.synced, [0.0, 0.0, 0.0]);
        assert_eq!(st.hsv[2], 0.0);
        assert!((st.hsv[0] - hue).abs() < 1e-6, "black keeps the previous hue");
    }
}
