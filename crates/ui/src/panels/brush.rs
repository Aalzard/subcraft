//! Tool and brush settings, stroke options and the brush material.

use egui::{RichText, Ui};
use subcraft_paint::{Alignment, PaintTarget, PolygonFillMode};

use crate::state::{Tool, UiState};
use crate::theme::{self, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{channel_value, hint, panel, prop_row, section, segmented, slider_row};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    egui::ScrollArea::vertical().id_salt("brush").auto_shrink([false, false]).show(ui, |ui| {
        panel(ui, |ui| {
            section(ui, "tool", "Tool", true, |ui| {
                prop_row(ui, "Tool", |ui| {
                    segmented(ui, &mut state.tool, &[(Tool::Brush, "Brush"), (Tool::Eraser, "Eraser"), (Tool::PolygonFill, "Fill")]);
                });
                if state.tool == Tool::PolygonFill {
                    prop_row(ui, "Fill by", |ui| {
                        segmented(
                            ui,
                            &mut state.fill_mode,
                            &[(PolygonFillMode::Triangle, "Triangle"), (PolygonFillMode::MeshElement, "Part"), (PolygonFillMode::UvIsland, "UV island")],
                        );
                    });
                }
                let b = &mut state.session.brush;
                prop_row(ui, "Paint on", |ui| {
                    segmented(ui, &mut b.target, &[(PaintTarget::Content, "Layer"), (PaintTarget::MaskBase, "Mask")]);
                });
            });

            section(ui, "presets", "Presets", true, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut state.preset_name).hint_text("Preset name").desired_width(150.0));
                    let can_save = state.user_library.is_some();
                    if ui
                        .add_enabled(can_save, egui::Button::new(format!("{}  Save preset", crate::icons::SAVE)))
                        .on_hover_text("Save size, flow, tip, jitter, symmetry and material to your library")
                        .clicked()
                    {
                        let name = if state.preset_name.trim().is_empty() { state.session.brush.alpha_name.clone() } else { state.preset_name.clone() };
                        let preset = state.session.brush_preset(&name);
                        let lib = state.user_library.clone().expect("enabled only with a library");
                        let now = ui.input(|i| i.time);
                        match subcraft_app::brush_preset::save_brush_preset(&lib, &preset) {
                            Ok(p) => {
                                state.set_status(format!("Saved brush preset \"{}\"", p.name), now);
                                state.brush_presets.push(p);
                                state.preset_name.clear();
                            }
                            Err(e) => state.set_status(format!("Save preset: {e}"), now),
                        }
                    }
                });
                hint(ui, "Saved presets appear first in Shelf › Brushes.");
            });

            let b = &mut state.session.brush;
            section(ui, "brush", "Brush", true, |ui| {
                prop_row(ui, "Tip", |ui| {
                    ui.label(RichText::new(&b.alpha_name).color(theme::TEXT));
                    ui.label(RichText::new("change in Shelf › Brushes").font(theme::font(11.0)).color(TEXT_FAINT));
                });
                let s = &mut b.settings;
                slider_row(ui, "Size", egui::Slider::new(&mut s.size, 0.1..=50.0).logarithmic(true).suffix(" %"));
                slider_row(ui, "Flow", egui::Slider::new(&mut s.flow, 0.01..=1.0));
                slider_row(ui, "Opacity", egui::Slider::new(&mut s.stroke_opacity, 0.0..=1.0));
                slider_row(ui, "Hardness", egui::Slider::new(&mut s.hardness, 0.0..=1.0));
                slider_row(ui, "Spacing", egui::Slider::new(&mut s.spacing, 0.02..=2.0).logarithmic(true));
                slider_row(ui, "Angle", egui::Slider::new(&mut s.angle_deg, -180.0..=180.0).suffix("°"));
                slider_row(ui, "Roundness", egui::Slider::new(&mut s.roundness, 0.05..=1.0));
                prop_row(ui, "Alignment", |ui| {
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
                });
                prop_row(ui, "", |ui| ui.checkbox(&mut s.follow_path, "Follow path"));
                prop_row(ui, "", |ui| ui.checkbox(&mut s.backface_culling, "Backface culling"));
                ui.add_enabled_ui(s.backface_culling, |ui| {
                    slider_row(ui, "Max angle", egui::Slider::new(&mut s.backface_angle_deg, 10.0..=90.0).suffix("°"));
                });
            });

            section(ui, "jitter", "Jitter", false, |ui| {
                let s = &mut b.settings;
                slider_row(ui, "Size", egui::Slider::new(&mut s.size_jitter, 0.0..=1.0));
                slider_row(ui, "Flow", egui::Slider::new(&mut s.flow_jitter, 0.0..=1.0));
                slider_row(ui, "Angle", egui::Slider::new(&mut s.angle_jitter_deg, 0.0..=180.0));
                slider_row(ui, "Position", egui::Slider::new(&mut s.position_jitter, 0.0..=2.0));
            });

            section(ui, "stroke", "Stroke", true, |ui| {
                let o = &mut b.options;
                prop_row(ui, "Symmetry", |ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                        let on = o.symmetry[i];
                        let text = RichText::new(axis).color(if on { theme::ACCENT } else { TEXT_DIM });
                        let fill = if on { theme::ACCENT_SOFT } else { theme::BG_3 };
                        if ui.add(egui::Button::new(text).fill(fill).min_size(egui::vec2(34.0, 26.0))).on_hover_text(format!("Mirror across {axis}")).clicked() {
                            o.symmetry[i] = !on;
                        }
                    }
                });
                slider_row(ui, "Lazy mouse", egui::Slider::new(&mut o.lazy_distance_px, 0.0..=100.0).suffix(" px"));
            });

            let target = state.session.brush.target;
            section(ui, "material", "Material", true, |ui| {
                if target == PaintTarget::MaskBase {
                    slider_row(ui, "Mask value", egui::Slider::new(&mut state.session.brush.material.mask_value, 0.0..=1.0));
                } else {
                    let channels = state.session.channels();
                    let mat = &mut state.session.brush.material;
                    for k in channels {
                        let entry = mat.channels.entry(k).or_insert(subcraft_paint::BrushChannel { enabled: false, value: k.default_paint_value() });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut entry.enabled, "");
                            let name = RichText::new(k.display_name()).color(if entry.enabled { theme::TEXT } else { TEXT_FAINT });
                            ui.allocate_ui_with_layout(egui::vec2(theme::LABEL_WIDTH - 26.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.set_min_width(theme::LABEL_WIDTH - 26.0);
                                ui.add(egui::Label::new(name).truncate());
                            });
                            ui.add_enabled_ui(entry.enabled, |ui| {
                                channel_value(ui, k, &mut entry.value);
                            });
                        });
                    }
                    ui.add_space(4.0);
                    hint(ui, "Unchecked channels are never touched by the stroke. Paint bumps with Height (above 0); Normal paints a fixed direction.");
                }
            });
        });
    });
}
