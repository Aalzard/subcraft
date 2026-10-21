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
    OpenRecent,
}

pub struct ExportUi {
    pub open: bool,
    pub settings: ExportSettings,
    pub template: usize,
    pub folder: String,
    /// Template name last stored in / read from the project settings.
    pub known_template: String,
}

impl Default for ExportUi {
    fn default() -> Self {
        #[cfg(target_arch = "wasm32")]
        let folder = PathBuf::from(crate::files::DOWNLOADS);
        #[cfg(not(target_arch = "wasm32"))]
        let folder = std::env::current_dir().map(|d| d.join("exports")).unwrap_or_else(|_| PathBuf::from("exports"));
        Self { open: false, settings: ExportSettings::default(), template: 0, folder: folder.display().to_string(), known_template: String::new() }
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
            state.welcome.remember(&p);
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
        // The New Project window shows what is in the model and the project options.
        Pending::OpenMesh => state.new_project.show_window(&state.recovery.prefs.new_project.clone()),
        Pending::OpenProject => crate::files::pick(ctx, crate::files::Purpose::OpenProject, PROJECT_FILTER, false, None),
        Pending::OpenRecent => {
            if let Some(path) = state.welcome.pending_recent.take() {
                match Session::open_recent(&path) {
                    Ok((s, warnings)) => {
                        state.set_session(s); state.welcome.remember(&path);
                        state.set_status(if warnings.is_empty() { format!("Opened {}", path.display()) } else { format!("Opened with {} warning(s): {}", warnings.len(), warnings[0]) }, _now);
                    }
                    Err(e) => state.set_status(format!("Could not open recent file: {e}"), _now),
                }
            }
        }
    }
}

pub fn open_project(state: &mut UiState, path: &std::path::Path, now: f64) {
    match Session::open_project(path) {
        Ok(s) => {
            state.set_session(s);
            state.welcome.remember(path);
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
                ui.label(dim("One ZIP download containing all texture maps."));
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
        #[cfg(not(target_arch = "wasm32"))]
        let folder = PathBuf::from(&ex.folder);
        ui.add_space(6.0);
        divider(ui);
        ui.add_space(4.0);
        export_options(ui, state);
        ui.add_space(4.0);
        divider(ui);
        ui.add_space(4.0);
        let choices = state.session.export_choices(&settings);
        let enabled = choices.iter().filter(|f| f.enabled).count();
        export_map_list(ui, state, &settings, &choices);
        let plan_len = enabled;
        ui.add_space(6.0);
        let go = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if plan_len == 0 {
                ui.label(dim("Check at least one map to export."));
            }
            ui.add_enabled_ui(plan_len > 0, |ui| primary_button(ui, format!("{}  Export", crate::icons::EXPORT))).inner.clicked()
        });
        if go.inner {
            #[cfg(not(target_arch = "wasm32"))]
            match state.session.export_textures(&folder, &settings) {
                Ok(files) => state.set_status(format!("Exported {} files to {}", files.len(), folder.display()), now),
                Err(e) => state.set_status(format!("Export failed: {e}"), now),
            }
            #[cfg(target_arch = "wasm32")]
            match state.session.export_archive(&settings) {
                Ok(archive)=>if crate::web::download(&archive.name,&archive.bytes) {state.set_status(format!("Downloaded {} maps in {}",archive.files,archive.name),now);} else {state.set_status("Could not start the ZIP download; export again to retry",now);},
                Err(e)=>state.set_status(format!("Export failed: {e}"),now),
            }
        }
    });
    state.export.open = open;
}

/// Project-wide export choices: template memory, normal-map convention, UDIM folders.
fn export_options(ui: &mut egui::Ui, state: &mut UiState) {
    let templates = ExportTemplate::builtin();
    let stored = state.session.settings().export_template.clone();
    if stored != state.export.known_template || state.export.known_template.is_empty() {
        // A different project (or first frame): follow its template; one that names none is not rewritten.
        match templates.iter().position(|t| t.name == stored) {
            Some(i) => { state.export.template = i; state.export.known_template = stored; }
            None => state.export.known_template = templates[state.export.template].name.clone(),
        }
    }
    let name = templates[state.export.template].name.clone();
    if name != state.export.known_template {
        let mut project = state.session.settings().clone();
        project.export_template = name.clone();
        state.session.set_settings(project);
        state.export.known_template = name;
    }
    let mut project = state.session.settings().clone();
    ui.horizontal(|ui| {
        ui.label(dim("Normal maps"));
        let text = project.normal_format.map_or("Template default", |f| f.label());
        egui::ComboBox::from_id_salt("export_normal_format").selected_text(text).show_ui(ui, |ui| {
            ui.selectable_value(&mut project.normal_format, None, "Template default");
            ui.selectable_value(&mut project.normal_format, Some(subcraft_doc::NormalFormat::OpenGl), subcraft_doc::NormalFormat::OpenGl.label());
            ui.selectable_value(&mut project.normal_format, Some(subcraft_doc::NormalFormat::DirectX), subcraft_doc::NormalFormat::DirectX.label());
        });
    });
    if state.session.udim_tile_count() >= 2 {
        ui.checkbox(&mut project.udim_folders, "Export Each UDIM to a Separate Folder")
            .on_hover_text("On: 1001/, 1002/ ... one sub-folder per tile. Off: one folder, file names end in .1001, .1002");
    }
    if project != *state.session.settings() {
        state.session.set_settings(project);
    }
}

/// What a click in the map list asks for (applied by the session, see `edit_export_selection`).
enum MapClick {
    Toggle(String, String, bool),
    Solo(String, String),
    Set(String, bool),
    All,
    None,
    Invert,
}

/// The per-map checklist with Select all / None / Invert. Alt+click solos a map.
fn export_map_list(ui: &mut egui::Ui, state: &mut UiState, settings: &ExportSettings, choices: &[subcraft_export::PlannedFile]) {
    let mut click = None;
    ui.horizontal(|ui| {
        ui.label(caption(&format!("{} of {} files", choices.iter().filter(|f| f.enabled).count(), choices.len())));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Invert").clicked() { click = Some(MapClick::Invert); }
            if ui.small_button("None").clicked() { click = Some(MapClick::None); }
            if ui.small_button("Select all").clicked() { click = Some(MapClick::All); }
        });
    });
    egui::Frame::new().fill(theme::FIELD).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let mut current: Option<&str> = None;
            for f in choices {
                if current != Some(f.set.as_str()) {
                    current = Some(&f.set);
                    let mut all_on = choices.iter().filter(|c| c.set == f.set).all(|c| c.enabled);
                    if ui.checkbox(&mut all_on, RichText::new(&f.set).strong()).clicked() {
                        click = Some(MapClick::Set(f.set.clone(), all_on));
                    }
                }
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    let mut on = f.enabled;
                    let r = ui.checkbox(&mut on, RichText::new(&f.suffix)).on_hover_text("Alt+click: export only this map (Alt+click again to bring the others back)");
                    if r.clicked() {
                        click = Some(if ui.input(|i| i.modifiers.alt) { MapClick::Solo(f.set.clone(), f.suffix.clone()) } else { MapClick::Toggle(f.set.clone(), f.suffix.clone(), on) });
                    }
                    ui.label(RichText::new(&f.path).monospace().small().color(if f.enabled { TEXT_DIM } else { theme::TEXT_DIM.gamma_multiply(0.5) }));
                });
            }
        });
    });
    if let Some(click) = click {
        state.session.edit_export_selection(settings, |sel, all| match click {
            MapClick::Toggle(set, suffix, on) => sel.set_on(&set, &suffix, on),
            MapClick::Solo(set, suffix) => sel.solo(&set, &suffix, all),
            MapClick::Set(set, on) => sel.set_set_on(&set, all, on),
            MapClick::All => sel.select_all(),
            MapClick::None => sel.select_none(all),
            MapClick::Invert => sel.invert(all),
        });
    }
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
    /// Kept in the dialog until another bake starts, including actionable failures.
    pub result: Option<Result<usize, String>>,
    pub preview: bool,
    pub cage: bool,
    pub live_maps: bool,
    pub displayed_map: Option<subcraft_core::MeshMapKind>,
    pub follow_maps: bool,
    pub preview_mesh: Option<std::sync::Arc<subcraft_mesh::Mesh>>,
    preview_source: Option<subcraft_app::bake_job::BakePreviewMesh>,
    preview_job: Option<subcraft_app::BakePreviewJob>,
    preview_path: String,
    preview_scene: Option<std::sync::Arc<subcraft_mesh::Mesh>>,
    pub preview_error: Option<String>,
    pub preview_uploaded: bool,
    preview_checked_at: f64,
}

impl Default for BakeUi {
    fn default() -> Self {
        Self { open: false, settings: subcraft_app::BakeSettings::default(), high_poly: String::new(), all_sets: true, job: None, result: None,
            preview: true, cage: true, live_maps: true, displayed_map: None, follow_maps: true, preview_mesh: None,
            preview_job: None, preview_source: None, preview_path: String::new(), preview_scene: None, preview_error: None, preview_uploaded: false, preview_checked_at: 0. }
    }
}

impl BakeUi {
    pub fn preview_pending(&self) -> bool { self.preview_job.is_some() }
    pub fn overlay_active(&self) -> bool {
        self.open && self.preview && self.job.is_none() && self.result.is_none() && self.displayed_map.is_none() && self.preview_mesh.is_some()
    }
}

/// Apply results of a running bake (call every frame).
pub fn poll_bake(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let b = &mut state.bake;
    let scene = &state.session.scene().mesh;
    let scene_changed = b.preview_scene.as_ref().is_some_and(|m| !std::sync::Arc::ptr_eq(m, scene));
    if b.preview_scene.is_none() { b.preview_scene = Some(scene.clone()); }
    if scene_changed || b.preview_path != b.high_poly.trim() {
        if !b.preview_path.is_empty() { b.result = None; b.displayed_map = None; }
        b.preview_job = None; b.preview_mesh = None; b.preview_uploaded = false; b.preview_error = None;
        b.preview_source = None;
        b.preview_path = b.high_poly.trim().to_string(); b.preview_scene = Some(scene.clone());
        if scene_changed { b.displayed_map = None; }
        if scene_changed { b.result = None; }
    }
    if b.open && b.preview && !b.preview_path.is_empty() { ctx.request_repaint_after(std::time::Duration::from_millis(250)); }
    if b.open && b.preview && !b.preview_path.is_empty() && b.preview_job.is_none() && now - b.preview_checked_at >= 0.25 {
        b.preview_checked_at = now;
        let changed = b.preview_source.as_ref().is_some_and(|source| !source.is_current(std::path::Path::new(&b.preview_path)));
        if changed || b.preview_error.is_some() {
            b.preview_job = Some(subcraft_app::BakePreviewJob::start(b.preview_path.clone().into()));
            b.preview_error = None;
            if b.job.is_none() { b.result = None; b.displayed_map = None; }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
    if b.open && b.preview && b.job.is_none() && b.result.is_none() && !b.preview_path.is_empty()
        && b.preview_mesh.is_none() && b.preview_job.is_none() && b.preview_error.is_none() {
        b.preview_job = Some(subcraft_app::BakePreviewJob::start(b.preview_path.clone().into()));
    }
    if let Some(job) = &b.preview_job {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
        if let Some(result) = job.poll() {
            b.preview_job = None;
            match result { Ok(source) => { b.preview_mesh = Some(source.mesh.clone()); b.preview_source = Some(source); b.preview_uploaded = false; }, Err(e) => b.preview_error = Some(e) }
        }
    }
    let Some(job) = state.bake.job.as_mut() else { return };
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    let result = job.poll(&mut state.session);
    if state.bake.follow_maps && let Some((_, kind)) = job.latest_map { state.bake.displayed_map = Some(kind); }
    if let Some(result) = result {
        state.bake.job = None;
        match &result {
            Ok(n) => {
                state.bake.displayed_map = None;
                state.bake.follow_maps = true;
                state.recovery.prefs.viewport.view_mode = 0;
                state.set_status(format!("Baked mesh maps for {n} texture set(s). See them in the 2D view."), now);
                state.recovery.autosaver.request_soon();
            }
            Err(e) => state.set_status(format!("Bake: {e}"), now),
        }
        state.bake.result = Some(result);
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
    egui::Window::new(window_title("Bake Mesh Maps")).open(&mut open).collapsible(false).default_width(440.0)
        .default_pos(egui::pos2(32., 100.)).show(ctx, |ui| {
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
            hint(ui, "High poly needs no UVs; keep it aligned with the low poly. Cage distances are fractions of the model diagonal. A normal bake adds a live Baked normal layer; AO and curvature feed shading and generators.");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.bake.preview, "High / low overlay");
            ui.checkbox(&mut state.bake.cage, "Cage wireframe");
            ui.checkbox(&mut state.bake.live_maps, "Live maps");
        });
        if state.bake.preview_pending() { ui.label(dim("Loading high-poly preview…")); }
        if let Some(error) = &state.bake.preview_error { ui.colored_label(theme::WARNING, error); }
        if !busy && !state.bake.high_poly.trim().is_empty() && secondary_button(ui, "Reload high poly").clicked() {
            state.bake.preview_mesh = None; state.bake.preview_source = None; state.bake.preview_job = None;
            state.bake.preview_error = None; state.bake.preview_uploaded = false; state.bake.result = None;
            state.bake.displayed_map = None;
        }
        hint(ui, "Blue: high poly · Red: exposed low poly · Pale lines: front/rear cage. Orbit to inspect. Imported transforms are preserved.");
        if state.session.mesh_maps(sel_id).is_some() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.bake.follow_maps, "Follow bake");
                egui::ComboBox::from_id_salt("bake_display_map")
                    .selected_text(state.bake.displayed_map.map_or("Material", |k| k.display_name())).show_ui(ui, |ui| {
                        if ui.selectable_value(&mut state.bake.displayed_map, None, "Material").clicked() { state.bake.follow_maps = false; }
                        if let Some(maps) = state.session.mesh_maps(sel_id) { for (kind, _) in maps.iter() {
                            if ui.selectable_value(&mut state.bake.displayed_map, Some(kind), kind.display_name()).clicked() { state.bake.follow_maps = false; }
                        }}
                    });
            });
        }
        if !busy && state.bake.preview_mesh.is_some() && state.bake.result.is_some()
            && secondary_button(ui, "Inspect high / low geometry").clicked() {
            state.bake.result = None; state.bake.displayed_map = None; state.bake.follow_maps = false;
        }
        ui.add_space(6.0);
        divider(ui);
        ui.add_space(6.0);
        if let Some(result) = &state.bake.result {
            match result {
                Ok(n) => { ui.label(dim(&format!("Bake complete: {n} texture set(s). Inspect Baked normal in Layers or mesh maps in 2D View."))); }
                Err(e) => { ui.colored_label(theme::WARNING, format!("Bake failed: {e}")); }
            }
            ui.add_space(6.0);
        }
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
                    state.bake.result = None;
                    state.bake.displayed_map = None;
                    state.bake.follow_maps = true;
                    let mut settings = state.bake.settings.clone();
                    settings.maps.sort();
                    let request = subcraft_app::BakeRequest {
                        settings,
                        high_poly: (!state.bake.high_poly.trim().is_empty()).then(|| PathBuf::from(state.bake.high_poly.trim())),
                        texture_sets: if state.bake.all_sets { all.clone() } else { vec![(sel_id, sel_slot)] },
                    };
                    let prepared = (state.bake.preview_path == state.bake.high_poly.trim()
                        && state.bake.preview_scene.as_ref().is_some_and(|m| std::sync::Arc::ptr_eq(m, &state.session.scene().mesh)))
                        .then(|| state.bake.preview_source.clone()).flatten();
                    state.bake.job = Some(state.session.start_bake_prepared(request, prepared));
                    state.set_status("Baking…", now);
                }
            }
        }
    });
    state.bake.open = open;
}

#[cfg(test)]
mod bake_tests {
    use super::*;
    #[test]
    fn a_successful_bake_always_returns_to_material() {
        let ctx = egui::Context::default();
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        state.bake.open = true; state.bake.displayed_map = Some(subcraft_core::MeshMapKind::Thickness);
        state.recovery.prefs.viewport.view_mode = 5;
        let ts = state.session.selected_texture_set();
        state.bake.job = Some(state.session.start_bake(subcraft_app::BakeRequest {
            high_poly: None, texture_sets: vec![(ts.id, ts.material_slot)],
            settings: subcraft_app::BakeSettings { size: 128, maps: vec![subcraft_core::MeshMapKind::Normal], ..Default::default() },
        }));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.bake.job.is_some() {
            poll_bake(&mut state, &ctx, 0.);
            assert!(std::time::Instant::now() < deadline); std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(matches!(state.bake.result, Some(Ok(1))));
        assert!(state.bake.displayed_map.is_none());
        assert_eq!(state.recovery.prefs.viewport.view_mode, 0);
    }

    #[test]
    fn replacing_a_high_poly_file_refreshes_its_preview_without_reload() {
        let path = std::env::temp_dir().join(format!("subcraft-watch-{}.obj", subcraft_core::LayerId::new()));
        std::fs::write(&path, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        let ctx = egui::Context::default();
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        state.bake.open = true; state.bake.high_poly = path.display().to_string();
        let wait = |state: &mut UiState, expected, time| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                poll_bake(state, &ctx, time);
                if state.bake.preview_mesh.as_ref().is_some_and(|m| m.bounds.max.x == expected) { break; }
                assert!(std::time::Instant::now() < deadline, "preview update timed out");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        wait(&mut state, 1., 1.);
        std::fs::write(&path, "v 20 0 0\nv 21 0 0\nv 20 1 0\nf 1 2 3\n").unwrap();
        wait(&mut state, 21., 2.);
        assert!(state.bake.preview_error.is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn preview_state_is_transient_and_invalidated_when_scene_or_path_changes() {
        let ctx = egui::Context::default();
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        let before = state.session.document().clone();
        state.bake.open = true;
        state.bake.preview_path = "old.obj".into();
        state.bake.high_poly = "old.obj".into();
        state.bake.preview_scene = Some(state.session.scene().mesh.clone());
        state.bake.preview_mesh = Some(std::sync::Arc::new(subcraft_mesh::primitives::cube(2.)));
        assert!(state.bake.overlay_active());
        state.bake.displayed_map = Some(subcraft_core::MeshMapKind::Normal);
        assert!(!state.bake.overlay_active());
        state.bake.high_poly.clear();
        poll_bake(&mut state, &ctx, 0.);
        assert!(state.bake.preview_mesh.is_none());
        assert!(state.bake.displayed_map.is_none());
        assert_eq!(state.session.document(), &before);
        state.bake.preview_mesh = Some(std::sync::Arc::new(subcraft_mesh::primitives::cube(2.)));
        state.bake.result = Some(Ok(1));
        state.session = Session::new(subcraft_mesh::primitives::cube(3.), 128);
        poll_bake(&mut state, &ctx, 1.);
        assert!(state.bake.preview_mesh.is_none());
        assert!(state.bake.result.is_none());
    }

    fn draw(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600., 940.))), events,
            ..Default::default()
        }, |_| bake_window(state, ctx, 0.));
        output.textures_delta.clear();
        output
    }

    #[test]
    fn clicking_bake_dispatches_a_job_and_clears_the_previous_result() {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        state.bake.open = true;
        state.bake.settings.size = 128;
        state.bake.settings.maps = vec![subcraft_core::MeshMapKind::Normal];
        state.bake.result = Some(Err("Previous failure".into()));
        draw(&mut state, &ctx, vec![]);
        let output = draw(&mut state, &ctx, vec![]);
        let pos = output.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text.ends_with("  Bake") => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        }).expect("visible Bake button");
        let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        draw(&mut state, &ctx, vec![egui::Event::PointerMoved(pos), button(true)]);
        draw(&mut state, &ctx, vec![button(false)]);
        assert!(state.bake.job.is_some(), "the actual button starts the background job");
        assert!(state.bake.result.is_none());
    }

    #[test]
    fn bake_errors_remain_visible_in_the_dialog() {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        state.bake.open = true;
        state.bake.result = Some(Err("align models and increase Cage front / rear".into()));
        // Floating windows need an initial sizing frame before they paint their text.
        draw(&mut state, &ctx, vec![]);
        draw(&mut state, &ctx, vec![]);
        for _ in 0..3 {
            let output = draw(&mut state, &ctx, vec![]);
            assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.job.text.contains("Bake failed: align models"))));
        }
    }
}
