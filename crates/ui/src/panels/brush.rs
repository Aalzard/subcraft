//! Tool and brush settings, stroke options and the brush material.

use egui::{RichText, Ui};
use subcraft_paint::{Alignment, PaintTarget, PolygonFillMode};

use crate::state::{Tool, UiState};
use crate::widgets::{channel_value, header};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    egui::ScrollArea::vertical().id_salt("brush").show(ui, |ui| {
        header(ui, "Tool");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.tool, Tool::Brush, "Brush").on_hover_text("B");
            ui.selectable_value(&mut state.tool, Tool::Eraser, "Eraser").on_hover_text("E");
            ui.selectable_value(&mut state.tool, Tool::PolygonFill, "Polygon fill");
        });
        if state.tool == Tool::PolygonFill {
            ui.horizontal(|ui| {
                for (m, n) in [(PolygonFillMode::Triangle, "Triangle"), (PolygonFillMode::MeshElement, "Mesh part"), (PolygonFillMode::UvIsland, "UV island")] {
                    ui.selectable_value(&mut state.fill_mode, m, n);
                }
            });
        }
        let b = &mut state.session.brush;
        ui.horizontal(|ui| {
            ui.label("Target");
            ui.selectable_value(&mut b.target, PaintTarget::Content, "Layer");
            ui.selectable_value(&mut b.target, PaintTarget::MaskBase, "Mask");
        });

        header(ui, "Brush");
        ui.label(format!("Tip: {}  (change in Shelf > Brushes)", b.alpha_name));
        let s = &mut b.settings;
        egui::Grid::new("brush_grid").num_columns(2).show(ui, |ui| {
            ui.label("Size");
            ui.add(egui::Slider::new(&mut s.size, 0.1..=50.0).logarithmic(true).suffix(" %"));
            ui.end_row();
            ui.label("Flow");
            ui.add(egui::Slider::new(&mut s.flow, 0.01..=1.0));
            ui.end_row();
            ui.label("Opacity");
            ui.add(egui::Slider::new(&mut s.stroke_opacity, 0.0..=1.0));
            ui.end_row();
            ui.label("Hardness");
            ui.add(egui::Slider::new(&mut s.hardness, 0.0..=1.0));
            ui.end_row();
            ui.label("Spacing");
            ui.add(egui::Slider::new(&mut s.spacing, 0.02..=2.0).logarithmic(true));
            ui.end_row();
            ui.label("Angle");
            ui.add(egui::Slider::new(&mut s.angle_deg, -180.0..=180.0).suffix("°"));
            ui.end_row();
            ui.label("Roundness");
            ui.add(egui::Slider::new(&mut s.roundness, 0.05..=1.0));
            ui.end_row();
            ui.label("Alignment");
            egui::ComboBox::from_id_salt("align")
                .selected_text(match s.alignment {
                    Alignment::TangentWrap => "Tangent | Wrap",
                    Alignment::TangentPlanar => "Tangent | Planar",
                    Alignment::Camera => "Camera",
                    Alignment::Uv => "UV",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.alignment, Alignment::TangentWrap, "Tangent | Wrap");
                    ui.selectable_value(&mut s.alignment, Alignment::TangentPlanar, "Tangent | Planar");
                    ui.selectable_value(&mut s.alignment, Alignment::Camera, "Camera");
                    ui.selectable_value(&mut s.alignment, Alignment::Uv, "UV");
                });
            ui.end_row();
        });
        ui.checkbox(&mut s.follow_path, "Follow path");
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.backface_culling, "Backface culling");
            ui.add_enabled(s.backface_culling, egui::Slider::new(&mut s.backface_angle_deg, 10.0..=90.0).suffix("°"));
        });
        egui::CollapsingHeader::new("Jitter").show(ui, |ui| {
            egui::Grid::new("jitter").num_columns(2).show(ui, |ui| {
                ui.label("Size");
                ui.add(egui::Slider::new(&mut s.size_jitter, 0.0..=1.0));
                ui.end_row();
                ui.label("Flow");
                ui.add(egui::Slider::new(&mut s.flow_jitter, 0.0..=1.0));
                ui.end_row();
                ui.label("Angle");
                ui.add(egui::Slider::new(&mut s.angle_jitter_deg, 0.0..=180.0));
                ui.end_row();
                ui.label("Position");
                ui.add(egui::Slider::new(&mut s.position_jitter, 0.0..=2.0));
                ui.end_row();
            });
        });

        header(ui, "Stroke");
        let o = &mut b.options;
        ui.horizontal(|ui| {
            ui.label("Symmetry");
            ui.toggle_value(&mut o.symmetry[0], "X").on_hover_text("X key");
            ui.toggle_value(&mut o.symmetry[1], "Y");
            ui.toggle_value(&mut o.symmetry[2], "Z");
        });
        ui.horizontal(|ui| {
            ui.label("Lazy mouse");
            ui.add(egui::Slider::new(&mut o.lazy_distance_px, 0.0..=100.0).suffix(" px"));
        });

        header(ui, "Material");
        if b.target == PaintTarget::MaskBase {
            ui.horizontal(|ui| {
                ui.label("Mask value");
                ui.add(egui::Slider::new(&mut b.material.mask_value, 0.0..=1.0));
            });
        } else {
            let channels = state.session.channels();
            let mat = &mut state.session.brush.material;
            for k in channels {
                let entry = mat.channels.entry(k).or_insert(subcraft_paint::BrushChannel { enabled: false, value: k.default_paint_value() });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut entry.enabled, "");
                    ui.add_sized([90.0, 18.0], egui::Label::new(k.display_name()));
                    ui.add_enabled_ui(entry.enabled, |ui| {
                        channel_value(ui, k, &mut entry.value);
                    });
                });
            }
            ui.label(RichText::new("Unchecked channels are never touched by the stroke.").small().weak());
            ui.label(RichText::new("Tip: paint bumps with Height (raise its value above 0). Normal paints a fixed direction; flat = no change.").small().weak());
        }
    });
}
