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
        Self { open: false, crashes }
    }
}

/// Record the GPU from the renderer for logs and reports.
pub fn record_gpu(render_state: Option<&egui_wgpu::RenderState>) {
    let Some(rs) = render_state else { return };
    let i = rs.adapter.get_info();
    // Browsers hide the adapter name.
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
        None if cfg!(target_arch = "wasm32") => "In the browser there is no log file: open the console (F12) and paste any red errors into the issue.".into(),
        None => "No log file is being written on this PC.".into(),
    }
}

/// Help → Report a bug… window.
pub fn report_window(state: &mut UiState, ctx: &egui::Context) {
    if !state.bug_report.open {
        return;
    }
    let info = SystemInfo::current();
    let mut open = true;
    let mut go = false;
    egui::Window::new(RichText::new("Report a bug").font(theme::font_strong(14.0))).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.set_width(420.0);
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
    egui::Window::new(RichText::new("SubCraft crashed last time").font(theme::font_strong(14.0)))
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
