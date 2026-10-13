//! Properties of the selected layer: name, fill channels + projection, mask and its effects.

use std::sync::Arc;

use egui::{RichText, Ui};
use subcraft_app::Command;
use subcraft_core::{ChannelValue, MeshMapKind};
use subcraft_doc::{
    FillChannel, FillSource, Generator, GeneratorKind, LayerContent, Levels, MaskEffect, MaskEffectKind, ProceduralKind,
    ProceduralParams, Projection, ProjectionMode,
};

use crate::state::UiState;
use crate::widgets::{channel_value, header};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let Some(layer) = state.session.selected_layer().cloned() else {
        ui.label(RichText::new("Select a layer to edit its properties.").weak());
        return;
    };
    let mut cmds = Vec::new();
    let id = layer.id;
    let channels = state.session.channels();

    egui::ScrollArea::vertical().id_salt("props").show(ui, |ui| {
        // Name
        ui.horizontal(|ui| {
            ui.label("Name");
            let buf = match &mut state.rename {
                Some((lid, s)) if *lid == id => s,
                slot => {
                    *slot = Some((id, layer.name.clone()));
                    &mut slot.as_mut().expect("just set").1
                }
            };
            let r = ui.text_edit_singleline(buf);
            if r.lost_focus() && *buf != layer.name && !buf.trim().is_empty() {
                cmds.push(Command::Rename { layer: id, name: buf.trim().to_string() });
            }
        });

        match &layer.content {
            LayerContent::Paint(p) => {
                header(ui, "Paint layer");
                if p.channels.is_empty() {
                    ui.label(RichText::new("Empty. Paint in the viewport with the Brush tool.").weak());
                } else {
                    let names: Vec<&str> = p.channels.keys().map(|k| k.display_name()).collect();
                    ui.label(format!("Painted channels: {}", names.join(", ")));
                }
            }
            LayerContent::Group(g) => {
                header(ui, "Folder");
                ui.label(format!("{} children", g.children.len()));
            }
            LayerContent::Fill(f) => {
                header(ui, "Projection");
                let mut proj = f.projection;
                let mut changed = false;
                egui::ComboBox::from_id_salt("proj_mode")
                    .selected_text(match proj.mode {
                        ProjectionMode::Uv => "UV",
                        ProjectionMode::Triplanar => "Tri-planar",
                        ProjectionMode::Planar => "Planar",
                    })
                    .show_ui(ui, |ui| {
                        for (m, n) in [(ProjectionMode::Uv, "UV"), (ProjectionMode::Triplanar, "Tri-planar"), (ProjectionMode::Planar, "Planar")] {
                            changed |= ui.selectable_value(&mut proj.mode, m, n).changed();
                        }
                    });
                egui::Grid::new("proj_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Scale");
                    changed |= ui.add(egui::DragValue::new(&mut proj.scale[0]).speed(0.05).range(0.01..=256.0)).changed();
                    proj.scale[1] = proj.scale[0];
                    ui.end_row();
                    ui.label("Rotation");
                    changed |= ui.add(egui::Slider::new(&mut proj.rotation_deg, -180.0..=180.0)).changed();
                    ui.end_row();
                    if proj.mode == ProjectionMode::Triplanar {
                        ui.label("Sharpness");
                        changed |= ui.add(egui::Slider::new(&mut proj.triplanar_sharpness, 1.0..=32.0)).changed();
                        ui.end_row();
                    }
                    if proj.mode == ProjectionMode::Planar {
                        ui.label("Axis");
                        ui.horizontal(|ui| {
                            for (a, n) in [(0u8, "X"), (1, "Y"), (2, "Z")] {
                                changed |= ui.selectable_value(&mut proj.planar_axis, a, n).changed();
                            }
                        });
                        ui.end_row();
                    }
                });
                if changed {
                    cmds.push(Command::SetFillProjection { layer: id, projection: proj });
                }

                header(ui, "Channels");
                for kind in &channels {
                    let mut ch = f.channels.get(kind).cloned().unwrap_or(FillChannel { enabled: false, source: FillSource::uniform(kind.default_paint_value()) });
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        changed |= ui.checkbox(&mut ch.enabled, kind.display_name()).changed();
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let current = match ch.source {
                                FillSource::Uniform { .. } => 0,
                                FillSource::Procedural { .. } => 1,
                                FillSource::MeshMap { .. } => 2,
                                FillSource::Bitmap { .. } => 3,
                            };
                            let mut sel = current;
                            egui::ComboBox::from_id_salt(("src", *kind))
                                .width(90.0)
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
                    ui.indent(("ch", *kind), |ui| match &mut ch.source {
                        FillSource::Uniform { value } => {
                            changed |= channel_value(ui, *kind, value).changed;
                        }
                        FillSource::Procedural { params, low, high } => {
                            egui::Grid::new(("proc", *kind)).num_columns(2).show(ui, |ui| {
                                ui.label("Pattern");
                                egui::ComboBox::from_id_salt(("kind", *kind)).selected_text(params.kind.label()).show_ui(ui, |ui| {
                                    for k in ProceduralKind::ALL {
                                        changed |= ui.selectable_value(&mut params.kind, k, k.label()).changed();
                                    }
                                });
                                ui.end_row();
                                ui.label("Scale");
                                changed |= ui.add(egui::Slider::new(&mut params.scale, 1.0..=64.0).step_by(1.0)).changed();
                                ui.end_row();
                                ui.label("Balance");
                                changed |= ui.add(egui::Slider::new(&mut params.balance, 0.0..=1.0)).changed();
                                ui.end_row();
                                ui.label("Contrast");
                                changed |= ui.add(egui::Slider::new(&mut params.contrast, 0.0..=1.0)).changed();
                                ui.end_row();
                                ui.label("Seed");
                                changed |= ui.add(egui::DragValue::new(&mut params.seed)).changed();
                                ui.end_row();
                                ui.label("Low");
                                changed |= channel_value(ui, *kind, low).changed;
                                ui.end_row();
                                ui.label("High");
                                changed |= channel_value(ui, *kind, high).changed;
                                ui.end_row();
                            });
                            changed |= ui.checkbox(&mut params.invert, "Invert").changed();
                        }
                        FillSource::MeshMap { map } => {
                            egui::ComboBox::from_id_salt(("map", *kind)).selected_text(map.display_name()).show_ui(ui, |ui| {
                                for m in MeshMapKind::ALL {
                                    changed |= ui.selectable_value(map, m, m.display_name()).changed();
                                }
                            });
                            ui.label(RichText::new("Mesh maps are not baked yet: shows the neutral value.").small().weak());
                        }
                        FillSource::Bitmap { .. } => {
                            ui.label("Bitmap resource");
                        }
                    });
                    if changed {
                        cmds.push(Command::SetFillChannel { layer: id, channel: *kind, fill: ch });
                    }
                }
            }
        }

        header(ui, "Mask");
        match &layer.mask {
            None => {
                ui.horizontal(|ui| {
                    if ui.button("Add white mask").clicked() {
                        cmds.push(Command::AddMask { layer: id, white: true });
                    }
                    if ui.button("Add black mask").clicked() {
                        cmds.push(Command::AddMask { layer: id, white: false });
                    }
                });
            }
            Some(mask) => {
                ui.horizontal(|ui| {
                    let mut en = mask.enabled;
                    if ui.checkbox(&mut en, "Enabled").changed() {
                        cmds.push(Command::SetMaskEnabled { layer: id, enabled: en });
                    }
                    if ui.button("Remove mask").clicked() {
                        cmds.push(Command::RemoveMask(id));
                    }
                });
                ui.label(RichText::new("Paint the mask: Brush panel, Target: Mask.").small().weak());
                ui.horizontal_wrapped(|ui| {
                    ui.label("Add:");
                    for (kind, name) in [
                        (GeneratorKind::Edges, "Edges"),
                        (GeneratorKind::Cavity, "Cavity"),
                        (GeneratorKind::Directional, "Directional"),
                        (GeneratorKind::PositionGradient, "Gradient"),
                    ] {
                        if ui.small_button(name).clicked() {
                            cmds.push(Command::AddMaskEffect { layer: id, kind: MaskEffectKind::Generator(Generator::new(kind)) });
                        }
                    }
                    if ui.small_button("Noise fill").clicked() {
                        cmds.push(Command::AddMaskEffect {
                            layer: id,
                            kind: MaskEffectKind::Fill { value: 1.0, procedural: Some(ProceduralParams::new(ProceduralKind::Grunge)), projection: Projection::default() },
                        });
                    }
                    if ui.small_button("Levels").clicked() {
                        cmds.push(Command::AddMaskEffect { layer: id, kind: MaskEffectKind::Levels(Levels::default()) });
                    }
                    if ui.small_button("Invert").clicked() {
                        cmds.push(Command::AddMaskEffect { layer: id, kind: MaskEffectKind::Invert });
                    }
                });
                for e in mask.effects.iter().rev() {
                    effect_editor(ui, id, e, &mut cmds);
                }
            }
        }
    });
    state.run(cmds, now);
}

fn effect_editor(ui: &mut Ui, layer: subcraft_core::LayerId, e: &Arc<MaskEffect>, cmds: &mut Vec<Command>) {
    let mut eff = (**e).clone();
    let mut changed = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut eff.visible, RichText::new(&eff.name).strong()).changed();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("x").on_hover_text("Remove effect").clicked() {
                    cmds.push(Command::RemoveMaskEffect { layer, effect: eff.id });
                }
            });
        });
        egui::Grid::new(("eff", eff.id)).num_columns(2).show(ui, |ui| {
            ui.label("Opacity");
            changed |= ui.add(egui::Slider::new(&mut eff.opacity, 0.0..=1.0)).changed();
            ui.end_row();
            match &mut eff.kind {
                MaskEffectKind::Generator(g) => {
                    ui.label("Level");
                    changed |= ui.add(egui::Slider::new(&mut g.level, 0.0..=1.0)).changed();
                    ui.end_row();
                    ui.label("Softness");
                    changed |= ui.add(egui::Slider::new(&mut g.softness, 0.0..=1.0)).changed();
                    ui.end_row();
                    ui.label("Breakup");
                    changed |= ui.add(egui::Slider::new(&mut g.noise_amount, 0.0..=1.0)).changed();
                    ui.end_row();
                    ui.label("");
                    changed |= ui.checkbox(&mut g.invert, "Invert").changed();
                    ui.end_row();
                }
                MaskEffectKind::Fill { procedural: Some(p), .. } => {
                    ui.label("Scale");
                    changed |= ui.add(egui::Slider::new(&mut p.scale, 1.0..=64.0).step_by(1.0)).changed();
                    ui.end_row();
                    ui.label("Balance");
                    changed |= ui.add(egui::Slider::new(&mut p.balance, 0.0..=1.0)).changed();
                    ui.end_row();
                }
                MaskEffectKind::Levels(l) => {
                    ui.label("In low");
                    changed |= ui.add(egui::Slider::new(&mut l.in_low, 0.0..=1.0)).changed();
                    ui.end_row();
                    ui.label("In high");
                    changed |= ui.add(egui::Slider::new(&mut l.in_high, 0.0..=1.0)).changed();
                    ui.end_row();
                    ui.label("Gamma");
                    changed |= ui.add(egui::Slider::new(&mut l.gamma, 0.1..=4.0)).changed();
                    ui.end_row();
                }
                _ => {}
            }
        });
    });
    if changed {
        cmds.push(Command::SetMaskEffect { layer, effect: eff });
    }
}
