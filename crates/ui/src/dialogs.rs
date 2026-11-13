//! File dialogs and windows: save / open project, export textures, unsaved-changes prompt.

use crate::i18n::tr;
use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::RichText;
use subcraft_app::{ExportSettings, ExportTemplate, Padding, Session};
use subcraft_core::TextureSetId;
use subcraft_doc::{ExportConfig, ExportOverrides};

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
    /// Preset editor state (Output templates tab).
    pub editor: crate::export_presets::PresetEditor,
    pub tab: ExportTab,
    /// Texture set shown on the Settings tab; `None` = Global settings.
    pub selected_set: Option<TextureSetId>,
    /// Texture sets' own file type, size and padding.
    pub set_overrides: BTreeMap<TextureSetId, ExportOverrides>,
    /// List of exports: files written by the last export, and its messages.
    pub log: Vec<ExportLogLine>,
    /// The project's saved export settings last applied to the window (`None` = never).
    loaded_config: Option<Option<ExportConfig>>,
    /// Settings when the window opened, for Cancel.
    snapshot: Option<Box<ExportSnapshot>>,
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
        Self { open: false, settings: ExportSettings::default(), preset: ExportTemplate::pbr_metal_rough().name, preset_list: None, editor: Default::default(), tab: ExportTab::default(), selected_set: None, set_overrides: BTreeMap::new(), log: Vec::new(), loaded_config: None, snapshot: None, folder: folder.display().to_string(), known_template: String::new(), view_2d_only: false, scene_format: 0, baked: Default::default(), baked_initialized: false, baked_selected_only: false }
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

/// Tabs of the Export Textures window (Painter's three).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportTab {
    #[default]
    Settings,
    OutputTemplates,
    ListOfExports,
}

/// One line of the List of exports tab: a written file (with its texture set) or a message.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportLogLine {
    pub set: Option<String>,
    pub text: String,
    pub error: bool,
}

/// What Cancel goes back to: the window's and the project's export settings when it opened.
struct ExportSnapshot {
    settings: ExportSettings,
    folder: String,
    preset: String,
    set_overrides: BTreeMap<TextureSetId, ExportOverrides>,
    project: subcraft_doc::ProjectSettings,
}

impl ExportUi {
    /// The window's settings as saved in a project (Save settings).
    pub fn config(&self) -> ExportConfig {
        let s = &self.settings;
        ExportConfig {
            folder: Some(self.folder.clone()),
            format: s.format_override,
            dither: s.dither,
            size: s.size,
            padding: Some(s.padding),
            height_to_normal: Some(s.height_to_normal),
            sets: self.set_overrides.iter().filter(|(_, o)| !o.is_empty()).map(|(k, v)| (*k, v.clone())).collect(),
        }
    }

    /// Take `config` (a project's saved settings; `None` = the defaults).
    pub fn apply_config(&mut self, config: Option<&ExportConfig>) {
        let d = ExportSettings::default();
        let c = config.cloned().unwrap_or_default();
        if let Some(folder) = c.folder {
            self.folder = folder;
        }
        self.settings.format_override = c.format;
        self.settings.dither = c.dither;
        self.settings.size = c.size;
        self.settings.padding = c.padding.unwrap_or(d.padding);
        if let Some(h) = c.height_to_normal {
            self.settings.height_to_normal = h;
        }
        self.set_overrides = c.sets;
    }

    /// Reset all: the export settings of a new project (folder and output template kept).
    pub fn reset(&mut self) {
        let keep_h2n = self.settings.height_to_normal;
        self.apply_config(None);
        self.settings.height_to_normal = keep_h2n;
        self.selected_set = None;
    }
}

/// Export-related project settings (what Cancel restores and Reset all clears).
fn restore_export_settings(project: &mut subcraft_doc::ProjectSettings, from: &subcraft_doc::ProjectSettings) {
    project.normal_format = from.normal_format;
    project.export_template = from.export_template.clone();
    project.export_maps_off = from.export_maps_off.clone();
    project.udim_folders = from.udim_folders;
    project.export = from.export.clone();
}

pub fn export_window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if !state.export.open {
        state.export.snapshot = None;
        return;
    }
    // A project's saved export settings (another project opened, or first use).
    let project_config = state.session.settings().export.clone();
    if state.export.loaded_config.as_ref() != Some(&project_config) {
        let first = state.export.loaded_config.is_none();
        if project_config.is_some() || !first {
            state.export.apply_config(project_config.as_ref());
        }
        state.export.loaded_config = Some(project_config);
    }
    if state.export.snapshot.is_none() {
        let ex = &state.export;
        state.export.snapshot = Some(Box::new(ExportSnapshot { settings: ex.settings.clone(), folder: ex.folder.clone(), preset: ex.preset.clone(), set_overrides: ex.set_overrides.clone(), project: state.session.settings().clone() }));
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
    // Per texture set overrides by name, for the export crate.
    let sets: Vec<(TextureSetId, String)> = state.session.document().texture_sets.iter().map(|ts| (ts.id, ts.name.clone())).collect();
    state.export.settings.set_overrides = sets.iter().filter_map(|(id, name)| state.export.set_overrides.get(id).filter(|o| !o.is_empty()).map(|o| (name.clone(), o.clone()))).collect();
    if state.export.selected_set.is_some_and(|id| !sets.iter().any(|(s, _)| *s == id)) {
        state.export.selected_set = None;
    }
    let title = if view_only { "Export 2D View" } else { "Export Textures" };
    let width = if state.export.tab == ExportTab::OutputTemplates && !view_only { 860.0 } else { 640.0 };
    let mut action = None;
    // Opened at a fixed height: a window only grows to its content a step per frame, and the
    // scrolling tab below fills the height it is given, so the button row would sit outside the
    // window until it reached full size.
    // The short 2D View body keeps egui's default (128).
    let height = if view_only { 128.0 } else { (ctx.content_rect().height() - 96.0).clamp(320.0, 900.0) };
    crate::widgets::window(ctx, window_title(title)).open(&mut open).collapsible(false).default_width(width).default_height(height).show(ctx, |ui| {
        if view_only {
            view_2d_body(ui, state, &lib, view, now);
            return;
        }
        ui.horizontal(|ui| {
            let ex = &mut state.export;
            for (tab, label) in [(ExportTab::Settings, tr("Settings")), (ExportTab::OutputTemplates, tr("Output templates")), (ExportTab::ListOfExports, tr("List of exports"))] {
                ui.selectable_value(&mut ex.tab, tab, RichText::new(label).strong());
            }
        });
        ui.add_space(4.0);
        divider(ui);
        ui.add_space(6.0);
        // The tab scrolls in what is left of the window above the button row (divider, spacing
        // and a 28 px button row: about 50 px, 64 with margin).
        let max_h = (ui.available_height() - 64.0).max(120.0);
        egui::ScrollArea::vertical().id_salt(("export_tab", state.export.tab as u8)).max_height(max_h).show(ui, |ui| match state.export.tab {
            ExportTab::Settings => settings_tab(ui, state, &lib, &sets, &mut preset_status, now),
            ExportTab::OutputTemplates => {
                let ex = &mut state.export;
                let list = ex.preset_list.get_or_insert_with(|| subcraft_app::export_presets::ExportPresets::load(lib.as_ref()));
                if let Some(s) = crate::export_presets::preset_bar(ui, list, &mut ex.preset, &mut ex.editor) {
                    preset_status = Some(s);
                }
                ui.add_space(6.0);
                let key = ex.preset.clone();
                if let Some(s) = crate::export_presets::preset_editor(ui, list, &key, &mut ex.editor, &mesh_name) {
                    preset_status = Some(s);
                }
            }
            ExportTab::ListOfExports => export_log(ui, &state.export.log),
        });
        ui.add_space(6.0);
        divider(ui);
        ui.add_space(4.0);
        // Painter's button row.
        let ex = &mut state.export;
        ex.settings.template = ex.current_template(lib.as_ref());
        let settings = ex.settings.clone();
        let choices = state.session.export_choices(&settings);
        let enabled = choices.iter().filter(|f| f.enabled).count();
        ui.horizontal(|ui| {
            if state.export.tab == ExportTab::Settings && secondary_button(ui, tr("Reset all")).on_hover_text(tr("Back to a new project's export settings: template file types, texture set sizes, infinite padding, every map checked")).clicked() {
                action = Some(WindowAction::Reset);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let export = ui.add_enabled_ui(enabled > 0, |ui| primary_button(ui, format!("{}  Export", crate::icons::EXPORT))).inner;
                if export.clicked() {
                    action = Some(WindowAction::Export);
                }
                if secondary_button(ui, tr("Save settings")).on_hover_text(tr("Keep these export settings in the project")).clicked() {
                    action = Some(WindowAction::Save);
                }
                if secondary_button(ui, tr("Cancel")).on_hover_text(tr("Close without keeping the changes made since the window opened")).clicked() {
                    action = Some(WindowAction::Cancel);
                }
                if enabled == 0 {
                    ui.label(dim("Check at least one map to export."));
                }
            });
        });
    });
    if let Some(s) = preset_status {
        state.set_status(s, now);
    }
    match action {
        Some(WindowAction::Export) => run_export(state, now),
        Some(WindowAction::Save) => {
            let config = state.export.config();
            let mut project = state.session.settings().clone();
            project.export = Some(config);
            state.session.set_settings(project.clone());
            state.export.loaded_config = Some(project.export.clone());
            if let Some(snap) = &mut state.export.snapshot {
                snap.project = project;
                snap.settings = state.export.settings.clone();
                snap.folder = state.export.folder.clone();
                snap.preset = state.export.preset.clone();
                snap.set_overrides = state.export.set_overrides.clone();
            }
            state.set_status("Export settings saved with the project", now);
        }
        Some(WindowAction::Reset) => {
            state.export.reset();
            let mut project = state.session.settings().clone();
            project.normal_format = None;
            project.udim_folders = false;
            project.export_maps_off.clear();
            if project != *state.session.settings() {
                state.session.set_settings(project);
            }
            state.set_status("Export settings reset", now);
        }
        Some(WindowAction::Cancel) => {
            if let Some(snap) = state.export.snapshot.take() {
                let ex = &mut state.export;
                ex.settings = snap.settings;
                ex.folder = snap.folder;
                ex.preset = snap.preset;
                ex.set_overrides = snap.set_overrides;
                let mut project = state.session.settings().clone();
                restore_export_settings(&mut project, &snap.project);
                if project != *state.session.settings() {
                    state.session.set_settings(project);
                }
            }
            open = false;
        }
        None => {}
    }
    state.export.open = open;
}

enum WindowAction {
    Export,
    Save,
    Reset,
    Cancel,
}

/// Export, log the files per texture set, and show the List of exports tab.
fn run_export(state: &mut UiState, now: f64) {
    let settings = state.export.settings.clone();
    let folder = PathBuf::from(&state.export.folder);
    let plan = state.session.export_plan(&settings);
    let mut log = Vec::new();
    match state.session.export_textures(&folder, &settings) {
        Ok(files) => {
            for (set, path) in plan {
                log.push(ExportLogLine { set: Some(set), text: folder.join(path).display().to_string(), error: false });
            }
            let text = format!("Exported {} files to {}", files.len(), folder.display());
            log.push(ExportLogLine { set: None, text: text.clone(), error: false });
            state.set_status(text, now);
        }
        Err(e) => {
            log.push(ExportLogLine { set: None, text: format!("Export failed: {e}"), error: true });
            state.set_status(format!("Export failed: {e}"), now);
        }
    }
    state.export.log = log;
    state.export.tab = ExportTab::ListOfExports;
}

/// List of exports: written files grouped by texture set, then the messages.
fn export_log(ui: &mut egui::Ui, log: &[ExportLogLine]) {
    if log.is_empty() {
        ui.label(dim(tr("Nothing exported yet in this session. Press Export to write the checked maps.")));
        return;
    }
    egui::ScrollArea::vertical().id_salt("export_log").max_height(380.0).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let mut current: Option<&str> = None;
        for line in log.iter().filter(|l| l.set.is_some()) {
            let set = line.set.as_deref().unwrap_or_default();
            if current != Some(set) {
                current = Some(set);
                ui.add_space(4.0);
                ui.label(RichText::new(set).strong());
            }
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.add(egui::Label::new(RichText::new(&line.text).monospace().small().color(TEXT_DIM)).truncate()).on_hover_text(&line.text);
            });
        }
        ui.add_space(8.0);
        ui.label(caption("Console"));
        for line in log.iter().filter(|l| l.set.is_none()) {
            ui.label(if line.error { RichText::new(&line.text).color(theme::WARNING) } else { RichText::new(&line.text) });
        }
    });
}

/// File > Export 2D view: folder, file type, size, padding and Export.
fn view_2d_body(ui: &mut egui::Ui, state: &mut UiState, lib: &Option<subcraft_app::assets::UserLibrary>, view: subcraft_export::View2d, now: f64) {
    egui::Grid::new("export_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        folder_row(ui, &mut state.export.folder);
        ui.label(dim("Image"));
        ui.label(format!("{} (as shown in the 2D view)", crate::panels::uv_view::label(state.uv.source)));
        ui.end_row();
        global_rows(ui, &mut state.export, false);
    });
    let ex = &mut state.export;
    ex.settings.template = ex.current_template(lib.as_ref());
    let settings = ex.settings.clone();
    let folder = PathBuf::from(&ex.folder);
    ui.add_space(6.0);
    let go = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| primary_button(ui, format!("{}  Export", crate::icons::EXPORT)).clicked());
    if go.inner {
        match state.session.export_view_2d(&folder, view, &settings) {
            Ok(files) if files.is_empty() => state.set_status("Nothing to export: the 2D view's source is missing", now),
            Ok(files) => state.set_status(format!("Exported {}", files[0].display()), now),
            Err(e) => state.set_status(format!("Export failed: {e}"), now),
        }
    }
}

fn folder_row(ui: &mut egui::Ui, folder: &mut String) {
    ui.label(dim("Output directory"));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(folder).desired_width(300.0));
        if ui.button(tr("Browse…")).clicked()
            && let Some(p) = rfd::FileDialog::new().set_directory(&*folder).pick_folder()
        {
            *folder = p.display().to_string();
        }
    });
    ui.end_row();
}

/// File type, size, padding and height to normal (the Global settings parameters).
fn global_rows(ui: &mut egui::Ui, ex: &mut ExportUi, with_preset_hints: bool) {
    ui.label(dim("File type"));
    ui.horizontal(|ui| {
        crate::export_presets::format_combo(ui, "export_format", &mut ex.settings.format_override, &mut ex.settings.dither, Some(tr("Based on output template")));
    });
    ui.end_row();
    let preset = with_preset_hints.then(|| ex.preset_list.as_ref().and_then(|l| l.find(&ex.preset)).map(|p| (p.size, p.padding))).flatten();
    ui.label(dim("Size"));
    ui.horizontal(|ui| {
        crate::export_presets::size_combo(ui, "export_size", &mut ex.settings.size, tr("Based on each texture set's size"));
        if let Some((Some(s), _)) = preset {
            ui.label(dim(&format!("the preset writes {s} × {s}"))).on_hover_text(tr("This preset chooses its own size; it wins over this setting"));
        }
    });
    ui.end_row();
    ui.label(dim("Padding"));
    ui.horizontal(|ui| {
        if let Some((_, Some(_))) = preset {
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
}

/// Settings tab: Global settings and the texture sets on the left, their parameters and output
/// maps on the right.
fn settings_tab(ui: &mut egui::Ui, state: &mut UiState, lib: &Option<subcraft_app::assets::UserLibrary>, sets: &[(TextureSetId, String)], preset_status: &mut Option<String>, now: f64) {
    {
        let ex = &mut state.export;
        ex.settings.template = ex.current_template(lib.as_ref());
    }
    let settings = state.export.settings.clone();
    let choices = state.session.export_choices(&settings);
    let mut set_click = None;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(170.0);
            let ex = &mut state.export;
            ui.selectable_value(&mut ex.selected_set, None, RichText::new(tr("Global settings")).strong());
            ui.add_space(4.0);
            for (id, name) in sets {
                ui.horizontal(|ui| {
                    let mine: Vec<&subcraft_export::PlannedFile> = choices.iter().filter(|c| c.set == *name).collect();
                    let mut all_on = !mine.is_empty() && mine.iter().all(|c| c.enabled);
                    if ui.add_enabled(!mine.is_empty(), egui::Checkbox::without_text(&mut all_on)).on_hover_text(tr("Export this texture set")).clicked() {
                        set_click = Some((name.clone(), all_on));
                    }
                    let overridden = ex.set_overrides.get(id).is_some_and(|o| !o.is_empty());
                    let label = if overridden { format!("{name} *") } else { name.clone() };
                    let r = ui.selectable_label(ex.selected_set == Some(*id), label);
                    let r = if overridden { r.on_hover_text(tr("Has its own export settings")) } else { r };
                    if r.clicked() {
                        ex.selected_set = Some(*id);
                    }
                });
            }
        });
        ui.separator();
        ui.vertical(|ui| match state.export.selected_set {
            None => global_settings(ui, state, lib, &settings, &choices, preset_status, now),
            Some(id) => {
                let name = sets.iter().find(|(s, _)| *s == id).map(|(_, n)| n.clone()).unwrap_or_default();
                set_settings(ui, &mut state.export, id, &name);
                ui.add_space(6.0);
                let mine: Vec<subcraft_export::PlannedFile> = choices.iter().filter(|c| c.set == name).cloned().collect();
                export_map_list(ui, state, &settings, &mine, Some(id));
            }
        });
    });
    if let Some((set, on)) = set_click {
        state.session.edit_export_selection(&settings, |sel, all| sel.set_set_on(&set, all, on));
    }
}

fn global_settings(ui: &mut egui::Ui, state: &mut UiState, lib: &Option<subcraft_app::assets::UserLibrary>, settings: &ExportSettings, choices: &[subcraft_export::PlannedFile], preset_status: &mut Option<String>, now: f64) {
    egui::Grid::new("export_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        let ex = &mut state.export;
        folder_row(ui, &mut ex.folder);
        ui.label(dim("Output template"));
        let list = ex.preset_list.get_or_insert_with(|| subcraft_app::export_presets::ExportPresets::load(lib.as_ref()));
        if let Some(s) = crate::export_presets::preset_bar(ui, list, &mut ex.preset, &mut ex.editor) {
            *preset_status = Some(s);
        }
        ui.end_row();
        global_rows(ui, ex, true);
    });
    ui.add_space(6.0);
    export_options(ui, state);
    let folder = PathBuf::from(&state.export.folder);
    baked_maps_section(ui, state, &folder, settings, now);
    ui.add_space(4.0);
    divider(ui);
    ui.add_space(4.0);
    export_map_list(ui, state, settings, choices, None);
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
        match state.session.export_scene(&folder, settings, format) {
            Ok(files) => state.set_status(format!("Exported {} files to {}", files.len(), folder.display()), now),
            Err(e) => state.set_status(format!("Export failed: {e}"), now),
        }
    }
}

/// A texture set's own file type, size and padding (each "Global" until set).
fn set_settings(ui: &mut egui::Ui, ex: &mut ExportUi, id: TextureSetId, name: &str) {
    ui.label(RichText::new(name).strong());
    ui.label(dim("Settings left on Global follow Global settings."));
    ui.add_space(4.0);
    let mut o = ex.set_overrides.get(&id).cloned().unwrap_or_default();
    egui::Grid::new(("export_set", id)).num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        ui.label(dim("File type"));
        ui.horizontal(|ui| {
            crate::export_presets::format_combo(ui, "set_format", &mut o.format, &mut o.dither, Some(tr("Global")));
        });
        ui.end_row();
        ui.label(dim("Size"));
        crate::export_presets::size_combo(ui, "set_size", &mut o.size, tr("Global"));
        ui.end_row();
        ui.label(dim("Padding"));
        crate::export_presets::padding_combo(ui, "set_padding", &mut o.padding, Some(tr("Global")));
        ui.end_row();
    });
    if o.is_empty() {
        ex.set_overrides.remove(&id);
    } else {
        ex.set_overrides.insert(id, o);
    }
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
/// With `set`, each row also has that texture set's own file type for the map.
fn export_map_list(ui: &mut egui::Ui, state: &mut UiState, settings: &ExportSettings, choices: &[subcraft_export::PlannedFile], set: Option<TextureSetId>) {
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
                    if let Some(id) = set {
                        // Painter's per-map file type and bit depth for this texture set.
                        let o = state.export.set_overrides.entry(id).or_default();
                        let mut format = o.maps.get(&f.suffix).map(|m| m.format);
                        let mut dither = o.maps.get(&f.suffix).map(|m| m.dither);
                        crate::export_presets::format_combo(ui, &format!("row_{}", f.suffix), &mut format, &mut dither, Some(tr("Template")));
                        match format {
                            Some(format) => {
                                o.maps.insert(f.suffix.clone(), subcraft_doc::MapFormat { format, dither: dither == Some(true) });
                            }
                            None => {
                                o.maps.remove(&f.suffix);
                            }
                        }
                        if o.is_empty() {
                            state.export.set_overrides.remove(&id);
                        }
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
    pub all_sets: bool,
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
        Self { open: false, settings: subcraft_app::BakeSettings::default(), high_poly: String::new(), all_sets: true, size_chosen: false, job: None, result: None, report: String::new(), notices: Vec::new(), log: Vec::new(), map_rows: Vec::new(),
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
    { let mut maps = state.bake.settings.maps.clone(); maps.sort(); state.bake.map_rows = job.map_progress(&maps); }
    if state.bake.log.len() != job.log().len() { state.bake.log = job.log().to_vec(); }
    if let Some(result) = result {
        let summary = state.bake.job.as_ref().map(|j| j.summary()).unwrap_or_default();
        { let mut maps = state.bake.settings.maps.clone(); maps.sort(); if let Some(j) = state.bake.job.as_ref() { state.bake.map_rows = j.map_progress(&maps); } }
        state.bake.job = None;
        let note = if state.bake.notices.is_empty() { "" } else { " Some low-poly parts had no high-poly match by name (see the Bake window)." };
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

pub fn bake_window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    use crate::bake_view::{card_title, icon_toggle, map_chip, status_row};
    use crate::icons;
    use crate::widgets::{card, icon_button, slider_row};
    use subcraft_core::MeshMapKind;
    if !state.bake.open {
        return;
    }
    let mut open = true;
    let selected = state.session.selected_texture_set();
    let (sel_id, sel_slot, sel_name) = (selected.id, selected.material_slot, selected.name.clone());
    let all: Vec<(subcraft_core::TextureSetId, u32)> = state.session.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    let low_name = state.session.scene().mesh.name.clone();
    // Maps below the texture set's resolution look blocky: default to the largest targeted set.
    let target_res = if state.bake.all_sets { state.session.document().texture_sets.iter().map(|t| t.resolution).max().unwrap_or(1024) } else { selected.resolution }.clamp(256, 4096);
    // Part matching and placement are project settings (saved with the project), edited here.
    let mut bake_match = state.session.settings().bake_match.clone();
    let mut placement = state.session.settings().bake_placement.clone();
    let low_box = { let b = state.session.scene().mesh.bounds; (b.min, b.max) };
    let high_box = state.bake.preview_mesh.as_ref().map(|m| (m.bounds.min, m.bounds.max));
    let baked_maps: Vec<MeshMapKind> = state.session.mesh_maps(sel_id).map(|m| m.iter().map(|(k, _)| k).collect()).unwrap_or_default();
    let reserve = if state.bake.job.is_some() || !state.bake.map_rows.is_empty() { 410.0 } else { 150.0 };
    let max_height = (ctx.content_rect().height() - 130.0 - reserve).max(160.0);
    crate::widgets::window(ctx, window_title(tr("Bake Mesh Maps"))).open(&mut open).collapsible(false).default_width(500.0)
        .default_pos(egui::pos2(32., if ctx.content_rect().height() < 800.0 { 48. } else { 80. })).show(ctx, |ui| {
        ui.set_max_width((ctx.content_rect().width() - 80.0).clamp(300.0, 540.0));
        egui::ScrollArea::vertical().id_salt("bake_scroll").max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let busy = state.bake.job.is_some();
        let has_high = !state.bake.high_poly.trim().is_empty();
        ui.add_enabled_ui(!busy, |ui| {
            let b = &mut state.bake;
            // --- Models -------------------------------------------------------------------
            card(ui, theme::ACCENT_DIM, |ui| {
                ui.set_width(ui.available_width());
                card_title(ui, icons::CUBE, tr("Models"), |_| {});
                egui::Grid::new("bake_sources").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
                    ui.label(dim(tr("Low poly")));
                    ui.label(RichText::new(format!("{}  {}", icons::CUBE, low_name)));
                    ui.end_row();
                    ui.label(dim(tr("High poly")));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut b.high_poly).hint_text(tr("optional (none = low on low)")).desired_width(230.0));
                        if icon_button(ui, icons::FOLDER_OPEN, tr("Browse…")).clicked() {
                            crate::files::pick(ui.ctx(), crate::files::Purpose::BakeHighPoly, crate::files::MESHES, false, None);
                        }
                        if !b.high_poly.is_empty() && icon_button(ui, icons::X, tr("Clear")).clicked() {
                            b.high_poly.clear();
                        }
                    });
                    ui.end_row();
                    if has_high { bake_match_rows(ui, &mut bake_match); }
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
                    crate::placement::placement_section(ui, &mut placement, low_box, high_box, &mut b.fit_mode, &mut b.high_opacity, &mut b.placement_message);
                });
            }
            // --- Maps ---------------------------------------------------------------------
            card(ui, theme::channel_accent(subcraft_core::ChannelKind::Normal), |ui| {
                ui.set_width(ui.available_width());
                card_title(ui, icons::IMAGE, tr("Maps to bake"), |ui| {
                    if ui.small_button(tr("None")).clicked() { b.settings.maps.clear(); }
                    if ui.small_button(tr("All")).clicked() { b.settings.maps = MeshMapKind::ALL.to_vec(); }
                });
                let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).floor();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    for k in MeshMapKind::ALL {
                        let on = b.settings.maps.contains(&k);
                        if map_chip(ui, k, on, width) {
                            if on { b.settings.maps.retain(|m| *m != k); } else { b.settings.maps.push(k); }
                        }
                    }
                });
            });
            // --- Quality ------------------------------------------------------------------
            card(ui, theme::channel_accent(subcraft_core::ChannelKind::Roughness), |ui| {
                ui.set_width(ui.available_width());
                card_title(ui, icons::SLIDERS, tr("Quality"), |_| {});
                egui::Grid::new("bake_quality").num_columns(2).spacing([16.0, 12.0]).show(ui, |ui| {
                    if !b.size_chosen { b.settings.size = target_res; }
                    ui.label(dim(tr("Resolution")));
                    egui::ComboBox::from_id_salt("bake_size").selected_text(format!("{0} × {0}", b.settings.size)).show_ui(ui, |ui| {
                        for r in [256u32, 512, 1024, 2048, 4096] {
                            if ui.selectable_value(&mut b.settings.size, r, format!("{r} × {r}")).clicked() { b.size_chosen = true; }
                        }
                    });
                    ui.end_row();
                    ui.label(dim(tr("Texture sets")));
                    let only = format!("{} {sel_name}", tr("Only"));
                    segmented(ui, &mut b.all_sets, &[(true, tr("All")), (false, only.as_str())]);
                    ui.end_row();
                });
                ui.add_space(6.0);
                let wants = |kinds: &[MeshMapKind]| kinds.iter().any(|k| b.settings.maps.contains(k));
                let (ao, curv) = (wants(&[MeshMapKind::AmbientOcclusion, MeshMapKind::BentNormal]), wants(&[MeshMapKind::Curvature]));
                if has_high {
                    slider_row(ui, tr("Cage front (reach above the surface)"), egui::Slider::new(&mut b.settings.cage_front, 0.0..=0.25).fixed_decimals(3).clamping(egui::SliderClamping::Never))
                        .on_hover_text(tr("Fraction of the model diagonal. Raise it when the high poly stands further out than this."));
                    ui.add_space(6.0);
                    slider_row(ui, tr("Cage rear (reach below the surface)"), egui::Slider::new(&mut b.settings.cage_rear, 0.0..=0.25).fixed_decimals(3).clamping(egui::SliderClamping::Never))
                        .on_hover_text(tr("Fraction of the model diagonal. Raise it when parts of the high poly sink under the low poly."));
                    ui.add_space(6.0);
                }
                if ao {
                    slider_row(ui, tr("AO samples"), egui::Slider::new(&mut b.settings.ao_samples, 4..=256).logarithmic(true));
                    ui.add_space(6.0);
                    slider_row(ui, tr("AO distance"), egui::Slider::new(&mut b.settings.ao_distance, 0.01..=1.0).logarithmic(true));
                    ui.add_space(6.0);
                }
                if b.settings.maps.contains(&MeshMapKind::Id) {
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
                    ui.add_space(6.0);
                }
                if curv {
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
                    if !baked_maps.is_empty() {
                        egui::ComboBox::from_id_salt("bake_display_map")
                            .selected_text(b.displayed_map.map_or(tr("Material"), |k| tr(k.display_name()))).show_ui(ui, |ui| {
                                if ui.selectable_value(&mut b.displayed_map, None, tr("Material")).clicked() { b.follow_maps = false; }
                                for &kind in &baked_maps {
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
        });
        });
        ui.add_space(6.0);
        // --- Progress and result (pinned below the scrolling cards, so Bake is always in view) ---
        card(ui, theme::ACCENT, |ui| {
            ui.set_width(ui.available_width());
            let mut go = false;
            let can = !state.bake.settings.maps.is_empty();
            let running = state.bake.job.as_ref();
            card_title(ui, icons::MAGIC_WAND, tr("Bake"), |ui| match &running {
                Some(job) => { if secondary_button(ui, tr("Cancel")).clicked() { job.cancel(); } }
                None => { go = ui.add_enabled_ui(can, |ui| primary_button(ui, format!("{}  {}", icons::MAGIC_WAND, tr("Bake")))).inner.clicked(); }
            });
            if let Some(job) = &state.bake.job {
                ui.label(dim(&job.stage()));
                ui.add(egui::ProgressBar::new(job.fraction()).show_percentage().fill(theme::ACCENT));
                ui.add_space(4.0);
            } else if let Some(result) = &state.bake.result {
                match result {
                    Ok(n) => { ui.label(RichText::new(format!("{}  {}", icons::CHECK, crate::i18n::trf("Bake complete: {} texture set(s)", &[n]))).color(crate::bake_view::OK_GREEN)); }
                    Err(e) => { ui.add(egui::Label::new(RichText::new(format!("{}  {}: {e}", icons::WARNING, tr("Bake failed"))).color(theme::WARNING)).wrap()); }
                }
                ui.add_space(4.0);
            }
            for p in &state.bake.map_rows { status_row(ui, p); }
            for note in &state.bake.notices {
                ui.add(egui::Label::new(RichText::new(format!("{}  {note}", icons::WARNING)).color(theme::WARNING)).wrap());
            }
            if !state.bake.map_rows.is_empty() || !state.bake.log.is_empty() { ui.add_space(4.0); }
            bake_log_section(ui, &state.bake.log);
            if go {
                        state.bake.result = None;
                        state.bake.notices.clear();
                        state.bake.log.clear();
                        state.bake.map_rows.clear();
                        state.bake.displayed_map = None;
                        state.bake.follow_maps = true;
                        let mut settings = state.bake.settings.clone();
                        settings.maps.sort();
                        settings.use_gpu = state.recovery.prefs.use_gpu_baking;
                        settings.name_match = state.session.settings().bake_match.clone();
                        settings.placement = state.session.settings().bake_placement.clone();
                        state.bake.map_rows = settings.maps.iter().map(|&kind| subcraft_app::MapProgress { kind, state: subcraft_app::MapState::Waiting, seconds: 0. }).collect();
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
        });
    });
    if bake_match != state.session.settings().bake_match || placement != state.session.settings().bake_placement {
        let mut project = state.session.settings().clone();
        project.bake_match = bake_match;
        project.bake_placement = placement;
        state.session.set_settings(project);
    }
    state.bake.open = open;
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
    ui.label(dim("Match"));
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
        for _ in 0..3 { draw(&mut state, &ctx, vec![]); }
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
}

#[cfg(test)]
mod bake_log_tests {
    use super::*;
    use subcraft_app::{BakeLogLevel, BakeLogLine, Session};

    #[test]
    fn the_bake_window_keeps_the_log_collapsed_under_details_and_counts_problems() {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::plane(2., 1), 128), None);
        state.bake.open = true; state.bake.preview = false;
        state.bake.log = vec![
            BakeLogLine { at: 0.0, level: BakeLogLevel::Info, text: "Baking 1 texture set(s)".into() },
            BakeLogLine { at: 1.2, level: BakeLogLevel::Warning, text: "Cage: 12 of 100 texels missed".into() },
        ];
        let frame = |state: &mut UiState| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 900.0))), ..Default::default() }, |ui| bake_window(state, ui.ctx(), 0.0));
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
