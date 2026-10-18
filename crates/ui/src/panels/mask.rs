//! The selected mask's controls; every document edit is an app command.
use crate::{state::UiState, theme, widgets::*};
use egui::{RichText, Ui};
use subcraft_app::{Command, LayerTarget};
use subcraft_core::LayerId;

pub(crate) fn actions(
    ui: &mut Ui,
    layer: LayerId,
    enabled: bool,
    paste: bool,
    cmds: &mut Vec<Command>,
) {
    if ui.button("Preview mask").clicked() {
        ui.ctx()
            .data_mut(|d| d.insert_temp(egui::Id::new("inspect_mask"), layer));
        ui.close();
    }
    if ui
        .button(if enabled {
            "Disable mask"
        } else {
            "Enable mask"
        })
        .clicked()
    {
        cmds.push(Command::SetMaskEnabled {
            layer,
            enabled: !enabled,
        });
        ui.close();
    }
    if ui.button("Invert mask (effect)").clicked() {
        cmds.push(Command::InvertMask(layer));
        ui.close();
    }
    if ui.button("Copy mask").clicked() {
        cmds.push(Command::CopyMask(layer));
        ui.close();
    }
    if ui
        .add_enabled(paste, egui::Button::new("Paste mask"))
        .clicked()
    {
        cmds.push(Command::PasteMask(layer));
        ui.close();
    }
    ui.separator();
    if ui.button("Remove mask").clicked() {
        cmds.push(Command::RemoveMask(layer));
        ui.close();
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let Some(layer) = state.session.selected_layer().cloned() else {
        return;
    };
    let Some(mask) = &layer.mask else { return };
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    egui::ScrollArea::vertical().id_salt(("mask_properties", layer.id)).auto_shrink([false, false]).show(ui, |ui| panel(ui, |ui| {
        ui.label(RichText::new(format!("{} · Mask", layer.name)).font(theme::font_strong(14.0)).color(theme::TEXT));
        hint(ui, "White reveals the material. Black hides it. Painting changes this mask's base.");
        section(ui, "mask_controls", "Mask", true, |ui| {
            let mut enabled = mask.enabled;
            if ui.checkbox(&mut enabled, "Enable mask").changed() { cmds.push(Command::SetMaskEnabled { layer: layer.id, enabled }); }
            let mut material = state.session.brush.material.clone();
            if slider_row(ui, "Paint value", egui::Slider::new(&mut material.mask_value, 0.0..=1.0)).changed() { cmds.push(Command::SetBrushMaterial(material)); }
            ui.horizontal_wrapped(|ui| {
                if secondary_button(ui, "Preview").clicked() { state.layer_previews.inspect = Some(layer.id); }
                if secondary_button(ui, "Invert").clicked() { cmds.push(Command::InvertMask(layer.id)); }
                if secondary_button(ui, "Copy").clicked() { cmds.push(Command::CopyMask(layer.id)); }
                if ui.add_enabled(state.session.can_paste_mask(), egui::Button::new("Paste")).clicked() { cmds.push(Command::PasteMask(layer.id)); }
            });
            ui.horizontal(|ui| {
                if secondary_button(ui, "Edit content").clicked() { cmds.push(Command::SelectLayerTarget { layer: layer.id, target: LayerTarget::Content }); }
                if secondary_button(ui, "Remove mask").clicked() { cmds.push(Command::RemoveMask(layer.id)); }
            });
        });
        section(ui, "mask_effects", "Effects", true, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, kind) in [
                    ("Fill", subcraft_doc::MaskEffectKind::Fill { value: 1.0, procedural: None, projection: Default::default() }),
                    ("Generator", subcraft_doc::MaskEffectKind::Generator(subcraft_doc::Generator::new(subcraft_doc::GeneratorKind::Edges))),
                    ("Levels", subcraft_doc::MaskEffectKind::Levels(Default::default())),
                ] { if secondary_button(ui, label).clicked() { cmds.push(Command::AddMaskEffect { layer: layer.id, kind }); } }
            });
            for (index, effect) in mask.effects.iter().enumerate().rev() {
                ui.push_id(effect.id, |ui| {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(index + 1 < mask.effects.len(), egui::Button::new("↑")).on_hover_text("Move effect up").clicked() { cmds.push(Command::MoveMaskEffect { layer: layer.id, effect: effect.id, index: index + 1 }); }
                        if ui.add_enabled(index > 0, egui::Button::new("↓")).on_hover_text("Move effect down").clicked() { cmds.push(Command::MoveMaskEffect { layer: layer.id, effect: effect.id, index: index - 1 }); }
                        if ui.small_button("Duplicate").clicked() { cmds.push(Command::DuplicateMaskEffect { layer: layer.id, effect: effect.id }); }
                    });
                    super::properties::effect_editor(ui, state, layer.id, effect, &mut cmds);
                });
            }
        });
    }));
    state.run(cmds, now);
}
