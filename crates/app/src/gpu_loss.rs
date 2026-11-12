//! What SubCraft does when the graphics device is lost while it runs (issue #119): a driver
//! reset (Windows TDR), a removed GPU, a driver crash. A lost device cannot draw again in this
//! process, and egui's renderer panics on its next frame, so from the first report on:
//!
//! 1. the loss is written to the startup log at once ([`startup_probe::GPU_LOST_MARKER`]), which
//!    makes the next start a safe-mode start whatever happens to this process afterwards;
//! 2. nothing more is drawn or submitted ([`may_render`] is false);
//! 3. the UI saves a recovery copy, tells the user, and restarts in safe mode ([`finish`]).
//!
//! No wgpu here: the render crate reports the loss, the UI supplies the [`Shell`].
use crate::startup_probe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// The steps of handling a loss. Pure, so it can be tested without a device.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LossState {
    /// The device works.
    #[default]
    Running,
    /// Lost, not handled yet: nothing may be drawn; someone must call [`LossState::begin_handling`].
    Lost { reason: String },
    /// The dialog / restart is under way (or done). Still nothing may be drawn.
    Handling { reason: String },
}

impl LossState {
    /// Record a loss. True for the first report only; later ones keep the first reason.
    pub fn report(&mut self, reason: &str) -> bool {
        if *self != Self::Running { return false; }
        *self = Self::Lost { reason: reason.to_string() };
        true
    }
    pub fn is_lost(&self) -> bool { *self != Self::Running }
    /// Whether a frame may be drawn and GPU work submitted.
    pub fn may_render(&self) -> bool { *self == Self::Running }
    pub fn reason(&self) -> Option<&str> {
        match self { Self::Running => None, Self::Lost { reason } | Self::Handling { reason } => Some(reason) }
    }
    /// Claim the handling of the loss. `Some(reason)` exactly once, for whoever gets there first
    /// (the UI frame, or the panic hook when the renderer panicked first).
    pub fn begin_handling(&mut self) -> Option<String> {
        let Self::Lost { reason } = self else { return None };
        let reason = std::mem::take(reason);
        *self = Self::Handling { reason: reason.clone() };
        Some(reason)
    }
}

static STATE: Mutex<LossState> = Mutex::new(LossState::Running);
static LOST: AtomicBool = AtomicBool::new(false);

fn state() -> std::sync::MutexGuard<'static, LossState> { STATE.lock().unwrap_or_else(|p| p.into_inner()) }

/// Record that the device is lost. The first report writes the safe-mode marker to the startup
/// log right away (flushed and synced), so the next start is safe even if this process dies now.
/// Safe to call from any thread, any number of times.
pub fn report(reason: &str) -> bool {
    let first = state().report(reason);
    if first {
        LOST.store(true, Ordering::SeqCst);
        startup_probe::step(&format!("{}: {reason}", startup_probe::GPU_LOST_MARKER));
        log::error!("the graphics device is lost ({reason}); drawing stops and SubCraft restarts in safe mode");
    }
    first
}
pub fn is_lost() -> bool { LOST.load(Ordering::SeqCst) }
/// False from the first [`report`] on: draw nothing, submit nothing.
pub fn may_render() -> bool { !is_lost() }
pub fn reason() -> Option<String> { state().reason().map(str::to_string) }
/// See [`LossState::begin_handling`].
pub fn begin_handling() -> Option<String> { state().begin_handling() }

/// Whether a panic is the renderer failing on a lost device rather than a bug of ours: it comes
/// from egui-wgpu / wgpu, and the loss is already on record, or the message says so, or it is
/// the "staging buffer" failure, which only happens when the device refuses every call (the
/// panic in issue #119).
pub fn panic_is_loss(location: &str, message: &str) -> bool {
    panic_is_loss_given(is_lost(), location, message)
}

fn panic_is_loss_given(lost: bool, location: &str, message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    let says_lost = ["device is lost", "device lost", "devicelost", "device removed", "0x887a0005", "0x887a0006", "0x887a0007"].iter().any(|n| m.contains(n));
    location.contains("wgpu") && (lost || says_lost || m.contains("staging buffer"))
}

/// What the user chose (or what happened) at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome { Restarted, Closed, RestartFailed }

/// The parts of the ending that touch the outside world; the UI implements it with a native
/// message box and a new process, tests with a recorder.
pub trait Shell {
    /// Show `text` and ask whether to restart in safe mode. Not called when not interactive.
    fn ask_restart(&self, text: &str) -> bool;
    /// Start SubCraft again with [`RESTART_ARG`].
    fn restart(&self) -> Result<(), String>;
    /// Tell the user something went wrong on the way out.
    fn tell(&self, text: &str);
}

/// Command-line flag of the restarted process.
pub const RESTART_ARG: &str = "--safe-mode";

/// What the user is told. `recovery_saved`: a recovery copy was written just now.
pub fn dialog_text(reason: &str, recovery_saved: bool) -> String {
    let work = if recovery_saved {
        "Your work was saved to a recovery copy: after the restart, choose Restore."
    } else {
        "The last automatic recovery copy is kept: after the restart, choose Restore if it is offered."
    };
    format!("The graphics driver stopped responding and was reset ({reason}). SubCraft cannot draw any more in this session.\n\n{work}\n\nRestart SubCraft in safe mode now? Safe mode keeps GPU compositing, GPU baking and anti-aliasing off, so this does not happen again. Choose No to close SubCraft.\n\nIf it keeps happening, updating the graphics driver usually helps.")
}

/// The ending: ask (when interactive), then restart in safe mode or close. The caller exits the
/// process afterwards in every case; nothing is drawn again.
pub fn finish(shell: &dyn Shell, reason: &str, recovery_saved: bool, interactive: bool) -> Outcome {
    if !interactive {
        // Scripted runs (screenshots, renders) must end, not wait for a click or loop.
        startup_probe::step("graphics device lost: scripted run ends");
        return Outcome::Closed;
    }
    if !shell.ask_restart(&dialog_text(reason, recovery_saved)) {
        startup_probe::step("graphics device lost: closed at the user's request");
        return Outcome::Closed;
    }
    match shell.restart() {
        Ok(()) => { startup_probe::step("graphics device lost: restarting in safe mode"); Outcome::Restarted }
        Err(e) => {
            startup_probe::step(&format!("graphics device lost: restart failed: {e}"));
            shell.tell(&format!("SubCraft could not restart by itself ({e}). Please start it again: it will open in safe mode."));
            Outcome::RestartFailed
        }
    }
}

static PANIC_HANDLER: OnceLock<fn(&str)> = OnceLock::new();
/// What the panic hook runs instead of writing a crash report when [`panic_is_loss`]: the UI's
/// ending (dialog, restart, exit). It must not return to drawing; it normally exits the process.
pub fn set_panic_handler(handler: fn(&str)) { let _ = PANIC_HANDLER.set(handler); }

/// Called by the panic hook. True when the panic was a lost device and has been dealt with (no
/// crash report wanted).
pub fn handle_panic(location: &str, message: &str) -> bool {
    if !panic_is_loss(location, message) { return false; }
    let Some(handler) = PANIC_HANDLER.get() else { return false };
    report(message);
    // Someone else (the UI frame) may already be showing the dialog; then this thread just ends.
    if let Some(reason) = begin_handling() { handler(&reason); }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn state_machine_reports_once_stops_rendering_and_is_handled_once() {
        let mut s = LossState::default();
        assert!(s.may_render() && !s.is_lost() && s.reason().is_none());
        assert_eq!(s.begin_handling(), None, "nothing to handle while running");
        assert!(s.report("Unknown: Device is lost"));
        assert!(!s.report("a later, different message"), "only the first report counts");
        assert!(s.is_lost() && !s.may_render());
        assert_eq!(s.reason(), Some("Unknown: Device is lost"));
        assert_eq!(s.begin_handling().as_deref(), Some("Unknown: Device is lost"));
        assert_eq!(s.begin_handling(), None, "handled exactly once");
        assert!(!s.may_render(), "never draws again, also while the dialog is up");
        assert!(!s.report("again"));
        assert_eq!(s.reason(), Some("Unknown: Device is lost"));
    }

    #[test]
    fn the_renderer_panic_of_issue_119_is_recognised_and_our_own_bugs_are_not() {
        let egui_wgpu = r"C:\Users\x\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\egui-wgpu-0.36.2\src\renderer.rs:984";
        let staging = "Failed to create staging buffer for index data. Index count: 24534. Required index buffer size: 98136. Actual size 160368 and capacity: 160368 (bytes)";
        assert!(panic_is_loss_given(false, egui_wgpu, staging));
        assert!(panic_is_loss_given(false, "wgpu-30.0.1/src/backend/wgpu_core.rs:1", "Error in Queue::submit: Parent device is lost"));
        assert!(!panic_is_loss_given(false, r"crates\ui\src\app.rs:10", "index out of bounds"));
        assert!(!panic_is_loss_given(true, r"crates\render\src\ray_bake.rs:250", "map accumulators: BufferAsyncError"), "a panic in our own code stays a crash report, lost device or not");
        assert!(!panic_is_loss_given(false, egui_wgpu, "attempt to subtract with overflow"));
        assert!(panic_is_loss_given(true, egui_wgpu, "anything the renderer says once the device is lost"));
    }

    struct Fake { answer: bool, restart: Result<(), String>, calls: RefCell<Vec<String>> }
    impl Fake {
        fn new(answer: bool, restart: Result<(), String>) -> Self { Self { answer, restart, calls: RefCell::default() } }
    }
    impl Shell for Fake {
        fn ask_restart(&self, text: &str) -> bool { self.calls.borrow_mut().push(format!("ask:{text}")); self.answer }
        fn restart(&self) -> Result<(), String> { self.calls.borrow_mut().push("restart".into()); self.restart.clone() }
        fn tell(&self, text: &str) { self.calls.borrow_mut().push(format!("tell:{text}")); }
    }

    #[test]
    fn the_ending_asks_then_restarts_closes_or_explains() {
        let yes = Fake::new(true, Ok(()));
        assert_eq!(finish(&yes, "Unknown: Device is lost", true, true), Outcome::Restarted);
        {
            let calls = yes.calls.borrow();
            assert_eq!(calls.len(), 2);
            assert!(calls[0].contains("Unknown: Device is lost") && calls[0].contains("safe mode") && calls[0].contains("saved to a recovery copy"), "{}", calls[0]);
            assert_eq!(calls[1], "restart");
        }

        let no = Fake::new(false, Ok(()));
        assert_eq!(finish(&no, "x", false, true), Outcome::Closed);
        assert_eq!(no.calls.borrow().len(), 1, "no restart after No");
        assert!(no.calls.borrow()[0].contains("last automatic recovery copy"));

        let broken = Fake::new(true, Err("access denied".into()));
        assert_eq!(finish(&broken, "x", true, true), Outcome::RestartFailed);
        assert!(broken.calls.borrow()[2].starts_with("tell:") && broken.calls.borrow()[2].contains("access denied"));

        let scripted = Fake::new(true, Ok(()));
        assert_eq!(finish(&scripted, "x", true, false), Outcome::Closed);
        assert!(scripted.calls.borrow().is_empty(), "a scripted run never waits for a click and never restarts");
    }
}
