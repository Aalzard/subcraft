//! File > Reload mesh: pick the updated model, see how its materials match the texture sets,
//! choose Keep UVs or Reproject, confirm (the reload clears the undo history) and watch the
//! background transfer. The work itself is `subcraft_app::mesh_reload`.

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
    pub check: Option<MeshReloadJob<PreparedReload>>,
    pub prepared: Option<Arc<PreparedReload>>,
    pub mode: Option<ReloadMode>,
    pub job: Option<MeshReloadJob<ReloadOutput>>,
    pub error: Option<String>,
    /// Scripted runs (`--reload-mesh`): reload with this mode as soon as the check finishes.
    pub auto: Option<ReloadMode>,
}

impl MeshReloadUi {
    pub fn busy(&self) -> bool {
        self.check.is_some() || self.job.is_some()
    }
}

/// Open the window, defaulting to the file the mesh came from, and check it right away.
pub fn open(state: &mut UiState) {
    let r = &mut state.mesh_reload;
    r.open = true;
    r.error = None;
    if r.path.is_empty()
        && let Some(p) = state.session.mesh_source()
    {
        r.path = p.display().to_string();
    }
    if !state.mesh_reload.path.is_empty() && !state.mesh_reload.busy() {
        start_check(state);
    }
}

/// Check the model at the current path (import + material matching, in the background).
pub fn start_check(state: &mut UiState) {
    let path = std::path::PathBuf::from(state.mesh_reload.path.trim());
    let r = &mut state.mesh_reload;
    r.prepared = None;
    r.mode = None;
    r.error = None;
    r.check = Some(state.session.start_mesh_reload_check(&path));
}

fn dim(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM)
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
                    state.set_status(report.summary(), now);
                    let r = &mut state.mesh_reload;
                    r.open = false;
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
    let mut cancel = false;
    egui::Window::new(RichText::new("Reload Mesh").font(theme::font_strong(14.0))).open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
        let busy = state.mesh_reload.busy();
        let r = &mut state.mesh_reload;
        ui.label(dim("Replace the model with an updated version and keep the painted layers."));
        ui.add_space(6.0);
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                ui.label(dim("Model"));
                let edit = ui.add(egui::TextEdit::singleline(&mut r.path).hint_text("OBJ, FBX, glTF or GLB").desired_width(270.0));
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    check = true;
                }
                if ui.button("Browse…").clicked() {
                    crate::files::pick(ui.ctx(), crate::files::Purpose::ReloadMesh, crate::files::MESHES, false, Some("Updated model"));
                }
                if ui.button("Check").clicked() {
                    check = true;
                }
            });
        });
        if let Some(job) = &r.check {
            ui.add_space(6.0);
            ui.add(egui::ProgressBar::new(job.progress().fraction()).text(job.progress().stage()));
        }
        if let Some(p) = r.prepared.clone() {
            ui.add_space(8.0);
            ui.label(caption("Texture sets"));
            for s in &p.sets {
                let text = match (s.old, s.uv_moved) {
                    (Some(_), Some(m)) if m > 0.02 => format!("{}  matched, UVs changed ({:.0}% moved)", s.name, m * 100.0),
                    (Some(_), _) => format!("{}  matched, UVs unchanged", s.name),
                    (None, _) => format!("{}  new material: new empty texture set", s.name),
                };
                ui.label(text);
            }
            for name in &p.unmatched_old {
                ui.label(RichText::new(format!("{name}  no matching material: kept, but not shown on the model")).color(theme::WARNING));
            }
            for w in &p.warnings {
                hint(ui, w);
            }
            ui.add_space(8.0);
            ui.label(caption("Painted layers"));
            ui.add_enabled_ui(!busy, |ui| {
                let mut mode = r.mode.unwrap_or(p.suggested);
                ui.radio_value(&mut mode, ReloadMode::KeepUvs, "Keep UVs: the UV layout did not change (layers stay exactly as they are)");
                ui.radio_value(&mut mode, ReloadMode::Reproject, "Reproject: the UVs changed (move paint through the surface onto the new layout)");
                r.mode = Some(mode);
            });
            ui.add_space(6.0);
            hint(ui, "Reloading cannot be undone: the undo history is cleared. Save first to keep this version. Baked mesh maps are kept but marked for rebaking.");
            ui.add_space(8.0);
            if let Some(job) = &r.job {
                ui.add(egui::ProgressBar::new(job.progress().fraction()).show_percentage().text(job.progress().stage()));
                if secondary_button(ui, "Cancel").clicked() {
                    cancel = true;
                }
            } else {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(ui, "Reload").clicked() {
                        start = Some((p.clone(), r.mode.unwrap_or(p.suggested)));
                    }
                });
            }
        }
        if let Some(e) = &r.error {
            ui.add_space(6.0);
            ui.label(RichText::new(e).color(theme::WARNING));
        }
    });
    if check {
        start_check(state);
    }
    if cancel && let Some(job) = state.mesh_reload.job.take() {
        job.cancel();
    }
    if let Some((p, mode)) = start {
        if state.session.is_stroking() {
            state.mesh_reload.error = Some("finish the stroke first".into());
        } else {
            state.mesh_reload.job = Some(state.session.start_mesh_reload(p, mode));
        }
    }
    if !open {
        // Closing cancels any running step (dropping a job cancels it).
        let r = &mut state.mesh_reload;
        r.open = false;
        r.check = None;
        r.job = None;
    }
}
