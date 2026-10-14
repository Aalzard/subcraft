//! File dialogs and windows: save / open project, export textures, unsaved-changes prompt.

use std::path::PathBuf;

use egui::RichText;
use subcraft_app::{ExportSettings, ExportTemplate, FileFormat, Padding, Session};

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};
use crate::widgets::{caption, divider, hint, primary_button, secondary_button, segmented};

fn dim(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM)
}

fn window_title(text: &str) -> RichText {
    RichText::new(text).font(theme::font_strong(14.0))
}

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
        #[cfg(target_arch = "wasm32")]
        let folder = PathBuf::from(crate::files::DOWNLOADS);
        #[cfg(not(target_arch = "wasm32"))]
        let folder = std::env::current_dir().map(|d| d.join("exports")).unwrap_or_else(|_| PathBuf::from("exports"));
        Self { open: false, settings: ExportSettings::default(), template: 0, folder: folder.display().to_string() }
    }
}

const PROJECT_FILTER: (&str, &[&str]) = ("SubCraft project", &[subcraft_app::PROJECT_EXTENSION]);

/// Save to the current path, or ask for one. Returns true when saved.
pub fn save(state: &mut UiState, save_as: bool, now: f64) -> bool {
    let path = if save_as || state.session.project_path().is_none() {
        let name = format!("{}.{}", state.session.export_name(), subcraft_app::PROJECT_EXTENSION);
        // In the browser there is no save dialog: the project is downloaded under its name.
        #[cfg(target_arch = "wasm32")]
        let picked = Some(std::path::Path::new(crate::files::DOWNLOADS).join(name));
        #[cfg(not(target_arch = "wasm32"))]
        let picked = rfd::FileDialog::new().add_filter(PROJECT_FILTER.0, PROJECT_FILTER.1).set_file_name(name).save_file();
        match picked {
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
pub fn run_pending(state: &mut UiState, action: Pending, ctx: &egui::Context, _now: f64) {
    match action {
        Pending::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Pending::NewSample => state.set_session(Session::sample()),
        // In the browser a .gltf can come with its .bin, so meshes allow several files.
        Pending::OpenMesh => crate::files::pick(ctx, crate::files::Purpose::OpenMesh, crate::files::MESHES, cfg!(target_arch = "wasm32"), None),
        Pending::OpenProject => crate::files::pick(ctx, crate::files::Purpose::OpenProject, PROJECT_FILTER, false, None),
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
        ui.label(window_title("Unsaved changes"));
        ui.add_space(4.0);
        ui.label(dim("Save your changes before continuing?"));
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary_button(ui, "Save").clicked() && save(state, false, now) {
                decided = Some(Some(pending));
            }
            if secondary_button(ui, "Don't save").clicked() {
                decided = Some(Some(pending));
            }
            if secondary_button(ui, "Cancel").clicked() {
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
    egui::Window::new(window_title("Export Textures")).open(&mut open).collapsible(false).default_width(480.0).show(ctx, |ui| {
        let ex = &mut state.export;
        egui::Grid::new("export_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.label(dim("Folder"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut ex.folder).desired_width(300.0));
                    if ui.button("Browse…").clicked()
                        && let Some(p) = rfd::FileDialog::new().set_directory(&ex.folder).pick_folder()
                    {
                        ex.folder = p.display().to_string();
                    }
                });
                ui.end_row();
            }
            #[cfg(target_arch = "wasm32")]
            {
                ui.label(dim("Files"));
                ui.label(dim("Downloaded by your browser (it may ask to allow several downloads)."));
                ui.end_row();
            }
            ui.label(dim("Template"));
            egui::ComboBox::from_id_salt("template").selected_text(&templates[ex.template].name).show_ui(ui, |ui| {
                for (i, t) in templates.iter().enumerate() {
                    ui.selectable_value(&mut ex.template, i, &t.name);
                }
            });
            ui.end_row();
            ui.label(dim("File format"));
            let fmt_text = ex.settings.format_override.map_or("Template default", |f| f.label());
            egui::ComboBox::from_id_salt("format").selected_text(fmt_text).show_ui(ui, |ui| {
                ui.selectable_value(&mut ex.settings.format_override, None, "Template default");
                for f in FileFormat::ALL {
                    ui.selectable_value(&mut ex.settings.format_override, Some(f), f.label());
                }
            });
            ui.end_row();
            ui.label(dim("Padding"));
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
            ui.label(dim("Height to normal"));
            ui.add(egui::Slider::new(&mut ex.settings.height_to_normal, 0.0..=0.1));
            ui.end_row();
        });
        ex.settings.template = templates[ex.template].clone();
        let settings = ex.settings.clone();
        let folder = PathBuf::from(&ex.folder);
        ui.add_space(6.0);
        divider(ui);
        ui.add_space(4.0);
        let plan = state.session.export_plan(&settings);
        ui.label(caption(&format!("{} files", plan.len())));
        egui::Frame::new().fill(theme::FIELD).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(170.0).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for (_, f) in &plan {
                    ui.label(RichText::new(f).monospace().small().color(TEXT_DIM));
                }
            });
        });
        ui.add_space(6.0);
        let go = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(!plan.is_empty(), |ui| primary_button(ui, format!("{}  Export", crate::icons::EXPORT))).inner.clicked()
        });
        if go.inner {
            match state.session.export_textures(&folder, &settings) {
                #[cfg(not(target_arch = "wasm32"))]
                Ok(files) => state.set_status(format!("Exported {} files to {}", files.len(), folder.display()), now),
                #[cfg(target_arch = "wasm32")]
                Ok(files) => state.set_status(format!("Downloaded {} files", files.len()), now),
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

// ----- baking ----------------------------------------------------------------------------------

pub struct BakeUi {
    pub open: bool,
    pub settings: subcraft_app::BakeSettings,
    pub high_poly: String,
    pub all_sets: bool,
    pub job: Option<subcraft_app::BakeJob>,
}

impl Default for BakeUi {
    fn default() -> Self {
        Self { open: false, settings: subcraft_app::BakeSettings::default(), high_poly: String::new(), all_sets: true, job: None }
    }
}

/// Apply results of a running bake (call every frame).
pub fn poll_bake(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(job) = state.bake.job.as_mut() else { return };
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    if let Some(result) = job.poll(&mut state.session) {
        state.bake.job = None;
        match result {
            Ok(n) => state.set_status(format!("Baked mesh maps for {n} texture set(s). See them in the 2D view."), now),
            Err(e) => state.set_status(format!("Bake: {e}"), now),
        }
    }
}

pub fn bake_window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if !state.bake.open {
        return;
    }
    let mut open = true;
    let selected = state.session.selected_texture_set();
    let (sel_id, sel_slot, sel_name) = (selected.id, selected.material_slot, selected.name.clone());
    let all: Vec<(subcraft_core::TextureSetId, u32)> = state.session.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    egui::Window::new(window_title("Bake Mesh Maps")).open(&mut open).collapsible(false).default_width(440.0).show(ctx, |ui| {
        let busy = state.bake.job.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            let b = &mut state.bake;
            ui.horizontal(|ui| {
                ui.label(dim("Bake"));
                ui.add_space(8.0);
                let only = format!("Only {sel_name}");
                segmented(ui, &mut b.all_sets, &[(true, "All texture sets"), (false, only.as_str())]);
            });
            ui.add_space(4.0);
            egui::Grid::new("bake_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label(dim("Size"));
                egui::ComboBox::from_id_salt("bake_size").selected_text(format!("{}²", b.settings.size)).show_ui(ui, |ui| {
                    for r in [256u32, 512, 1024, 2048, 4096] {
                        ui.selectable_value(&mut b.settings.size, r, format!("{r}²"));
                    }
                });
                ui.end_row();
                ui.label(dim("High poly"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut b.high_poly).hint_text("optional (none = low on low)").desired_width(220.0));
                    if ui.button("Browse…").clicked() {
                        crate::files::pick(ui.ctx(), crate::files::Purpose::BakeHighPoly, crate::files::MESHES, cfg!(target_arch = "wasm32"), None);
                    }
                    if !b.high_poly.is_empty() && crate::widgets::icon_button(ui, crate::icons::X, "Clear").clicked() {
                        b.high_poly.clear();
                    }
                });
                ui.end_row();
                if !b.high_poly.is_empty() {
                    ui.label(dim("Cage front / rear"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut b.settings.cage_front).speed(0.001).range(0.0..=0.5));
                        ui.add(egui::DragValue::new(&mut b.settings.cage_rear).speed(0.001).range(0.0..=0.5));
                    });
                    ui.end_row();
                }
                ui.label(dim("AO samples"));
                ui.add(egui::Slider::new(&mut b.settings.ao_samples, 4..=256).logarithmic(true));
                ui.end_row();
                ui.label(dim("AO distance"));
                ui.add(egui::Slider::new(&mut b.settings.ao_distance, 0.01..=1.0).logarithmic(true));
                ui.end_row();
                ui.label(dim("Curvature radius"));
                ui.add(egui::Slider::new(&mut b.settings.curvature_radius, 0.002..=0.1).logarithmic(true));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(caption("Maps"));
            ui.horizontal_wrapped(|ui| {
                for k in subcraft_core::MeshMapKind::ALL {
                    let mut on = b.settings.maps.contains(&k);
                    if ui.checkbox(&mut on, k.display_name()).changed() {
                        if on {
                            b.settings.maps.push(k);
                        } else {
                            b.settings.maps.retain(|m| *m != k);
                        }
                    }
                }
            });
            ui.add_space(2.0);
            hint(ui, "Curvature feeds the Edges generator, AO the Cavity generator; baked AO also shades the 3D view and fills AO in exports.");
        });
        ui.add_space(6.0);
        divider(ui);
        ui.add_space(6.0);
        match &state.bake.job {
            Some(job) => {
                ui.label(dim(&job.stage()));
                ui.add(egui::ProgressBar::new(job.fraction()).show_percentage().fill(theme::ACCENT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if secondary_button(ui, "Cancel").clicked() {
                        job.cancel();
                    }
                });
            }
            None => {
                let can = !state.bake.settings.maps.is_empty();
                let go = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(can, |ui| primary_button(ui, format!("{}  Bake", crate::icons::MAGIC_WAND))).inner.clicked()
                });
                if go.inner {
                    let mut settings = state.bake.settings.clone();
                    settings.maps.sort();
                    let request = subcraft_app::BakeRequest {
                        settings,
                        high_poly: (!state.bake.high_poly.trim().is_empty()).then(|| PathBuf::from(state.bake.high_poly.trim())),
                        texture_sets: if state.bake.all_sets { all.clone() } else { vec![(sel_id, sel_slot)] },
                    };
                    state.bake.job = Some(state.session.start_bake(request));
                    state.set_status("Baking…", now);
                }
            }
        }
    });
    state.bake.open = open;
}
