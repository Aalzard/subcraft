//! Autosave in the UI: the "didn't close properly" prompt, the Preferences window and, in the
//! browser (no files to autosave to), a reminder to download a backup.

use std::time::Duration;

use egui::RichText;
use serde::{Deserialize, Serialize};
use subcraft_app::recovery::{self, Autosaver, RecoveryStore, SessionInfo};
use subcraft_app::{AutosaveEvent, CrashedSession, Session};

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};
use crate::widgets::{primary_button, secondary_button};

/// eframe storage key of [`Preferences`].
pub const PREFS_KEY: &str = "subcraft_prefs_v1";
pub const DEFAULT_UI_SIZE: u32 = 95;
pub const UI_SIZE_RANGE: std::ops::RangeInclusive<u32> = 75..=150;

/// User settings kept between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Scales all text, icons and widgets together, independently of native display DPI.
    pub ui_size_percent: u32,
    /// Minutes between autosaves; 0 turns the timer off.
    pub autosave_minutes: u32,
    /// Recovery copies kept.
    pub autosave_keep: usize,
    pub show_welcome: bool,
    /// Desktop: look for a newer release in the background at startup and point at the update
    /// button when there is one.
    pub check_updates_on_start: bool,
    /// 3D view toolbar settings.
    pub viewport: crate::viewport_bar::ViewportPrefs,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            ui_size_percent: DEFAULT_UI_SIZE,
            autosave_minutes: (recovery::DEFAULT_INTERVAL.as_secs() / 60) as u32,
            autosave_keep: recovery::DEFAULT_KEEP,
            show_welcome: true,
            check_updates_on_start: true,
            viewport: Default::default(),
        }
    }
}

impl Preferences {
    pub fn apply_ui_size(&mut self, ctx: &egui::Context) {
        self.ui_size_percent = self.ui_size_percent.clamp(*UI_SIZE_RANGE.start(), *UI_SIZE_RANGE.end());
        let zoom = self.ui_size_percent as f32 / 100.0;
        if (ctx.zoom_factor() - zoom).abs() > 0.0001 {
            ctx.set_zoom_factor(zoom);
        }
    }
    pub fn interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.autosave_minutes) * 60)
    }
}

/// Autosave + recovery state of the window.
pub struct RecoveryUi {
    pub autosaver: Autosaver,
    /// Crashed sessions still to ask about (newest first).
    pub crashed: Vec<CrashedSession>,
    pub store: Option<RecoveryStore>,
    pub prefs: Preferences,
    pub show_prefs: bool,
    /// Time of the last finished autosave (for the status bar).
    pub last_saved: Option<std::time::SystemTime>,
    /// Web: when the backup reminder was last shown or dismissed (app time, seconds).
    pub reminded_at: f64,
    pub show_reminder: bool,
    /// Web: last value written to `window.subcraftUnsaved`.
    pub page_unsaved: Option<bool>,
}

impl RecoveryUi {
    /// Look for crashed sessions, then claim a folder for this one. `interval` overrides the
    /// preference (command line).
    pub fn start(session: &Session, prefs: Preferences, interval: Option<Duration>) -> Self {
        let store = RecoveryStore::default_store();
        let crashed = store.as_ref().map(RecoveryStore::crashed).unwrap_or_default();
        let live = store.as_ref().and_then(|s| match s.begin_session(&SessionInfo::for_session(session), prefs.autosave_keep) {
            Ok(l) => Some(l),
            Err(e) => {
                log::warn!("autosave disabled: could not create the recovery folder in {}: {e}", s.root().display());
                None
            }
        });
        if let Some(l) = &live {
            log::info!("recovery copies go to {}", l.dir().display());
        }
        let autosaver = Autosaver::new(live, interval.unwrap_or(prefs.interval()));
        Self { autosaver, crashed, store, prefs, show_prefs: false, last_saved: None, reminded_at: 0.0, show_reminder: false, page_unsaved: None }
    }

    /// No autosave, no prompt (tests, scripted runs without a data folder).
    pub fn disabled() -> Self {
        Self {
            autosaver: Autosaver::disabled(),
            crashed: Vec::new(),
            store: None,
            prefs: Preferences::default(),
            show_prefs: false,
            last_saved: None,
            reminded_at: 0.0,
            show_reminder: false,
            page_unsaved: None,
        }
    }
}

/// Recover the newest crashed session into `state` (prompt button or `--recover`).
pub fn recover(state: &mut UiState, crashed: CrashedSession, now: f64) {
    match Session::recover(&crashed) {
        Ok(session) => {
            let name = crashed.info.name.clone();
            state.set_session(session);
            if let Some(live) = state.recovery.autosaver.live()
                && let Err(e) = live.adopt(crashed)
            {
                log::warn!("recovery copies not moved: {e}");
            }
            state.set_status(format!("Recovered \"{name}\". Save it to keep it."), now);
        }
        Err(e) => state.set_status(format!("Could not recover: {e}"), now),
    }
}

/// Per frame: autosave tick, status, and the web unsaved flag.
pub fn tick(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let rec = &mut state.recovery;
    if let Some(event) = rec.autosaver.tick(&state.session, web_time::Instant::now()) {
        match event {
            AutosaveEvent::Saved(path) => {
                log::info!("autosaved to {}", path.display());
                rec.last_saved = Some(std::time::SystemTime::now());
            }
            AutosaveEvent::Failed(e) => {
                log::warn!("autosave failed: {e}");
                state.set_status(format!("Autosave failed: {e}"), now);
            }
        }
    }
    let rec = &mut state.recovery;
    if rec.autosaver.is_saving() {
        ctx.request_repaint_after(Duration::from_millis(200));
    } else if state.session.is_modified() && rec.autosaver.is_enabled() {
        // Wake up now and then so the timer fires even while the user is idle.
        ctx.request_repaint_after(Duration::from_secs(15));
    }
    web_backup_tick(state, ctx, now);
}

#[cfg(target_arch = "wasm32")]
fn web_backup_tick(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let unsaved = state.session.is_modified() || state.pen.is_editing() || state.pen.is_pending() || state.asset_preview.is_applying();
    let rec = &mut state.recovery;
    if rec.page_unsaved != Some(unsaved) {
        // Read by the page's `beforeunload` handler (web/index.html): closing the tab asks first.
        if let Some(w) = web_sys::window() {
            let _ = js_sys::Reflect::set(&w, &"subcraftUnsaved".into(), &unsaved.into());
        }
        rec.page_unsaved = Some(unsaved);
    }
    if !unsaved {
        rec.reminded_at = now;
        rec.show_reminder = false;
        return;
    }
    let interval = rec.prefs.interval().as_secs_f64();
    if interval > 0.0 && now - rec.reminded_at >= interval {
        rec.show_reminder = true;
    }
    if rec.show_reminder {
        ctx.request_repaint_after(Duration::from_secs(1));
    } else if interval > 0.0 {
        ctx.request_repaint_after(Duration::from_secs(15));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn web_backup_tick(_state: &mut UiState, _ctx: &egui::Context, _now: f64) {}

/// Web: small window offering to download a backup of unsaved work.
pub fn backup_reminder(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if !state.recovery.show_reminder {
        return;
    }
    let mut download = false;
    let mut later = false;
    egui::Window::new(RichText::new("Unsaved work").font(theme::font_strong(14.0)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -40.0])
        .show(ctx, |ui| {
            ui.set_width(300.0);
            ui.label(RichText::new("The browser version can't autosave to disk. Download a backup of your project so a closed tab or a crash doesn't cost you your work.").color(TEXT_DIM));
            ui.add_space(10.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                download = primary_button(ui, "Download backup").clicked();
                later = secondary_button(ui, "Later").clicked();
            });
        });
    if download {
        crate::dialogs::save(state, true, now);
    }
    if download || later {
        state.recovery.show_reminder = false;
        state.recovery.reminded_at = now;
    }
}

/// "SubCraft didn't close properly" modal, one crashed session at a time.
pub fn prompt(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(crashed) = state.recovery.crashed.first().cloned() else { return };
    enum Choice {
        Recover,
        Discard,
        Folder,
    }
    let mut choice = None;
    egui::Modal::new(egui::Id::new("recover_prompt")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new("SubCraft didn't close properly").font(theme::font_strong(15.0)));
        ui.add_space(4.0);
        ui.label(RichText::new("Recover your last session?").color(TEXT_DIM));
        ui.add_space(10.0);
        egui::Grid::new("recover_grid").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            let dim = |t: &str| RichText::new(t).color(theme::TEXT_FAINT);
            ui.label(dim("Project"));
            ui.label(&crashed.info.name);
            ui.end_row();
            if let Some(p) = &crashed.info.project_path {
                ui.label(dim("File"));
                let name = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
                ui.label(name).on_hover_text(format!("{}

Saving the recovered project writes back to this file.", p.display()));
                ui.end_row();
            }
            ui.label(dim("Last autosave"));
            ui.label(ago(crashed.saved_at));
            ui.end_row();
        });
        if state.recovery.crashed.len() > 1 {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("{} more session(s) to recover after this one.", state.recovery.crashed.len() - 1)).color(theme::TEXT_FAINT));
        }
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary_button(ui, "Recover").clicked() {
                choice = Some(Choice::Recover);
            }
            if secondary_button(ui, "Discard").on_hover_text("Delete the recovery copies").clicked() {
                choice = Some(Choice::Discard);
            }
            if secondary_button(ui, "Show folder").clicked() {
                choice = Some(Choice::Folder);
            }
        });
    });
    match choice {
        Some(Choice::Recover) => {
            let c = state.recovery.crashed.remove(0);
            recover(state, c, now);
        }
        Some(Choice::Discard) => {
            let c = state.recovery.crashed.remove(0);
            if let Err(e) = c.discard() {
                state.set_status(format!("Could not delete the recovery copies: {e}"), now);
            }
        }
        Some(Choice::Folder) => crate::panels::shelf::show_in_folder(crashed.latest()),
        None => {}
    }
}

/// "3 min ago" style age of a file time.
fn ago(t: std::time::SystemTime) -> String {
    let secs = t.elapsed().map_or(0, |d| d.as_secs());
    match secs {
        0..60 => "less than a minute ago".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86400),
    }
}

/// File / Edit → Preferences. UI size is the first setting and applies without a restart.
pub fn preferences_window(state: &mut UiState, ctx: &egui::Context) {
    let graphics = &mut state.graphics;
    let rec = &mut state.recovery;
    if !rec.show_prefs {
        return;
    }
    let mut open = true;
    egui::Window::new(RichText::new("Preferences").font(theme::font_strong(14.0))).open(&mut open).collapsible(false).resizable(false).default_height(620.0).show(ctx, |ui| {
        ui.set_width(380.0);
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 110.0).max(200.0)).show(ui, |ui| {
        ui.label(RichText::new("Interface").font(theme::font_strong(12.5)));
        ui.horizontal(|ui| {
            ui.label("UI size");
            ui.spacing_mut().slider_width = 155.0;
            ui.add(egui::Slider::new(&mut rec.prefs.ui_size_percent, UI_SIZE_RANGE).step_by(5.0).suffix("%"));
            if secondary_button(ui, "Reset").on_hover_text("Reset UI size to 95%").clicked() {
                rec.prefs.ui_size_percent = DEFAULT_UI_SIZE;
            }
        });
        ui.label(RichText::new("Scale text, icons and controls together. Default: 95%.").small().color(TEXT_DIM));
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        crate::graphics::controls(graphics, ui);
        ui.checkbox(&mut rec.prefs.show_welcome, "Show welcome screen on startup");
        if !cfg!(target_arch = "wasm32") {
            ui.checkbox(&mut rec.prefs.check_updates_on_start, "Check for updates when SubCraft starts")
                .on_hover_text("Asks GitHub for the latest release in the background and points at the update button when there is a new one.");
        }
        ui.add_space(10.0);
        ui.label(RichText::new("Autosave").font(theme::font_strong(12.5)));
        ui.add_space(4.0);
        let desc = if cfg!(target_arch = "wasm32") {
            "The browser can't save in the background: SubCraft reminds you to download a backup instead."
        } else {
            "Recovery copies of the open project are written in the background to SubCraft's data folder, never over your own file."
        };
        ui.label(RichText::new(desc).color(TEXT_DIM));
        ui.add_space(8.0);
        egui::Grid::new("prefs_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label(RichText::new(if cfg!(target_arch = "wasm32") { "Remind every" } else { "Every" }).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut rec.prefs.autosave_minutes, 0..=30).suffix(" min").custom_formatter(|v, _| if v == 0.0 { "off".into() } else { format!("{v}") }));
            ui.end_row();
            if !cfg!(target_arch = "wasm32") {
                ui.label(RichText::new("Keep").color(TEXT_DIM));
                ui.add(egui::Slider::new(&mut rec.prefs.autosave_keep, 1..=10).suffix(" copies"));
                ui.end_row();
            }
        });
        rec.autosaver.interval = rec.prefs.interval();
        rec.autosaver.set_keep(rec.prefs.autosave_keep);
        if let Some(store) = &rec.store {
            ui.add_space(8.0);
            if secondary_button(ui, "Open recovery folder").clicked() {
                let _ = std::fs::create_dir_all(store.root());
                crate::panels::shelf::show_in_folder(store.root());
            }
        }
        });
    });
    rec.prefs.apply_ui_size(ctx);
    rec.show_prefs = open;
}
