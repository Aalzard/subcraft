//! Settings > Shortcuts (Painter's editable shortcut list).
//!
//! Every action is a row with its keys. The keyboard button replaces the keys with the next combination
//! pressed, the plus adds one, the cross on a key removes it, and a right-click (or the reset
//! arrow) puts the key set's default back. Camera mouse gestures are rebound the same way by
//! clicking the capture pad with the new button and modifiers. Rebinds are saved with the
//! preferences ([`crate::recovery::Preferences::custom_keys`] / `custom_mouse`) and apply on top
//! of the chosen key set.

use egui::{RichText, Ui};
use subcraft_app::{ACTIONS, Action, Keymap, KeymapPreset, Shortcut};

use crate::i18n::tr;
use crate::nav::{MOUSE_ACTIONS, MouseButton, MouseCombo, NavMode, NavScheme};
use crate::recovery::Preferences;
use crate::state::UiState;
use crate::theme;

/// What the editor is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capture {
    /// The next key replaces the action's keys (`add` = it is added instead).
    Key { action: Action, add: bool },
    /// The next click on the pad replaces the gesture's mouse combination.
    Mouse(NavMode),
}

/// What this frame's key events mean for a key capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressed {
    Nothing,
    Cancel,
    Key(Shortcut),
}

/// Read one key press from `events` (a plain Tab arrives as `tab`, see `app::take_tab_press`).
/// Esc alone cancels; modifier keys alone are not events, so the combination ends on its last key.
pub fn read_key(events: &[egui::Event], tab: bool) -> Pressed {
    for e in events {
        if let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = e {
            if *key == egui::Key::Escape && !modifiers.any() {
                return Pressed::Cancel;
            }
            if let Some(k) = crate::app::map_key(*key) {
                return Pressed::Key(Shortcut { ctrl: modifiers.command || modifiers.ctrl, shift: modifiers.shift, alt: modifiers.alt, key: k });
            }
        }
    }
    if tab { Pressed::Key(Shortcut::key(subcraft_app::Key::Tab)) } else { Pressed::Nothing }
}

fn action_label(a: Action) -> &'static str {
    subcraft_app::actions::info(a).label
}

/// Store a captured key; returns the message for the user.
pub fn apply_key(prefs: &mut Preferences, action: Action, add: bool, shortcut: Shortcut) -> String {
    let mut km = prefs.keys();
    let moved = if add { km.bind(action, shortcut) } else { km.rebind(action, shortcut) };
    prefs.custom_keys = km.custom;
    let key = crate::keys::native_string(shortcut.label());
    match moved {
        Some(other) => format!("{key} now runs \"{}\" (it was \"{}\").", action_label(action), action_label(other)),
        None => format!("{key} now runs \"{}\".", action_label(action)),
    }
}

/// Store a captured mouse combination; returns the message for the user.
pub fn apply_mouse(prefs: &mut Preferences, mode: NavMode, combo: MouseCombo) -> String {
    let name = |m: NavMode| MOUSE_ACTIONS.iter().find(|(x, _)| *x == m).map_or("", |(_, n)| *n);
    match prefs.custom_mouse.rebind(mode, combo) {
        Some(other) => format!("{} now does \"{}\" (it was \"{}\").", combo.label(), name(mode), name(other)),
        None => format!("{} now does \"{}\".", combo.label(), name(mode)),
    }
}

fn message_id() -> egui::Id {
    egui::Id::new("shortcut_editor_message")
}

fn chip(ui: &mut Ui, text: String, custom: bool) -> bool {
    let mut removed = false;
    egui::Frame::new().fill(theme::BG_3).corner_radius(3.0).inner_margin(egui::Margin::symmetric(5, 1)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(RichText::new(text).monospace().color(if custom { theme::WARNING } else { theme::ACCENT }));
        removed = ui.add(egui::Button::new(RichText::new(crate::icons::X).small().color(theme::TEXT_FAINT)).frame(false)).on_hover_text(tr("Remove this key")).clicked();
    });
    removed
}

/// The Shortcuts tab of the Settings window.
pub fn show(state: &mut UiState, ui: &mut Ui) {
    let capture_before = state.recovery.capture;
    let ctx = ui.ctx().clone();
    let mut message: String = ctx.data_mut(|d| d.get_temp::<String>(message_id()).unwrap_or_default());

    // A key capture eats this frame's key events so nothing else reacts to them.
    if let Some(Capture::Key { action, add }) = capture_before {
        let tab = std::mem::take(&mut state.tab_pressed);
        let pressed = ctx.input(|i| read_key(&i.events, tab));
        if pressed != Pressed::Nothing {
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. })));
            state.recovery.capture = None;
        }
        if let Pressed::Key(s) = pressed {
            message = apply_key(&mut state.recovery.prefs, action, add, s);
        }
    }

    let prefs = &mut state.recovery.prefs;
    ui.horizontal(|ui| {
        ui.label(tr("Key set"));
        egui::ComboBox::from_id_salt("keymap_preset").selected_text(prefs.keymap.label()).show_ui(ui, |ui| {
            for preset in KeymapPreset::ALL {
                ui.selectable_value(&mut prefs.keymap, preset, preset.label());
            }
        });
        let changed = !prefs.custom_keys.is_empty() || !prefs.custom_mouse.is_empty();
        if ui.add_enabled(changed, egui::Button::new(tr("Reset all"))).on_hover_text(tr("Forget every key and mouse binding you changed")).clicked() {
            prefs.custom_keys = Default::default();
            prefs.custom_mouse.reset_all();
            message = tr("Every shortcut is back to the key set's default.").to_string();
        }
    });
    ui.label(RichText::new(tr("The keyboard button replaces an action's keys with the next combination you press (Esc cancels), + adds one, × removes one. Right-click a row to put its default back. Changed keys are orange.")).small().color(theme::TEXT_DIM));
    let search_id = egui::Id::new("shortcut_editor_search");
    let mut query: String = ctx.data_mut(|d| d.get_temp::<String>(search_id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.label(tr("Search"));
        ui.add(egui::TextEdit::singleline(&mut query).hint_text(tr("action or key")).desired_width(200.0));
    });
    if !message.is_empty() {
        ui.label(RichText::new(&message).small().color(theme::ACCENT));
    }
    ui.add_space(4.0);

    let km: Keymap = prefs.keys();
    let mut capture = state.recovery.capture;
    egui::ScrollArea::vertical().id_salt("shortcut_editor_list").max_height(ui.available_height().clamp(160.0, 420.0)).show(ui, |ui| {
        ui.label(RichText::new(tr("Keys")).font(theme::font_strong(12.5)));
        egui::Grid::new("shortcut_editor_keys").num_columns(3).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
            for info in ACTIONS {
                let a = info.action;
                let keys = km.shortcuts(a);
                let names: Vec<String> = keys.iter().map(|s| crate::keys::native_string(s.label())).collect();
                if !crate::shortcuts_window::matches(&query, &format!("{} {}", info.label, names.join(" "))) {
                    continue;
                }
                let custom = !km.is_default(a);
                let label = ui.add(egui::Label::new(if custom { RichText::new(info.label).strong() } else { RichText::new(info.label) }).sense(egui::Sense::click()));
                let mut reset = false;
                label.context_menu(|ui| {
                    if ui.add_enabled(custom, egui::Button::new(tr("Reset to default"))).clicked() {
                        reset = true;
                        ui.close();
                    }
                });
                ui.horizontal(|ui| {
                    if capture == Some(Capture::Key { action: a, add: false }) || capture == Some(Capture::Key { action: a, add: true }) {
                        ui.label(RichText::new(tr("Press the new combination… (Esc cancels)")).italics().color(theme::WARNING));
                        return;
                    }
                    if keys.is_empty() {
                        ui.label(RichText::new(tr("no key")).color(theme::TEXT_FAINT));
                    }
                    for (s, name) in keys.iter().zip(names) {
                        if chip(ui, name, custom) {
                            prefs.custom_keys = {
                                let mut k = prefs.keys();
                                k.unbind(a, *s);
                                k.custom
                            };
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.small_button(crate::icons::KEYBOARD).on_hover_text(tr("Replace the keys with the next combination you press")).clicked() {
                        capture = Some(Capture::Key { action: a, add: false });
                    }
                    if ui.small_button(crate::icons::PLUS).on_hover_text(tr("Add a key")).clicked() {
                        capture = Some(Capture::Key { action: a, add: true });
                    }
                    if custom && ui.small_button(crate::icons::UNDO).on_hover_text(tr("Reset to default")).clicked() {
                        reset = true;
                    }
                });
                if reset {
                    let mut k = prefs.keys();
                    k.reset(a);
                    prefs.custom_keys = k.custom;
                    message = format!("\"{}\" has its default keys again.", info.label);
                }
                ui.end_row();
            }
        });

        ui.add_space(10.0);
        ui.label(RichText::new(tr("Camera mouse gestures")).font(theme::font_strong(12.5)));
        if prefs.nav_scheme != NavScheme::Painter {
            ui.label(RichText::new(tr("These apply to Painter navigation; Blender navigation keeps its own buttons.")).small().color(theme::WARNING));
        }
        egui::Grid::new("shortcut_editor_mouse").num_columns(3).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
            for (mode, name) in MOUSE_ACTIONS {
                let combos = prefs.custom_mouse.combos(mode);
                let custom = prefs.custom_mouse.is_custom(mode);
                let label = ui.add(egui::Label::new(if custom { RichText::new(name).strong() } else { RichText::new(name) }).sense(egui::Sense::click()));
                let mut reset = false;
                label.context_menu(|ui| {
                    if ui.add_enabled(custom, egui::Button::new(tr("Reset to default"))).clicked() {
                        reset = true;
                        ui.close();
                    }
                });
                ui.horizontal(|ui| {
                    if capture == Some(Capture::Mouse(mode)) {
                        let pad = ui.add(egui::Button::new(RichText::new(tr("Click here with the new button, holding Ctrl / Shift / Alt")).color(theme::WARNING)).sense(egui::Sense::click()));
                        let button = if pad.clicked() { Some(MouseButton::Left) } else if pad.clicked_by(egui::PointerButton::Middle) { Some(MouseButton::Middle) } else if pad.secondary_clicked() { Some(MouseButton::Right) } else { None };
                        if let Some(button) = button {
                            let m = ui.input(|i| i.modifiers);
                            message = apply_mouse(prefs, mode, MouseCombo::new(m.command || m.ctrl, m.shift, m.alt, button));
                            capture = None;
                        }
                        return;
                    }
                    let text = if combos.is_empty() { tr("none").to_string() } else { combos.iter().map(MouseCombo::label).collect::<Vec<_>>().join(" / ") };
                    ui.label(RichText::new(text).monospace().color(if custom { theme::WARNING } else { theme::ACCENT }));
                });
                ui.horizontal(|ui| {
                    if capture == Some(Capture::Mouse(mode)) {
                        if ui.small_button(tr("Cancel")).clicked() {
                            capture = None;
                        }
                    } else if ui.small_button(crate::icons::CURSOR).on_hover_text(tr("Replace with the next mouse button you click")).clicked() {
                        capture = Some(Capture::Mouse(mode));
                    }
                    if custom && ui.small_button(crate::icons::UNDO).on_hover_text(tr("Reset to default")).clicked() {
                        reset = true;
                    }
                });
                if reset {
                    prefs.custom_mouse.reset(mode);
                }
                ui.end_row();
            }
        });

        ui.add_space(10.0);
        ui.label(RichText::new(tr("Not editable")).font(theme::font_strong(12.5)));
        egui::Grid::new("shortcut_editor_fixed").num_columns(2).spacing([12.0, 4.0]).striped(true).show(ui, |ui| {
            for (what, how) in crate::shortcuts_window::FIXED_GESTURES {
                if !crate::shortcuts_window::matches(&query, &format!("{what} {how}")) {
                    continue;
                }
                ui.label(RichText::new(*what).color(theme::TEXT_DIM));
                ui.label(RichText::new(*how).monospace().color(theme::TEXT_DIM));
                ui.end_row();
            }
        });
    });
    state.recovery.capture = capture;
    ctx.data_mut(|d| {
        d.insert_temp(search_id, query);
        d.insert_temp(message_id(), message);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::Key;

    fn key(k: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn a_capture_ends_on_the_first_key_and_esc_cancels() {
        assert_eq!(read_key(&[], false), Pressed::Nothing);
        assert_eq!(read_key(&[key(egui::Key::Escape, egui::Modifiers::NONE)], false), Pressed::Cancel);
        assert_eq!(read_key(&[key(egui::Key::Escape, egui::Modifiers::SHIFT)], false), Pressed::Key(Shortcut::shift(Key::Escape)), "Shift+Esc is a combination");
        assert_eq!(read_key(&[key(egui::Key::Z, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT)], false), Pressed::Key(Shortcut::ctrl_shift(Key::Z)));
        assert_eq!(read_key(&[], true), Pressed::Key(Shortcut::key(Key::Tab)), "a plain Tab comes through the Tab flag");
        let release = egui::Event::Key { key: egui::Key::Q, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE };
        assert_eq!(read_key(&[release], false), Pressed::Nothing, "releases are ignored");
    }

    #[test]
    fn captured_keys_and_clicks_are_stored_in_the_preferences_and_survive_a_save() {
        let mut prefs = Preferences::default();
        let msg = apply_key(&mut prefs, Action::BrushTool, true, Shortcut::key(Key::E));
        assert!(msg.contains("it was \"Eraser\""), "{msg}");
        assert_eq!(prefs.keys().action_for(Shortcut::key(Key::E)), Some(Action::BrushTool));
        apply_key(&mut prefs, Action::Undo, false, Shortcut::alt(Key::Z));
        assert_eq!(prefs.keys().shortcuts(Action::Undo), [Shortcut::alt(Key::Z)]);
        let msg = apply_mouse(&mut prefs, NavMode::Zoom, MouseCombo::new(false, false, false, MouseButton::Right));
        assert!(msg.contains("it was \"Camera rotate\""), "{msg}");

        let back = Preferences::from_json_lenient(&serde_json::to_string(&prefs).unwrap());
        assert_eq!(back, prefs);
        assert_eq!(back.keys().action_for(Shortcut::alt(Key::Z)), Some(Action::Undo));
        assert_eq!(back.custom_mouse.combos(NavMode::Zoom), [MouseCombo::new(false, false, false, MouseButton::Right)]);
        // Older preference files have neither field.
        let old = Preferences::from_json_lenient(r#"{"keymap":"Painter"}"#);
        assert!(old.custom_keys.is_empty() && old.custom_mouse.is_empty());
        assert_eq!(old.keys(), Keymap::from(KeymapPreset::Painter));
    }
}
