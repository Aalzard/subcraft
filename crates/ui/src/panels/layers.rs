//! Layer stack: tree with visibility, selection, creation/reorder buttons and per-channel blending.

use std::sync::Arc;

use egui::{RichText, Ui};
use subcraft_app::{Command, NewLayer};
use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, Layer, LayerKind};

use crate::state::UiState;
use crate::theme::ACCENT;

fn rows<'a>(list: &'a [Arc<Layer>], depth: usize, out: &mut Vec<(usize, &'a Layer)>) {
    for l in list.iter().rev() {
        out.push((depth, l));
        if let subcraft_doc::LayerContent::Group(g) = &l.content
            && g.expanded
        {
            rows(&g.children, depth + 1, out);
        }
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    let sel = state.session.selection().layer;

    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Paint").on_hover_text("New paint layer (Ctrl+Shift+N)").clicked() {
            cmds.push(Command::AddLayer(NewLayer::Paint));
        }
        if ui.button("+ Fill").on_hover_text("New fill layer (Ctrl+Shift+F)").clicked() {
            cmds.push(Command::AddLayer(NewLayer::Fill));
        }
        if ui.button("+ Folder").on_hover_text("New folder (Ctrl+G)").clicked() {
            cmds.push(Command::AddLayer(NewLayer::Folder));
        }
        ui.add_enabled_ui(sel.is_some(), |ui| {
            let id = sel.unwrap_or_default();
            if ui.button("Up").on_hover_text("Move up").clicked() {
                cmds.push(Command::MoveUp(id));
            }
            if ui.button("Down").on_hover_text("Move down").clicked() {
                cmds.push(Command::MoveDown(id));
            }
            if ui.button("Dup").on_hover_text("Duplicate (Ctrl+D)").clicked() {
                cmds.push(Command::DuplicateLayer(id));
            }
            if ui.button("Del").on_hover_text("Delete (Del)").clicked() {
                cmds.push(Command::DeleteLayer(id));
            }
        });
    });

    let channels = state.session.channels();
    if !channels.contains(&state.active_channel) {
        state.active_channel = channels.first().copied().unwrap_or(ChannelKind::BaseColor);
    }
    ui.horizontal(|ui| {
        ui.label("Channel");
        egui::ComboBox::from_id_salt("active_channel").selected_text(state.active_channel.display_name()).show_ui(ui, |ui| {
            for k in &channels {
                ui.selectable_value(&mut state.active_channel, *k, k.display_name());
            }
        });
    });
    ui.separator();

    let ts = state.session.selected_texture_set();
    let mut list = Vec::new();
    rows(&ts.layers, 0, &mut list);
    let active = state.active_channel;
    egui::ScrollArea::vertical().id_salt("layer_rows").max_height(ui.available_height() - 70.0).show(ui, |ui| {
        if list.is_empty() {
            ui.label(RichText::new("No layers. Add one above.").weak());
        }
        for (depth, l) in &list {
            ui.horizontal(|ui| {
                ui.add_space(*depth as f32 * 14.0);
                let mut vis = l.visible;
                if ui.checkbox(&mut vis, "").on_hover_text("Visible").changed() {
                    cmds.push(Command::SetVisible { layer: l.id, visible: vis });
                }
                let icon = match l.kind() {
                    LayerKind::Paint => "P",
                    LayerKind::Fill => "F",
                    LayerKind::Folder => "D",
                };
                let b = l.blend_for(active);
                ui.label(RichText::new(icon).monospace().small().color(ACCENT)).on_hover_text(format!("{:?} layer", l.kind()));
                let mut text = RichText::new(l.name.as_str());
                if !l.visible {
                    text = text.weak();
                }
                let selected = sel == Some(l.id);
                if ui.selectable_label(selected, text).clicked() {
                    cmds.push(Command::SelectLayer(Some(l.id)));
                }
                if l.mask.is_some() {
                    ui.label(RichText::new("mask").small().color(ACCENT)).on_hover_text("Has a mask");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} {:.0}", b.mode.short_label(), b.opacity * 100.0)).small().weak());
                });
            });
        }
    });

    ui.separator();
    if let Some(l) = state.session.selected_layer() {
        let id = l.id;
        let folder = l.is_folder();
        let mut b = l.blend_for(active);
        ui.horizontal(|ui| {
            ui.label("Blend");
            egui::ComboBox::from_id_salt("blend_mode").selected_text(b.mode.label()).show_ui(ui, |ui| {
                for m in BlendMode::available(active == ChannelKind::Normal, folder) {
                    if ui.selectable_label(b.mode == m, m.label()).clicked() {
                        cmds.push(Command::SetBlendMode { layer: id, channel: active, mode: m });
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label("Opacity");
            let r = ui.add(egui::Slider::new(&mut b.opacity, 0.0..=1.0).max_decimals(2));
            if r.changed() {
                cmds.push(Command::SetBlendOpacity { layer: id, channel: active, opacity: b.opacity });
            }
        });
    }
    state.run(cmds, now);
}
