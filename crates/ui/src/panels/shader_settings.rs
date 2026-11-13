//! Painter's SHADER SETTINGS panel: the shader instance of the selected texture set, its name,
//! Restore defaults, and the common parameters the viewport shader implements. Instances are
//! created and assigned from the texture set's shader menu (Texture Set List / Settings).

use egui::{RichText, Ui};

use crate::i18n::tr;
use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let ts = state.session.selected_texture_set().id;
    let instance = state.session.shader_instance_of(ts);
    let users: Vec<String> = state
        .session
        .document()
        .texture_sets
        .iter()
        .filter(|t| state.session.shader_instance_of(t.id).id == instance.id)
        .map(|t| t.name.clone())
        .collect();
    egui::ScrollArea::vertical().id_salt("shader_settings").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        egui::Grid::new("shader_settings_head").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("Instance name"));
            // Renamed when the field is left (Enter or a click elsewhere); names stay unique.
            let id = ui.make_persistent_id(("shader_instance_name", instance.id));
            let mut text = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| instance.name.clone());
            let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(180.0));
            if field.changed() {
                ui.data_mut(|d| d.insert_temp(id, text.clone()));
            }
            if field.lost_focus() {
                ui.data_mut(|d| d.remove_temp::<String>(id));
                if text.trim() != instance.name {
                    state.session.rename_shader_instance(instance.id, &text);
                }
            }
            ui.end_row();
            ui.label(tr("Used by"));
            ui.label(RichText::new(users.join(", ")).color(TEXT_DIM())).on_hover_text(tr("Texture sets drawn with this shader instance"));
            ui.end_row();
        });
        ui.add_space(4.0);
        if ui.add_enabled(instance.params != subcraft_doc::ShaderParams::default(), egui::Button::new(tr("Restore defaults")))
            .on_hover_text(tr("Put every parameter of this instance back to the shader's default"))
            .clicked()
        {
            state.session.restore_shader_defaults(instance.id);
        }
        ui.add_space(6.0);
        ui.label(RichText::new(tr("COMMON PARAMETERS")).font(theme::font_strong(11.0)).color(TEXT_DIM()));
        let mut p = instance.params;
        egui::Grid::new("shader_settings_common").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label(tr("AO Intensity"));
            ui.add(egui::Slider::new(&mut p.ao_intensity, subcraft_doc::ShaderParams::AO_RANGE)).on_hover_text(tr("How much the Ambient occlusion channel darkens the environment light"));
            ui.end_row();
            ui.label(tr("Emissive Intensity"));
            ui.add(egui::Slider::new(&mut p.emissive_intensity, subcraft_doc::ShaderParams::EMISSIVE_RANGE).logarithmic(true)).on_hover_text(tr("Multiplier of the Emissive channel"));
            ui.end_row();
            ui.label(tr("Horizon Fading"));
            ui.add(egui::Slider::new(&mut p.horizon_fading, subcraft_doc::ShaderParams::HORIZON_RANGE)).on_hover_text(tr("Fades reflections that the normal map tilts below the surface, which would otherwise see through the model"));
            ui.end_row();
        });
        if p != instance.params {
            state.session.set_shader_params(instance.id, p);
        }
        ui.add_space(8.0);
        ui.label(RichText::new(tr("New instances and which instance a texture set uses: the shader button of the Texture Set List or Texture Set Settings.")).small().color(TEXT_DIM()));
    });
}

/// The shader instance menu of texture set `ts` (Texture Set List row and Settings): pick an
/// instance, make a new one from the current, or open Shader Settings.
pub fn instance_menu(state: &UiState, ui: &mut Ui, ts: subcraft_core::TextureSetId, ops: &mut Vec<ShaderOp>) {
    let current = state.session.shader_instance_of(ts).id;
    for instance in state.session.shader_instances() {
        if ui.selectable_label(instance.id == current, &instance.name).clicked() {
            if instance.id != current {
                ops.push(ShaderOp::Assign(ts, instance.id));
            }
            ui.close();
        }
    }
    ui.separator();
    if ui.button(tr("New shader instance")).on_hover_text(tr("A copy of the current instance, used by this texture set only")).clicked() {
        ops.push(ShaderOp::New(ts, current));
        ui.close();
    }
    if ui.button(tr("Shader settings…")).clicked() {
        ops.push(ShaderOp::OpenPanel);
        ui.close();
    }
}

/// What the shader menus asked for (applied after the texture set loops release the session).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShaderOp {
    Assign(subcraft_core::TextureSetId, subcraft_doc::ShaderInstanceId),
    /// A new instance copied from the given one, assigned to the texture set.
    New(subcraft_core::TextureSetId, subcraft_doc::ShaderInstanceId),
    OpenPanel,
}

/// Carry out the menus' requests.
pub fn apply(state: &mut UiState, ops: Vec<ShaderOp>) {
    for op in ops {
        match op {
            ShaderOp::Assign(ts, id) => {
                state.session.assign_shader_instance(ts, id);
            }
            ShaderOp::New(ts, from) => {
                let id = state.session.new_shader_instance(Some(from));
                state.session.assign_shader_instance(ts, id);
                state.open_shader_settings = true;
            }
            ShaderOp::OpenPanel => state.open_shader_settings = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_ops_create_assign_and_open_the_panel() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let ts = state.session.selected_texture_set().id;
        let main = state.session.shader_instance_of(ts).id;
        apply(&mut state, vec![ShaderOp::New(ts, main)]);
        let made = state.session.shader_instance_of(ts);
        assert_ne!(made.id, main);
        assert_eq!(made.name, "Main shader (2)");
        assert!(state.open_shader_settings, "a new instance opens Shader Settings to edit it");
        state.open_shader_settings = false;
        apply(&mut state, vec![ShaderOp::Assign(ts, main), ShaderOp::OpenPanel]);
        assert_eq!(state.session.shader_instance_of(ts).id, main);
        assert!(state.open_shader_settings);
    }

    #[test]
    fn the_panel_edits_reach_the_instance() {
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut texts = Vec::new();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 900.0))), ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| show(&mut state, ui));
            });
            out.textures_delta.clear();
            texts = out.shapes.iter().filter_map(|s| match &s.shape { egui::epaint::Shape::Text(t) => Some(t.galley.text().to_string()), _ => None }).collect();
        }
        for want in ["Instance name", "COMMON PARAMETERS", "AO Intensity", "Emissive Intensity", "Horizon Fading", "Restore defaults"] {
            assert!(texts.iter().any(|t| t == want), "missing {want}: {texts:?}");
        }
        let ts = state.session.selected_texture_set().id;
        let id = state.session.shader_instance_of(ts).id;
        state.session.set_shader_params(id, subcraft_doc::ShaderParams { ao_intensity: 0.1, ..Default::default() });
        assert_eq!(state.session.shader_params_of(ts).ao_intensity, 0.1);
    }
}
