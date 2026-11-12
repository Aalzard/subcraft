//! Autosave in the UI: the "didn't close properly" prompt and the Preferences window.

use crate::i18n::tr;
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

/// The pages of the Settings window (Painter: General, Shortcuts, Libraries).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    General,
    Shortcuts,
}

/// User settings kept between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Scales all text, icons and widgets together, independently of native display DPI.
    pub ui_size_percent: u32,
    /// Display sizes only; independent of source images and document texture resolution.
    pub shelf_preview_size: u32,
    pub layer_preview_size: u32,
    /// Minutes between autosaves; 0 turns the timer off.
    pub autosave_minutes: u32,
    /// Recovery copies kept.
    pub autosave_keep: usize,
    pub show_welcome: bool,
    /// Desktop: look for a newer release in the background at startup and point at the update
    /// button when there is one.
    pub check_updates_on_start: bool,
    /// Bake ambient occlusion and thickness on the graphics card when one is available (the
    /// processor is the fallback and the reference).
    pub use_gpu_baking: bool,
    /// Mouse navigation of the 3D view.
    pub nav_scheme: crate::nav::NavScheme,
    /// Orbit around the point of the model under the cursor (as Painter does) instead of the
    /// centre of the view.
    pub orbit_around_cursor: bool,
    /// Window > Toolbars: the tool rail and the status bar.
    pub show_tools_toolbar: bool,
    pub show_status_bar: bool,
    /// Which keyboard shortcut table is active (SubCraft default or Substance Painter).
    pub keymap: subcraft_app::KeymapPreset,
    /// Keys the user rebound in Settings > Shortcuts, on top of `keymap`.
    pub custom_keys: subcraft_app::CustomKeys,
    /// Camera mouse gestures the user rebound (Painter navigation).
    pub custom_mouse: crate::nav::MouseBindings,
    /// Blend layers on the graphics card when one is available and the edit is big (the
    /// processor is the fallback and the reference).
    pub use_gpu_compositing: bool,
    /// 3D view toolbar settings.
    pub viewport: crate::viewport_bar::ViewportPrefs,
    /// The choices made in the New Project window last time.
    pub new_project: subcraft_app::new_project::NewProjectSettings,
    /// Interface language (English when absent from an older settings file).
    pub language: crate::i18n::Language,
    /// Last brush colours picked (linear RGB, newest first, at most
    /// [`crate::color_picker::MAX_RECENT`]).
    pub recent_colors: Vec<[f32; 3]>,
}

impl Preferences {
    /// Read stored preferences, keeping every setting that still parses. One value this build does
    /// not know (a language or keymap written by a newer version, a `null` number) used to fail
    /// the whole parse and silently reset all preferences.
    pub fn from_json_lenient(json: &str) -> Self {
        // Only an object is a preferences file (serde would also take an array as the fields in order).
        let Ok(serde_json::Value::Object(stored)) = serde_json::from_str::<serde_json::Value>(json) else { return Self::default() };
        if let Ok(p) = serde_json::from_str(json) {
            return p;
        }
        let Ok(serde_json::Value::Object(mut merged)) = serde_json::to_value(Self::default()) else { return Self::default() };
        for (key, value) in stored {
            let Some(slot) = merged.get_mut(&key) else { continue };
            let default = std::mem::replace(slot, value);
            if serde_json::from_value::<Self>(serde_json::Value::Object(merged.clone())).is_err() {
                log::warn!("preference \"{key}\" has a value this version does not understand; using the default");
                merged.insert(key, default);
            }
        }
        serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or_default()
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            ui_size_percent: DEFAULT_UI_SIZE,
            shelf_preview_size: 88,
            layer_preview_size: 36,
            autosave_minutes: (recovery::DEFAULT_INTERVAL.as_secs() / 60) as u32,
            autosave_keep: recovery::DEFAULT_KEEP,
            show_welcome: true,
            check_updates_on_start: true,
            use_gpu_baking: true,
            nav_scheme: Default::default(),
            orbit_around_cursor: true,
            show_tools_toolbar: true,
            show_status_bar: true,
            keymap: Default::default(),
            custom_keys: Default::default(),
            custom_mouse: Default::default(),
            use_gpu_compositing: true,
            viewport: Default::default(),
            new_project: Default::default(),
            language: Default::default(),
            recent_colors: Vec::new(),
        }
    }
}

impl Preferences {
    /// The key bindings in force: the chosen key set plus the user's rebinds.
    pub fn keys(&self) -> subcraft_app::Keymap {
        subcraft_app::Keymap::new(self.keymap, self.custom_keys.clone())
    }
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
    /// The page shown in the Settings window.
    pub prefs_tab: SettingsTab,
    /// Settings > Shortcuts is waiting for a key or a click (global shortcuts are off meanwhile).
    pub capture: Option<crate::shortcut_editor::Capture>,
    /// Time of the last finished autosave (for the status bar).
    pub last_saved: Option<std::time::SystemTime>,
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
        Self { autosaver, crashed, store, prefs, show_prefs: false, prefs_tab: SettingsTab::General, capture: None, last_saved: None }
    }

    /// No autosave, no prompt (tests, scripted runs without a data folder).
    pub fn disabled() -> Self {
        Self {
            autosaver: Autosaver::disabled(),
            crashed: Vec::new(),
            store: None,
            prefs: Preferences::default(),
            show_prefs: false,
            prefs_tab: SettingsTab::General,
            capture: None,
            last_saved: None,
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

/// Per frame: autosave tick and status.
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
        ui.label(RichText::new(tr("SubCraft didn't close properly")).font(theme::font_strong(15.0)));
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Recover your last session?")).color(TEXT_DIM));
        ui.add_space(10.0);
        egui::Grid::new("recover_grid").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            let dim = |t: &str| RichText::new(t).color(theme::TEXT_FAINT);
            ui.label(dim("Project"));
            ui.add(egui::Label::new(&crashed.info.name).wrap());
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
            if primary_button(ui, tr("Recover")).clicked() {
                choice = Some(Choice::Recover);
            }
            if secondary_button(ui, tr("Discard")).on_hover_text(tr("Delete the recovery copies")).clicked() {
                choice = Some(Choice::Discard);
            }
            if secondary_button(ui, tr("Show folder")).clicked() {
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
    if rec.prefs_tab == SettingsTab::Shortcuts {
        shortcuts_page(state, ctx);
        return;
    }
    let mut open = true;
    let mut tab = SettingsTab::General;
    crate::widgets::window(ctx, RichText::new(tr("Settings")).font(theme::font_strong(14.0))).id(egui::Id::new("settings_window")).open(&mut open).collapsible(false).resizable(false).default_height(620.0).show(ctx, |ui| {
        settings_tabs(ui, &mut tab);
        ui.set_width(380.0);
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 140.0).max(200.0)).show(ui, |ui| {
        ui.label(RichText::new(tr("Interface")).font(theme::font_strong(12.5)));
        ui.horizontal(|ui| {
            ui.label(tr("Language"));
            let combo = egui::ComboBox::from_id_salt("ui_language").selected_text(rec.prefs.language.native_name()).show_ui(ui, |ui| {
                for language in crate::i18n::Language::ALL {
                    ui.selectable_value(&mut rec.prefs.language, language, language.native_name());
                }
            });
            // `--capture-tool language-picker` opens the dropdown (egui's combo popup id is the button id + "popup").
            if ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::new("capture_language_picker"))).unwrap_or(false) {
                egui::Popup::open_id(ui.ctx(), combo.response.id.with("popup"));
            }
        });
        if !crate::i18n::font_available(rec.prefs.language) {
            ui.label(RichText::new(tr("No system font with these characters was found, so text may show as boxes. Set SUBCRAFT_CJK_FONT to a .ttf or .ttc file.")).small().color(theme::WARNING));
        }
        ui.horizontal(|ui| {
            ui.label(tr("UI size"));
            ui.spacing_mut().slider_width = 155.0;
            ui.add(egui::Slider::new(&mut rec.prefs.ui_size_percent, UI_SIZE_RANGE).step_by(5.0).suffix("%"));
            if secondary_button(ui, tr("Reset")).on_hover_text(tr("Reset UI size to 95%")).clicked() {
                rec.prefs.ui_size_percent = DEFAULT_UI_SIZE;
            }
        });
        ui.label(RichText::new(tr("Scale text, icons and controls together. Default: 95%.")).small().color(TEXT_DIM));
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        crate::graphics::controls(graphics, ui);
        ui.checkbox(&mut rec.prefs.show_welcome, tr("Show welcome screen on startup"));
        ui.checkbox(&mut rec.prefs.check_updates_on_start, tr("Check for updates when SubCraft starts"))
            .on_hover_text(tr("Asks GitHub for the latest release in the background and points at the update button when there is a new one."));
        ui.checkbox(&mut rec.prefs.use_gpu_baking, tr("Use GPU for baking"))
            .on_hover_text("Bakes ambient occlusion and thickness on the graphics card, much faster on big meshes. If it is off or fails, the processor bakes them instead. The Bake window shows which one ran.");
        ui.horizontal(|ui| {
            ui.label(tr("3D navigation"));
            egui::ComboBox::from_id_salt("nav_scheme").selected_text(rec.prefs.nav_scheme.label()).show_ui(ui, |ui| {
                for scheme in crate::nav::NavScheme::ALL {
                    ui.selectable_value(&mut rec.prefs.nav_scheme, scheme, scheme.label());
                }
            });
        });
        for (what, how) in rec.prefs.nav_scheme.help() {
            ui.label(RichText::new(format!("{what}: {how}")).small().color(TEXT_DIM));
        }
        ui.checkbox(&mut rec.prefs.orbit_around_cursor, "Orbit around the point under the cursor")
            .on_hover_text("Starting an orbit over the model turns the view around that point of the surface, as Substance Painter does. Over the background the last point is kept. Off: the view always turns around its centre. F frames the model and forgets the point.");
        ui.horizontal(|ui| {
            ui.label(tr("Keyboard shortcuts"));
            ui.label(RichText::new(rec.prefs.keymap.label()).color(theme::ACCENT));
            if ui.button(tr("Edit…")).clicked() {
                tab = SettingsTab::Shortcuts;
            }
        });
        if ui.checkbox(&mut rec.prefs.use_gpu_compositing, tr("Use GPU for compositing"))
            .on_hover_text("Blends layers on the graphics card for big edits (opening a project, changing a layer under many others) on texture sets of 2048 and larger. Procedural fills, generator masks and blur-type filters are still computed by the processor, and sets with blur-type filters use the processor entirely. Small edits such as brush strokes always use the processor.")
            .changed()
        {
            subcraft_compositor::gpu::set_gpu_compositing_enabled(rec.prefs.use_gpu_compositing && !subcraft_app::safe_mode::is_active());
        }
        ui.add_space(10.0);
        ui.label(RichText::new(tr("Rendering mode")).font(theme::font_strong(12.5)));
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Path-traced preview of the 3D view (F10 or Render in its toolbar).")).color(TEXT_DIM));
        crate::viewport_bar::rendering_settings(ui, &mut rec.prefs.viewport);
        ui.add_space(10.0);
        ui.label(RichText::new(tr("Autosave")).font(theme::font_strong(12.5)));
        ui.add_space(4.0);
        ui.label(RichText::new(tr("Recovery copies of the open project are written in the background to SubCraft's data folder, never over your own file.")).color(TEXT_DIM));
        ui.add_space(8.0);
        egui::Grid::new("prefs_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label(RichText::new(tr("Every")).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut rec.prefs.autosave_minutes, 0..=30).suffix(" min").custom_formatter(|v, _| if v == 0.0 { "off".into() } else { format!("{v}") }));
            ui.end_row();
            ui.label(RichText::new(tr("Keep")).color(TEXT_DIM));
            ui.add(egui::Slider::new(&mut rec.prefs.autosave_keep, 1..=10).suffix(" copies"));
            ui.end_row();
        });
        rec.autosaver.interval = rec.prefs.interval();
        rec.autosaver.set_keep(rec.prefs.autosave_keep);
        if let Some(store) = &rec.store {
            ui.add_space(8.0);
            if secondary_button(ui, tr("Open recovery folder")).clicked() {
                let _ = std::fs::create_dir_all(store.root());
                crate::panels::shelf::show_in_folder(store.root());
            }
        }
        });
    });
    rec.prefs.apply_ui_size(ctx);
    rec.prefs_tab = tab;
    rec.show_prefs = open;
}

/// The page buttons at the top of the Settings window.
fn settings_tabs(ui: &mut egui::Ui, tab: &mut SettingsTab) {
    ui.horizontal(|ui| {
        for (t, name) in [(SettingsTab::General, tr("General")), (SettingsTab::Shortcuts, tr("Shortcuts"))] {
            ui.selectable_value(tab, t, RichText::new(name).font(theme::font_strong(12.5)));
        }
    });
    ui.separator();
}

/// Settings > Shortcuts.
fn shortcuts_page(state: &mut UiState, ctx: &egui::Context) {
    let mut open = true;
    let mut tab = SettingsTab::Shortcuts;
    crate::widgets::window(ctx, RichText::new(tr("Settings")).font(theme::font_strong(14.0))).id(egui::Id::new("settings_window")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        settings_tabs(ui, &mut tab);
        ui.set_width(560.0);
        crate::shortcut_editor::show(state, ui);
    });
    state.recovery.prefs_tab = tab;
    state.recovery.show_prefs = open;
    if !open || tab != SettingsTab::Shortcuts {
        state.recovery.capture = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA 2026-11: one value this build does not know reset every preference to its default.
    #[test]
    fn one_unknown_value_does_not_reset_the_other_preferences() {
        let stored = r#"{"ui_size_percent":120,"autosave_minutes":2,"language":"ja","keymap":"from_the_future","show_welcome":false,"added_later":7}"#;
        assert!(serde_json::from_str::<Preferences>(stored).is_err(), "the strict parse rejects the file");
        let p = Preferences::from_json_lenient(stored);
        let d = Preferences::default();
        assert_eq!((p.ui_size_percent, p.autosave_minutes, p.show_welcome), (120, 2, false), "known settings are kept");
        assert_eq!((p.language, p.keymap), (d.language, d.keymap), "unknown values fall back to their defaults");

        let null = r#"{"ui_size_percent":110,"autosave_keep":null}"#;
        let p = Preferences::from_json_lenient(null);
        assert_eq!((p.ui_size_percent, p.autosave_keep), (110, d.autosave_keep));

        assert_eq!(Preferences::from_json_lenient("not json"), d);
        assert_eq!(Preferences::from_json_lenient("[1, 2]"), d);
        let mut custom = Preferences::default();
        custom.ui_size_percent = 130;
        custom.language = crate::i18n::Language::Ko;
        assert_eq!(Preferences::from_json_lenient(&serde_json::to_string(&custom).unwrap()), custom, "a valid file round-trips");
    }
}
