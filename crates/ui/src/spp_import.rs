//! Opening a Painter project in the background: the window stays responsive, the project appears
//! as soon as the model and the flattened textures are read, and the rebuilt layer stack is
//! installed when it is done (see `subcraft_app::SppImportJob`). Progress and Cancel are in the
//! status bar.

use std::path::{Path, PathBuf};

use subcraft_app::{SppImportJob, SppPoll};

use crate::state::UiState;

pub struct SppImportUi {
    job: SppImportJob,
    name: String,
}

impl SppImportUi {
    pub fn cancel(&self) {
        self.job.cancel();
    }

    /// `Opening Cube.spp: Rebuilding the Painter stack (120 of 374 layers)`, with the share done.
    pub fn progress(&self) -> (String, Option<f32>) {
        let control = self.job.control();
        let what = if self.job.is_rebuilding() { "Rebuilding layers of" } else { "Opening" };
        (format!("{what} {}: {}", self.name, control.stage()), control.fraction())
    }
}

pub fn is_spp(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("spp"))
}

/// Start importing `path`, replacing a running import.
pub fn start(state: &mut UiState, path: &Path) {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    state.spp_import = Some(SppImportUi { job: SppImportJob::start(path), name });
}

/// Hand finished results to the session; call once per frame.
pub fn poll(state: &mut UiState, ctx: &egui::Context, now: f64) {
    loop {
        let Some(import) = state.spp_import.as_mut() else { return };
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let path: PathBuf = import.job.path.clone();
        let name = import.name.clone();
        match import.job.poll(&mut state.session) {
            SppPoll::Idle => return,
            SppPoll::Opened(session, notes) => {
                // `set_session` cancels any running import; this one continues with the stack rebuild.
                let running = state.spp_import.take();
                state.set_session(*session);
                state.spp_import = running;
                state.welcome.remember(&path);
                let status = match notes.first() {
                    Some(first) if notes.len() > 1 => format!("Opened {name}; rebuilding its layers in the background. {first}"),
                    _ => format!("Opened {name}; rebuilding its layers in the background"),
                };
                state.set_status(status, now);
            }
            SppPoll::StackInstalled(notes) => {
                let summary = notes.iter().find(|n| n.contains("Rebuilt stack")).map_or_else(|| "The Painter layer stack is ready".to_string(), |n| n.trim().to_string());
                log::info!("Painter import: {summary}");
                state.set_status(format!("{name}: {summary}"), now);
            }
            SppPoll::Discarded => {
                state.spp_import = None;
                return;
            }
            SppPoll::Finished(result) => {
                let opened = import.job.is_rebuilding();
                state.spp_import = None;
                match result {
                    Ok(()) => {}
                    Err(e) if e == "cancelled" && opened => state.set_status(format!("Rebuilding the layers of {name} was cancelled; the flattened Painter result stays."), now),
                    Err(e) if e == "cancelled" => state.set_status(format!("Opening {name} was cancelled"), now),
                    Err(e) => state.set_status(format!("Could not open {name}: {e}"), now),
                }
                return;
            }
        }
    }
}
