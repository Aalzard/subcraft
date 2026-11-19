//! File > Save as template (Painter): name the open project's setup (channels, resolution,
//! normal format, export preset, UDIM tiles) and offer it in New Project's Template list. The
//! work is `subcraft_app::user_templates`.

use egui::RichText;
use subcraft_app::user_templates::{self, UserTemplate};

use crate::i18n::tr;
use crate::state::UiState;
use crate::theme::TEXT_DIM;

/// Open the window, suggesting the project's name.
pub fn open(state: &mut UiState) {
    let name = state.session.project_path().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    state.save_template = Some(name);
}

/// Save the open project's setup as the template `name`; returns what to tell the user.
pub fn save(state: &UiState, name: &str) -> Result<String, String> {
    let dir = user_templates::templates_dir().ok_or("no folder for user data on this system")?;
    let template = UserTemplate::new(name, state.session.template_settings());
    let replaced = user_templates::list(&dir).iter().any(|t| t.name.eq_ignore_ascii_case(&template.name));
    user_templates::save(&dir, &template)?;
    Ok(format!("{} template \"{}\": it is in New Project's Template list", if replaced { "Replaced the" } else { "Saved the" }, template.name))
}

pub fn window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(mut name) = state.save_template.clone() else { return };
    let mut open = true;
    let mut submit = false;
    let mut cancel = false;
    let summary = crate::new_project::user_template_summary(&UserTemplate::new(name.clone(), state.session.template_settings()));
    crate::widgets::window(ctx, "Save as template").open(&mut open).resizable(false).collapsible(false).default_width(360.0).show(ctx, |ui| {
        ui.label(tr("Template name"));
        let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(320.0));
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submit = true;
        }
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(summary.replacen("Your template: ", "", 1)).small().color(TEXT_DIM())).wrap());
        ui.label(RichText::new(tr("Layers are not saved in a template; use a smart material for those.")).small().color(TEXT_DIM()));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tr("Save"))).clicked() {
                submit = true;
            }
            if ui.button(tr("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    state.save_template = Some(name.clone());
    if submit && !name.trim().is_empty() {
        let result = save(state, &name);
        match result {
            Ok(message) => {
                state.set_status(message, now);
                state.save_template = None;
            }
            Err(e) => state.set_status(format!("Could not save the template: {e}"), now),
        }
    } else if cancel || !open {
        state.save_template = None;
    }
}
