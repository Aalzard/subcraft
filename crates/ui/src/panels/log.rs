//! Painter's Log window (Window > Views > Log): the application's messages in a General and an
//! Assets tab, each with its warning / error count, a severity filter, Clear, Export and the log
//! file. The messages come from [`subcraft_app::diagnostics::log_entries`]; what the window shows
//! (tab, filter, clear point) lives in egui memory, so nothing here touches the project.

use std::sync::Arc;

use egui::{Color32, RichText, Ui};
use subcraft_app::log_buffer::{self, LogEntry, LogLevel, LogTab};

use crate::i18n::{tr, trf};
use crate::state::UiState;
use crate::theme::{self, TEXT_DIM, TEXT_FAINT};
use crate::widgets::striped_list_row;
use crate::icons;

/// Error messages' colour.
const ERROR: Color32 = Color32::from_rgb(235, 110, 100);

/// What the window shows, kept between frames.
#[derive(Clone)]
struct View {
    tab: LogTab,
    min: LogLevel,
    /// Messages up to this sequence number are hidden (Clear).
    cleared: u64,
    /// The messages as last fetched, and the newest sequence number then.
    entries: Arc<Vec<LogEntry>>,
    seq: Option<u64>,
}

impl Default for View {
    fn default() -> Self {
        Self { tab: LogTab::General, min: LogLevel::Info, cleared: 0, entries: Arc::new(Vec::new()), seq: None }
    }
}

fn tab_label(tab: LogTab) -> &'static str {
    match tab {
        LogTab::General => tr("General"),
        LogTab::Assets => tr("Assets"),
    }
}

fn filter_label(min: LogLevel) -> &'static str {
    match min {
        LogLevel::Info => tr("All messages"),
        LogLevel::Warning => tr("Warnings and errors"),
        LogLevel::Error => tr("Errors only"),
    }
}

fn level_color(level: LogLevel) -> Color32 {
    match level {
        LogLevel::Info => TEXT_DIM,
        LogLevel::Warning => theme::WARNING,
        LogLevel::Error => ERROR,
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let id = egui::Id::new("log_window_view");
    let mut view: View = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    // Refetch only when a message arrived (the logger is never waited on).
    let latest = subcraft_app::diagnostics::log_last_seq();
    if latest.is_some() && latest != view.seq
        && let Some((entries, seq)) = subcraft_app::diagnostics::log_entries()
    {
        view.entries = Arc::new(entries);
        view.seq = Some(seq);
    }
    let now = ui.input(|i| i.time);
    ui.horizontal(|ui| {
        for tab in LogTab::ALL {
            let (warnings, errors) = log_buffer::problems(&view.entries, tab, view.cleared);
            let mut text = tab_label(tab).to_string();
            if warnings + errors > 0 {
                text.push_str(&format!("  {} {}", icons::WARNING, warnings + errors));
            }
            let color = if errors > 0 { ERROR } else if warnings > 0 { theme::WARNING } else { theme::TEXT };
            if ui.selectable_label(view.tab == tab, RichText::new(text).color(color))
                .on_hover_text(trf("{} warning(s), {} error(s) since the last Clear", &[&warnings, &errors]))
                .clicked()
            {
                view.tab = tab;
            }
        }
        ui.separator();
        egui::ComboBox::from_id_salt("log_filter").selected_text(filter_label(view.min)).width(150.0).show_ui(ui, |ui| {
            for min in [LogLevel::Info, LogLevel::Warning, LogLevel::Error] {
                ui.selectable_value(&mut view.min, min, filter_label(min));
            }
        });
        if ui.button(tr("Clear")).on_hover_text(tr("Hide the messages so far (the log file keeps them)")).clicked() {
            view.cleared = view.seq.unwrap_or(0);
        }
        if ui.button(tr("Export…")).on_hover_text(tr("Save a copy of this session's log file")).clicked() {
            export_log(state, now);
        }
        if let Some(file) = subcraft_app::diagnostics::log_file()
            && ui.button(icons::FOLDER_OPEN).on_hover_text(tr("Show the log file")).clicked()
        {
            crate::panels::shelf::show_in_folder(&file);
        }
    });
    ui.separator();
    let shown: Vec<&LogEntry> = log_buffer::visible(&view.entries, view.tab, view.min, view.cleared).collect();
    if shown.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new(tr("No messages")).color(TEXT_FAINT));
    }
    egui::ScrollArea::vertical().id_salt("log_rows").auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, 18.0, shown.len(), |ui, rows| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in rows {
            let e = shown[i];
            striped_list_row(ui, i, false, egui::Sense::hover(), |ui| {
                ui.label(RichText::new(&e.time).font(theme::font(10.5)).color(TEXT_FAINT));
                let icon = match e.level {
                    LogLevel::Info => icons::INFO,
                    LogLevel::Warning | LogLevel::Error => icons::WARNING,
                };
                ui.label(RichText::new(icon).color(level_color(e.level))).on_hover_text(e.level.label());
                ui.add(egui::Label::new(RichText::new(&e.text).color(level_color(e.level))).truncate()).on_hover_text(&e.text);
            });
        }
    });
    ui.data_mut(|d| d.insert_temp(id, view));
}

/// Help > Export log and the Log window's Export button: ask where, then save a copy.
pub fn export_log(state: &mut UiState, now: f64) {
    let Some(dest) = rfd::FileDialog::new().add_filter("Log file", &["log", "txt"]).set_file_name("subcraft.log").save_file() else {
        return;
    };
    match subcraft_app::diagnostics::export_log(&dest) {
        Ok(()) => state.set_status(format!("Log saved to {}", dest.display()), now),
        Err(e) => state.set_status(format!("Could not save the log: {e}"), now),
    }
}
