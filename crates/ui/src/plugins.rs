//! Plugins menu: SubCraft scripts (`.rhai`) run on the open project. The script engine lives in
//! the `subcraft-mcp` crate (above this one), so the desktop binary hands it in with
//! [`set_runner`]; without a runner the menu says scripting is unavailable.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use subcraft_app::Session;

use crate::state::UiState;

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

/// The file extension of plugins.
pub const EXTENSION: &str = "rhai";

/// Environment variable that points the Plugins menu at another folder.
pub const ENV_VAR: &str = "SUBCRAFT_PLUGINS";

/// Example plugins that ship with the app (Plugins > Examples), from `docs/scripting/examples`.
pub const EXAMPLES: &[(&str, &str)] = &[
    ("Add grime to every texture set", include_str!("../../../docs/scripting/examples/add_grime.rhai")),
    ("List the layer stack", include_str!("../../../docs/scripting/examples/list_layers.rhai")),
    ("Hide all paint layers", include_str!("../../../docs/scripting/examples/hide_paint_layers.rhai")),
];

/// Where the user's plugins live: `SUBCRAFT_PLUGINS`, else a `Plugins` folder next to the user
/// library (Documents/SubCraft/Plugins).
pub fn folder() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(ENV_VAR).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    subcraft_app::assets::UserLibrary::default_root().and_then(|root| root.parent().map(|p| p.join("Plugins")))
}

/// Scripts in a plugins folder, sorted by name: (display name, path). Underscores and dashes in
/// file names read as spaces.
pub fn list(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION)))
        .map(|p| (p.file_stem().map(|s| s.to_string_lossy().replace(['_', '-'], " ")).unwrap_or_default(), p))
        .collect();
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
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

/// The Plugins menu's contents.
pub fn menu(state: &mut UiState, ui: &mut egui::Ui, now: f64) {
    use crate::i18n::tr;
    if runner().is_none() {
        ui.weak(tr("Scripting is not available in this build"));
        return;
    }
    let dir = folder();
    let plugins = dir.as_deref().map(list).unwrap_or_default();
    if plugins.is_empty() {
        ui.weak(tr("No plugins yet: put .rhai scripts in the plugins folder"));
    }
    for (name, path) in &plugins {
        if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
            ui.close();
            run_file(state, path, now);
        }
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
    if let Some(dir) = dir {
        if ui.button(tr("Open plugins folder")).on_hover_text(dir.display().to_string()).clicked() {
            ui.close();
            let _ = std::fs::create_dir_all(&dir);
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file://{}", dir.display())));
        }
    }
    if ui.button(tr("Scripting guide")).clicked() {
        ui.close();
        ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/Aalzard/subcraft/blob/main/docs/scripting.md"));
    }
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
}
