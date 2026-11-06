//! Keyboard shortcuts window: every action with its keys, grouped like the menus. Click a key to
//! bind a new one (the next key pressed, Esc cancels); a key taken from another action is moved,
//! never shared. The user's bindings are saved with the preferences.
use egui::RichText;
use subcraft_app::{ACTIONS, Action};

use crate::state::UiState;
use crate::theme;
use crate::widgets::secondary_button;

pub fn window(state: &mut UiState, ctx: &egui::Context) {
    let mut open = state.show_shortcuts;
    if !open {
        state.capturing_shortcut = None;
        return;
    }
    if state.capturing_shortcut.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.capturing_shortcut = None;
    }
    crate::widgets::window(ctx, "Keyboard shortcuts").open(&mut open).resizable(false).collapsible(false).default_height(560.0).show(ctx, |ui| {
        ui.label(RichText::new("Click a shortcut, then press the new key (with Ctrl, Shift or Alt if you like). Esc cancels.").color(theme::TEXT_DIM).font(theme::font(11.0)));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 160.0).max(220.0)).show(ui, |ui| {
            let mut group = "";
            egui::Grid::new("shortcuts").num_columns(3).spacing([24.0, 6.0]).show(ui, |ui| {
                for a in ACTIONS {
                    if a.group != group {
                        group = a.group;
                        ui.label(RichText::new(group).font(theme::font_strong(12.5)).color(theme::TEXT));
                        ui.end_row();
                    }
                    ui.label(RichText::new(a.label).color(theme::TEXT_DIM));
                    shortcut_cell(state, ui, a.action);
                    ui.end_row();
                }
                let nav_help = state.recovery.prefs.nav_scheme.help();
                ui.label(RichText::new("3D view").font(theme::font_strong(12.5)).color(theme::TEXT));
                ui.end_row();
                for (what, how) in nav_help.into_iter().chain([
                    ("Brush size / hardness", "Ctrl+RMB drag (left-right / up-down)"),
                    ("Rotate the light", "Shift+RMB drag"),
                    ("Rotate the environment", "Shift+MMB drag"),
                    ("Straight line", "Shift+click"),
                    ("Part actions", "RMB click"),
                    ("Isolate / hide part", "I / H in the 3D view"),
                    ("Restore all parts", "Esc in the 3D view"),
                    ("Pick material", "Shift+click with eyedropper"),
                ]) {
                    ui.label(RichText::new(what).color(theme::TEXT_DIM));
                    ui.label(RichText::new(how).monospace().color(theme::TEXT_FAINT));
                    ui.end_row();
                }
            });
        });
        ui.add_space(6.0);
        let custom = !state.recovery.prefs.keymap.custom.is_empty();
        if ui.add_enabled(custom, egui::Button::new("Reset all to defaults")).clicked() {
            state.recovery.prefs.keymap.reset_all();
            state.capturing_shortcut = None;
        }
    });
    state.show_shortcuts = open;
}

fn shortcut_cell(state: &mut UiState, ui: &mut egui::Ui, action: Action) {
    let keymap = &state.recovery.prefs.keymap;
    let capturing = state.capturing_shortcut == Some(action);
    let keys = keymap.shortcuts(action);
    let text = if capturing {
        "Press a key…".to_string()
    } else if keys.is_empty() {
        "—".to_string()
    } else {
        keys.iter().map(|s| s.label()).collect::<Vec<_>>().join(" / ")
    };
    let custom = keymap.is_custom(action);
    let color = if capturing { theme::TEXT } else if custom { theme::ACCENT_HOVER } else { theme::ACCENT };
    ui.horizontal(|ui| {
        let b = egui::Button::new(RichText::new(text).monospace().color(color)).min_size(egui::vec2(120.0, 0.0)).selected(capturing);
        if ui.add(b).on_hover_text("Click, then press the new shortcut").clicked() {
            state.capturing_shortcut = if capturing { None } else { Some(action) };
            ui.memory_mut(|m| m.stop_text_input());
        }
        if !keys.is_empty() && !capturing && ui.small_button("×").on_hover_text("Remove this shortcut").clicked() {
            state.recovery.prefs.keymap.set(action, Vec::new());
        }
    });
    if custom {
        if secondary_button(ui, "Reset").on_hover_text("Back to the default shortcut").clicked() {
            state.recovery.prefs.keymap.reset(action);
        }
    } else {
        ui.label("");
    }
}
