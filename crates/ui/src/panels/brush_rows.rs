//! Rows of the Brush and Alpha blocks of the tool Properties: sliders with a pen-pressure toggle
//! at the right of the row, the Size Space dropdown and the alpha resource slot.

use egui::{Color32, Id, Response, RichText, Sense, Stroke, Ui};
use subcraft_paint::{BrushSettings, SizeSpace, MAX_PRESSURE_MIN};

use super::shelf::ShelfDrag;
use crate::state::UiState;
use crate::theme::{self, ACCENT, ACCENT_SOFT, BG_4, TEXT, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{hint, prop_row, slider_row, slider_row_with};

/// Stable id of the pressure toggle of a row (`size`, `flow`, `size_jitter`, ...).
pub fn pressure_toggle_id(key: &str) -> Id {
    Id::new(("pressure-toggle", key))
}

/// Stable ids of the alpha slot and its remove button.
pub fn alpha_slot_id() -> Id {
    Id::new("alpha-slot")
}
pub fn alpha_remove_id() -> Id {
    Id::new("alpha-slot-remove")
}

/// Small pen-pressure toggle: a pen drawn over a pressure wedge, accent when on.
fn pressure_toggle(ui: &mut Ui, key: &str, on: bool, what: &str) -> Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 18.0), Sense::hover());
    let resp = ui.interact(rect, pressure_toggle_id(key), Sense::click());
    let hovered = resp.hovered() && ui.is_enabled();
    let (bg, fg) = if on { (ACCENT_SOFT(), ACCENT()) } else if hovered { (BG_4(), TEXT()) } else { (Color32::TRANSPARENT, TEXT_FAINT()) };
    let p = ui.painter();
    p.rect_filled(rect, 3.0, bg);
    let c = rect.center();
    // Pen: a slanted barrel ending in a tip at the lower left.
    let tip = c + egui::vec2(-5.0, 5.0);
    p.line_segment([tip + egui::vec2(2.0, -2.0), c + egui::vec2(5.0, -5.0)], Stroke::new(2.4, fg));
    p.line_segment([tip, tip + egui::vec2(2.2, -2.2)], Stroke::new(1.0, fg));
    // Pressure wedge under the tip, growing to the right.
    p.add(egui::Shape::convex_polygon(vec![tip + egui::vec2(1.0, 2.0), tip + egui::vec2(11.0, 2.0), tip + egui::vec2(11.0, -1.0)], fg.gamma_multiply(0.75), Stroke::NONE));
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, format!("Pen pressure: {what}")));
    resp.on_hover_text(format!("Pen pressure controls {what} ({}). A mouse is unchanged", if on { "on" } else { "off" }))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Size, Flow (each with its pressure toggle and minimum) and Stroke opacity.
pub fn size_flow_opacity(ui: &mut Ui, s: &mut BrushSettings, brush: bool) {
    ui.add_enabled_ui(brush, |ui| {
        // Size: pen pressure toggle at the right of the row; the minimum appears while it is on.
        let mut toggle = false;
        let on = s.size_pressure();
        slider_row_with(ui, "Size", egui::Slider::new(&mut s.size, 0.1..=100.0).logarithmic(true).max_decimals(1).suffix("%"), |ui| {
            toggle = pressure_toggle(ui, "size", on, "size").clicked();
        })
        .on_hover_text(match s.size_space {
            SizeSpace::Object => "Brush diameter as a percentage of the model diagonal",
            SizeSpace::Viewport => "Brush diameter as a percentage of the viewport height",
            SizeSpace::Texture => "Brush diameter as a percentage of the texture",
        });
        let memory = Id::new("brush-min-size");
        if toggle {
            let min = ui.data(|d| d.get_temp::<f32>(memory)).unwrap_or(0.0);
            s.set_size_pressure(!on, min);
        }
        if s.size_pressure() {
            let mut min = s.min_size();
            if slider_row(ui, "Minimum Size", egui::Slider::new(&mut min, 0.0..=MAX_PRESSURE_MIN).max_decimals(2))
                .on_hover_text("Size at the lightest pen pressure, as a fraction of Size")
                .changed()
            {
                s.set_size_pressure(true, min);
            }
            ui.data_mut(|d| d.insert_temp(memory, s.min_size()));
        }

        let mut toggle = false;
        let on = s.flow_pressure();
        let mut shown = s.flow * 100.0;
        if slider_row_with(ui, "Flow", egui::Slider::new(&mut shown, 0.0..=100.0).max_decimals(0).suffix("%"), |ui| {
            toggle = pressure_toggle(ui, "flow", on, "flow").clicked();
        })
        .on_hover_text("Paint each stamp adds")
        .changed()
        {
            s.flow = (shown * 0.01).clamp(0.0, 1.0);
        }
        let memory = Id::new("brush-min-flow");
        if toggle {
            let min = ui.data(|d| d.get_temp::<f32>(memory)).unwrap_or(0.0);
            s.set_flow_pressure(!on, min);
        }
        if s.flow_pressure() {
            let mut min = s.min_flow();
            if slider_row(ui, "Minimum Flow", egui::Slider::new(&mut min, 0.0..=MAX_PRESSURE_MIN).max_decimals(2))
                .on_hover_text("Flow at the lightest pen pressure, as a fraction of Flow")
                .changed()
            {
                s.set_flow_pressure(true, min);
            }
            ui.data_mut(|d| d.insert_temp(memory, s.min_flow()));
        }
    });
    let mut shown = s.stroke_opacity * 100.0;
    if slider_row(ui, "Stroke opacity", egui::Slider::new(&mut shown, 0.0..=100.0).max_decimals(0).suffix("%"))
        .on_hover_text("The most opacity one stroke can reach, however many stamps overlap")
        .changed()
    {
        s.stroke_opacity = (shown * 0.01).clamp(0.0, 1.0);
    }
}

/// The four jitters, 0..1 each, with their pen-pressure toggles.
pub fn jitters(ui: &mut Ui, s: &mut BrushSettings) {
    let row = |ui: &mut Ui, label: &str, key: &str, value: &mut f32, pressure: &mut bool| {
        let on = *pressure;
        let mut toggle = false;
        // `Edits`: egui's default clamps the stored value every time the slider is drawn, so just
        // showing Properties cut the built-in Scatter Dots' Position Jitter of 1.5 down to 1.
        slider_row_with(ui, label, egui::Slider::new(value, 0.0..=1.0).max_decimals(2).clamping(egui::SliderClamping::Edits), |ui| {
            toggle = pressure_toggle(ui, key, on, &label.to_lowercase()).clicked();
        });
        if toggle {
            *pressure = !on;
        }
    };
    row(ui, "Size Jitter", "size_jitter", &mut s.size_jitter, &mut s.size_jitter_pressure);
    row(ui, "Flow Jitter", "flow_jitter", &mut s.flow_jitter, &mut s.flow_jitter_pressure);
    // Stored as ± degrees; 1 = any angle.
    let mut angle = (s.angle_jitter_deg / 180.0).clamp(0.0, 1.0);
    let before = angle;
    row(ui, "Angle Jitter", "angle_jitter", &mut angle, &mut s.angle_jitter_pressure);
    if angle != before {
        s.angle_jitter_deg = angle * 180.0;
    }
    row(ui, "Position Jitter", "position_jitter", &mut s.position_jitter, &mut s.position_jitter_pressure);
    prop_row(ui, "Position Jitter Axis", |ui| {
        use subcraft_paint::JitterAxis;
        egui::ComboBox::from_id_salt("position_jitter_axis").selected_text(s.position_jitter_axis.label()).show_ui(ui, |ui| {
            for axis in JitterAxis::ALL {
                ui.selectable_value(&mut s.position_jitter_axis, axis, axis.label()).on_hover_text(match axis {
                    JitterAxis::Both => "Stamps move any way around the stroke",
                    JitterAxis::One => "Stamps move only across the stroke and keep their spacing along it",
                });
            }
        });
    });
}

/// Size Space dropdown: what the Size percentage is measured against.
pub fn size_space(ui: &mut Ui, s: &mut BrushSettings) {
    prop_row(ui, "Size Space", |ui| {
        egui::ComboBox::from_id_salt("size_space").selected_text(s.size_space.label()).show_ui(ui, |ui| {
            for space in SizeSpace::ALL {
                ui.selectable_value(&mut s.size_space, space, space.label()).on_hover_text(match space {
                    SizeSpace::Object => "Size relative to the model: the brush keeps its size on the surface",
                    SizeSpace::Viewport => "Size relative to the viewport: the brush keeps its size on screen when you zoom (3D view)",
                    SizeSpace::Texture => "Size relative to the texture: the brush covers the same texels everywhere",
                });
            }
        });
    });
}

/// What the alpha slot asked for this frame.
enum SlotAction {
    Pick(usize),
    Remove,
}

/// Alpha block: the resource slot (thumbnail, name, remove cross). A click opens a small
/// browser of the tips; a tip dragged from the Shelf can be dropped on the slot.
pub fn alpha_slot(state: &mut UiState, ui: &mut Ui) {
    let name = state.session.brush.alpha_name.clone();
    let has_image = state.session.brush.alpha.is_some();
    let shown = name.strip_prefix("Starter: ").unwrap_or(&name).to_string();
    let thumb = super::shelf::tip_thumbnail(state, &name);
    let mut action = None;
    let side = 44.0;
    let row = ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
        let slot = ui.interact(rect, alpha_slot_id(), Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, 3.0, theme::FIELD());
        match thumb {
            Some(t) => {
                p.image(t, rect.shrink(1.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            // The procedural round tip (or a thumbnail not rendered yet): a soft disc.
            None => {
                p.circle_filled(rect.center(), side * 0.3, TEXT_DIM().gamma_multiply(0.6));
                p.circle_filled(rect.center(), side * 0.18, TEXT());
            }
        }
        let dropping = slot.dnd_hover_payload::<ShelfDrag>().is_some_and(|d| matches!(*d, ShelfDrag::Alpha(_)));
        let stroke = if dropping { Stroke::new(1.5, ACCENT()) } else if slot.hovered() { Stroke::new(1.0, theme::ACCENT_HOVER()) } else { Stroke::new(1.0, theme::BORDER_STRONG()) };
        p.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Inside);
        if let Some(d) = slot.dnd_release_payload::<ShelfDrag>()
            && let ShelfDrag::Alpha(i) = *d
        {
            action = Some(SlotAction::Pick(i));
        }
        let slot = slot.on_hover_text("Click to choose an alpha, or drop one from Shelf › Brushes").on_hover_cursor(egui::CursorIcon::PointingHand);
        egui::Popup::menu(&slot).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            if let Some(i) = mini_browser(state, ui, &name) {
                action = Some(SlotAction::Pick(i));
                ui.close();
            }
        });
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(&shown).color(TEXT())).truncate());
            ui.label(RichText::new(if has_image { "Bitmap alpha" } else { "Procedural round tip" }).small().color(TEXT_FAINT()));
        });
        if has_image {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(22.0), Sense::hover());
                let x = ui.interact(rect, alpha_remove_id(), Sense::click());
                let fg = if x.hovered() { TEXT() } else { TEXT_DIM() };
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, crate::icons::X, theme::font(13.0), fg);
                if x.on_hover_text("Remove the alpha (back to the round tip)").clicked() {
                    action = Some(SlotAction::Remove);
                }
            });
        }
    });
    let _ = row;
    let tip = match action {
        Some(SlotAction::Pick(i)) => state.library.alphas.get(i).cloned(),
        Some(SlotAction::Remove) => state.library.alphas.iter().find(|a| a.bitmap.is_none()).cloned(),
        None => None,
    };
    if let Some(tip) = tip {
        state.session.set_brush_alpha(&tip);
        state.selected_brush = None;
    }
}

/// Small asset browser of the brush tips: a search field over a grid. Returns the chosen index.
fn mini_browser(state: &mut UiState, ui: &mut Ui, current: &str) -> Option<usize> {
    ui.set_width(268.0);
    let search_id = Id::new("alpha-slot-search");
    let mut query = ui.data(|d| d.get_temp::<String>(search_id)).unwrap_or_default();
    ui.add(egui::TextEdit::singleline(&mut query).hint_text("Search alphas").desired_width(f32::INFINITY));
    ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
    let needle = query.trim().to_lowercase();
    let mut chosen = None;
    let mut shown = 0;
    egui::ScrollArea::vertical().id_salt("alpha-slot-browser").max_height(236.0).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (i, a) in state.library.alphas.iter().enumerate() {
                if a.name.starts_with("Starter: ") || (!needle.is_empty() && !a.name.to_lowercase().contains(&needle)) {
                    continue;
                }
                shown += 1;
                let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(48.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(rect, 2.0, theme::FIELD());
                match state.shelf.thumb(&a.id) {
                    Some(t) => {
                        p.image(t.id(), rect.shrink(1.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        let mut job = egui::text::LayoutJob::simple(a.name.clone(), theme::font(9.5), TEXT_DIM(), 44.0);
                        job.wrap.max_rows = 3;
                        let g = p.layout_job(job);
                        p.galley(rect.min + egui::vec2(2.0, 2.0), g, TEXT_DIM());
                    }
                }
                let selected = a.name == current;
                let stroke = if selected { Stroke::new(2.0, ACCENT()) } else if resp.hovered() { Stroke::new(1.0, theme::ACCENT_HOVER()) } else { Stroke::new(1.0, theme::BORDER()) };
                p.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
                if resp.on_hover_text(&a.name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    chosen = Some(i);
                }
            }
        });
    });
    if shown == 0 {
        hint(ui, "No alpha matches the search.");
    }
    chosen
}
