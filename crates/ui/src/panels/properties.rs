//! Properties of the selected layer: name, fill channels + projection, mask and its effects.

use std::sync::Arc;

use egui::{RichText, Ui};
use subcraft_app::Command;
use subcraft_core::{ChannelValue, MeshMapKind};
use subcraft_doc::{
    FillChannel, FillSource, Generator, GeneratorKind, LayerContent, Levels, MaskEffect, MaskEffectKind, ProceduralKind,
    ProceduralParams, Projection, ProjectionMode,
};

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, ACCENT, BG_2, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{channel_value, empty_state, fill_width, hint, icon_button, panel, prop_row, secondary_button, section, section_with, segmented, slider_row};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let Some(layer) = state.session.selected_layer().cloned() else {
        panel(ui, |ui| empty_state(ui, icons::SLIDERS, "Select a layer to edit its properties."));
        return;
    };
    let mut cmds = Vec::new();
    let id = layer.id;
    let channels = state.session.channels();

    egui::ScrollArea::vertical().id_salt("props").auto_shrink([false, false]).show(ui, |ui| panel(ui, |ui| {
        // Name, with the layer type as an icon.
        ui.horizontal(|ui| {
            let icon = match &layer.content {
                LayerContent::Paint(_) => icons::PAINT_BRUSH,
                LayerContent::Fill(_) => icons::PAINT_BUCKET,
                LayerContent::Group(_) => icons::FOLDER_OPEN,
            };
            ui.label(RichText::new(icon).font(theme::font(18.0)).color(ACCENT));
            fill_width(ui);
            let buf = match &mut state.rename {
                Some((lid, s)) if *lid == id => s,
                slot => {
                    *slot = Some((id, layer.name.clone()));
                    &mut slot.as_mut().expect("just set").1
                }
            };
            let r = ui.add(egui::TextEdit::singleline(buf).font(theme::font_strong(14.0)).margin(egui::vec2(8.0, 5.0)).desired_width(f32::INFINITY));
            if r.lost_focus() && *buf != layer.name && !buf.trim().is_empty() {
                cmds.push(Command::Rename { layer: id, name: buf.trim().to_string() });
            }
        });

        ui.add_space(8.0);

        match &layer.content {
            LayerContent::Paint(p) => {
                section(ui, "paint", "Paint layer", true, |ui| {
                    if p.channels.is_empty() {
                        hint(ui, "Empty. Paint in the viewport with the Brush tool.");
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            for k in p.channels.keys() {
                                crate::widgets::badge(ui, k.display_name(), TEXT_DIM);
                            }
                        });
                    }
                });
            }
            LayerContent::Group(g) => {
                section(ui, "folder", "Folder", true, |ui| {
                    ui.label(RichText::new(format!("{} layers inside", g.children.len())).color(TEXT_DIM));
                });
            }
            LayerContent::Fill(f) => {
                section(ui, "projection", "Projection", true, |ui| {
                    let mut proj = f.projection;
                    let mut changed = false;
                    prop_row(ui, "Mode", |ui| {
                        changed |= segmented(ui, &mut proj.mode, &[(ProjectionMode::Uv, "UV"), (ProjectionMode::Triplanar, "Tri-planar"), (ProjectionMode::Planar, "Planar")]);
                    });
                    prop_row(ui, "Scale", |ui| {
                        changed |= ui.add(egui::DragValue::new(&mut proj.scale[0]).speed(0.05).range(0.01..=256.0)).changed();
                    });
                    proj.scale[1] = proj.scale[0];
                    changed |= slider_row(ui, "Rotation", egui::Slider::new(&mut proj.rotation_deg, -180.0..=180.0).suffix("°")).changed();
                    if proj.mode == ProjectionMode::Triplanar {
                        changed |= slider_row(ui, "Sharpness", egui::Slider::new(&mut proj.triplanar_sharpness, 1.0..=32.0)).changed();
                    }
                    if proj.mode == ProjectionMode::Planar {
                        prop_row(ui, "Axis", |ui| {
                            changed |= segmented(ui, &mut proj.planar_axis, &[(0u8, "X"), (1, "Y"), (2, "Z")]);
                        });
                    }
                    if changed {
                        cmds.push(Command::SetFillProjection { layer: id, projection: proj });
                    }
                });

                section(ui, "channels", "Channels", true, |ui| {
                for kind in &channels {
                    let mut ch = f.channels.get(kind).cloned().unwrap_or(FillChannel { enabled: false, source: FillSource::uniform(kind.default_paint_value()) });
                    let mut changed = false;
                    egui::Frame::new().fill(BG_2).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let name = RichText::new(kind.display_name()).color(if ch.enabled { theme::TEXT } else { TEXT_FAINT });
                        changed |= ui.checkbox(&mut ch.enabled, name).changed();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let current = match ch.source {
                                FillSource::Uniform { .. } => 0,
                                FillSource::Procedural { .. } => 1,
                                FillSource::MeshMap { .. } => 2,
                                FillSource::Bitmap { .. } => 3,
                            };
                            let mut sel = current;
                            egui::ComboBox::from_id_salt(("src", *kind))
                                .width(110.0)
                                .selected_text(["Uniform", "Procedural", "Mesh map", "Bitmap"][current])
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut sel, 0, "Uniform");
                                    ui.selectable_value(&mut sel, 1, "Procedural");
                                    ui.selectable_value(&mut sel, 2, "Mesh map");
                                });
                            if sel != current {
                                ch.source = match sel {
                                    0 => FillSource::uniform(kind.default_paint_value()),
                                    1 => FillSource::Procedural {
                                        params: ProceduralParams::new(ProceduralKind::Clouds),
                                        low: ChannelValue::scalar(0.0),
                                        high: ChannelValue::scalar(1.0),
                                    },
                                    _ => FillSource::MeshMap { map: MeshMapKind::AmbientOcclusion },
                                };
                                changed = true;
                            }
                        });
                    });
                    if ch.enabled {
                        ui.add_space(2.0);
                        match &mut ch.source {
                            FillSource::Uniform { value } => {
                                prop_row(ui, "Value", |ui| changed |= channel_value(ui, *kind, value).changed);
                            }
                            FillSource::Procedural { params, low, high } => {
                                prop_row(ui, "Pattern", |ui| {
                                    egui::ComboBox::from_id_salt(("kind", *kind)).selected_text(params.kind.label()).show_ui(ui, |ui| {
                                        for k in ProceduralKind::ALL {
                                            changed |= ui.selectable_value(&mut params.kind, k, k.label()).changed();
                                        }
                                    });
                                });
                                changed |= slider_row(ui, "Scale", egui::Slider::new(&mut params.scale, 1.0..=64.0).step_by(1.0)).changed();
                                changed |= slider_row(ui, "Balance", egui::Slider::new(&mut params.balance, 0.0..=1.0)).changed();
                                changed |= slider_row(ui, "Contrast", egui::Slider::new(&mut params.contrast, 0.0..=1.0)).changed();
                                prop_row(ui, "Seed", |ui| changed |= ui.add(egui::DragValue::new(&mut params.seed)).changed());
                                prop_row(ui, "Low", |ui| changed |= channel_value(ui, *kind, low).changed);
                                prop_row(ui, "High", |ui| changed |= channel_value(ui, *kind, high).changed);
                                prop_row(ui, "", |ui| changed |= ui.checkbox(&mut params.invert, "Invert").changed());
                            }
                            FillSource::MeshMap { map } => {
                                prop_row(ui, "Map", |ui| {
                                    egui::ComboBox::from_id_salt(("map", *kind)).selected_text(map.display_name()).show_ui(ui, |ui| {
                                        for m in MeshMapKind::ALL {
                                            changed |= ui.selectable_value(map, m, m.display_name()).changed();
                                        }
                                    });
                                });
                                hint(ui, "Uses the baked map (Ctrl+B); neutral until baked.");
                            }
                            FillSource::Bitmap { .. } => {
                                ui.label(RichText::new("Bitmap resource").color(TEXT_DIM));
                            }
                        }
                    }
                    });
                    ui.add_space(2.0);
                    if changed {
                        cmds.push(Command::SetFillChannel { layer: id, channel: *kind, fill: ch });
                    }
                }
                });
            }
        }

        let mut head_cmds = Vec::new();
        section_with(
            ui,
            "mask",
            "Mask",
            true,
            |ui| {
                if let Some(mask) = &layer.mask {
                    if icon_button(ui, icons::TRASH, "Remove mask").clicked() {
                        head_cmds.push(Command::RemoveMask(id));
                    }
                    let (icon, tip) = if mask.enabled { (icons::EYE, "Disable mask") } else { (icons::EYE_SLASH, "Enable mask") };
                    if icon_button(ui, icon, tip).clicked() {
                        head_cmds.push(Command::SetMaskEnabled { layer: id, enabled: !mask.enabled });
                    }
                }
            },
            |ui| match &layer.mask {
                None => {
                    hint(ui, "Masks hide parts of the layer. Start white (all visible) or black (all hidden).");
                    ui.horizontal(|ui| {
                        if secondary_button(ui, "Add white mask").clicked() {
                            cmds.push(Command::AddMask { layer: id, white: true });
                        }
                        if secondary_button(ui, "Add black mask").clicked() {
                            cmds.push(Command::AddMask { layer: id, white: false });
                        }
                    });
                }
                Some(mask) => {
                    hint(ui, "Paint the mask with the mask toggle in the tool strip, or stack generators below.");
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                        let mut add = |ui: &mut Ui, label: &str, kind: MaskEffectKind| {
                            if ui.add(egui::Button::new(RichText::new(format!("{}  {label}", icons::PLUS)).font(theme::font(12.0)))).clicked() {
                                cmds.push(Command::AddMaskEffect { layer: id, kind });
                            }
                        };
                        add(ui, "Edges", MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)));
                        add(ui, "Cavity", MaskEffectKind::Generator(Generator::new(GeneratorKind::Cavity)));
                        add(ui, "Directional", MaskEffectKind::Generator(Generator::new(GeneratorKind::Directional)));
                        add(ui, "Gradient", MaskEffectKind::Generator(Generator::new(GeneratorKind::PositionGradient)));
                        add(
                            ui,
                            "Noise",
                            MaskEffectKind::Fill { value: 1.0, procedural: Some(ProceduralParams::new(ProceduralKind::Grunge)), projection: Projection::default() },
                        );
                        add(ui, "Levels", MaskEffectKind::Levels(Levels::default()));
                        add(ui, "Invert", MaskEffectKind::Invert);
                    });
                    ui.add_space(6.0);
                    for e in mask.effects.iter().rev() {
                        effect_editor(ui, id, e, &mut cmds);
                    }
                }
            },
        );
        cmds.extend(head_cmds);
    }));
    state.run(cmds, now);
}

fn effect_editor(ui: &mut Ui, layer: subcraft_core::LayerId, e: &Arc<MaskEffect>, cmds: &mut Vec<Command>) {
    let mut eff = (**e).clone();
    let mut changed = false;
    egui::Frame::new().fill(BG_2).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (icon, tip) = if eff.visible { (icons::EYE, "Hide effect") } else { (icons::EYE_SLASH, "Show effect") };
            if icon_button(ui, icon, tip).clicked() {
                eff.visible = !eff.visible;
                changed = true;
            }
            ui.label(RichText::new(&eff.name).font(theme::font_strong(12.5)).color(if eff.visible { theme::TEXT } else { TEXT_FAINT }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, icons::X, "Remove effect").clicked() {
                    cmds.push(Command::RemoveMaskEffect { layer, effect: eff.id });
                }
            });
        });
        if eff.visible {
            changed |= slider_row(ui, "Opacity", egui::Slider::new(&mut eff.opacity, 0.0..=1.0)).changed();
            match &mut eff.kind {
                MaskEffectKind::Generator(g) => {
                    changed |= slider_row(ui, "Level", egui::Slider::new(&mut g.level, 0.0..=1.0)).changed();
                    changed |= slider_row(ui, "Softness", egui::Slider::new(&mut g.softness, 0.0..=1.0)).changed();
                    changed |= slider_row(ui, "Breakup", egui::Slider::new(&mut g.noise_amount, 0.0..=1.0)).changed();
                    prop_row(ui, "", |ui| changed |= ui.checkbox(&mut g.invert, "Invert").changed());
                }
                MaskEffectKind::Fill { procedural: Some(p), .. } => {
                    changed |= slider_row(ui, "Scale", egui::Slider::new(&mut p.scale, 1.0..=64.0).step_by(1.0)).changed();
                    changed |= slider_row(ui, "Balance", egui::Slider::new(&mut p.balance, 0.0..=1.0)).changed();
                }
                MaskEffectKind::Levels(l) => {
                    changed |= slider_row(ui, "In low", egui::Slider::new(&mut l.in_low, 0.0..=1.0)).changed();
                    changed |= slider_row(ui, "In high", egui::Slider::new(&mut l.in_high, 0.0..=1.0)).changed();
                    changed |= slider_row(ui, "Gamma", egui::Slider::new(&mut l.gamma, 0.1..=4.0)).changed();
                }
                _ => {}
            }
        }
    });
    ui.add_space(4.0);
    if changed {
        cmds.push(Command::SetMaskEffect { layer, effect: eff });
    }
}
