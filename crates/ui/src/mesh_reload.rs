//! Edit > Project configuration (Painter's window): the project's 3D mesh with Reimport mesh,
//! the import settings (Preserve strokes positions on mesh, Import cameras) and the UV tiles the
//! project uses, with OK / Cancel. Reimporting picks the updated model, shows how its materials
//! match the texture sets, and runs the background transfer (it clears the undo history). The
//! work itself is `subcraft_app::mesh_reload`; Ctrl+Shift+R opens the window ready to reimport.

use std::path::Path;
use std::sync::Arc;

use egui::RichText;
use subcraft_app::{MeshReloadJob, PreparedReload, ReloadMode, ReloadOutput};

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};
use crate::widgets::{caption, hint, primary_button, secondary_button};

#[derive(Default)]
pub struct MeshReloadUi {
    pub open: bool,
    pub path: String,
    /// Painter's "Reimport mesh": off until the user asks for it or picks another file.
    pub reimport: bool,
    pub check: Option<MeshReloadJob<PreparedReload>>,
    pub prepared: Option<Arc<PreparedReload>>,
    pub mode: Option<ReloadMode>,
    pub job: Option<MeshReloadJob<ReloadOutput>>,
    pub error: Option<String>,
    /// Scripted runs (`--reload-mesh`): reload with this mode as soon as the check finishes.
    pub auto: Option<ReloadMode>,
    /// Replace the project's cameras with the reloaded file's (Painter: off unless toggled).
    pub import_cameras: bool,
}

impl MeshReloadUi {
    pub fn busy(&self) -> bool {
        self.check.is_some() || self.job.is_some()
    }
}

/// Painter's "Preserve strokes positions on mesh": on moves the paint through the surface onto
/// the new UV layout (Reproject), off keeps the texels where they are (Keep UVs).
pub fn preserve_strokes(mode: ReloadMode) -> bool {
    mode == ReloadMode::Reproject
}

pub fn mode_for(preserve: bool) -> ReloadMode {
    if preserve { ReloadMode::Reproject } else { ReloadMode::KeepUvs }
}

/// Whether `path` names another file than the one the mesh came from (Painter ticks Reimport
/// mesh by itself then).
pub fn is_other_file(path: &str, source: Option<&Path>) -> bool {
    let path = path.trim();
    !path.is_empty() && source.is_none_or(|s| Path::new(path) != s)
}

/// What OK does now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OkAction {
    /// Nothing to reimport: close the window.
    Close,
    /// Start the reimport (the window closes when it finishes).
    Reimport,
    /// Reimport is ticked but the model is not checked yet (or a step runs): OK waits.
    Wait,
}

pub fn ok_action(reimport: bool, checked: bool, busy: bool) -> OkAction {
    match (reimport, checked, busy) {
        (false, _, false) => OkAction::Close,
        (true, true, false) => OkAction::Reimport,
        _ => OkAction::Wait,
    }
}

/// The UV Tiles section (read-only, as in Painter): the tiles the texture sets use.
pub fn uv_tile_summary(tiles: impl IntoIterator<Item = Option<u32>>) -> String {
    let set: std::collections::BTreeSet<u32> = tiles.into_iter().flatten().collect();
    match set.len() {
        0 => "UV tile workflow: off (one texture set per material)".to_string(),
        n => {
            let list: Vec<String> = set.iter().take(8).map(u32::to_string).collect();
            let more = if n > 8 { format!(" and {} more", n - 8) } else { String::new() };
            format!("UV tile workflow: on, {n} tile{} ({}{more})", if n == 1 { "" } else { "s" }, list.join(", "))
        }
    }
}

/// Open the window (Edit > Project configuration), defaulting the mesh path to the file the
/// mesh came from. Reimport stays off unless a scripted run asked for it.
pub fn open(state: &mut UiState) {
    let r = &mut state.mesh_reload;
    r.open = true;
    r.error = None;
    if r.path.is_empty()
        && let Some(p) = state.session.mesh_source()
    {
        r.path = p.display().to_string();
    }
    let scripted = state.mesh_reload.auto.is_some();
    if scripted && !state.mesh_reload.path.is_empty() && !state.mesh_reload.busy() {
        start_check(state);
    }
}

/// Ctrl+Shift+R (Painter's Reimport mesh): the window with Reimport mesh ticked and the model
/// file checked right away.
pub fn open_reimport(state: &mut UiState) {
    open(state);
    if state.mesh_reload.path.trim().is_empty() {
        state.mesh_reload.reimport = true;
        state.mesh_reload.error = Some("The model file is not known: choose it with Select…".into());
    } else if !state.mesh_reload.busy() {
        start_check(state);
    }
}

/// Check the model at the current path (import + material matching, in the background); this
/// ticks Reimport mesh.
pub fn start_check(state: &mut UiState) {
    let path = std::path::PathBuf::from(state.mesh_reload.path.trim());
    let r = &mut state.mesh_reload;
    r.reimport = true;
    r.prepared = None;
    r.mode = None;
    r.error = None;
    r.check = Some(state.session.start_mesh_reload_check(&path));
}

fn dim(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM())
}

/// Poll the background steps (call every frame).
pub fn poll(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if let Some(job) = &mut state.mesh_reload.check {
        ctx.request_repaint();
        if let Some(result) = job.poll() {
            state.mesh_reload.check = None;
            match result {
                Ok(p) => {
                    let p = Arc::new(p);
                    state.mesh_reload.mode = Some(p.suggested);
                    state.mesh_reload.prepared = Some(p.clone());
                    if let Some(mode) = state.mesh_reload.auto.take() {
                        state.mesh_reload.mode = Some(mode);
                        state.mesh_reload.job = Some(state.session.start_mesh_reload(p, mode));
                    }
                }
                Err(e) => state.mesh_reload.error = Some(e),
            }
        }
    }
    if let Some(job) = &mut state.mesh_reload.job {
        ctx.request_repaint();
        if let Some(result) = job.poll() {
            state.mesh_reload.job = None;
            match result.and_then(|out| state.session.finish_mesh_reload(out)) {
                Ok(report) => {
                    state.mesh_reloaded();
                    let mut status = report.summary();
                    if state.mesh_reload.import_cameras {
                        let path = std::path::PathBuf::from(state.mesh_reload.path.trim());
                        match state.session.import_mesh_cameras(&path) {
                            Ok(n) => status.push_str(&format!(" · {n} camera{}", if n == 1 { "" } else { "s" })),
                            Err(e) => status.push_str(&format!(" · {e}")),
                        }
                    }
                    state.set_status(status, now);
                    let r = &mut state.mesh_reload;
                    r.open = false;
                    r.reimport = false;
                    r.prepared = None;
                    r.mode = None;
                }
                Err(e) => state.mesh_reload.error = Some(e),
            }
        }
    }
}

pub fn window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    poll(state, ctx, now);
    if !state.mesh_reload.open {
        return;
    }
    let mut open = true;
    let mut start = None;
    let mut check = false;
    let mut cancel_job = false;
    let mut close = false;
    let source = state.session.mesh_source().map(Path::to_path_buf);
    let tiles = uv_tile_summary(state.session.document().texture_sets.iter().map(|t| t.udim_number()));
    egui::Window::new(RichText::new("Project configuration").font(theme::font_strong(14.0))).open(&mut open).collapsible(false).default_width(480.0).show(ctx, |ui| {
        let busy = state.mesh_reload.busy();
        let r = &mut state.mesh_reload;
        // ----- 3D mesh -----
        ui.label(caption("3D mesh"));
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                let edit = ui.add(egui::TextEdit::singleline(&mut r.path).hint_text("OBJ, FBX, glTF, GLB, PLY or STL").desired_width(290.0));
                if edit.changed() && is_other_file(&r.path, source.as_deref()) {
                    r.reimport = true;
                    r.prepared = None;
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    check = true;
                }
                if ui.button("Select…").clicked() {
                    crate::files::pick(ui.ctx(), crate::files::Purpose::ReloadMesh, crate::files::MESHES, false, Some("Updated model"));
                }
            });
            ui.horizontal(|ui| {
                if ui.checkbox(&mut r.reimport, "Reimport mesh").on_hover_text("Replace the model with the file above and keep the painted layers (ticks itself when another file is chosen)").changed() && r.reimport && r.prepared.is_none() {
                    check = true;
                }
                if r.reimport && r.prepared.is_none() && r.check.is_none() && ui.button("Check").clicked() {
                    check = true;
                }
            });
        });
        if let Some(job) = &r.check {
            ui.add(egui::ProgressBar::new(job.progress().fraction()).text(job.progress().stage()));
        }
        if r.reimport && let Some(p) = r.prepared.clone() {
            ui.add_space(4.0);
            for s in &p.sets {
                let text = match (s.old, s.uv_moved) {
                    (Some(_), Some(m)) if m > 0.02 => format!("{}  matched, UVs changed ({:.0}% moved)", s.name, m * 100.0),
                    (Some(_), _) => format!("{}  matched, UVs unchanged", s.name),
                    (None, _) => format!("{}  new material: new empty texture set", s.name),
                };
                ui.label(text);
            }
            for name in &p.unmatched_old {
                ui.label(RichText::new(format!("{name}  no matching material: kept, but not shown on the model")).color(theme::WARNING()));
            }
            for w in &p.warnings {
                hint(ui, w);
            }
        }
        // ----- UV tiles (read-only) -----
        ui.add_space(8.0);
        ui.label(caption("UV Tiles"));
        ui.label(dim(&tiles));
        // ----- Import settings -----
        ui.add_space(8.0);
        ui.label(caption("Import settings"));
        ui.add_enabled_ui(!busy, |ui| {
            let suggested = r.prepared.as_ref().map(|p| p.suggested).unwrap_or(ReloadMode::Reproject);
            let mut preserve = preserve_strokes(r.mode.unwrap_or(suggested));
            if ui.checkbox(&mut preserve, "Preserve strokes positions on mesh")
                .on_hover_text("On: the paint moves through the surface onto the new UV layout (for UVs that changed). Off: the texels stay where they are (for an unchanged UV layout).")
                .changed()
            {
                r.mode = Some(mode_for(preserve));
            }
            ui.checkbox(&mut r.import_cameras, "Import cameras").on_hover_text("Replace the project's cameras with the ones stored in the model file (glTF, GLB or FBX) when reimporting");
        });
        if r.reimport {
            ui.add_space(6.0);
            hint(ui, "Reimporting cannot be undone: the undo history is cleared. Save first to keep this version. Baked mesh maps are kept but marked for rebaking.");
        }
        if let Some(e) = &r.error {
            ui.add_space(6.0);
            ui.label(RichText::new(e).color(theme::WARNING()));
        }
        // ----- footer -----
        ui.add_space(8.0);
        if let Some(job) = &r.job {
            ui.add(egui::ProgressBar::new(job.progress().fraction()).show_percentage().text(job.progress().stage()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let action = ok_action(r.reimport, r.prepared.is_some(), busy);
            let ok = ui.add_enabled_ui(action != OkAction::Wait, |ui| primary_button(ui, "OK")).inner;
            let ok = if action == OkAction::Wait && r.reimport && !busy { ok.on_disabled_hover_text("Check the model first") } else { ok };
            if ok.clicked() {
                match action {
                    OkAction::Close => close = true,
                    OkAction::Reimport => {
                        if let Some(p) = r.prepared.clone() {
                            start = Some((p.clone(), r.mode.unwrap_or(p.suggested)));
                        }
                    }
                    OkAction::Wait => {}
                }
            }
            if secondary_button(ui, "Cancel").clicked() {
                if r.job.is_some() { cancel_job = true } else { close = true }
            }
        });
    });
    if check {
        start_check(state);
    }
    if cancel_job && let Some(job) = state.mesh_reload.job.take() {
        job.cancel();
    }
    if let Some((p, mode)) = start {
        if state.session.is_stroking() {
            state.mesh_reload.error = Some("finish the stroke first".into());
        } else {
            state.mesh_reload.job = Some(state.session.start_mesh_reload(p, mode));
        }
    }
    if !open || close {
        // Closing cancels any running step (dropping a job cancels it).
        let r = &mut state.mesh_reload;
        r.open = false;
        r.reimport = false;
        r.check = None;
        r.job = None;
        r.prepared = None;
        r.mode = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserve_strokes_is_reproject_and_off_keeps_uvs() {
        assert!(preserve_strokes(ReloadMode::Reproject) && !preserve_strokes(ReloadMode::KeepUvs));
        for preserve in [true, false] {
            assert_eq!(preserve_strokes(mode_for(preserve)), preserve);
        }
    }

    #[test]
    fn another_file_ticks_reimport_but_the_same_one_does_not() {
        let src = Path::new("D:/models/robot.fbx");
        assert!(!is_other_file("D:/models/robot.fbx", Some(src)));
        assert!(!is_other_file("  D:/models/robot.fbx ", Some(src)), "surrounding spaces don't count");
        assert!(is_other_file("D:/models/robot_v2.fbx", Some(src)));
        assert!(is_other_file("D:/models/robot.fbx", None), "unknown source: any file is another one");
        assert!(!is_other_file("   ", Some(src)) && !is_other_file("", None));
    }

    #[test]
    fn ok_closes_reimports_or_waits_for_the_check() {
        assert_eq!(ok_action(false, false, false), OkAction::Close);
        assert_eq!(ok_action(false, true, false), OkAction::Close, "a checked model is not reimported unless ticked");
        assert_eq!(ok_action(true, true, false), OkAction::Reimport);
        assert_eq!(ok_action(true, false, false), OkAction::Wait, "reimport ticked, model not checked yet");
        assert_eq!(ok_action(true, true, true), OkAction::Wait, "a step is running");
        assert_eq!(ok_action(false, false, true), OkAction::Wait);
    }

    #[test]
    fn uv_tiles_section_lists_the_tiles_once_in_order() {
        assert_eq!(uv_tile_summary([None, None]), "UV tile workflow: off (one texture set per material)");
        assert_eq!(uv_tile_summary([Some(1002), None, Some(1001), Some(1002)]), "UV tile workflow: on, 2 tiles (1001, 1002)");
        assert_eq!(uv_tile_summary([Some(1001)]), "UV tile workflow: on, 1 tile (1001)");
        let many = uv_tile_summary((0..10).map(|i| Some(1001 + i)));
        assert!(many.starts_with("UV tile workflow: on, 10 tiles (1001, ") && many.ends_with("1008 and 2 more)"), "{many}");
    }
}
