//! The UI side of a lost graphics device (issue #119): stop drawing before egui's renderer can
//! panic on the dead device, save a recovery copy, tell the user with a native message box (egui
//! cannot draw any more) and restart in safe mode. The rules live in `subcraft_app::gpu_loss`.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use subcraft_app::gpu_loss::{self, Outcome, Shell};

use crate::state::UiState;

static CONTEXT: OnceLock<egui::Context> = OnceLock::new();
/// False for scripted runs (screenshots, renders): they end without a dialog or a restart.
static INTERACTIVE: AtomicBool = AtomicBool::new(true);

/// Connect the renderer's device-lost report and the panic hook to the handling here. Called
/// once when the window is created.
pub fn install(ctx: &egui::Context, interactive: bool) {
    let _ = CONTEXT.set(ctx.clone());
    INTERACTIVE.store(interactive, Ordering::Relaxed);
    subcraft_render::caps::set_device_lost_hook(|text| {
        // On a wgpu thread, possibly in the middle of a frame: only record it (the marker for
        // the next start is written here) and wake the UI, which handles it before it draws.
        gpu_loss::report(text);
        if let Some(ctx) = CONTEXT.get() { ctx.request_repaint(); }
    });
    // The renderer got there first (the device died between the frame's logic and its
    // painting): same ending, without the session (the automatic recovery copies remain).
    gpu_loss::set_panic_handler(|reason| end(&Native, reason, false, INTERACTIVE.load(Ordering::Relaxed)));
}

/// Whether a frame may run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate { Render, Stop }

/// Call first in every frame. `Stop` means the device is lost: the caller draws nothing. The
/// first `Stop` also runs the ending (`save` writes a recovery copy and says whether it did).
pub fn gate(shell: &dyn Shell, save: impl FnOnce() -> bool, interactive: bool, exit: impl FnOnce(Outcome)) -> Gate {
    if gpu_loss::may_render() { return Gate::Render; }
    if let Some(reason) = gpu_loss::begin_handling() {
        let saved = save();
        exit(gpu_loss::finish(shell, &reason, saved, interactive));
    }
    Gate::Stop
}

/// The gate of the real window: saves a recovery copy of the session, then ends the process.
pub fn frame_gate(state: &mut UiState) -> Gate {
    gate(&Native, || { let saved = save_recovery_copy(state); release_session(state); saved }, INTERACTIVE.load(Ordering::Relaxed), exit)
}

/// Write a recovery copy now and wait for it (the window is frozen anyway). True when written.
fn save_recovery_copy(state: &mut UiState) -> bool {
    use subcraft_app::recovery::AutosaveEvent;
    let saver = &mut state.recovery.autosaver;
    // Finishes a copy already being written, then writes one with the latest edits if needed.
    match saver.save_now(&state.session) {
        Some(AutosaveEvent::Saved(path)) => { log::info!("recovery copy written after the graphics device was lost: {}", path.display()); true }
        Some(AutosaveEvent::Failed(e)) => { log::error!("recovery copy after the graphics device was lost failed: {e}"); false }
        None => { log::info!("no unsaved work to copy after the graphics device was lost"); false }
    }
}

/// Let the restarted SubCraft see this session as one to restore.
fn release_session(state: &mut UiState) {
    state.recovery.autosaver.release_for_restart();
}

fn end(shell: &dyn Shell, reason: &str, saved: bool, interactive: bool) {
    exit(gpu_loss::finish(shell, reason, saved, interactive));
}

/// Leave without unwinding through the renderer: its buffers belong to a dead device, and
/// dropping the window would draw once more.
fn exit(outcome: Outcome) {
    log::info!("closing after the graphics device was lost: {outcome:?}");
    log::logger().flush();
    std::process::exit(if outcome == Outcome::Restarted { 0 } else { 3 });
}

/// Native message boxes and a new process.
struct Native;

impl Shell for Native {
    fn ask_restart(&self, text: &str) -> bool {
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("SubCraft: the graphics driver was reset")
            .set_description(text)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        matches!(answer, rfd::MessageDialogResult::Yes)
    }

    fn restart(&self) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        std::process::Command::new(exe).arg(gpu_loss::RESTART_ARG).spawn().map(|_| ()).map_err(|e| e.to_string())
    }

    fn tell(&self, text: &str) {
        rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("SubCraft").set_description(text).set_buttons(rfd::MessageButtons::Ok).show();
    }
}
