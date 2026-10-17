//! Compact viewport brush controls. Values are edits of the session's existing tool state.
use egui::{RichText, Ui};
use subcraft_paint::BrushSettings;

use crate::{
    state::Tool,
    theme::{self, TEXT_DIM},
};

pub(crate) fn active(tool: Tool) -> bool {
    matches!(tool, Tool::Brush | Tool::Eraser | Tool::PolygonFill)
}

/// Primary controls remain in the top bar. The menu offers them again when the row is tiny.
pub(crate) fn show(ui: &mut Ui, tool: &mut Tool, settings: &mut BrushSettings, width: f32) {
    ui.spacing_mut().item_spacing.x = 7.0;
    let collapsed = width < 500.0;
    let sliders = width >= 650.0;
    ui.menu_button(
        match tool {
            Tool::Eraser => "Eraser",
            Tool::PolygonFill => "Fill",
            _ => "Brush",
        },
        |ui| {
            for (value, label) in [
                (Tool::Brush, "Brush"),
                (Tool::Eraser, "Eraser"),
                (Tool::PolygonFill, "Fill"),
            ] {
                if ui.selectable_value(tool, value, label).clicked() {
                    ui.close();
                }
            }
            if collapsed {
                ui.separator();
                menu_controls(ui, settings);
            }
        },
    )
    .response
    .on_hover_text("Paint tool · drag number boxes, or double-click to type");
    if collapsed {
        if width >= 190.0 {
            size(ui, &mut settings.size, false);
        }
        if width >= 310.0 {
            percent(
                ui,
                "Opacity",
                &mut settings.stroke_opacity,
                false,
                "Maximum opacity of the entire stroke",
            );
        }
    } else {
        ui.add_enabled_ui(*tool != Tool::PolygonFill, |ui| {
            size(ui, &mut settings.size, sliders)
        });
        ui.add_enabled_ui(*tool != Tool::PolygonFill, |ui| {
            percent(
                ui,
                "Flow",
                &mut settings.flow,
                false,
                "Paint deposited by each stamp",
            )
        });
        percent(
            ui,
            "Opacity",
            &mut settings.stroke_opacity,
            false,
            "Maximum opacity of the entire stroke",
        );
        ui.add_enabled_ui(*tool != Tool::PolygonFill, |ui| {
            percent(
                ui,
                "Hardness",
                &mut settings.hardness,
                sliders,
                "Hard center before the tip's soft falloff",
            )
        });
    }
}

fn menu_controls(ui: &mut Ui, s: &mut BrushSettings) {
    ui.horizontal(|ui| size(ui, &mut s.size, false));
    ui.horizontal(|ui| {
        percent(
            ui,
            "Flow",
            &mut s.flow,
            false,
            "Paint deposited by each stamp",
        )
    });
    ui.horizontal(|ui| {
        percent(
            ui,
            "Opacity",
            &mut s.stroke_opacity,
            false,
            "Maximum opacity of the entire stroke",
        )
    });
    ui.horizontal(|ui| percent(ui, "Hardness", &mut s.hardness, false, "Tip falloff"));
}

fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::font(11.5)).color(TEXT_DIM));
}

fn size(ui: &mut Ui, value: &mut f32, slider: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        label(ui, "Size");
        if slider {
            ui.spacing_mut().slider_width = 54.0;
            ui.add(
                egui::Slider::new(value, 0.1..=100.0)
                    .logarithmic(true)
                    .show_value(false),
            )
            .on_hover_text("Brush diameter as a percentage of the model diagonal");
        }
        ui.add_sized(
            [54.0, 22.0],
            egui::DragValue::new(value)
                .speed(0.1)
                .range(0.1..=100.0)
                .suffix("%")
                .max_decimals(1),
        )
        .on_hover_text("Brush size · drag left/right or double-click to type");
    });
}

fn percent(ui: &mut Ui, text: &str, value: &mut f32, slider: bool, help: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        label(ui, text);
        if slider {
            ui.spacing_mut().slider_width = 48.0;
            ui.add(egui::Slider::new(value, 0.0..=1.0).show_value(false))
                .on_hover_text(help);
        }
        let mut display = *value * 100.0;
        if ui
            .add_sized(
                [48.0, 22.0],
                egui::DragValue::new(&mut display)
                    .speed(0.5)
                    .range(0.0..=100.0)
                    .suffix("%")
                    .max_decimals(0),
            )
            .on_hover_text(format!("{help} · drag left/right or double-click to type"))
            .changed()
        {
            *value = display * 0.01;
        }
    });
}
