//! File dialogs and windows: save / open project, export textures, unsaved-changes prompt.

use std::path::PathBuf;

use egui::RichText;
use subcraft_app::{ExportSettings, ExportTemplate, FileFormat, Padding, Session};

use crate::state::UiState;

/// Something that would discard unsaved work; asked about first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Close,
    NewSample,
    OpenMesh,
    OpenProject,
}

pub struct ExportUi {
    pub open: bool,
    pub settings: ExportSettings,
    pub template: usize,
    pub folder: String,
}

impl Default for ExportUi {
    fn default() -> Self {
        let folder = std::env::current_dir().map(|d| d.join("exports")).unwrap_or_else(|_| PathBuf::from("exports"));
        Self { open: false, settings: ExportSettings::default(), template: 0, folder: folder.display().to_string() }
    }
}

const PROJECT_FILTER: (&str, &[&str]) = ("SubCraft project", &[subcraft_app::PROJECT_EXTENSION]);

/// Save to the current path, or ask for one. Returns true when saved.
pub fn save(state: &mut UiState, save_as: bool, now: f64) -> bool {
    let path = if save_as || state.session.project_path().is_none() {
        let name = format!("{}.{}", state.session.export_name(), subcraft_app::PROJECT_EXTENSION);
        match rfd::FileDialog::new().add_filter(PROJECT_FILTER.0, PROJECT_FILTER.1).set_file_name(name).save_file() {
            Some(mut p) => {
                if p.extension().is_none() {
                    p.set_extension(subcraft_app::PROJECT_EXTENSION);
                }
                Some(p)
            }
            None => return false,
        }
    } else {
        None
    };
    match state.session.save(path.as_deref()) {
        Ok(p) => {
            state.set_status(format!("Saved {}", p.display()), now);
            true
        }
        Err(e) => {
            state.set_status(format!("Save failed: {e}"), now);
            false
        }
    }
}

/// Run an action that replaces the session (after any unsaved-changes prompt).
pub fn run_pending(state: &mut UiState, action: Pending, ctx: &egui::Context, now: f64) {
    match action {
        Pending::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Pending::NewSample => state.set_session(Session::sample()),
        Pending::OpenMesh => {
            if let Some(path) = rfd::FileDialog::new().add_filter("Meshes", &subcraft_mesh::SUPPORTED_EXTENSIONS).pick_file() {
                match Session::open_mesh(&path, subcraft_app::DEFAULT_RESOLUTION) {
                    Ok((s, _)) => {
                        state.set_session(s);
                        state.set_status(format!("Opened {}", path.display()), now);
                    }
                    Err(e) => state.set_status(format!("Could not open mesh: {e}"), now),
                }
            }
        }
        Pending::OpenProject => {
            if let Some(path) = rfd::FileDialog::new().add_filter(PROJECT_FILTER.0, PROJECT_FILTER.1).pick_file() {
                open_project(state, &path, now);
            }
        }
    }
}

pub fn open_project(state: &mut UiState, path: &std::path::Path, now: f64) {
    match Session::open_project(path) {
        Ok(s) => {
            state.set_session(s);
            state.set_status(format!("Opened {}", path.display()), now);
        }
        Err(e) => state.set_status(format!("Could not open project: {e}"), now),
    }
}

/// "Save changes?" modal. Returns the action to run once the user decided.
pub fn unsaved_prompt(state: &mut UiState, ctx: &egui::Context, now: f64) -> Option<Pending> {
    let pending = state.confirm?;
    let mut decided = None;
    egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.heading("Unsaved changes");
        ui.label("Save your changes before continuing?");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() && save(state, false, now) {
                decided = Some(Some(pending));
            }
            if ui.button("Don't save").clicked() {
                decided = Some(Some(pending));
            }
            if ui.button("Cancel").clicked() {
                decided = Some(None);
            }
        });
    });
    match decided {
        Some(result) => {
            state.confirm = None;
            if result == Some(Pending::Close) {
                state.allow_close = true;
            }
            result
        }
        None => None,
    }
}

pub fn export_window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if !state.export.open {
        return;
    }
    let mut open = true;
    let templates = ExportTemplate::builtin();
    egui::Window::new("Export Textures").open(&mut open).default_width(460.0).show(ctx, |ui| {
        let ex = &mut state.export;
        egui::Grid::new("export_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Folder");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut ex.folder).desired_width(300.0));
                if ui.button("Browse…").clicked()
                    && let Some(p) = rfd::FileDialog::new().set_directory(&ex.folder).pick_folder()
                {
                    ex.folder = p.display().to_string();
                }
            });
            ui.end_row();
            ui.label("Template");
            egui::ComboBox::from_id_salt("template").selected_text(&templates[ex.template].name).show_ui(ui, |ui| {
                for (i, t) in templates.iter().enumerate() {
                    ui.selectable_value(&mut ex.template, i, &t.name);
                }
            });
            ui.end_row();
            ui.label("File format");
            let fmt_text = ex.settings.format_override.map_or("Template default", |f| f.label());
            egui::ComboBox::from_id_salt("format").selected_text(fmt_text).show_ui(ui, |ui| {
                ui.selectable_value(&mut ex.settings.format_override, None, "Template default");
                for f in FileFormat::ALL {
                    ui.selectable_value(&mut ex.settings.format_override, Some(f), f.label());
                }
            });
            ui.end_row();
            ui.label("Padding");
            ui.horizontal(|ui| {
                let mut kind = match ex.settings.padding {
                    Padding::Infinite => 0,
                    Padding::Texels(_) => 1,
                    Padding::None => 2,
                };
                let before = kind;
                ui.selectable_value(&mut kind, 0, "Infinite");
                ui.selectable_value(&mut kind, 1, "Texels");
                ui.selectable_value(&mut kind, 2, "None");
                if kind != before {
                    ex.settings.padding = match kind {
                        0 => Padding::Infinite,
                        1 => Padding::Texels(8),
                        _ => Padding::None,
                    };
                }
                if let Padding::Texels(t) = &mut ex.settings.padding {
                    ui.add(egui::DragValue::new(t).range(1..=64));
                }
            });
            ui.end_row();
            ui.label("Height to normal");
            ui.add(egui::Slider::new(&mut ex.settings.height_to_normal, 0.0..=0.1));
            ui.end_row();
        });
        ex.settings.template = templates[ex.template].clone();
        let settings = ex.settings.clone();
        let folder = PathBuf::from(&ex.folder);
        ui.separator();
        let plan = state.session.export_plan(&settings);
        ui.label(RichText::new(format!("{} files", plan.len())).strong());
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            for (_, f) in &plan {
                ui.label(RichText::new(f).monospace().small());
            }
        });
        ui.separator();
        if ui.add_enabled(!plan.is_empty(), egui::Button::new(RichText::new("Export").strong())).clicked() {
            match state.session.export_textures(&folder, &settings) {
                Ok(files) => state.set_status(format!("Exported {} files to {}", files.len(), folder.display()), now),
                Err(e) => state.set_status(format!("Export failed: {e}"), now),
            }
        }
    });
    state.export.open = open;
}

/// Window title with project name and unsaved marker.
pub fn title(state: &UiState) -> String {
    let name = state
        .session
        .project_path()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} (unsaved)", state.session.scene().mesh.name));
    let star = if state.session.is_modified() { " *" } else { "" };
    format!("SubCraft — {name}{star}")
}
