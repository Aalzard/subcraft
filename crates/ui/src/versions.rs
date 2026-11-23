//! File > Version history: the earlier saves of the open project ([`subcraft_app::versions`]).
//! Restoring one replaces the open project after the usual unsaved-changes question; the
//! project file itself only changes on the next Save.

use std::path::PathBuf;
use std::time::SystemTime;

use egui::RichText;
use subcraft_app::versions::{self, Version};

use crate::dialogs::Pending;
use crate::i18n::tr;
use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};

#[derive(Default)]
pub struct VersionsUi {
    pub open: bool,
    /// The project the list was read for, and its versions (newest first).
    listed: Option<(PathBuf, Vec<Version>)>,
    /// The version to restore once the unsaved-changes question is answered.
    pub pending: Option<PathBuf>,
}

impl VersionsUi {
    /// Read the list again next frame (after a save, or when the window opens).
    pub fn refresh(&mut self) {
        self.listed = None;
    }
}

/// Open the version picked in the window in place of the project (after any unsaved-changes prompt).
pub fn restore_pending(state: &mut UiState, now: f64) {
    let Some(version) = state.versions.pending.take() else { return };
    let Some(project) = state.session.project_path().map(PathBuf::from) else { return };
    match subcraft_app::Session::open_version(&project, &version) {
        Ok(s) => {
            state.set_session(s);
            state.set_status(format!("Restored the version from {}: save to keep it", versions::utc_date(version_time(&project, &version))), now);
        }
        Err(e) => state.set_status(format!("Could not open that version: {e}"), now),
    }
    state.versions.refresh();
}

/// "2.4 KB", "13.0 MB".
fn file_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn version_time(project: &std::path::Path, version: &std::path::Path) -> SystemTime {
    versions::list(project).into_iter().find(|v| v.path == version).map_or(SystemTime::now(), |v| v.saved)
}

/// The window; returns the action to request when a version is picked.
pub fn window(state: &mut UiState, ctx: &egui::Context) -> Option<Pending> {
    if !state.versions.open {
        return None;
    }
    let project = state.session.project_path().map(PathBuf::from);
    if state.versions.listed.as_ref().map(|l| &l.0) != project.as_ref() {
        state.versions.listed = project.as_ref().map(|p| (p.clone(), versions::list(p)));
    }
    let keep = state.recovery.prefs.keep_versions;
    let mut open = true;
    let mut picked = None;
    let now = SystemTime::now();
    crate::widgets::window(ctx, RichText::new(tr("Version history")).strong()).open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
        let Some((project, list)) = &state.versions.listed else {
            ui.label(RichText::new(tr("Save the project first: each later Save keeps the file it replaces here.")).color(TEXT_DIM()));
            return;
        };
        ui.label(RichText::new(format!("{} {}", tr("Earlier saves of"), project.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())).color(TEXT_DIM()));
        if keep == 0 {
            ui.label(RichText::new(tr("Version history is off (Settings > Autosave > Versions).")).color(theme::WARNING()));
        }
        ui.add_space(6.0);
        if list.is_empty() {
            ui.label(tr("No earlier versions yet."));
        }
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            egui::Grid::new("versions").num_columns(4).spacing([14.0, 6.0]).striped(true).show(ui, |ui| {
                for v in list {
                    ui.label(versions::describe_age(v.saved, now)).on_hover_text(versions::utc_date(v.saved));
                    ui.label(RichText::new(versions::utc_date(v.saved)).color(TEXT_DIM()).small());
                    ui.label(RichText::new(file_size(v.bytes)).color(TEXT_DIM()).small());
                    if ui.button(tr("Restore")).on_hover_text(tr("Open this version in place of the project; Save writes it back")).clicked() {
                        picked = Some(v.path.clone());
                    }
                    ui.end_row();
                }
            });
        });
        ui.add_space(6.0);
        ui.label(RichText::new(format!("{} {}", tr("Versions are kept in"), versions::folder(project).display())).color(TEXT_DIM()).small());
    });
    if !open {
        state.versions.open = false;
    }
    picked.map(|p| {
        state.versions.pending = Some(p);
        Pending::RestoreVersion
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::Session;

    #[test]
    fn restoring_a_version_opens_it_as_unsaved_changes_to_the_same_file() {
        let dir = std::env::temp_dir().join(format!("ui-versions-{}", subcraft_core::ObjectId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.subcraft");
        let mut session = Session::new(subcraft_mesh::primitives::cube(1.0), 128);
        session.set_keep_versions(5);
        session.save(Some(&path)).unwrap();
        let first_name = session.selected_texture_set().name.clone();
        let ts = session.selected_texture_set().id;
        session.execute(subcraft_app::Command::RenameTextureSet { texture_set: ts, name: "Second".into() }).unwrap();
        session.save(None).unwrap();
        let mut state = UiState::new(session, None);
        state.versions.open = true;
        let ctx = egui::Context::default();
        let mut picked = None;
        crate::theme::apply(&ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| picked = window(&mut state, ui.ctx()));
        out.textures_delta.clear();
        assert!(picked.is_none(), "nothing picked yet");
        let list = versions::list(&path);
        assert_eq!(list.len(), 1, "the first save had no file to replace; the second kept one");
        state.versions.pending = Some(list[0].path.clone());
        restore_pending(&mut state, 0.0);
        assert_eq!(state.session.selected_texture_set().name, first_name);
        assert_eq!(state.session.project_path(), Some(path.as_path()));
        assert!(state.session.is_modified(), "Save writes the restored version back");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sizes_read_plainly() {
        assert_eq!(file_size(512), "512 B");
        assert_eq!(file_size(2431), "2.4 KB");
        assert_eq!(file_size(13 * 1_048_576), "13.0 MB");
    }
}
