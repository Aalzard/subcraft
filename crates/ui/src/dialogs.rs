//! File dialogs and windows: save / open project, export textures, unsaved-changes prompt.

use crate::i18n::tr;
use std::path::PathBuf;

use egui::RichText;
use subcraft_app::{ExportSettings, ExportTemplate, Padding, Session};

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
    /// Import a Painter `.spp`: the dropped one (`UiState::dropped_model`), else pick one.
    ImportPainter,
}

pub struct ExportUi {
    pub open: bool,
    pub settings: ExportSettings,
    /// Key of the selected preset ([`ExportTemplate::key`]: built-in name or user preset id).
    pub preset: String,
    /// Built-ins + user presets, loaded from the library on first use.
    pub preset_list: Option<subcraft_app::export_presets::ExportPresets>,
    /// Preset editor open, and its state.
    pub edit_presets: bool,
    pub editor: crate::export_presets::PresetEditor,
    pub folder: String,
    /// Template name last stored in / read from the project settings.
    pub known_template: String,
    /// Opened from File > Export 2D view: write only what the 2D panel shows.
    pub view_2d_only: bool,
    pub scene_format: usize,
    /// Raw baked mesh maps the "Baked Mesh Maps" section exports (issue #93); maps not baked
    /// in the project are not offered. Empty until the section is first shown (then all on).
    pub baked: std::collections::BTreeSet<subcraft_core::MeshMapKind>,
    pub baked_initialized: bool,
    /// Export only the selected texture set's baked maps.
    pub baked_selected_only: bool,
}

impl Default for ExportUi {
    fn default() -> Self {
        let folder = std::env::current_dir().map(|d| d.join("exports")).unwrap_or_else(|_| PathBuf::from("exports"));
        Self { open: false, settings: ExportSettings::default(), preset: ExportTemplate::pbr_metal_rough().name, preset_list: None, edit_presets: false, editor: Default::default(), folder: folder.display().to_string(), known_template: String::new(), view_2d_only: false, scene_format: 0, baked: Default::default(), baked_initialized: false, baked_selected_only: false }
    }
}

impl ExportUi {
    /// The preset list, loading it from `lib` on first use.
    pub fn presets(&mut self, lib: Option<&subcraft_app::assets::UserLibrary>) -> &mut subcraft_app::export_presets::ExportPresets {
        self.preset_list.get_or_insert_with(|| subcraft_app::export_presets::ExportPresets::load(lib))
    }

    /// The selected preset (the default template when the key is unknown).
    pub fn current_template(&mut self, lib: Option<&subcraft_app::assets::UserLibrary>) -> ExportTemplate {
        let key = self.preset.clone();
        self.presets(lib).find(&key).cloned().unwrap_or_else(ExportTemplate::pbr_metal_rough)
    }
}

const PROJECT_FILTER: (&str, &[&str]) = ("SubCraft project", &[subcraft_app::PROJECT_EXTENSION]);

/// Save to the current path, or ask for one. Returns true when saved.
pub fn save(state: &mut UiState, save_as: bool, now: f64) -> bool {
    let path = if save_as || state.session.project_path().is_none() {
        let name = format!("{}.{}", state.session.export_name(), subcraft_app::PROJECT_EXTENSION);
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
        Pending::NewSample => state.set_session(Session::default_model()),
        // The New Project window shows what is in the model and the project options.
        Pending::OpenMesh => {
            state.new_project.show_window(&state.recovery.prefs.new_project.clone());
            if let Some(model) = state.dropped_model.take() { state.new_project.load_model(&model); }
        }
        Pending::OpenProject => crate::files::pick_with_filters(crate::files::Purpose::OpenProject, &crate::files::OPEN_FILTERS, None),
        Pending::ImportPainter => match state.dropped_model.take() {
            Some(path) => crate::spp_import::start(state, &path),
            None => crate::files::pick(ctx, crate::files::Purpose::OpenProject, crate::files::PAINTER_PROJECTS, false, None),
        },
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
            use subcraft_app::unsaved::{Answer, Outcome, resolve};
            let answer = if primary_button(ui, tr("Save")).clicked() { Some(Answer::Save) }
                else if secondary_button(ui, tr("Don't save")).clicked() { Some(Answer::DontSave) }
                else if secondary_button(ui, tr("Cancel")).clicked() { Some(Answer::Cancel) }
                else { None };
            if let Some(answer) = answer {
                let saved = answer == Answer::Save && save(state, false, now);
                // A cancelled file dialog or failed save keeps the question open; nothing is discarded.
                if answer != Answer::Save || saved {
                    decided = Some(if resolve(answer, saved) == Outcome::Run { Some(pending) } else { None });
                }
            }
        });
    });
    match decided {
        Some(result) => {
            state.confirm = None;
            if result == Some(Pending::Close) {
                state.allow_close = true;
            }
            if result.is_none() {
                // Cancelled: the file that was dropped or picked must not open with the next request.
                state.dropped_model = None;
                state.welcome.pending_recent = None;
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
    let lib = state.user_library.clone();
    let load_errors = state.export.preset_list.is_none().then(|| state.export.presets(lib.as_ref()).load_errors.clone()).unwrap_or_default();
    if let Some(e) = load_errors.first() {
        state.set_status(format!("Some export presets could not be read: {e}"), now);
    }
    let mesh_name = state.session.export_name();
    let mut preset_status = None;
    // Every export knows what the 2D panel shows, so the "2D View" template (and custom
    // presets using `Source::View2d`) write it.
    let view = crate::panels::uv_view::export_view(state.uv.source);
    state.export.settings.view_2d = Some(view);
    let view_only = state.export.view_2d_only;
    let title = if view_only { "Export 2D View" } else { "Export Textures" };
    let width = if state.export.edit_presets && !view_only { 860.0 } else { 520.0 };
    crate::widgets::window(ctx, window_title(title)).open(&mut open).collapsible(false).default_width(width).show(ctx, |ui| {
        let ex = &mut state.export;
        egui::Grid::new("export_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            {
                ui.label(dim("Folder"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut ex.folder).desired_width(300.0));
                    if ui.button(tr("Browse…")).clicked()
                        && let Some(p) = rfd::FileDialog::new().set_directory(&ex.folder).pick_folder()
                    {
                        ex.folder = p.display().to_string();
                    }
                });
                ui.end_row();
            }
            if view_only {
                ui.label(dim("Image"));
                ui.label(format!("{} (as shown in the 2D view)", crate::panels::uv_view::label(state.uv.source)));
            } else {
                ui.label(dim("Preset"));
                let list = ex.preset_list.get_or_insert_with(|| subcraft_app::export_presets::ExportPresets::load(lib.as_ref()));
                if let Some(s) = crate::export_presets::preset_bar(ui, list, &mut ex.preset, &mut ex.editor) {
                    preset_status = Some(s);
                }
            }
            ui.end_row();
            ui.label(dim("File type"));
            ui.horizontal(|ui| {
                crate::export_presets::format_combo(ui, "export_format", &mut ex.settings.format_override, &mut ex.settings.dither, Some(tr("Based on output template")));
            });
            ui.end_row();
            ui.label(dim("Size"));
            ui.horizontal(|ui| {
                crate::export_presets::size_combo(ui, "export_size", &mut ex.settings.size, tr("Based on each texture set's size"));
                let preset_size = ex.preset_list.as_ref().and_then(|l| l.find(&ex.preset)).and_then(|p| p.size);
                if let Some(s) = preset_size {
                    ui.label(dim(&format!("the preset writes {s} × {s}"))).on_hover_text(tr("This preset chooses its own size; it wins over this setting"));
                }
            });
            ui.end_row();
            ui.label(dim("Padding"));
            ui.horizontal(|ui| {
                let preset_padding = ex.preset_list.as_ref().and_then(|l| l.find(&ex.preset)).and_then(|p| p.padding);
                if preset_padding.is_some() {
                    ui.label(dim("set by the preset")).on_hover_text(tr("This preset chooses its own padding; maps without one use it"));
                    return;
                }
                let mut padding = Some(ex.settings.padding);
                crate::export_presets::padding_combo(ui, "export_padding", &mut padding, None);
                ex.settings.padding = padding.unwrap_or(Padding::Infinite);
            });
            ui.end_row();
            ui.label(dim("Height to normal"));
            ui.add(egui::Slider::new(&mut ex.settings.height_to_normal, 0.0..=0.1));
            ui.end_row();
        });
        if !view_only {
            let header = egui::CollapsingHeader::new(RichText::new("Preset editor").strong()).id_salt("preset_editor").open(Some(ex.edit_presets)).show(ui, |ui| {
                let list = ex.preset_list.get_or_insert_with(|| subcraft_app::export_presets::ExportPresets::load(lib.as_ref()));
                let key = ex.preset.clone();
                // Scrolls on its own so the map checklist and Export stay on screen.
                let max_h = (ui.ctx().content_rect().height() * 0.42).max(260.0);
                egui::ScrollArea::vertical().id_salt("preset_editor_scroll").max_height(max_h).show(ui, |ui| {
                    if let Some(s) = crate::export_presets::preset_editor(ui, list, &key, &mut ex.editor, &mesh_name) {
                        preset_status = Some(s);
                    }
                });
            });
            if header.header_response.clicked() {
                ex.edit_presets = !ex.edit_presets;
            }
        }
        ex.settings.template = ex.current_template(lib.as_ref());
        let settings = ex.settings.clone();
        let folder = PathBuf::from(&ex.folder);
        if view_only {
            ui.add_space(6.0);
            let go = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| primary_button(ui, format!("{}  Export", crate::icons::EXPORT)).clicked());
            if go.inner {
                match state.session.export_view_2d(&folder, view, &settings) {
                    Ok(files) if files.is_empty() => state.set_status("Nothing to export: the 2D view's source is missing", now),
                    Ok(files) => state.set_status(format!("Exported {}", files[0].display()), now),
                    Err(e) => state.set_status(format!("Export failed: {e}"), now),
                }
            }
            return;
        }
        ui.add_space(6.0);
        divider(ui);
        ui.add_space(4.0);
        export_options(ui, state);
        baked_maps_section(ui, state, &folder, &settings, now);
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
        ui.separator();
        let formats = subcraft_export::scene::SceneFormat::ALL;
        let mut scene_go = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("scene_format").selected_text(formats[state.export.scene_format.min(formats.len() - 1)].label()).show_ui(ui, |ui| {
                for (i, f) in formats.iter().enumerate() { ui.selectable_value(&mut state.export.scene_format, i, f.label()); }
            });
            scene_go = ui.button(tr("Export model + textures")).on_hover_text(tr("Writes the mesh with a textured PBR material (glTF PBR, USD preview surface, or OBJ with separate maps) into the folder")).clicked();
        });
        if scene_go {
            let format = formats[state.export.scene_format.min(formats.len() - 1)];
            match state.session.export_scene(&folder, &settings, format) {
                Ok(files) => state.set_status(format!("Exported {} files to {}", files.len(), folder.display()), now),
                Err(e) => state.set_status(format!("Export failed: {e}"), now),
            }
        }
        if go.inner {
            match state.session.export_textures(&folder, &settings) {
                Ok(files) => state.set_status(format!("Exported {} files to {}", files.len(), folder.display()), now),
                Err(e) => state.set_status(format!("Export failed: {e}"), now),
            }
        }
    });
    if let Some(s) = preset_status {
        state.set_status(s, now);
    }
    state.export.open = open;
}

/// "Baked Mesh Maps": export raw baked maps (Normal, AO, Curvature...) of every texture set at once,
/// one file per set and map (`<mesh>_<set>_baked_<map>`), apart from the material textures.
fn baked_maps_section(ui: &mut egui::Ui, state: &mut UiState, folder: &std::path::Path, settings: &ExportSettings, now: f64) {
    let available = state.session.baked_map_kinds();
    let ex = &mut state.export;
    if !ex.baked_initialized && !available.is_empty() {
        ex.baked = available.iter().copied().collect();
        ex.baked_initialized = true;
    }
    let mut go = false;
    egui::CollapsingHeader::new("Baked Mesh Maps").id_salt("baked_maps").default_open(false).show(ui, |ui| {
        ui.label(dim("Raw baked maps, not the material textures. One file per texture set and map, named after both."));
        if available.is_empty() {
            ui.label(dim("Nothing baked yet: bake mesh maps first (Bake in the top bar)."));
            return;
        }
        ui.horizontal(|ui| {
            if ui.small_button("All").clicked() { ex.baked = available.iter().copied().collect(); }
            if ui.small_button("None").clicked() { ex.baked.clear(); }
        });
        ui.horizontal_wrapped(|ui| {
            for kind in &available {
                let mut on = ex.baked.contains(kind);
                if ui.checkbox(&mut on, kind.display_name()).changed() {
                    if on { ex.baked.insert(*kind); } else { ex.baked.remove(kind); }
                }
            }
        });
        ui.checkbox(&mut ex.baked_selected_only, "Selected texture set only");
        let chosen = available.iter().filter(|k| ex.baked.contains(k)).count();
        ui.horizontal(|ui| {
            go = ui.add_enabled(chosen > 0, egui::Button::new(format!("{}  Export baked maps", crate::icons::EXPORT))).clicked();
            if chosen == 0 { ui.label(dim("Check at least one map.")); }
        });
    });
    if go {
        let kinds: Vec<_> = available.into_iter().filter(|k| state.export.baked.contains(k)).collect();
        match state.session.export_baked_maps(folder, &kinds, state.export.baked_selected_only, settings) {
            Ok(files) if files.is_empty() => state.set_status("No baked maps to export for the chosen texture sets", now),
            Ok(files) => state.set_status(format!("Exported {} baked maps to {}", files.len(), folder.display()), now),
            Err(e) => state.set_status(format!("Export failed: {e}"), now),
        }
    }
}

/// Project-wide export choices: template memory, normal-map convention, UDIM folders.
fn export_options(ui: &mut egui::Ui, state: &mut UiState) {
    let lib = state.user_library.clone();
    let stored = state.session.settings().export_template.clone();
    if stored != state.export.known_template || state.export.known_template.is_empty() {
        // A different project (or first frame): follow its preset (user presets by id, built-ins
        // by name); one that names none, or a preset missing from this library, is not rewritten.
        match state.export.presets(lib.as_ref()).find(&stored).map(|p| p.key().to_string()) {
            Some(key) => { state.export.preset = key; state.export.known_template = stored; }
            None => state.export.known_template = state.export.preset.clone(),
        }
    }
    let name = state.export.current_template(lib.as_ref()).key().to_string();
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
            ui.selectable_value(&mut project.normal_format, None, tr("Template default"));
            ui.selectable_value(&mut project.normal_format, Some(subcraft_doc::NormalFormat::OpenGl), subcraft_doc::NormalFormat::OpenGl.label());
            ui.selectable_value(&mut project.normal_format, Some(subcraft_doc::NormalFormat::DirectX), subcraft_doc::NormalFormat::DirectX.label());
        });
    });
    if state.session.udim_tile_count() >= 2 {
        ui.checkbox(&mut project.udim_folders, tr("Export Each UDIM to a Separate Folder"))
            .on_hover_text(tr("On: 1001/, 1002/ ... one sub-folder per tile. Off: one folder, file names end in .1001, .1002"));
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
            if ui.small_button(tr("Invert")).clicked() { click = Some(MapClick::Invert); }
            if ui.small_button(tr("None")).clicked() { click = Some(MapClick::None); }
            if ui.small_button(tr("Select all")).clicked() { click = Some(MapClick::All); }
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
                    let r = ui.checkbox(&mut on, RichText::new(&f.suffix)).on_hover_text(tr("Alt+click: export only this map (Alt+click again to bring the others back)"));
                    if r.clicked() {
                        click = Some(if ui.input(|i| i.modifiers.alt) { MapClick::Solo(f.set.clone(), f.suffix.clone()) } else { MapClick::Toggle(f.set.clone(), f.suffix.clone(), on) });
                    }
                    ui.add(egui::Label::new(RichText::new(&f.path).monospace().small().color(if f.enabled { TEXT_DIM } else { theme::TEXT_DIM.gamma_multiply(0.5) })).truncate()).on_hover_text(&f.path);
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
    /// Texture sets unticked in the baking list (every other set is baked).
    pub skip_sets: std::collections::HashSet<subcraft_core::TextureSetId>,
    /// The baker whose settings show in Mesh map settings (`None`: Common settings).
    pub selected_baker: Option<subcraft_core::MeshMapKind>,
    pub log_tab: BakeLogTab,
    /// Maps of the running or last bake started from the panels (a quick bake has one).
    pub job_maps: Vec<subcraft_core::MeshMapKind>,
    /// The user picked a size; until then it follows the texture set resolution.
    pub size_chosen: bool,
    pub job: Option<subcraft_app::BakeJob>,
    /// Kept in the dialog until another bake starts, including actionable failures.
    pub result: Option<Result<usize, String>>,
    /// Which path baked each map and how long it took (from the last finished job).
    pub report: String,
    /// Non-fatal notes of the last bake (low parts without a high part of the same name).
    pub notices: Vec<String>,
    /// Log of the running or last bake: per-map timing and path (CPU / GPU), warnings, errors.
    pub log: Vec<subcraft_app::BakeLogLine>,
    /// One status row per requested map for the running or last bake.
    pub map_rows: Vec<subcraft_app::MapProgress>,
    pub preview: bool,
    pub cage: bool,
    pub fit_mode: subcraft_core::FitMode,
    /// Opacity of the high poly in the overlay (1 = solid).
    pub high_opacity: f32,
    pub placement_message: Option<&'static str>,
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
    /// Ray-miss heatmap on the low poly of the overlay (red = no high-poly surface reached).
    pub miss_heatmap: bool,
    pub cage_job: Option<subcraft_app::cage_check::CageCheckJob>,
    pub cage_result: Option<subcraft_app::cage_check::CageCheckResult>,
    /// The current `cage_result` is on the GPU.
    pub miss_uploaded: bool,
}

impl Default for BakeUi {
    fn default() -> Self {
        Self { open: false, settings: subcraft_app::BakeSettings::default(), high_poly: String::new(), skip_sets: Default::default(), selected_baker: None, log_tab: BakeLogTab::default(), job_maps: Vec::new(), size_chosen: false, job: None, result: None, report: String::new(), notices: Vec::new(), log: Vec::new(), map_rows: Vec::new(),
            preview: true, cage: true, fit_mode: Default::default(), high_opacity: 0.55, placement_message: None, live_maps: true, displayed_map: None, follow_maps: true, preview_mesh: None,
            preview_job: None, preview_source: None, preview_path: String::new(), preview_scene: None, preview_error: None, preview_uploaded: false, preview_checked_at: 0.,
            miss_heatmap: false, cage_job: None, cage_result: None, miss_uploaded: false }
    }
}

/// Preview size of the ray-miss maps (misses depend on the surface, not on the map size).
pub const MISS_MAP_SIZE: u32 = 512;

impl BakeUi {
    pub fn preview_pending(&self) -> bool { self.preview_job.is_some() }
    /// The heatmap is drawn: toggled on, the overlay is showing, and a check has finished.
    pub fn heatmap_shown(&self) -> bool { self.miss_heatmap && self.overlay_active() && self.cage_result.is_some() }
    pub fn overlay_active(&self) -> bool {
        self.open && self.preview && self.job.is_none() && self.result.is_none() && self.displayed_map.is_none() && self.preview_mesh.is_some()
    }
}

/// Keep the ray-miss check in step with the cage, placement and name matching while the heatmap
/// is on (a new check starts whenever one of them changes; the BVH of the placed high poly is
/// reused).
fn poll_cage_check(state: &mut UiState, ctx: &egui::Context) {
    let b = &mut state.bake;
    if let Some(job) = &b.cage_job && let Some(result) = job.poll() {
        b.cage_job = None;
        if let Some(r) = result { b.cage_result = Some(r); b.miss_uploaded = false; }
    }
    if !(b.open && b.preview && b.miss_heatmap) { return; }
    let Some(high) = b.preview_mesh.clone() else { b.cage_result = None; return };
    let low = state.session.scene().mesh.clone();
    let mut settings = b.settings.clone();
    settings.placement = state.session.settings().bake_placement.clone();
    let key = subcraft_app::cage_check::CageCheckKey::new(&low, &high, &settings, MISS_MAP_SIZE);
    let current = b.cage_result.as_ref().is_some_and(|r| r.key == key);
    let running = b.cage_job.as_ref().is_some_and(|j| j.key == key);
    if !current && !running {
        let sets: Vec<_> = state.session.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
        let placed = b.cage_result.as_ref().map(|r| r.placed.clone());
        b.cage_job = Some(subcraft_app::cage_check::CageCheckJob::start(low, high, placed, settings, sets, MISS_MAP_SIZE));
    }
    if b.cage_job.is_some() { ctx.request_repaint_after(std::time::Duration::from_millis(50)); }
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
    poll_cage_check(state, ctx);
    let Some(job) = state.bake.job.as_mut() else { return };
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    let result = job.poll(&mut state.session);
    if state.bake.follow_maps && let Some((_, kind)) = job.latest_map { state.bake.displayed_map = Some(kind); }
    state.bake.notices = job.notices().to_vec();
    let mut maps = if state.bake.job_maps.is_empty() { state.bake.settings.maps.clone() } else { state.bake.job_maps.clone() };
    maps.sort();
    state.bake.map_rows = job.map_progress(&maps);
    if state.bake.log.len() != job.log().len() { state.bake.log = job.log().to_vec(); }
    if let Some(result) = result {
        let summary = state.bake.job.as_ref().map(|j| j.summary()).unwrap_or_default();
        if let Some(j) = state.bake.job.as_ref() { state.bake.map_rows = j.map_progress(&maps); }
        state.bake.job = None;
        let note = if state.bake.notices.is_empty() { "" } else { " Some low-poly parts had no high-poly match by name (see the Baking log)." };
        match &result {
            Ok(n) => {
                state.bake.displayed_map = None;
                state.bake.report = summary;
                state.bake.follow_maps = true;
                state.recovery.prefs.viewport.view_mode = 0;
                state.recovery.prefs.viewport.solo = None;
                let detail = if state.bake.report.is_empty() { String::new() } else { format!(" ({})", state.bake.report) };
                state.set_status(format!("Baked mesh maps for {n} texture set(s). See them in the 2D view.{detail}{note}"), now);
                state.recovery.autosaver.request_soon();
            }
            Err(e) => state.set_status(format!("Bake: {e}"), now),
        }
        state.bake.result = Some(result);
    }
}

/// Tabs of the Baking log panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BakeLogTab {
    #[default]
    Process,
    HighPoly,
    Matching,
}

/// What the baking panels read from the session at the start of a frame.
struct BakeFrame {
    sel_id: subcraft_core::TextureSetId,
    sel_slot: u32,
    /// Every texture set: id, material slot, name, resolution.
    sets: Vec<(subcraft_core::TextureSetId, u32, String, u32)>,
    low_name: String,
    low_box: (glam::Vec3, glam::Vec3),
    high_box: Option<(glam::Vec3, glam::Vec3)>,
    /// Maps the selected texture set already has.
    baked_maps: Vec<subcraft_core::MeshMapKind>,
    match_mode: subcraft_core::BakeMatchMode,
}

impl BakeFrame {
    /// The texture sets ticked for baking.
    fn targets(&self, b: &BakeUi) -> Vec<(subcraft_core::TextureSetId, u32)> {
        self.sets.iter().filter(|s| !b.skip_sets.contains(&s.0)).map(|s| (s.0, s.1)).collect()
    }
}

/// Title of a baking panel, in capitals like the docked panels.
fn panel_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).font(theme::font_strong(11.5)).color(TEXT_DIM));
    ui.add_space(4.0);
}

/// Baking mode (F8): the baking panels around the views, replacing the painting panels until
/// F9 or "Return to painting mode". Left: the texture sets to bake, the mesh map bakers and the
/// selected baker's settings. Right: the common settings. Below the views: the baking log. Over
/// the views: Bake and Return. Call before the views are laid out.
pub fn bake_mode(state: &mut UiState, ui: &mut egui::Ui, now: f64) {
    use crate::icons;
    if !state.bake.open {
        return;
    }
    let frame = {
        let selected = state.session.selected_texture_set();
        let bounds = state.session.scene().mesh.bounds;
        BakeFrame {
            sel_id: selected.id,
            sel_slot: selected.material_slot,
            sets: state.session.document().texture_sets.iter().map(|t| (t.id, t.material_slot, t.name.clone(), t.resolution)).collect(),
            low_name: state.session.scene().mesh.name.clone(),
            low_box: (bounds.min, bounds.max),
            high_box: state.bake.preview_mesh.as_ref().map(|m| (m.bounds.min, m.bounds.max)),
            baked_maps: state.session.mesh_maps(selected.id).map(|m| m.iter().map(|(k, _)| k).collect()).unwrap_or_default(),
            match_mode: state.session.settings().bake_match.mode,
        }
    };
    state.bake.skip_sets.retain(|id| frame.sets.iter().any(|s| s.0 == *id));
    // Part matching and placement are project settings (saved with the project), edited here.
    let mut bake_match = state.session.settings().bake_match.clone();
    let mut placement = state.session.settings().bake_placement.clone();
    let busy = state.bake.job.is_some();
    let mut select_set = None;
    let mut quick = None;
    let side = || egui::Frame::new().fill(theme::BG_1).inner_margin(egui::Margin::same(8));
    egui::Panel::left("bake_left").default_size(300.0).size_range(240.0..=420.0).frame(side()).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("bake_left_scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(!busy, |ui| {
                bake_set_list(ui, &mut state.bake, &frame, &mut select_set);
                ui.add_space(10.0);
                bakers_panel(ui, &mut state.bake, &frame, &mut quick);
                ui.add_space(10.0);
                map_settings_panel(ui, &mut state.bake);
            });
        });
    });
    egui::Panel::right("bake_right").default_size(380.0).size_range(300.0..=520.0).frame(side()).show(ui, |ui| {
        panel_title(ui, tr("Common settings"));
        egui::ScrollArea::vertical().id_salt("bake_scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add_enabled_ui(!busy, |ui| common_settings(ui, &mut state.bake, &frame, &mut bake_match, &mut placement));
        });
    });
    egui::Panel::bottom("bake_log").default_size(190.0).size_range(120.0..=420.0).frame(side()).show(ui, |ui| bake_log_panel(ui, &mut state.bake, &frame));
    // --- Bake / Return, over the bottom of the views -------------------------------------------
    let views = ui.available_rect_before_wrap();
    let can = !state.bake.settings.maps.is_empty() && !frame.targets(&state.bake).is_empty();
    let (mut go, mut leave) = (false, false);
    egui::Area::new(egui::Id::new("bake_actions")).order(egui::Order::Middle).pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(egui::pos2(views.center().x, views.bottom() - 16.0)).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(theme::BG_2).corner_radius(8).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                match &state.bake.job {
                    Some(job) => {
                        ui.add(egui::ProgressBar::new(job.fraction()).desired_width(220.0).show_percentage().fill(theme::ACCENT));
                        if secondary_button(ui, tr("Cancel")).clicked() { job.cancel(); }
                    }
                    None => {
                        go = ui.add_enabled_ui(can, |ui| primary_button(ui, format!("{}  {}", icons::MAGIC_WAND, tr("Bake selected textures")))).inner
                            .on_hover_text(tr("Bake the ticked mesh maps for the ticked texture sets")).clicked();
                    }
                }
                leave = secondary_button(ui, format!("{}  {}", icons::PAINT_BRUSH, tr("Return to painting mode"))).clicked();
            });
        });
    });
    if let Some(id) = select_set && let Err(e) = state.session.execute(subcraft_app::Command::SelectTextureSet(id)) {
        state.set_status(e.to_string(), now);
    }
    if go {
        start_bake(state, None, &frame, now);
    } else if let Some(kind) = quick {
        start_bake(state, Some(kind), &frame, now);
    }
    if bake_match != state.session.settings().bake_match || placement != state.session.settings().bake_placement {
        let mut project = state.session.settings().clone();
        project.bake_match = bake_match;
        project.bake_placement = placement;
        state.session.set_settings(project);
    }
    if leave {
        state.bake.open = false;
    }
}

/// Start a bake of the ticked maps, or of `only` (quick bake), for the ticked texture sets.
fn start_bake(state: &mut UiState, only: Option<subcraft_core::MeshMapKind>, frame: &BakeFrame, now: f64) {
    let mut texture_sets = frame.targets(&state.bake);
    if texture_sets.is_empty() {
        // A quick bake with nothing ticked bakes the selected texture set.
        texture_sets.push((frame.sel_id, frame.sel_slot));
    }
    state.bake.result = None;
    state.bake.notices.clear();
    state.bake.log.clear();
    state.bake.displayed_map = None;
    state.bake.follow_maps = true;
    state.bake.log_tab = BakeLogTab::Process;
    let mut settings = state.bake.settings.clone();
    if let Some(kind) = only {
        settings.maps = vec![kind];
    }
    settings.maps.sort();
    settings.use_gpu = state.recovery.prefs.use_gpu_baking;
    settings.name_match = state.session.settings().bake_match.clone();
    settings.placement = state.session.settings().bake_placement.clone();
    state.bake.job_maps = settings.maps.clone();
    state.bake.map_rows = settings.maps.iter().map(|&kind| subcraft_app::MapProgress { kind, state: subcraft_app::MapState::Waiting, seconds: 0. }).collect();
    let request = subcraft_app::BakeRequest {
        settings,
        high_poly: (!state.bake.high_poly.trim().is_empty()).then(|| PathBuf::from(state.bake.high_poly.trim())),
        texture_sets,
    };
    let prepared = (state.bake.preview_path == state.bake.high_poly.trim()
        && state.bake.preview_scene.as_ref().is_some_and(|m| std::sync::Arc::ptr_eq(m, &state.session.scene().mesh)))
        .then(|| state.bake.preview_source.clone()).flatten();
    state.bake.job = Some(state.session.start_bake_prepared(request, prepared));
    state.set_status("Baking…", now);
}

/// The texture sets, each with its bake tick. Clicking a name selects the set.
fn bake_set_list(ui: &mut egui::Ui, b: &mut BakeUi, f: &BakeFrame, select: &mut Option<subcraft_core::TextureSetId>) {
    panel_title(ui, tr("Texture Set List"));
    for (id, _, name, resolution) in &f.sets {
        ui.horizontal(|ui| {
            let mut on = !b.skip_sets.contains(id);
            if ui.checkbox(&mut on, "").on_hover_text(tr("Bake this texture set")).changed() {
                if on { b.skip_sets.remove(id); } else { b.skip_sets.insert(*id); }
            }
            if ui.selectable_label(*id == f.sel_id, name.as_str()).clicked() {
                *select = Some(*id);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(dim(&resolution.to_string())));
        });
    }
}

/// The Common settings entry, then one row per mesh map: bake tick, name (click to edit its
/// settings), show on the model, quick bake.
fn bakers_panel(ui: &mut egui::Ui, b: &mut BakeUi, f: &BakeFrame, quick: &mut Option<subcraft_core::MeshMapKind>) {
    use crate::icons;
    use subcraft_core::MeshMapKind;
    ui.horizontal(|ui| {
        panel_title(ui, tr("Mesh map bakers"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(icons::SLIDERS, |ui| {
                if ui.button(tr("Check all")).clicked() { b.settings.maps = MeshMapKind::ALL.to_vec(); ui.close(); }
                if ui.button(tr("Uncheck all")).clicked() { b.settings.maps.clear(); ui.close(); }
                if ui.button(tr("Invert checked mesh maps")).clicked() {
                    b.settings.maps = MeshMapKind::ALL.into_iter().filter(|k| !b.settings.maps.contains(k)).collect();
                    ui.close();
                }
            });
        });
    });
    if ui.selectable_label(b.selected_baker.is_none(), format!("{}   {}", icons::SLIDERS, tr("Common settings"))).clicked() {
        b.selected_baker = None;
    }
    for kind in MeshMapKind::ALL {
        ui.horizontal(|ui| {
            let mut on = b.settings.maps.contains(&kind);
            if ui.checkbox(&mut on, "").changed() {
                if on { b.settings.maps.push(kind); } else { b.settings.maps.retain(|m| *m != kind); }
            }
            let (icon, tip) = crate::bake_view::map_info(kind);
            if ui.selectable_label(b.selected_baker == Some(kind), format!("{icon}   {}", tr(kind.display_name()))).on_hover_text(tr(tip)).clicked() {
                b.selected_baker = Some(kind);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::widgets::icon_button(ui, icons::LIGHTNING, tr("Quick bake: bake only this map")).clicked() {
                    *quick = Some(kind);
                }
                let shown = b.displayed_map == Some(kind);
                let baked = f.baked_maps.contains(&kind);
                let eye = ui.add_enabled_ui(baked, |ui| crate::widgets::icon_toggle(ui, icons::EYE, shown, tr("Show this map on the model (once it is baked)"))).inner;
                if eye.clicked() {
                    b.displayed_map = if shown { None } else { Some(kind) };
                    b.follow_maps = false;
                }
            });
        });
    }
}

/// Settings of the baker selected in the list (nothing while Common settings is selected).
fn map_settings_panel(ui: &mut egui::Ui, b: &mut BakeUi) {
    use crate::widgets::slider_row;
    use subcraft_core::MeshMapKind;
    let Some(kind) = b.selected_baker else { return };
    panel_title(ui, &format!("{} > {}", tr("Mesh map settings"), tr(kind.display_name())));
    match kind {
        MeshMapKind::AmbientOcclusion | MeshMapKind::BentNormal => {
            slider_row(ui, tr("AO samples"), egui::Slider::new(&mut b.settings.ao_samples, 4..=256).logarithmic(true));
            ui.add_space(6.0);
            slider_row(ui, tr("AO distance"), egui::Slider::new(&mut b.settings.ao_distance, 0.01..=1.0).logarithmic(true));
        }
        MeshMapKind::Id => {
            let label = |s: subcraft_bake::IdSource| match s {
                subcraft_bake::IdSource::VertexColor => tr("Vertex color"),
                subcraft_bake::IdSource::MaterialColor => tr("Material color"),
                subcraft_bake::IdSource::MeshId => tr("Mesh ID / Polygroup"),
                subcraft_bake::IdSource::Submesh => tr("Submesh (object)"),
            };
            ui.horizontal(|ui| {
                ui.label(tr("ID colors from"));
                egui::ComboBox::from_id_salt("bake_id_source").selected_text(label(b.settings.id_source)).show_ui(ui, |ui| {
                    for source in subcraft_bake::IdSource::ALL {
                        ui.selectable_value(&mut b.settings.id_source, source, label(source));
                    }
                });
            })
            .response
            .on_hover_text(tr("Where the ID map takes its colors: the model's vertex colors or material colors, or one generated color per mesh element or per object. With a high poly, its colors are used."));
        }
        MeshMapKind::Curvature => {
            ui.checkbox(&mut b.settings.curvature_from_normal, tr("Curvature from the normal map"))
                .on_hover_text(tr("Measure curvature from the baked normal map along the surface: one clean band per edge, continuous across UV seams and on curved parts"));
            ui.add_space(4.0);
            if b.settings.curvature_from_normal {
                slider_row(ui, tr("Curvature width (texels)"), egui::Slider::new(&mut b.settings.normal_curvature.radius_texels, 0.5..=8.0));
                ui.add_space(6.0);
                slider_row(ui, tr("Curvature intensity"), egui::Slider::new(&mut b.settings.normal_curvature.intensity, 0.1..=4.0).logarithmic(true));
            } else {
                slider_row(ui, tr("Curvature radius"), egui::Slider::new(&mut b.settings.curvature_radius, 0.002..=0.1).logarithmic(true));
            }
        }
        _ => hint(ui, tr("This mesh map has no settings of its own.")),
    }
}

/// Common settings: models, placement, output size and cage, viewport overlays.
fn common_settings(ui: &mut egui::Ui, b: &mut BakeUi, f: &BakeFrame, bake_match: &mut subcraft_core::BakeMatch, placement: &mut subcraft_core::BakePlacement) {
    use crate::bake_view::{card_title, icon_toggle};
    use crate::icons;
    use crate::widgets::{card, icon_button, slider_row};
    let has_high = !b.high_poly.trim().is_empty();
    // Maps below the texture set's resolution look blocky: default to the largest ticked set.
    let target_res = f.sets.iter().filter(|s| !b.skip_sets.contains(&s.0)).map(|s| s.3).max().unwrap_or(1024).clamp(256, 4096);
    // --- Models -------------------------------------------------------------------
    card(ui, theme::ACCENT_DIM, |ui| {
        ui.set_width(ui.available_width());
        card_title(ui, icons::CUBE, tr("Models"), |_| {});
        egui::Grid::new("bake_sources").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
            ui.label(dim(tr("Low poly")));
            ui.label(RichText::new(format!("{}  {}", icons::CUBE, f.low_name)));
            ui.end_row();
            ui.label(dim(tr("High poly")));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut b.high_poly).hint_text(tr("optional (none = low on low)")).desired_width(180.0));
                if icon_button(ui, icons::FOLDER_OPEN, tr("Browse…")).clicked() {
                    crate::files::pick(ui.ctx(), crate::files::Purpose::BakeHighPoly, crate::files::MESHES, false, None);
                }
                if !b.high_poly.is_empty() && icon_button(ui, icons::X, tr("Clear")).clicked() {
                    b.high_poly.clear();
                }
            });
            ui.end_row();
            if has_high { bake_match_rows(ui, bake_match); }
        });
        if b.preview_pending() { ui.add_space(4.0); ui.label(dim(tr("Loading high-poly preview…"))); }
        if let Some(error) = &b.preview_error { ui.add_space(4.0); ui.add(egui::Label::new(RichText::new(error).color(theme::WARNING)).wrap()); }
        if has_high {
            ui.add_space(4.0);
            if secondary_button(ui, format!("{}  {}", icons::HISTORY, tr("Reload high poly"))).on_hover_text(tr("Read the high-poly file again")).clicked() {
                b.preview_mesh = None; b.preview_source = None; b.preview_job = None;
                b.preview_error = None; b.preview_uploaded = false; b.result = None;
                b.displayed_map = None;
            }
        }
    });
    // --- Placement ----------------------------------------------------------------
    if has_high {
        card(ui, theme::ACCENT, |ui| {
            ui.set_width(ui.available_width());
            card_title(ui, icons::SELECTION, tr("Placement"), |_| {});
            crate::placement::placement_section(ui, placement, f.low_box, f.high_box, &mut b.fit_mode, &mut b.high_opacity, &mut b.placement_message);
        });
    }
    // --- Output and cage ----------------------------------------------------------
    card(ui, theme::channel_accent(subcraft_core::ChannelKind::Roughness), |ui| {
        ui.set_width(ui.available_width());
        card_title(ui, icons::SLIDERS, tr("Output"), |_| {});
        egui::Grid::new("bake_quality").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
            if !b.size_chosen { b.settings.size = target_res; }
            ui.label(dim(tr("Resolution")));
            egui::ComboBox::from_id_salt("bake_size").selected_text(format!("{0} × {0}", b.settings.size)).show_ui(ui, |ui| {
                for r in [256u32, 512, 1024, 2048, 4096] {
                    if ui.selectable_value(&mut b.settings.size, r, format!("{r} × {r}")).clicked() { b.size_chosen = true; }
                }
            });
            ui.end_row();
        });
        if has_high {
            ui.add_space(6.0);
            slider_row(ui, tr("Cage front (reach above the surface)"), egui::Slider::new(&mut b.settings.cage_front, 0.0..=0.25).fixed_decimals(3).clamping(egui::SliderClamping::Never))
                .on_hover_text(tr("Fraction of the model diagonal. Raise it when the high poly stands further out than this."));
            ui.add_space(6.0);
            slider_row(ui, tr("Cage rear (reach below the surface)"), egui::Slider::new(&mut b.settings.cage_rear, 0.0..=0.25).fixed_decimals(3).clamping(egui::SliderClamping::Never))
                .on_hover_text(tr("Fraction of the model diagonal. Raise it when parts of the high poly sink under the low poly."));
        }
    });
    // --- Viewport -----------------------------------------------------------------
    card(ui, theme::ACCENT_DIM, |ui| {
        ui.set_width(ui.available_width());
        card_title(ui, icons::EYE, tr("Viewport"), |_| {});
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            icon_toggle(ui, icons::EYE, &mut b.preview, "High / low overlay: blue high poly, red exposed low poly");
            icon_toggle(ui, icons::CUBE_WIRE, &mut b.cage, "Cage wireframe: the front and rear reach of the bake");
            if has_high {
                icon_toggle(ui, icons::WARNING, &mut b.miss_heatmap, "Ray misses: color the low poly red where the bake's rays reach no high-poly surface (fix with Cage front / rear before baking)");
            }
            icon_toggle(ui, icons::LIGHTNING, &mut b.live_maps, "Live maps: show each map on the model as soon as it is baked");
            icon_toggle(ui, icons::CROSSHAIR, &mut b.follow_maps, "Follow bake: switch the model to the map being baked");
            if !f.baked_maps.is_empty() {
                egui::ComboBox::from_id_salt("bake_display_map")
                    .selected_text(b.displayed_map.map_or(tr("Material"), |k| tr(k.display_name()))).show_ui(ui, |ui| {
                        if ui.selectable_value(&mut b.displayed_map, None, tr("Material")).clicked() { b.follow_maps = false; }
                        for &kind in &f.baked_maps {
                            if ui.selectable_value(&mut b.displayed_map, Some(kind), tr(kind.display_name())).clicked() { b.follow_maps = false; }
                        }
                    });
            }
        });
        if b.preview_mesh.is_some() && b.result.is_some()
            && secondary_button(ui, format!("{}  {}", icons::CUBE_WIRE, tr("Inspect high / low geometry"))).clicked() {
            b.result = None; b.displayed_map = None; b.follow_maps = false;
        }
        if has_high && b.miss_heatmap {
            let text = match (&b.cage_result, &b.cage_job) {
                (_, Some(_)) => tr("Checking where rays miss the high poly…").to_string(),
                (Some(r), None) if r.missed == 0 => tr("Every texel's ray reaches the high poly.").to_string(),
                (Some(r), None) => format!("{} {:.1}% {}", tr("Ray misses:"), 100.0 * r.miss_fraction(), tr("of texels reach no high-poly surface (red). Raise Cage front / rear or fix the placement.")),
                (None, None) => String::new(),
            };
            if !text.is_empty() {
                let color = if b.cage_result.as_ref().is_some_and(|r| r.missed > 0) && b.cage_job.is_none() { theme::WARNING } else { TEXT_DIM };
                ui.add(egui::Label::new(egui::RichText::new(text).color(color)).wrap());
            }
            hint(ui, tr("Gray: rays reach the high poly · Red: rays miss it · Lower the high-poly opacity to see the heatmap underneath."));
        } else if has_high { hint(ui, tr("Blue: high poly · Red: exposed low poly · Pale lines: cage. Orbit to inspect.")); }
    });
}

/// The Baking log panel: the running or last bake, the high-poly file, and the name matching.
fn bake_log_panel(ui: &mut egui::Ui, b: &mut BakeUi, f: &BakeFrame) {
    use crate::icons;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(tr("Baking log").to_uppercase()).font(theme::font_strong(11.5)).color(TEXT_DIM));
        ui.add_space(12.0);
        for (tab, label) in [(BakeLogTab::Process, tr("Baking process")), (BakeLogTab::HighPoly, tr("High poly parameters")), (BakeLogTab::Matching, tr("Matching by name"))] {
            if ui.selectable_label(b.log_tab == tab, label).clicked() { b.log_tab = tab; }
        }
    });
    ui.add_space(4.0);
    let warning = |ui: &mut egui::Ui, text: &str| {
        ui.add(egui::Label::new(RichText::new(format!("{}  {text}", icons::WARNING)).color(theme::WARNING)).wrap());
    };
    egui::ScrollArea::vertical().id_salt("bake_log_tab").auto_shrink([false, false]).show(ui, |ui| match b.log_tab {
        BakeLogTab::Process => {
            if let Some(job) = &b.job {
                ui.label(dim(&job.stage()));
            } else if let Some(result) = &b.result {
                match result {
                    Ok(n) => { ui.label(RichText::new(format!("{}  {}", icons::CHECK, crate::i18n::trf("Bake complete: {} texture set(s)", &[n]))).color(crate::bake_view::OK_GREEN)); }
                    Err(e) => { ui.add(egui::Label::new(RichText::new(format!("{}  {}: {e}", icons::WARNING, tr("Bake failed"))).color(theme::WARNING)).wrap()); }
                }
            } else if b.map_rows.is_empty() && b.log.is_empty() {
                ui.label(dim(tr("Nothing baked yet in this session.")));
            }
            for note in &b.notices { warning(ui, note); }
            bake_log_section(ui, &b.log);
            if !b.map_rows.is_empty() {
                ui.add_space(4.0);
                // Three columns keep the rows of every map in view in the short panel.
                ui.columns(3, |columns| {
                    for (i, p) in b.map_rows.iter().enumerate() { crate::bake_view::status_row(&mut columns[i % 3], p); }
                });
            }
        }
        BakeLogTab::HighPoly => {
            let path = b.high_poly.trim();
            if path.is_empty() {
                ui.label(dim(tr("No high-poly file: the low poly is baked onto itself.")));
                return;
            }
            ui.add(egui::Label::new(path).wrap());
            if b.preview_pending() { ui.label(dim(tr("Loading high-poly preview…"))); }
            if let Some(error) = &b.preview_error { warning(ui, error); }
            if let Some(mesh) = &b.preview_mesh {
                let size = mesh.bounds.max - mesh.bounds.min;
                ui.label(dim(&format!("{} {}", tr("Triangles:"), mesh.indices.len() / 3)));
                ui.label(dim(&format!("{} {}", tr("Named parts:"), mesh.objects.len())));
                ui.label(dim(&format!("{} {:.3} × {:.3} × {:.3}", tr("Size:"), size.x, size.y, size.z)));
            }
        }
        BakeLogTab::Matching => {
            ui.label(format!("{}: {}", tr("Match"), f.match_mode.label()));
            if f.match_mode != subcraft_core::BakeMatchMode::ByName {
                ui.label(dim(tr("Every low-poly part is baked against the whole high poly.")));
            } else if b.notices.is_empty() {
                ui.label(dim(tr("No low-poly part without a high-poly part of the same name was reported.")));
            }
            for note in &b.notices { warning(ui, note); }
        }
    });
}

/// The bake log, collapsed by default under "Details": what ran where and for how long, warnings
/// in amber, errors in red, with a Copy button. Nothing of it is shown unless the owner opens it.
fn bake_log_section(ui: &mut egui::Ui, lines: &[subcraft_app::BakeLogLine]) {
    use subcraft_app::BakeLogLevel;
    if lines.is_empty() { return; }
    let problems = lines.iter().filter(|l| l.level != BakeLogLevel::Info).count();
    let title = if problems == 0 { tr("Details").to_string() } else { format!("{} ({problems} warning(s) or error(s))", tr("Details")) };
    egui::CollapsingHeader::new(RichText::new(title).color(TEXT_DIM)).id_salt("bake_log").default_open(false).show(ui, |ui| {
        if ui.small_button(tr("Copy log")).on_hover_text(tr("Copies the whole log as plain text")).clicked() {
            ui.ctx().copy_text(subcraft_app::bake_log_text(lines));
        }
        egui::ScrollArea::vertical().id_salt("bake_log_scroll").max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
            for line in lines {
                let text = RichText::new(line.plain()).monospace().size(11.0);
                let text = match line.level {
                    BakeLogLevel::Info => text.color(theme::TEXT_DIM),
                    BakeLogLevel::Warning => text.color(theme::WARNING),
                    BakeLogLevel::Error => text.color(egui::Color32::from_rgb(235, 100, 100)),
                };
                ui.add(egui::Label::new(text).wrap());
            }
        });
    });
}

fn bake_match_rows(ui: &mut egui::Ui, m: &mut subcraft_core::BakeMatch) {
    use subcraft_core::BakeMatchMode;
    ui.label(dim(tr("Match")));
    let options: Vec<(BakeMatchMode, &str)> = BakeMatchMode::ALL.iter().map(|&k| (k, k.label())).collect();
    segmented(ui, &mut m.mode, &options);
    ui.end_row();
    if m.mode == BakeMatchMode::ByName {
        ui.label(dim("Low / high suffix"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut m.low_suffix).desired_width(70.0).hint_text(tr("_low")))
                .on_hover_text(tr("Low-poly part names end with this (case-insensitive; _lo also works)"));
            ui.add(egui::TextEdit::singleline(&mut m.high_suffix).desired_width(70.0).hint_text(tr("_high")))
                .on_hover_text(tr("High-poly part names end with this (case-insensitive; _hi also works)"));
        });
        ui.end_row();
    }
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
        }, |ui| bake_mode(state, ui, 0.));
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
        for _ in 0..3 { draw(&mut state, &ctx, vec![]); }
        let output = draw(&mut state, &ctx, vec![]);
        let pos = output.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text.ends_with("Bake selected textures") => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        }).expect("visible Bake button over the views");
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
        for _ in 0..4 { draw(&mut state, &ctx, vec![]); }
        for _ in 0..3 {
            let output = draw(&mut state, &ctx, vec![]);
            assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.job.text.contains("Bake failed: align models"))));
        }
    }

    fn text_pos(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text == text => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        })
    }

    #[test]
    fn match_by_name_is_a_project_setting_with_suffix_fields_and_unmatched_notes_show() {
        use subcraft_core::BakeMatchMode;
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        state.bake.open = true;
        state.bake.preview = false;
        draw(&mut state, &ctx, vec![]);
        let output = draw(&mut state, &ctx, vec![]);
        assert!(text_pos(&output, "Match").is_none(), "matching only applies with a high poly");
        state.bake.high_poly = "parts_high.fbx".into();
        draw(&mut state, &ctx, vec![]);
        let output = draw(&mut state, &ctx, vec![]);
        assert!(text_pos(&output, "Low / high suffix").is_none());
        let pos = text_pos(&output, "By mesh name").expect("Match choice");
        let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        draw(&mut state, &ctx, vec![egui::Event::PointerMoved(pos), button(true)]);
        draw(&mut state, &ctx, vec![button(false)]);
        assert_eq!(state.session.settings().bake_match.mode, BakeMatchMode::ByName);
        assert!(state.session.is_modified(), "saved with the project");
        let output = draw(&mut state, &ctx, vec![]);
        assert!(text_pos(&output, "Low / high suffix").is_some());
        state.bake.notices = vec!["Match by name: 1 low-poly part(s) have no matching high-poly part: Clip_low".into()];
        let output = draw(&mut state, &ctx, vec![]);
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.job.text.contains("Clip_low"))));
    }

    fn has_text(output: &egui::FullOutput, needle: &str) -> bool {
        output.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.job.text.contains(needle)))
    }

    fn click(state: &mut UiState, ctx: &egui::Context, pos: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        draw(state, ctx, vec![egui::Event::PointerMoved(pos), button(true)]);
        draw(state, ctx, vec![button(false)]);
    }

    #[test]
    fn baking_mode_shows_its_four_panels_and_return_leaves_it() {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        // Painting mode: nothing of the baking layout is drawn.
        let output = draw(&mut state, &ctx, vec![]);
        assert!(!has_text(&output, "MESH MAP BAKERS"));
        state.bake.open = true; state.bake.preview = false;
        for _ in 0..3 { draw(&mut state, &ctx, vec![]); }
        let output = draw(&mut state, &ctx, vec![]);
        for title in ["TEXTURE SET LIST", "MESH MAP BAKERS", "COMMON SETTINGS", "BAKING LOG", "Baking process", "High poly parameters", "Matching by name"] {
            assert!(has_text(&output, title), "{title}");
        }
        assert!(!has_text(&output, "MESH MAP SETTINGS"), "Common settings is selected at first");
        // Selecting a baker shows its settings.
        let ao = output.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text.ends_with("Ambient Occlusion") => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        }).expect("the Ambient Occlusion baker row");
        click(&mut state, &ctx, ao);
        assert_eq!(state.bake.selected_baker, Some(subcraft_core::MeshMapKind::AmbientOcclusion));
        let output = draw(&mut state, &ctx, vec![]);
        assert!(has_text(&output, "MESH MAP SETTINGS > AMBIENT OCCLUSION") && has_text(&output, "AO samples"));
        // The log tabs.
        let tab = output.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text == "High poly parameters" => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        }).unwrap();
        click(&mut state, &ctx, tab);
        assert_eq!(state.bake.log_tab, BakeLogTab::HighPoly);
        let output = draw(&mut state, &ctx, vec![]);
        assert!(has_text(&output, "No high-poly file"));
        // Return to painting mode.
        let back = output.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text.ends_with("Return to painting mode") => Some(t.pos + t.galley.size() * 0.5),
            _ => None,
        }).expect("Return button over the views");
        click(&mut state, &ctx, back);
        assert!(!state.bake.open);
    }

    #[test]
    fn a_bake_covers_the_ticked_texture_sets_and_a_quick_bake_one_map() {
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        let (id, slot) = { let ts = state.session.selected_texture_set(); (ts.id, ts.material_slot) };
        let frame = BakeFrame {
            sel_id: id, sel_slot: slot, sets: vec![(id, slot, "Set".into(), 128)],
            low_name: String::new(), low_box: (glam::Vec3::ZERO, glam::Vec3::ONE), high_box: None, baked_maps: Vec::new(),
            match_mode: subcraft_core::BakeMatchMode::Always,
        };
        assert_eq!(frame.targets(&state.bake), vec![(id, slot)], "every set is ticked at first");
        state.bake.skip_sets.insert(id);
        assert!(frame.targets(&state.bake).is_empty());
        // A quick bake bakes only its map, for the selected set when none is ticked, and leaves
        // the ticked maps as they were.
        state.bake.settings.size = 128;
        let ticked = state.bake.settings.maps.clone();
        start_bake(&mut state, Some(subcraft_core::MeshMapKind::Position), &frame, 0.);
        assert_eq!(state.bake.job_maps, vec![subcraft_core::MeshMapKind::Position]);
        assert_eq!(state.bake.settings.maps, ticked);
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while state.bake.job.is_some() {
            poll_bake(&mut state, &ctx, 0.);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(matches!(state.bake.result, Some(Ok(1))), "{:?}", state.bake.result);
        assert_eq!(state.bake.map_rows.len(), 1, "one status row for the quick bake");
        let maps: Vec<_> = state.session.mesh_maps(id).unwrap().iter().map(|(k, _)| k).collect();
        assert_eq!(maps, vec![subcraft_core::MeshMapKind::Position]);
    }
}

#[cfg(test)]
mod bake_log_tests {
    use super::*;
    use subcraft_app::{BakeLogLevel, BakeLogLine, Session};

    #[test]
    fn the_baking_log_keeps_the_lines_collapsed_under_details_and_counts_problems() {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        state.bake.open = true; state.bake.preview = false;
        state.bake.log = vec![
            BakeLogLine { at: 0.0, level: BakeLogLevel::Info, text: "Baking 1 texture set(s)".into() },
            BakeLogLine { at: 1.2, level: BakeLogLevel::Warning, text: "Cage: 12 of 100 texels missed".into() },
        ];
        let frame = |state: &mut UiState| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 900.0))), ..Default::default() }, |ui| bake_mode(state, ui, 0.0));
            out.textures_delta.clear(); out
        };
        for _ in 0..5 { frame(&mut state); }
        let out = frame(&mut state);
        let has = |needle: &str| out.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(t) if t.galley.job.text.contains(needle)));
        assert!(has("Details (1 warning(s) or error(s))"), "problems are counted in the collapsed header");
        assert!(!has("Cage: 12 of 100 texels missed") && !has("[   1.2 s]"), "the log stays closed until the owner opens it");
        assert!(!has("Baking 1 texture set(s)"), "no console text in the window by default");
        assert_eq!(subcraft_app::bake_log_text(&state.bake.log).lines().count(), 2, "Copy gives the whole log");
    }
}
