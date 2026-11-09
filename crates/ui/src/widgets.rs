//! Shared building blocks: sections, property rows, icon / tool buttons, list rows and editors.

use egui::{Align, Color32, CornerRadius, Layout, Rect, Response, RichText, Sense, Stroke, Ui, UiBuilder, Vec2};
use subcraft_core::{ChannelKind, ChannelValue};

use crate::icons;
use crate::theme::{self, ACCENT, ACCENT_SOFT, BG_3, BG_4, BORDER_SOFT, CONTROL_H, LABEL_SIZE, LABEL_WIDTH, PAD, RADIUS, RAIL, ROW_HEIGHT, SP_M, SP_S, SP_XS, TEXT, TEXT_DIM, TEXT_FAINT, VALUE_W};

/// Result of an editor: `changed` this frame, `done` when an interaction finished.
#[derive(Clone, Copy, Default)]
pub struct Edit {
    pub changed: bool,
    pub done: bool,
}

impl Edit {
    pub fn from(r: &egui::Response) -> Self {
        Self { changed: r.changed(), done: r.drag_stopped() || r.lost_focus() }
    }
    pub fn or(self, o: Edit) -> Edit {
        Edit { changed: self.changed || o.changed, done: self.done || o.done }
    }
}

/// Make sliders, combo boxes and text fields in `ui` stretch to the available width.
pub fn fill_width(ui: &mut Ui) {
    let w = ui.available_width();
    let s = ui.spacing_mut();
    s.slider_width = (w - VALUE_W - s.item_spacing.x).max(48.0);
    s.combo_width = w.max(60.0);
    s.text_edit_width = w.max(60.0);
}

/// Make the numeric field next to a slider a quiet, fixed-width inset instead of a raised button.
fn quiet_value_field(ui: &mut Ui) {
    ui.spacing_mut().interact_size.x = VALUE_W;
    // The theme's roomy button padding would make the box wider than VALUE_W and push sliders past the panel edge.
    ui.spacing_mut().button_padding.x = 4.0;
    let w = &mut ui.visuals_mut().widgets;
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
        state.weak_bg_fill = RAIL;
    }
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
}

/// Edit a channel value: linear color picker for color channels, encoded RGB for normals,
/// a slider for scalars (Height is signed).
pub fn channel_value(ui: &mut Ui, kind: ChannelKind, v: &mut ChannelValue) -> Edit {
    if kind.components() == 3 {
        let r = ui.color_edit_button_rgb(&mut v.0);
        Edit::from(&r)
    } else {
        let range = if kind.is_signed() { -1.0..=1.0 } else { 0.0..=1.0 };
        let mut x = v.0[0];
        let r = ui
            .scope(|ui| {
                quiet_value_field(ui);
                fill_width(ui);
                ui.add(egui::Slider::new(&mut x, range).max_decimals(3))
            })
            .inner;
        if r.changed() {
            *v = ChannelValue::scalar(x);
        }
        Edit::from(&r)
    }
}

/// Panel body with the standard padding.
pub fn panel<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new().inner_margin(egui::Margin::same(PAD)).show(ui, add).inner
}

/// Small uppercase caption.
pub fn caption(text: &str) -> RichText {
    RichText::new(text.to_uppercase()).font(theme::font_strong(10.5)).color(TEXT_FAINT)
}

/// Faint helper text, wrapped.
pub fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(TEXT_FAINT)).wrap());
}

/// Static section header: caption with a hairline under it.
pub fn header(ui: &mut Ui, text: &str) {
    ui.add_space(SP_XS);
    ui.label(caption(text));
    divider(ui);
}

/// Full-width hairline.
pub fn divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, BORDER_SOFT));
}

/// Height of a section header row.
pub const SECTION_HEIGHT: f32 = 32.0;

/// Collapsible section: a full-width header row (caret + title + optional right-side
/// widgets) with a soft separator under it. Open state persists per `id`.
pub fn section(ui: &mut Ui, id: &str, title: &str, default_open: bool, body: impl FnOnce(&mut Ui)) {
    section_with(ui, id, title, default_open, |_| {}, body);
}

pub fn section_with(ui: &mut Ui, id: &str, title: &str, default_open: bool, right: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(("section", id));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    let open = state.is_open();
    // Group gap: sections are further apart than rows.
    ui.add_space(SP_XS);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), SECTION_HEIGHT), Sense::click());
    if resp.clicked() {
        state.toggle(ui);
    }
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(RADIUS), theme::BG_2);
    }
    let fg = if resp.hovered() { TEXT } else { TEXT_DIM };
    let caret = if open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
    painter.text(rect.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, caret, theme::font(12.0), TEXT_FAINT);
    // Painter-style: bold capitals, no icon.
    painter.text(rect.left_center() + egui::vec2(27.0, 0.0), egui::Align2::LEFT_CENTER, title.to_uppercase(), theme::font_strong(11.5), fg);
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, BORDER_SOFT));
    // The header already owns this rectangle. A layout scope would advance the parent cursor
    // a second time (even when empty), allowing the first body widget to overlap the header.
    let mut actions = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(egui::vec2(4.0, 2.0))).layout(Layout::right_to_left(Align::Center)));
    right(&mut actions);
    state.store(ui.ctx());
    if open {
        ui.add_space(SP_XS);
        body(ui);
        ui.add_space(SP_M);
    }
}

/// Dimmed, small row label.
pub fn row_label(text: &str) -> RichText {
    RichText::new(text).font(theme::font(LABEL_SIZE)).color(TEXT_DIM)
}

/// Labelled property row: a dimmed label column, then the editor filling the rest at a
/// consistent control height. The label column shrinks in narrow panels.
pub fn prop_row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    // Narrow panels: the label goes on its own line so the control keeps the full width.
    if ui.available_width() < STACK_BELOW && !label.is_empty() {
        return ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.y = SP_XS;
                ui.add(egui::Label::new(row_label(label)).truncate());
                fill_width(ui);
                ui.horizontal(|ui| {
                    ui.set_min_height(CONTROL_H);
                    add(ui)
                })
                .inner
            })
            .inner;
    }
    let label_w = (ui.available_width() * 0.38).clamp(56.0, LABEL_WIDTH);
    ui.horizontal(|ui| {
        ui.set_min_height(CONTROL_H);
        ui.allocate_ui_with_layout(egui::vec2(label_w, CONTROL_H), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(label_w);
            ui.add(egui::Label::new(row_label(label)).truncate());
        });
        fill_width(ui);
        add(ui)
    })
    .inner
}

/// Below this panel width, property labels stack above their controls.
const STACK_BELOW: f32 = 300.0;

/// Slider property row: label on its own line, the slider beneath at full width with a quiet
/// numeric field at the right edge.
pub fn slider_row(ui: &mut Ui, label: &str, slider: egui::Slider<'_>) -> Response {
    slider_row_with(ui, label, slider, |_| {})
}

/// Like [`slider_row`], with small secondary actions (Image..., reset) aligned to the right end of the
/// label line. `actions` runs in a right-to-left layout.
pub fn slider_row_with(ui: &mut Ui, label: &str, slider: egui::Slider<'_>, actions: impl FnOnce(&mut Ui)) -> Response {
    slider_row_boxed(ui, label, slider, VALUE_W, actions)
}

/// [`slider_row_with`] for sliders whose number carries a long suffix ("% width"): the value box is `box_w` wide
/// and the slider shrinks to match, so the row still fits its container.
pub fn slider_row_boxed(ui: &mut Ui, label: &str, slider: egui::Slider<'_>, box_w: f32, actions: impl FnOnce(&mut Ui)) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = SP_XS;
        quiet_value_field(ui);
        if !label.is_empty() {
            let w = ui.available_width();
            ui.allocate_ui_with_layout(egui::vec2(w, 18.0), Layout::right_to_left(Align::Center), |ui| {
                actions(ui);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| ui.add(egui::Label::new(row_label(label)).truncate()));
            });
        }
        fill_width(ui);
        if box_w != VALUE_W {
            let sp = ui.spacing().item_spacing.x;
            let w = ui.available_width();
            ui.spacing_mut().interact_size.x = box_w;
            ui.spacing_mut().slider_width = (w - box_w - sp).max(48.0);
        }
        ui.add(slider)
    })
    .inner
}

/// A small outlined toggle chip (channel toggles). `selected` shows the accent outline.
pub fn chip(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::font(12.0), TEXT);
    let size = egui::vec2((galley.size().x + 18.0).max(36.0), 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered() && ui.is_enabled();
    let (fill, stroke, fg) = if selected {
        (ACCENT_SOFT, ACCENT.gamma_multiply(0.75), TEXT)
    } else if hovered {
        (BG_3, theme::BORDER_STRONG, TEXT)
    } else {
        (Color32::TRANSPARENT, theme::BORDER_STRONG.gamma_multiply(0.8), TEXT_DIM)
    };
    ui.painter().rect(rect, CornerRadius::same(RADIUS), fill, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    let color = if ui.is_enabled() { fg } else { TEXT_FAINT };
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Square frameless icon button.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    icon_toggle(ui, icon, false, tooltip)
}

/// Square icon button that shows a selected state in the accent color.
pub fn icon_toggle(ui: &mut Ui, icon: &str, selected: bool, tooltip: &str) -> Response {
    let size = Vec2::splat(26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let (bg, fg) = if selected {
        (ACCENT_SOFT, ACCENT)
    } else if resp.hovered() && enabled {
        (BG_4, TEXT)
    } else {
        (Color32::TRANSPARENT, if enabled { TEXT_DIM } else { TEXT_FAINT.gamma_multiply(0.6) })
    };
    ui.painter().rect_filled(rect, CornerRadius::same(RADIUS), bg);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, theme::font(15.0), fg);
    let resp = resp.on_hover_text(tooltip);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// Large tool button for the tool rail: accent bar on the left when selected.
pub fn tool_button(ui: &mut Ui, icon: &str, selected: bool, tooltip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::ZERO, ACCENT_SOFT);
        painter.rect_filled(Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())), CornerRadius::ZERO, ACCENT);
    } else if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, BG_3);
    }
    let fg = if selected { ACCENT } else if resp.hovered() { TEXT } else { TEXT_DIM };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, icon, theme::font(19.0), fg);
    resp.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Filled accent button for the main action of an area.
pub fn primary_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    let text = RichText::new(text.into()).font(theme::font_strong(13.0)).color(Color32::from_rgb(24, 14, 8));
    let fill = if ui.is_enabled() { ACCENT } else { BG_3 };
    ui.add(egui::Button::new(text).fill(fill).stroke(Stroke::NONE).corner_radius(RADIUS).min_size(egui::vec2(0.0, 28.0)))
}

/// Outline button (secondary action).
pub fn secondary_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(egui::Button::new(RichText::new(text.into()).color(TEXT_DIM)).fill(BG_3).stroke(Stroke::NONE).corner_radius(RADIUS).min_size(egui::vec2(0.0, 28.0)))
}

/// Calm secondary action: a compact icon, readable hit target, soft corners and no hard outline.
pub fn quiet_button(ui: &mut Ui, icon: &str, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(format!("{icon}  {text}")).font(theme::font(12.0)).color(TEXT_DIM))
        .fill(theme::BG_2).stroke(Stroke::NONE).corner_radius(RADIUS).min_size(egui::vec2(0.0, 28.0)))
}

/// Joined segmented control with rounded outer corners; returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    let last = options.len().saturating_sub(1);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (i, (v, label)) in options.iter().enumerate() {
            let selected = *value == *v;
            let text = RichText::new(*label).color(if selected { ACCENT } else { TEXT_DIM });
            let r = RADIUS;
            let radius = CornerRadius { nw: if i == 0 { r } else { 0 }, sw: if i == 0 { r } else { 0 }, ne: if i == last { r } else { 0 }, se: if i == last { r } else { 0 } };
            let b = egui::Button::new(text).fill(if selected { ACCENT_SOFT } else { BG_3 }).stroke(Stroke::NONE).corner_radius(radius).min_size(egui::vec2(0.0, CONTROL_H));
            if ui.add(b).clicked() && !selected {
                *value = *v;
                changed = true;
            }
        }
    });
    changed
}

/// Small rectangular tag.
pub fn badge(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_uppercase(), theme::font_strong(9.5), color);
    let size = galley.size() + egui::vec2(10.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, color.gamma_multiply(0.55)), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// A full-width selectable list row. `content` lays out left-to-right inside the row; widgets
/// added there take clicks before the row does.
pub fn list_row(ui: &mut Ui, selected: bool, content: impl FnOnce(&mut Ui)) -> Response {
    list_row_sense(ui, selected, Sense::click(), content)
}

/// A list row that can also accept a drag, while its child buttons keep their own clicks.
pub fn list_row_sense(ui: &mut Ui, selected: bool, sense: Sense, content: impl FnOnce(&mut Ui)) -> Response {
    list_row_background(ui, selected, sense, None, content)
}

/// Full-width list row with stable visible-order striping. Selection/hover take priority.
pub fn striped_list_row(ui: &mut Ui, index: usize, selected: bool, sense: Sense, content: impl FnOnce(&mut Ui)) -> Response {
    striped_list_row_height(ui, index, selected, ROW_HEIGHT, sense, content)
}

/// A shared selection/hover contract with heights suited to material and effect rows.
pub fn striped_list_row_height(ui: &mut Ui, index: usize, selected: bool, height: f32, sense: Sense, content: impl FnOnce(&mut Ui)) -> Response {
    list_row_sized(ui, selected, sense, Some(theme::ROW_BANDS[index % 2]), height, content)
}

/// A list row with its own band color and height (folder rows, compact sub-rows).
pub fn tree_row(ui: &mut Ui, height: f32, band: Option<Color32>, selected: bool, sense: Sense, content: impl FnOnce(&mut Ui)) -> Response {
    list_row_sized(ui, selected, sense, band, height, content)
}

fn list_row_background(ui: &mut Ui, selected: bool, sense: Sense, band: Option<Color32>, content: impl FnOnce(&mut Ui)) -> Response {
    list_row_sized(ui, selected, sense, band, ROW_HEIGHT, content)
}

fn list_row_sized(ui: &mut Ui, selected: bool, sense: Sense, band: Option<Color32>, height: f32, content: impl FnOnce(&mut Ui)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), sense);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::ZERO, ACCENT_SOFT);
        painter.rect_filled(Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())), CornerRadius::ZERO, ACCENT);
    } else if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, if band.is_some() { BG_3 } else { theme::BG_2 });
    } else if let Some(color) = band {
        painter.rect_filled(rect, CornerRadius::ZERO, color);
    }
    if band.is_some() {
        // Painter splits stacked rows with a dark hairline instead of striping them.
        painter.hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, theme::ROW_SEPARATOR));
    }
    let inner = rect.shrink2(egui::vec2(8.0, 0.0));
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        content(ui);
    });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Fixed footer beneath a scrollable asset/layer list. Size is a UI preference, not document data.
pub fn preview_size_footer(ui: &mut Ui, value: &mut u32, range: std::ops::RangeInclusive<u32>) -> Response {
    *value = (*value).clamp(*range.start(), *range.end());
    divider(ui);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Preview size").font(theme::font(10.5)).color(TEXT_DIM));
            ui.spacing_mut().slider_width = (ui.available_width() - 36.0).clamp(36.0, 110.0);
            let response = ui.add(egui::Slider::new(value, range).show_value(false));
            ui.label(RichText::new(format!("{}", *value)).small().color(TEXT_FAINT));
            response.on_hover_text("Preview size in UI points; texture resolution stays unchanged")
        }).inner
    }).inner
}

/// Text that is dim unless `strong`.
pub fn row_text(text: &str, strong: bool) -> RichText {
    if strong { RichText::new(text).color(TEXT) } else { RichText::new(text).color(TEXT_DIM) }
}

/// Empty-state message centered in the remaining space.
pub fn empty_state(ui: &mut Ui, icon: &str, text: &str) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon).font(theme::font(28.0)).color(TEXT_FAINT));
        ui.add_space(4.0);
        ui.label(RichText::new(text).color(TEXT_FAINT));
    });
}

/// A card for one stacked block (a channel, a mask effect): a soft raised surface with a faint
/// outline and a group gap above it. Returns the card's response; the accent bar is painted for you.
pub fn card<R>(ui: &mut Ui, accent: egui::Color32, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    ui.add_space(SP_S);
    let response = egui::Frame::new()
        .fill(crate::theme::CARD)
        .stroke(egui::Stroke::new(1.0, crate::theme::CARD_BORDER))
        .corner_radius(6)
        .inner_margin(egui::Margin { left: 16, right: 12, top: 10, bottom: 12 })
        .show(ui, add);
    card_accent(ui, response.response.rect, accent);
    response
}

/// The coloured bar on a card's left edge.
pub fn card_accent(ui: &Ui, rect: egui::Rect, color: egui::Color32) {
    let bar = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 4.0, rect.max.y));
    ui.painter().rect_filled(bar, egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, color);
}

/// Header line of a card: name on the left, a thin rule under it.
pub fn card_header_rule(ui: &mut Ui) {
    ui.add(egui::Separator::default().spacing(SP_S));
}

/// A window that can never grow past the screen: its width is capped (long text wraps) and its
/// height is capped with a vertical scroll, so buttons and titles cannot end up off-screen.
pub fn window<'a>(ctx: &egui::Context, title: impl Into<egui::WidgetText>) -> egui::Window<'a> {
    let screen = ctx.content_rect();
    let (max_w, max_h) = window_limits(screen.size());
    egui::Window::new(title).max_width(max_w).max_height(max_h)
}

/// Largest window size for a screen of this size.
pub fn window_limits(screen: Vec2) -> (f32, f32) {
    ((screen.x - 24.0).max(280.0), (screen.y - 48.0).max(200.0))
}
