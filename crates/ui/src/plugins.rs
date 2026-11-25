//! Plugins menu and Plugins window. Plugins are folders with a `plugin.json` (see
//! `subcraft_app::plugins`): their scripts (`.rhai`) run on the open project from the menu and
//! their assets join the Assets panel as a library while they are on. Loose `.rhai` files in the
//! plugins folder still run too. The script engine lives in the `subcraft-mcp` crate (above this
//! one), so the desktop binary hands it in with [`set_runner`]; without a runner the menu says
//! scripting is unavailable (plugin assets still load).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use egui::RichText;
use subcraft_app::Session;
use subcraft_app::assets::UserLibrary;
use subcraft_app::plugins::{self, Plugin};

use crate::state::UiState;
use crate::theme;

/// What a script run reports back to the app.
pub struct RunReport {
    /// One line for the status bar.
    pub summary: String,
    /// The script's `print` output.
    pub output: Vec<String>,
    pub ok: bool,
}

/// Runs a script's source on the open project and hands the project back.
pub type Runner = fn(Session, &str) -> (Session, RunReport);

static RUNNER: OnceLock<Runner> = OnceLock::new();

/// Install the script engine (the desktop binary does this at start-up).
pub fn set_runner(runner: Runner) {
    let _ = RUNNER.set(runner);
}

pub fn runner() -> Option<Runner> {
    RUNNER.get().copied()
}

/// The file extension of plugin scripts.
pub const EXTENSION: &str = subcraft_app::plugins::SCRIPT_EXTENSION;

/// Environment variable that points the Plugins menu at another folder.
pub const ENV_VAR: &str = "SUBCRAFT_PLUGINS";

/// This build's version, which plugins may require (`min_subcraft_version`).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Example scripts that ship with the app (Plugins > Examples), from `docs/scripting/examples`.
pub const EXAMPLES: &[(&str, &str)] = &[
    ("Add grime to every texture set", include_str!("../../../docs/scripting/examples/add_grime.rhai")),
    ("List the layer stack", include_str!("../../../docs/scripting/examples/list_layers.rhai")),
    ("Hide all paint layers", include_str!("../../../docs/scripting/examples/hide_paint_layers.rhai")),
];

macro_rules! example_files {
    ($dir:literal: $($file:literal),* $(,)?) => {
        &[$(($file, include_bytes!(concat!("../../../docs/plugins/examples/", $dir, "/", $file)) as &[u8])),*]
    };
}

/// Example plugins built into the app (Plugins window > Install example plugins), from
/// `docs/plugins/examples`: each is its files as (path inside the plugin, bytes).
pub const EXAMPLE_PLUGINS: &[&[(&str, &[u8])]] = &[
    example_files!("weathering-tools":
        "plugin.json",
        "scripts/Add_edge_wear.rhai",
        "scripts/Add_ground_grime.rhai",
        "scripts/List_the_layer_stack.rhai",
    ),
    example_files!("painted-metal-pack":
        "plugin.json",
        "scripts/Paint_every_texture_set_yellow.rhai",
        "library/smart-materials/Hazard Yellow Painted Metal.scsmart",
        "library/smart-materials/Olive Drab Painted Metal.scsmart",
        "library/smart-masks/Heavy Edge Scratches.scmask",
        "library/smart-masks/Light Dust.scmask",
        "library/export-presets/a3b1f0c2-5e7d-4f9a-8c6b-1d2e3f4a5b60.json",
        "library/export-presets/6d0f5d1e-7c43-4a8e-b1f0-2f6c5e0a9b31.json",
    ),
];

/// Where the user's plugins live: `SUBCRAFT_PLUGINS`, else a `Plugins` folder next to the user
/// library (Documents/SubCraft/Plugins).
pub fn folder() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(ENV_VAR).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    subcraft_app::assets::UserLibrary::default_root().and_then(|root| root.parent().map(|p| p.join("Plugins")))
}

/// Loose scripts directly in a plugins folder, sorted by name: (display name, path).
/// Underscores and dashes in file names read as spaces.
pub fn list(dir: &Path) -> Vec<(String, PathBuf)> {
    subcraft_app::plugins::list_scripts(dir)
}

/// The installed plugins and the Plugins window.
#[derive(Default)]
pub struct PluginsUi {
    /// Plugins found in the plugins folder (read at start-up, when the window opens and after
    /// every change made in it).
    pub list: Vec<Plugin>,
    pub open: bool,
    /// The plugin folder whose Remove waits for a second click.
    confirm_remove: Option<PathBuf>,
    /// Name typed for a new plugin.
    new_name: String,
}

impl PluginsUi {
    /// Plugins that are on and usable.
    pub fn enabled<'a>(&'a self, disabled: &'a [String]) -> impl Iterator<Item = &'a Plugin> + 'a {
        self.list.iter().filter(move |p| p.usable() && plugins::is_enabled(p, disabled))
    }

    /// Asset libraries of the plugins that are on: (plugin name, library).
    pub fn active_libraries(&self, disabled: &[String]) -> Vec<(String, UserLibrary)> {
        self.enabled(disabled).filter_map(|p| p.library().map(|l| (p.name(), l))).collect()
    }
}

impl UiState {
    /// Read the plugins folder again and use the plugins that are on.
    pub fn reload_plugins(&mut self) {
        self.plugins.list = folder().map(|d| plugins::discover(&d, APP_VERSION)).unwrap_or_default();
        self.apply_plugins();
    }

    /// Start-up: find the plugins; their assets load with the libraries.
    pub fn load_plugins_at_start(&mut self) {
        self.plugins.list = folder().map(|d| plugins::discover(&d, APP_VERSION)).unwrap_or_default();
        let active = self.plugins.active_libraries(&self.recovery.prefs.disabled_plugins);
        plugins::set_active_libraries(active.iter().map(|(_, l)| l.clone()).collect());
        log::info!("plugins: {} found, {} with assets on", self.plugins.list.len(), active.len());
    }

    /// Use the plugins that are on: their assets in the Assets panel and the Export / Bake preset
    /// lists (read again on next use).
    pub fn apply_plugins(&mut self) {
        let active = self.plugins.active_libraries(&self.recovery.prefs.disabled_plugins);
        plugins::set_active_libraries(active.into_iter().map(|(_, l)| l).collect());
        self.export.preset_list = None;
        self.bake.presets = None;
        let (extra, default) = (self.recovery.prefs.extra_libraries.clone(), self.recovery.prefs.default_library.clone());
        self.set_libraries(&extra, &default);
    }
}

/// Run `source` (named `name` in messages) on the open project.
pub fn run(state: &mut UiState, name: &str, source: &str, now: f64) {
    let Some(runner) = runner() else {
        state.set_status("Scripting is not available in this build", now);
        return;
    };
    if state.session.is_stroking() {
        state.set_status("Finish the stroke before running a plugin", now);
        return;
    }
    // The project goes into the script engine for the run and comes back edited; a tiny stand-in
    // holds its place meanwhile (the run is synchronous, nothing draws it).
    let stand_in = Session::new(subcraft_mesh::primitives::cube(1.0), 16);
    let session = std::mem::replace(&mut state.session, stand_in);
    let (session, report) = runner(session, source);
    state.session = session;
    log::info!("plugin: {name}: {}", report.summary);
    for line in &report.output {
        log::info!("plugin {name}: {line}");
    }
    let last = report.output.last().map(|l| format!(" - {l}")).unwrap_or_default();
    state.set_status(if report.ok { format!("{name}: {}{last}", report.summary) } else { format!("{name}: {}", report.summary) }, now);
}

/// Run the plugin file at `path`.
pub fn run_file(state: &mut UiState, path: &Path, now: f64) {
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    match std::fs::read_to_string(path) {
        Ok(source) => run(state, &name, &source, now),
        Err(e) => state.set_status(format!("Cannot read {}: {e}", path.display()), now),
    }
}

fn open_folder(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    crate::panels::shelf::show_in_folder(dir);
}

/// The Plugins menu's contents.
pub fn menu(state: &mut UiState, ui: &mut egui::Ui, now: f64) {
    use crate::i18n::tr;
    let dir = folder();
    if runner().is_none() {
        ui.weak(tr("Scripting is not available in this build"));
    } else {
        // Scripts of the plugins that are on, one submenu per plugin.
        let mut any = false;
        let with_scripts: Vec<(String, Vec<(String, PathBuf)>)> =
            state.plugins.enabled(&state.recovery.prefs.disabled_plugins).filter(|p| !p.scripts.is_empty()).map(|p| (p.name(), p.scripts.clone())).collect();
        for (name, scripts) in with_scripts {
            any = true;
            ui.menu_button(name, |ui| {
                for (script, path) in &scripts {
                    if ui.button(script).on_hover_text(path.display().to_string()).clicked() {
                        ui.close();
                        run_file(state, path, now);
                    }
                }
            });
        }
        // Loose scripts directly in the plugins folder (the 0.6 way, still supported).
        for (name, path) in dir.as_deref().map(list).unwrap_or_default() {
            any = true;
            if ui.button(&name).on_hover_text(path.display().to_string()).clicked() {
                ui.close();
                run_file(state, &path, now);
            }
        }
        if !any {
            ui.weak(tr("No plugin scripts yet"));
        }
        ui.separator();
        ui.menu_button(tr("Examples"), |ui| {
            for (name, source) in EXAMPLES {
                if ui.button(*name).clicked() {
                    ui.close();
                    run(state, name, source, now);
                }
            }
        });
        if ui.button(tr("Run script file...")).clicked() {
            ui.close();
            if let Some(path) = rfd::FileDialog::new().add_filter("SubCraft script", &[EXTENSION]).pick_file() {
                run_file(state, &path, now);
            }
        }
    }
    ui.separator();
    if ui.button(tr("Manage plugins...")).clicked() {
        ui.close();
        state.reload_plugins();
        state.plugins.open = true;
    }
    if let Some(dir) = dir {
        if ui.button(tr("Open plugins folder")).on_hover_text(dir.display().to_string()).clicked() {
            ui.close();
            open_folder(&dir);
        }
    }
    if ui.button(tr("Plugin guide")).clicked() {
        ui.close();
        ui.ctx().open_url(egui::OpenUrl::new_tab(GUIDE_URL));
    }
    if ui.button(tr("Scripting guide")).clicked() {
        ui.close();
        ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/Aalzard/subcraft/blob/main/docs/scripting.md"));
    }
}

/// The guide to writing plugins.
pub const GUIDE_URL: &str = "https://github.com/Aalzard/subcraft/blob/main/docs/plugins.md";

/// What the Plugins window asked for this frame.
enum WindowAction {
    Toggle(usize, bool),
    Remove(usize),
    InstallZip,
    InstallExamples,
    Create(String),
    Reload,
}

/// Install every example plugin into `dir`; returns how many were installed.
pub fn install_examples(dir: &Path) -> Result<usize, String> {
    for files in EXAMPLE_PLUGINS {
        plugins::install_files(files, dir, APP_VERSION)?;
    }
    Ok(EXAMPLE_PLUGINS.len())
}

/// Plugins > Manage plugins...: the installed plugins with on/off switches, install from a zip,
/// the example plugins, a new empty plugin.
pub fn window(state: &mut UiState, ctx: &egui::Context) {
    use crate::i18n::tr;
    if !state.plugins.open {
        return;
    }
    let now = ctx.input(|i| i.time);
    let dir = folder();
    let mut open = true;
    let mut action = None;
    let disabled = state.recovery.prefs.disabled_plugins.clone();
    crate::widgets::window(ctx, RichText::new(tr("Plugins")).font(theme::font_strong(14.0))).id(egui::Id::new("plugins_window")).open(&mut open).collapsible(false).resizable(true).default_width(520.0).show(ctx, |ui| {
        ui.set_min_width(480.0);
        ui.add(egui::Label::new(RichText::new(tr("Plugins add scripts to the Plugins menu and assets to the Assets panel. Scripts can change and save your project: only install plugins from people you trust.")).color(theme::TEXT_DIM())).wrap());
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(tr("Install from .zip...")).clicked() {
                action = Some(WindowAction::InstallZip);
            }
            if ui.button(tr("Install example plugins")).on_hover_text(tr("Weathering Tools and Painted Metal Pack")).clicked() {
                action = Some(WindowAction::InstallExamples);
            }
            if let Some(dir) = &dir {
                if ui.button(tr("Open plugins folder")).on_hover_text(dir.display().to_string()).clicked() {
                    open_folder(dir);
                }
            }
            if ui.button(tr("Reload")).on_hover_text(tr("Read the plugins folder again")).clicked() {
                action = Some(WindowAction::Reload);
            }
        });
        ui.separator();
        if state.plugins.list.is_empty() {
            ui.weak(tr("No plugins installed"));
        }
        egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
            for (i, p) in state.plugins.list.iter().enumerate() {
                let on = plugins::is_enabled(p, &disabled);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let mut checked = on && p.usable();
                        let toggle = ui.add_enabled(p.usable(), egui::Checkbox::without_text(&mut checked)).on_hover_text(tr("Switch the plugin on or off"));
                        if toggle.changed() {
                            action = Some(WindowAction::Toggle(i, checked));
                        }
                        ui.label(RichText::new(p.name()).strong());
                        if let Some(m) = &p.manifest {
                            if !m.version.trim().is_empty() {
                                ui.weak(m.version.trim());
                            }
                            if !m.author.trim().is_empty() {
                                ui.weak(format!("· {}", m.author.trim()));
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let confirming = state.plugins.confirm_remove.as_deref() == Some(p.dir.as_path());
                            if ui.button(if confirming { tr("Click again to remove") } else { tr("Remove") }).on_hover_text(tr("Delete the plugin's folder")).clicked() {
                                if confirming {
                                    action = Some(WindowAction::Remove(i));
                                } else {
                                    state.plugins.confirm_remove = Some(p.dir.clone());
                                }
                            }
                            if ui.button(tr("Show folder")).on_hover_text(p.dir.display().to_string()).clicked() {
                                open_folder(&p.dir);
                            }
                        });
                    });
                    if let Some(m) = &p.manifest {
                        if !m.description.trim().is_empty() {
                            ui.add(egui::Label::new(m.description.trim()).wrap());
                        }
                        if !m.homepage.trim().is_empty() && m.homepage.trim().starts_with("https://") {
                            ui.hyperlink(m.homepage.trim());
                        }
                    }
                    ui.weak(p.contents_summary());
                    if let Some(problem) = &p.problem {
                        ui.add(egui::Label::new(RichText::new(problem).color(theme::ERROR())).wrap());
                    }
                });
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(tr("New plugin:"));
            ui.add(egui::TextEdit::singleline(&mut state.plugins.new_name).hint_text(tr("Name")).desired_width(200.0));
            if ui.add_enabled(!state.plugins.new_name.trim().is_empty(), egui::Button::new(tr("Create"))).on_hover_text(tr("Make an empty plugin folder to fill in")).clicked() {
                action = Some(WindowAction::Create(state.plugins.new_name.trim().to_string()));
            }
        });
        if ui.link(tr("How to write a plugin")).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(GUIDE_URL));
        }
    });
    state.plugins.open = open;
    if !open {
        state.plugins.confirm_remove = None;
    }
    let Some(action) = action else { return };
    let Some(dir) = dir else {
        state.set_status("Plugins need a Documents folder (or SUBCRAFT_PLUGINS)", now);
        return;
    };
    let status = match action {
        WindowAction::Toggle(i, on) => {
            if let Some(p) = state.plugins.list.get(i).cloned() {
                plugins::set_enabled(&p, on, &mut state.recovery.prefs.disabled_plugins);
                state.apply_plugins();
                format!("{} {}", p.name(), if on { "is on" } else { "is off" })
            } else {
                return;
            }
        }
        WindowAction::Remove(i) => {
            state.plugins.confirm_remove = None;
            let Some(p) = state.plugins.list.get(i).cloned() else { return };
            match plugins::uninstall(&p, &dir) {
                Ok(()) => {
                    state.reload_plugins();
                    format!("Removed plugin {}", p.name())
                }
                Err(e) => format!("Cannot remove plugin: {e}"),
            }
        }
        WindowAction::InstallZip => {
            let Some(zip) = rfd::FileDialog::new().add_filter("SubCraft plugin", &["zip"]).pick_file() else { return };
            match plugins::install_zip(&zip, &dir, APP_VERSION) {
                Ok(p) => {
                    state.reload_plugins();
                    format!("Installed plugin {}: {}", p.name(), p.contents_summary())
                }
                Err(e) => format!("Cannot install {}: {e}", zip.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            }
        }
        WindowAction::InstallExamples => match install_examples(&dir) {
            Ok(n) => {
                state.reload_plugins();
                format!("Installed {n} example plugins")
            }
            Err(e) => format!("Cannot install the example plugins: {e}"),
        },
        WindowAction::Create(name) => match plugins::create_plugin(&name, "", &dir, APP_VERSION) {
            Ok(p) => {
                state.plugins.new_name.clear();
                state.reload_plugins();
                open_folder(&p.dir);
                format!("Created plugin {} in {}", p.name(), p.dir.display())
            }
            Err(e) => format!("Cannot create the plugin: {e}"),
        },
        WindowAction::Reload => {
            state.reload_plugins();
            format!("{} plugins found", state.plugins.list.len())
        }
    };
    state.set_status(status, now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_are_listed_by_name_from_their_folder() {
        let dir = std::env::temp_dir().join(format!("subcraft-plugins-{}", subcraft_core::ObjectId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["weather_all_sets.rhai", "Add-Grime.RHAI", "notes.txt"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let names: Vec<String> = list(&dir).into_iter().map(|p| p.0).collect();
        assert_eq!(names, ["Add Grime", "weather all sets"]);
        std::fs::remove_dir_all(&dir).ok();
        assert!(list(&dir).is_empty(), "a missing folder lists nothing");
    }

    fn report(session: Session, source: &str) -> (Session, RunReport) {
        // Stand-in engine for the ui crate's tests: renames the selected texture set.
        let mut session = session;
        let ts = session.selected_texture_set().id;
        session.execute(subcraft_app::Command::RenameTextureSet { texture_set: ts, name: source.to_string() }).unwrap();
        (session, RunReport { summary: "Script finished (1 tool call)".into(), output: vec!["done".into()], ok: true })
    }

    #[test]
    fn a_run_edits_the_open_project_in_place_and_reports_in_the_status_bar() {
        set_runner(report);
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(1.0), 64), None);
        let ts = state.session.selected_texture_set().id;
        run(&mut state, "Rename", "Renamed by plugin", 0.0);
        assert_eq!(state.session.selected_texture_set().id, ts, "the same project came back");
        assert_eq!(state.session.selected_texture_set().name, "Renamed by plugin");
        let status = state.status.as_ref().map(|s| s.0.clone()).unwrap_or_default();
        assert!(status.contains("Rename: Script finished (1 tool call) - done"), "{status}");
    }

    #[test]
    fn the_built_in_example_plugins_install_and_their_assets_join_the_libraries() {
        let dir = std::env::temp_dir().join(format!("subcraft-plugins-{}", subcraft_core::ObjectId::new()));
        assert_eq!(install_examples(&dir).unwrap(), 2);
        // Installing again updates them in place.
        assert_eq!(install_examples(&dir).unwrap(), 2);
        let found = plugins::discover(&dir, APP_VERSION);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(Plugin::usable), "{:?}", found.iter().map(|p| &p.problem).collect::<Vec<_>>());
        // The embedded files are the whole example folders.
        let docs = plugins::discover(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plugins/examples"), APP_VERSION);
        let summary = |list: &[Plugin]| list.iter().map(|p| (p.name(), p.contents_summary())).collect::<Vec<_>>();
        assert_eq!(summary(&found), summary(&docs));

        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(1.0), 64), None);
        state.plugins.list = found;
        state.apply_plugins();
        let ids: Vec<&str> = state.libraries.iter().filter(|l| l.plugin).map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["Painted Metal Pack"], "only the plugin with assets is a library");
        assert!(state.user_library.is_none(), "a plugin never becomes the default library");
        let has = |state: &UiState, name: &str| state.library.smart_materials.iter().any(|m| m.name == name);
        assert!(has(&state, "Hazard Yellow Painted Metal"));
        assert!(state.export.presets(None).find("Roblox SurfaceAppearance").is_some());

        // Off: its assets and presets leave.
        let metal = state.plugins.list.iter().find(|p| p.name() == "Painted Metal Pack").unwrap().clone();
        plugins::set_enabled(&metal, false, &mut state.recovery.prefs.disabled_plugins);
        state.apply_plugins();
        assert!(!has(&state, "Hazard Yellow Painted Metal"));
        assert!(state.export.presets(None).find("Roblox SurfaceAppearance").is_none());
        assert_eq!(state.plugins.enabled(&state.recovery.prefs.disabled_plugins).count(), 1);
        plugins::set_active_libraries(Vec::new());
        std::fs::remove_dir_all(&dir).ok();
    }
}
