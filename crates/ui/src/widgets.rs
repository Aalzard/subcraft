//! Shared building blocks: sections, property rows, icon / tool buttons, list rows and editors.

use egui::{Align, Color32, CornerRadius, Layout, Rect, Response, RichText, Sense, Stroke, Ui, UiBuilder, Vec2};
use subcraft_core::{ChannelKind, ChannelValue};

use crate::icons;
use crate::theme::{self, ACCENT, ACCENT_SOFT, BG_3, BG_4, BORDER, LABEL_WIDTH, PAD, ROW_HEIGHT, TEXT, TEXT_DIM, TEXT_FAINT};

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
    s.slider_width = (w - 64.0).max(60.0);
    s.combo_width = w.max(60.0);
    s.text_edit_width = w.max(60.0);
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
        fill_width(ui);
        let r = ui.add(egui::Slider::new(&mut x, range).max_decimals(3));
        if r.changed() {
            *v = ChannelValue::scalar(x);
        }
        Edit::from(&r)
    }
}

/// Panel body with the standard padding.
pub fn panel<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new().inner_margin(egui::Margin::same(PAD)).show(ui, |ui| {
        let key = ui.unique_id().with("row_bands");
        let pass = ui.ctx().cumulative_pass_nr();
        ui.data_mut(|data| data.insert_temp(key, PanelBands { pass, sections: 0, properties: 0 }));
        add(ui)
    }).inner
}

#[derive(Clone, Copy)]
struct PanelBands { pass: u64, sections: usize, properties: usize }

/// Find the containing panel through nested horizontal/enabled scopes; reset every paint pass.
fn next_band(ui: &Ui, section: bool) -> Option<Color32> {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|data| {
        for ancestor in ui.stack().iter() {
            let key = ancestor.id.with("row_bands");
            if let Some(mut bands) = data.get_temp::<PanelBands>(key).filter(|b| b.pass == pass) {
                let index = if section { &mut bands.sections } else { &mut bands.properties };
                let color = if section { theme::SECTION_BANDS[*index % 2] } else { theme::ROW_BANDS[*index % 2] };
                *index += 1;
                data.insert_temp(key, bands);
                return Some(color);
            }
        }
        None
    })
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
    ui.add_space(6.0);
    ui.label(caption(text));
    divider(ui);
}

/// Full-width hairline.
pub fn divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, BORDER));
}

/// Collapsible section: a full-width header row (caret + caption + optional right-side
/// widgets) and an indented-free body. Open state persists per `id`.
pub fn section(ui: &mut Ui, id: &str, title: &str, default_open: bool, body: impl FnOnce(&mut Ui)) {
    section_with(ui, id, title, default_open, |_| {}, body);
}

pub fn section_with(ui: &mut Ui, id: &str, title: &str, default_open: bool, right: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(("section", id));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    let open = state.is_open();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), Sense::click());
    if resp.clicked() {
        state.toggle(ui);
    }
    let painter = ui.painter();
    if let Some(color) = next_band(ui, true) {
        painter.rect_filled(rect, CornerRadius::ZERO, if resp.hovered() { BG_3 } else { color });
    }
    let fg = if resp.hovered() { TEXT } else { TEXT_DIM };
    let caret = if open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
    painter.text(rect.left_center() + egui::vec2(7.0, 0.0), egui::Align2::LEFT_CENTER, caret, theme::font(12.0), TEXT_FAINT);
    let icon = match title {
        "Tool" | "Brush" | "Paint layer" => Some(icons::PAINT_BRUSH),
        "Presets" => Some(icons::STACK),
        "Stencil" => Some(icons::IMAGE),
        "Material" | "Fill layer" => Some(icons::PAINT_BUCKET),
        "Jitter" => Some(icons::SPARKLE),
        "Stroke" | "Pen path" => Some(icons::CURVE),
        "Parts" => Some(icons::CUBE),
        "Mask" | "Paint target" => Some(icons::MASK),
        "Projection" => Some(icons::CUBE_WIRE),
        _ => None,
    };
    if let Some(icon) = icon {
        painter.text(rect.left_center() + egui::vec2(25.0, 0.0), egui::Align2::LEFT_CENTER, icon, theme::font(13.0), TEXT_DIM);
    }
    painter.text(rect.left_center() + egui::vec2(if icon.is_some() { 45.0 } else { 25.0 }, 0.0), egui::Align2::LEFT_CENTER, title.to_uppercase(), theme::font_strong(10.5), fg);
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, BORDER));
    // The header already owns this rectangle. A layout scope would advance the parent cursor
    // a second time (even when empty), allowing the first body widget to overlap the header.
    let mut actions = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(egui::vec2(0.0, 2.0))).layout(Layout::right_to_left(Align::Center)));
    right(&mut actions);
    state.store(ui.ctx());
    if open {
        ui.add_space(8.0);
        body(ui);
        ui.add_space(10.0);
    } else {
        ui.add_space(2.0);
    }
}

/// Labelled property row: fixed-width dim label, then the editor filling the rest.
pub fn prop_row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let color = next_band(ui, false);
    let background = ui.painter().add(egui::Shape::Noop);
    let row = ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.allocate_ui_with_layout(egui::vec2(LABEL_WIDTH, 22.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(LABEL_WIDTH);
            ui.add(egui::Label::new(RichText::new(label).color(TEXT_DIM)).truncate());
        });
        fill_width(ui);
        add(ui)
    });
    if let Some(color) = color {
        ui.painter().set(background, egui::Shape::rect_filled(row.response.rect, CornerRadius::ZERO, color));
    }
    row.inner
}

/// Slider property row.
pub fn slider_row(ui: &mut Ui, label: &str, slider: egui::Slider<'_>) -> Response {
    prop_row(ui, label, |ui| ui.add(slider))
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
    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
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
    ui.add(egui::Button::new(text).fill(fill).stroke(Stroke::NONE).min_size(egui::vec2(0.0, 28.0)))
}

/// Outline button (secondary action).
pub fn secondary_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(egui::Button::new(RichText::new(text.into()).color(TEXT_DIM)).fill(BG_3).stroke(Stroke::NONE).corner_radius(2).min_size(egui::vec2(0.0, 28.0)))
}

/// Calm secondary action: a compact icon, readable hit target, soft corners and no hard outline.
pub fn quiet_button(ui: &mut Ui, icon: &str, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(format!("{icon}  {text}")).font(theme::font(12.0)).color(TEXT_DIM))
        .fill(theme::BG_2).stroke(Stroke::NONE).corner_radius(2).min_size(egui::vec2(0.0, 28.0)))
}

/// Joined segmented control; returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (v, label) in options {
            let selected = *value == *v;
            let text = RichText::new(*label).color(if selected { ACCENT } else { TEXT_DIM });
            let b = egui::Button::new(text).fill(if selected { ACCENT_SOFT } else { BG_3 }).stroke(Stroke::NONE).min_size(egui::vec2(0.0, 26.0));
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
    ui.painter().rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, color.gamma_multiply(0.55)), egui::StrokeKind::Inside);
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
    let inner = rect.shrink2(egui::vec2(8.0, 0.0));
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        content(ui);
    });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
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
