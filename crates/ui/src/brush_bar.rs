//! Compact viewport brush controls, editing the session's existing tool state.
use egui::{RichText, Ui};
use subcraft_paint::{BrushSettings, StrokeOptions};

use crate::{
    icons,
    state::Tool,
    theme::{self, ACCENT, BORDER_STRONG, TEXT, TEXT_DIM},
    widgets,
};

pub(crate) fn active(tool: Tool) -> bool {
    matches!(tool, Tool::Brush | Tool::Eraser | Tool::PolygonFill)
}

/// Icons select tools directly; scrubbable values and slider popovers keep the row quiet.
pub(crate) fn show(
    ui: &mut Ui,
    tool: &mut Tool,
    settings: &mut BrushSettings,
    options: &mut StrokeOptions,
    guides: &mut bool,
    width: f32,
) {
    ui.spacing_mut().item_spacing.x = 9.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (value, icon, help) in [
            (Tool::Brush, icons::PAINT_BRUSH, "Brush (B)"),
            (Tool::Eraser, icons::ERASER, "Eraser (E)"),
            (Tool::PolygonFill, icons::PAINT_BUCKET, "Polygon Fill"),
        ] {
            let response = widgets::icon_toggle(ui, icon, *tool == value, help);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    *tool == value,
                    help,
                )
            });
            if response.clicked() {
                *tool = value;
            }
        }
    });
    stroke_options(ui, options, guides);
    let brush = *tool != Tool::PolygonFill;
    if width < 520.0 {
        let response = widgets::icon_button(ui, icons::SLIDERS, "Brush settings");
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| menu_controls(ui, settings, brush));
        if width >= 250.0 {
            ui.add_enabled_ui(brush, |ui| size(ui, &mut settings.size));
        }
        if width >= 380.0 {
            percent(ui, "Opacity", &mut settings.stroke_opacity);
        }
    } else {
        ui.add_enabled_ui(brush, |ui| size(ui, &mut settings.size));
        ui.add_enabled_ui(brush, |ui| {
            popup_percent(ui, "Flow", &mut settings.flow, "Paint per stamp", false)
        });
        percent(ui, "Opacity", &mut settings.stroke_opacity);
        ui.add_enabled_ui(brush, |ui| {
            popup_percent(ui, "Hardness", &mut settings.hardness, "Tip edge", true)
        });
    }
}

fn stroke_options(ui: &mut Ui, options: &mut StrokeOptions, guides: &mut bool) {
    let on = options.symmetry.iter().any(|&axis| axis) || options.lazy_distance_px > 0.0;
    let response = widgets::icon_toggle(
        ui,
        icons::SYMMETRY,
        on,
        "Symmetry & Lazy Mouse · click for stroke options",
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, "Symmetry & Lazy Mouse")
    });
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(220.0);
            ui.label(RichText::new("Mirror symmetry").color(TEXT_DIM));
            ui.horizontal(|ui| {
                for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                    let value = &mut options.symmetry[i];
                    if ui
                        .selectable_label(*value, axis)
                        .on_hover_text(format!("Mirror across {axis} through the model center"))
                        .clicked()
                    {
                        *value = !*value;
                    }
                }
            });
            ui.checkbox(guides, "Show symmetry guides");
            ui.separator();
            ui.label(RichText::new("Lazy Mouse").color(TEXT_DIM));
            ui.spacing_mut().slider_width = 155.0;
            ui.add(egui::Slider::new(&mut options.lazy_distance_px, 0.0..=100.0).suffix(" px"));
            ui.label(
                RichText::new("0 px = off · X shortcut toggles mirror X")
                    .small()
                    .color(TEXT_DIM),
            );
        });
}

fn menu_controls(ui: &mut Ui, s: &mut BrushSettings, brush: bool) {
    ui.add_enabled_ui(brush, |ui| {
        size(ui, &mut s.size);
        ui.horizontal(|ui| percent(ui, "Flow", &mut s.flow));
        ui.horizontal(|ui| percent(ui, "Hardness", &mut s.hardness));
    });
    ui.horizontal(|ui| percent(ui, "Opacity", &mut s.stroke_opacity));
}

fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::font(11.5)).color(TEXT_DIM));
}

/// Only the resting drag button is frameless; text-entry mode retains a readable field.
fn scrub(ui: &mut Ui, value: &mut f32, size: bool) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for visuals in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            visuals.weak_bg_fill = egui::Color32::TRANSPARENT;
            visuals.bg_fill = egui::Color32::TRANSPARENT;
            visuals.bg_stroke = egui::Stroke::NONE;
        }
        ui.visuals_mut().override_text_color = Some(if size { ACCENT } else { TEXT });
        ui.spacing_mut().button_padding.x = 1.0;
        ui.spacing_mut().interact_size.x = 36.0;
        ui.add(
            egui::DragValue::new(value)
                .speed(if size { 0.1 } else { 0.5 })
                .range(if size { 0.1..=100.0 } else { 0.0..=100.0 })
                .suffix("%")
                .max_decimals(usize::from(size)),
        )
    })
    .inner
    .on_hover_text("Drag left/right to adjust · Shift for precision · click to type")
}

fn size(ui: &mut Ui, value: &mut f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        label(ui, "Size");
        scrub(ui, value, true)
            .on_hover_text("Brush diameter as a percentage of the model diagonal");
    });
}

fn percent(ui: &mut Ui, text: &str, value: &mut f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        label(ui, text);
        let mut display = *value * 100.0;
        if scrub(ui, &mut display, false).changed() {
            *value = display * 0.01;
        }
    });
}

fn popup_percent(ui: &mut Ui, name: &str, value: &mut f32, help: &str, tip: bool) {
    // Each popover has its own stable ID: changing its numeric label must not close it.
    ui.push_id(name, |ui| {
        let font = theme::font(11.5);
        let label = ui
            .painter()
            .layout_no_wrap(name.into(), font.clone(), TEXT_DIM);
        let number = ui
            .painter()
            .layout_no_wrap(format!("{:.0}%", *value * 100.0), font, TEXT);
        // Reserve maximum-width values so the row doesn't jump while scrubbing the slider.
        let width = label.size().x + 47.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::click());
        let enabled = ui.is_enabled();
        let painter = ui.painter();
        let color = if response.hovered() && enabled {
            ACCENT
        } else {
            TEXT_DIM
        };
        painter.galley(
            egui::pos2(rect.left(), rect.center().y - label.size().y / 2.0),
            label,
            TEXT_DIM,
        );
        painter.galley(
            egui::pos2(
                rect.right() - 12.0 - number.size().x,
                rect.center().y - number.size().y / 2.0,
            ),
            number,
            TEXT,
        );
        if tip {
            let center = egui::pos2(rect.right() - 4.0, rect.center().y);
            painter.circle_stroke(center, 4.0, egui::Stroke::new(1.0, color));
            painter.circle_filled(center, *value * 3.5, color);
        } else {
            let y = rect.bottom() - 3.0;
            painter.hline(rect.x_range(), y, egui::Stroke::new(2.0, BORDER_STRONG));
            painter.hline(
                rect.left()..=rect.left() + rect.width() * *value,
                y,
                egui::Stroke::new(2.0, ACCENT),
            );
            painter.text(
                egui::pos2(rect.right() - 4.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                icons::CARET_DOWN,
                theme::font(10.0),
                color,
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                enabled,
                format!("{name} {:.0}%", *value * 100.0),
            )
        });
        let response = response
            .on_hover_text(format!("{help} · click for slider"))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.label(RichText::new(help).color(TEXT_DIM));
                ui.spacing_mut().slider_width = 160.0;
                let mut display = *value * 100.0;
                if ui
                    .add(egui::Slider::new(&mut display, 0.0..=100.0).suffix("%"))
                    .changed()
                {
                    *value = display * 0.01;
                }
            });
    });
}
