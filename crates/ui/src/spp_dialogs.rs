//! Background open of Substance Painter `.spp` projects.

use crate::state::UiState;

/// Start opening a Painter `.spp` on a background thread (see `poll_spp_open`).
pub fn start_spp_open(path: std::path::PathBuf) -> subcraft_app::SppOpenJob {
    subcraft_app::SppOpenJob::start(path)
}

pub fn finish_spp_open(state: &mut UiState, result: subcraft_app::SppOpenResult, now: f64) {
    subcraft_app::spp_log_step("UI: SPP open finished, applying session");
    if let Some(session) = result.session {
        state.set_session(session);
    }
    if !result.summary.sbsar_paths.is_empty() {
        state.remember_sbsar(&result.summary.sbsar_paths);
    }
    state.set_status(result.summary.status_message(), now);
}

/// Poll a background `.spp` open started with [`start_spp_open`].
pub fn poll_spp_open(
    state: &mut UiState,
    job: &mut Option<subcraft_app::SppOpenJob>,
    now: f64,
) {
    let Some(j) = job else { return };
    if j.elapsed().as_secs() > 2 {
        let log = subcraft_app::spp_open_log_path();
        state.set_status(
            format!(
                "Opening {}… ({:.0}s) — details: {}",
                j.path().file_name().unwrap_or_default().to_string_lossy(),
                j.elapsed().as_secs_f64(),
                log.display()
            ),
            now,
        );
    }
    if let Some(result) = j.poll() {
        match result {
            Ok(open) => finish_spp_open(state, open, now),
            Err(e) => state.set_status(format!("Could not open SPP: {e}"), now),
        }
        *job = None;
    }
}
