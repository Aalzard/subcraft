//! Help → Report a bug, and the "SubCraft crashed last time" window. Nothing is sent
//! automatically: the user opens a prefilled GitHub issue and attaches the files themself.

use std::path::PathBuf;

use egui::RichText;
use subcraft_app::diagnostics::{self, SystemInfo};
use subcraft_render::egui_wgpu;

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{primary_button, secondary_button};

#[derive(Default)]
pub struct BugReportUi {
    pub open: bool,
    /// Crash reports from earlier runs not shown yet: (file, text).
    pub crashes: Vec<(PathBuf, String)>,
    pub safe_notice_dismissed: bool,
    /// The device-lost recovery copy was attempted (once per run).
    pub lost_handled: bool,
}

impl BugReportUi {
    /// Look for crash reports written by earlier runs.
    pub fn load() -> Self {
        let dir = subcraft_app::recovery::data_dir().map(|d| d.join("crashes"));
        let crashes = dir
            .map(|d| diagnostics::unseen_crash_reports(&d))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| std::fs::read_to_string(&p).ok().map(|t| (p, t)))
            .collect();
        Self { open: false, crashes, safe_notice_dismissed: false, lost_handled: false }
    }
}

/// Set once the work was copied to the recovery folder after a device loss.
static LOST_SAVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The GPU device is gone (issue #119): copy unsaved work to the recovery folder once, and tell the
/// caller to draw nothing. egui-wgpu panics ("Failed to create staging buffer") as soon as it is
/// given anything to paint on a lost device, which used to end the session with a crash report and
/// without a recovery copy of the latest edits. Returns whether the frame must stay empty.
pub fn stop_after_device_loss(state: &mut UiState, lost: Option<String>) -> bool {
    if lost.is_none() {
        return false;
    }
    if !state.bug_report.lost_handled {
        state.bug_report.lost_handled = true;
        match state.recovery.autosaver.save_now(&state.session) {
            Some(subcraft_app::AutosaveEvent::Saved(p)) => log::info!("GPU device lost: work copied to {}", p.display()),
            Some(subcraft_app::AutosaveEvent::Failed(e)) => log::error!("GPU device lost: recovery copy failed: {e}"),
            None => log::info!("GPU device lost: no unsaved work to copy"),
        }
        LOST_SAVED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    true
}

/// Record the GPU from the renderer for logs and reports.
pub fn record_gpu(render_state: Option<&egui_wgpu::RenderState>) {
    subcraft_render::caps::set_device_lost_hook(|text| {
        subcraft_app::startup_probe::step(&format!("GPU device lost: {text}"));
        let text = format!("The graphics device stopped responding ({text}).

This is usually a driver reset or a GPU that was removed or ran out of memory. SubCraft cannot continue drawing. Your work is kept in the automatic recovery copy: start SubCraft again and choose Restore. The next start turns GPU compositing and GPU baking off (you can turn them back on in Preferences). If it keeps happening, start with the --safe-mode option and see the log in the SubCraft data folder.");
        std::thread::spawn(move || {
            rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("SubCraft: graphics device lost").set_description(text).set_buttons(rfd::MessageButtons::Ok).show();
            // Nothing can be drawn any more: end the process once the recovery copy is written (the
            // UI thread does that on its next frame; don't wait for ever if the driver hung it).
            // Not a normal close, which would delete the recovery folder.
            let start = std::time::Instant::now();
            while !LOST_SAVED.load(std::sync::atomic::Ordering::SeqCst) && start.elapsed() < std::time::Duration::from_secs(20) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            std::process::exit(1);
        });
    });
    let Some(rs) = render_state else { return };
    let i = rs.adapter.get_info();
    let name = if i.name.trim().is_empty() { "unknown GPU" } else { i.name.as_str() };
    let mut gpu = format!("{name} ({:?}", i.backend);
    if !i.driver.is_empty() || !i.driver_info.is_empty() {
        gpu.push_str(&format!(", driver {} {}", i.driver, i.driver_info).trim_end().to_string());
    }
    gpu.push(')');
    diagnostics::set_gpu(gpu);
}

fn log_hint() -> String {
    match diagnostics::log_file() {
        Some(p) => format!("Please attach the log file to the issue (drag it into the text box):\n{}", p.display()),
        None => "No log file is being written on this PC.".into(),
    }
}

/// Plain-text diagnostic report: app-level facts plus adapter limits, display scale and window.
pub fn collect_diagnostics(ctx: &egui::Context) -> String {
    let screen = ctx.content_rect();
    let display = format!("pixels_per_point: {:.2} (native {:?})
screen (points): {:.0} x {:.0}
zoom factor: {:.2}", ctx.pixels_per_point(), ctx.native_pixels_per_point(), screen.width(), screen.height(), ctx.zoom_factor());
    let mut gpu = subcraft_render::caps::get().report();
    if subcraft_render::caps::is_probed() { } else { gpu = "GPU capabilities were not probed.".into(); }
    if let Some(l) = subcraft_render::caps::device_lost() { gpu.push_str(&format!("
DEVICE LOST: {l}")); }
    let errors = subcraft_render::caps::recent_errors();
    if !errors.is_empty() { gpu.push_str(&format!("
GPU errors:
{}", errors.join("
"))); }
    diagnostics::diagnostic_report(&[("GPU", gpu), ("Display", display)])
}

/// Help → Report a bug… window.
pub fn report_window(state: &mut UiState, ctx: &egui::Context) {
    if !state.bug_report.open {
        return;
    }
    let info = SystemInfo::current();
    let mut open = true;
    let mut go = false;
    crate::widgets::window(ctx, RichText::new("Report a bug").font(theme::font_strong(14.0))).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new("This opens a new issue on GitHub in your browser with the details below already filled in. Nothing is sent until you submit it there.").color(TEXT_DIM));
        ui.add_space(10.0);
        egui::Grid::new("bug_info").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            for (k, v) in [("Version", &info.version), ("OS", &info.os), ("GPU", &info.gpu)] {
                ui.label(RichText::new(k).color(TEXT_FAINT));
                ui.label(v.as_str());
                ui.end_row();
            }
        });
        ui.add_space(10.0);
        ui.label(RichText::new(log_hint()).color(TEXT_DIM));
        ui.add_space(12.0);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            go = primary_button(ui, "Open GitHub issue").clicked();
            if secondary_button(ui, "Copy diagnostic info").on_hover_text("Copies version, OS, GPU, limits, display scale and the log tail to the clipboard").clicked() {
                let text = collect_diagnostics(ctx);
                ctx.copy_text(text);
            }
            if let Some(p) = diagnostics::log_file()
                && secondary_button(ui, "Show log file").clicked()
            {
                crate::panels::shelf::show_in_folder(&p);
            }
        });
    });
    if go {
        let url = diagnostics::bug_report_url(&info, None, diagnostics::log_file().as_deref());
        log::info!("opening bug report: {url}");
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    state.bug_report.open = open && !go;
}

/// Notice shown while safe mode is on, with the way back to normal mode.
pub fn safe_mode_window(state: &mut UiState, ctx: &egui::Context) {
    let Some(reason) = subcraft_app::safe_mode::active() else { return };
    if state.bug_report.safe_notice_dismissed || !state.bug_report.crashes.is_empty() { return; }
    let (mut dismiss, mut retry, mut copy) = (false, false, false);
    crate::widgets::window(ctx, RichText::new("Safe mode").font(theme::font_strong(14.0))).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(reason.text());
        ui.add_space(6.0);
        ui.label(RichText::new("Off for this run: GPU compositing, GPU baking, anti-aliasing, path tracing and the remembered window position. Painting, layers and export still work, only slower.").color(TEXT_DIM));
        ui.add_space(6.0);
        ui.label(RichText::new("The next start is normal again automatically once this one runs fine. If it keeps failing, copy the diagnostic info into a bug report.").color(TEXT_DIM));
        ui.add_space(12.0);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            retry = primary_button(ui, "Restart normally").clicked();
            copy = secondary_button(ui, "Copy diagnostic info").clicked();
            dismiss = secondary_button(ui, "Continue in safe mode").clicked();
        });
    });
    if copy { ctx.copy_text(collect_diagnostics(ctx)); }
    if retry {
        match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg("--normal-mode").spawn()) {
            Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => { state.set_status(format!("Could not restart: {e}"), 0.0); }
        }
    }
    if dismiss { state.bug_report.safe_notice_dismissed = true; }
}

/// "SubCraft crashed last time" window for crash reports of earlier runs.
pub fn crash_window(state: &mut UiState, ctx: &egui::Context) {
    // The recovery prompt comes first.
    if state.bug_report.crashes.is_empty() || !state.recovery.crashed.is_empty() {
        return;
    }
    let (path, text) = state.bug_report.crashes.last().cloned().expect("not empty");
    let message = diagnostics::crash_message(&text).to_string();
    let count = state.bug_report.crashes.len();
    let mut close = false;
    let mut report = false;
    crate::widgets::window(ctx, RichText::new("SubCraft crashed last time").font(theme::font_strong(14.0)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Sorry about that. A crash report was saved. Reporting it helps get it fixed; nothing is sent unless you submit the issue.").color(TEXT_DIM));
            ui.add_space(10.0);
            egui::Frame::new().fill(theme::BG_0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&message).monospace().color(theme::TEXT));
            });
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Report: {}", path.display())).color(TEXT_FAINT).small());
            if count > 1 {
                ui.label(RichText::new(format!("{} crash reports since the last start.", count)).color(TEXT_FAINT).small());
            }
            ui.add_space(12.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                report = primary_button(ui, "Report a bug…").clicked();
                if secondary_button(ui, "Show report").clicked() {
                    crate::panels::shelf::show_in_folder(&path);
                }
                close = secondary_button(ui, "Close").clicked();
            });
        });
    if report {
        let url = diagnostics::bug_report_url(&SystemInfo::current(), Some(&message), Some(&path));
        log::info!("opening crash report issue: {url}");
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
    if report || close {
        for (p, _) in state.bug_report.crashes.drain(..) {
            if let Err(e) = diagnostics::mark_seen(&p) {
                log::warn!("crash report {} not marked as seen: {e}", p.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::recovery::{Autosaver, RecoveryStore, SessionInfo, list_autosaves};
    use subcraft_app::{Command, NewLayer, Session};

    /// Issue #119: after the device is lost the frame stays empty (egui-wgpu panics when it has
    /// anything to paint) and the unsaved work is copied to the recovery folder, once.
    #[test]
    fn a_lost_device_saves_the_work_once_and_keeps_the_frame_empty() {
        let root = std::env::temp_dir().join(format!("subcraft-lost-device-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut state = UiState::new(Session::sample(), None);
        let live = RecoveryStore::new(&root).begin_session(&SessionInfo::for_session(&state.session), 3).unwrap();
        let dir = live.dir().to_path_buf();
        state.recovery.autosaver = Autosaver::new(Some(live), std::time::Duration::from_secs(3600));
        state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();

        assert!(!stop_after_device_loss(&mut state, None), "a healthy device draws as usual");
        assert!(list_autosaves(&dir).is_empty());

        assert!(stop_after_device_loss(&mut state, Some("Unknown: Device is lost".into())));
        assert_eq!(list_autosaves(&dir).len(), 1, "the work is in the recovery folder");
        state.session.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
        assert!(stop_after_device_loss(&mut state, Some("Unknown: Device is lost".into())));
        assert_eq!(list_autosaves(&dir).len(), 1, "later frames only stay empty");
        drop(state);
        let _ = std::fs::remove_dir_all(&root);
    }
}
