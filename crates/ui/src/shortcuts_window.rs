//! Help > Keyboard shortcuts: a searchable list of every binding in the active key table with its
//! Substance Painter equivalent, the viewport mouse gestures, and Painter's whole documented list
//! with how each entry is handled here.

use egui::{Context, RichText};
use subcraft_app::{ACTIONS, KeymapPreset, PAINTER_REFERENCE, Support, shortcuts_in};

use crate::state::UiState;
use crate::theme;

/// Case-insensitive match of every word of `query` against `haystack`.
pub fn matches(query: &str, haystack: &str) -> bool {
    let hay = haystack.to_lowercase();
    query.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

fn join(list: &[subcraft_app::Shortcut]) -> String {
    if list.is_empty() { "-".into() } else { list.iter().map(|s| crate::keys::native_string(s.label())).collect::<Vec<_>>().join(" / ") }
}

/// Painter keys (as documented) that map to `action`.
fn painter_keys(action: subcraft_app::Action) -> String {
    let keys: Vec<&str> = PAINTER_REFERENCE.iter().filter(|e| e.action == Some(action)).map(|e| e.keys).collect();
    if keys.is_empty() { "-".into() } else { keys.join(" / ") }
}

fn support_label(s: Support) -> &'static str {
    match s {
        Support::Same => "same",
        Support::Added => "added",
        Support::PainterPreset => "Painter preset",
        Support::Gesture => "mouse / hold key",
        Support::None => "not available",
    }
}

pub fn show(state: &mut UiState, ctx: &Context) {
    let mut open = state.show_shortcuts;
    if !open {
        return;
    }
    let id = egui::Id::new("shortcut_search");
    let mut query: String = ctx.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
    let mut preset = state.recovery.prefs.keymap;
    let nav_help = state.recovery.prefs.nav_scheme.help();
    crate::widgets::window(ctx, "Keyboard shortcuts").open(&mut open).resizable(true).collapsible(false).default_width(680.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(&mut query).hint_text("action or key, e.g. brush, F5, ctrl").desired_width(220.0));
            ui.label("Key set");
            egui::ComboBox::from_id_salt("shortcut_preset").selected_text(preset.label()).show_ui(ui, |ui| {
                for p in KeymapPreset::ALL {
                    ui.selectable_value(&mut preset, p, p.label());
                }
            });
        });
        ui.add_space(6.0);
        let key = |t: String| RichText::new(t).monospace().color(theme::ACCENT);
        let dim = |t: &str| RichText::new(t.to_string()).color(theme::TEXT_DIM);
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height().min(520.0)).show(ui, |ui| {
            ui.label(RichText::new("Keys").font(theme::font_strong(12.5)));
            egui::Grid::new("shortcuts").num_columns(4).spacing([24.0, 6.0]).striped(true).show(ui, |ui| {
                ui.label(RichText::new("Action").small().color(theme::TEXT_FAINT));
                ui.label(RichText::new(format!("Active: {}", preset.label())).small().color(theme::TEXT_FAINT));
                ui.label(RichText::new("Substance Painter key").small().color(theme::TEXT_FAINT));
                ui.label(RichText::new("SubCraft default").small().color(theme::TEXT_FAINT));
                ui.end_row();
                for a in ACTIONS {
                    let (active, default, painter) = (join(shortcuts_in(preset, a.action)), join(shortcuts_in(KeymapPreset::SubCraft, a.action)), painter_keys(a.action));
                    if !matches(&query, &format!("{} {active} {default} {painter}", a.label)) {
                        continue;
                    }
                    ui.label(dim(a.label));
                    ui.label(key(active));
                    ui.label(dim(&painter));
                    ui.label(dim(&default));
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            ui.label(RichText::new("Mouse and hold keys").font(theme::font_strong(12.5)));
            egui::Grid::new("shortcut_mouse").num_columns(2).spacing([24.0, 6.0]).striped(true).show(ui, |ui| {
                let extra = [
                    ("Straight line", "Shift+click"),
                    ("Straight line snapped to 5°", "Ctrl+Shift+click"),
                    ("Focus the depth of field", "Ctrl+MMB click on the model (Painter navigation)"),
                    ("Select the texture set under the cursor", "Ctrl+Alt+RMB click (Painter navigation)"),
                    ("Stencil rotation snapped to 90°", "hold Shift+S, then LMB drag"),
                    ("Show a layer's mask", "Alt+click its mask thumbnail"),
                    ("Disable / enable a layer's mask", "Shift+click its mask thumbnail"),
                    ("Part actions", "RMB click"),
                    ("Isolate / hide part", "I / H in the 3D view"),
                    ("Restore all parts", "Esc in the 3D view"),
                    ("Pick material", "Shift+click with eyedropper"),
                    ("Brush size / hardness", "Ctrl+RMB drag (left-right / up-down)"),
                    ("Brush flow / angle", "Ctrl+LMB drag (left-right / up-down)"),
                    ("Stencil move / rotate / scale", "hold S, then MMB / LMB / RMB drag; hold N to ignore it"),
                    ("Clone source", "Ctrl+click or hold V and click"),
                ];
                let camera_extra = [("Pan (alternative)", "Ctrl+Alt+LMB drag")];
                for (what, how) in nav_help.into_iter().chain(extra).chain(camera_extra) {
                    if !matches(&query, &format!("{what} {how}")) {
                        continue;
                    }
                    ui.label(dim(what));
                    ui.label(key(how.to_string()));
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            egui::CollapsingHeader::new(RichText::new(format!("Substance Painter's list ({} entries)", PAINTER_REFERENCE.len())).font(theme::font_strong(12.5)))
                .default_open(!query.trim().is_empty())
                .show(ui, |ui| {
                    egui::Grid::new("painter_reference").num_columns(4).spacing([20.0, 6.0]).striped(true).show(ui, |ui| {
                        for e in PAINTER_REFERENCE {
                            if !matches(&query, &format!("{} {} {} {} {}", e.group, e.keys, e.what, e.note, support_label(e.support))) {
                                continue;
                            }
                            ui.label(key(e.keys.to_string()));
                            ui.label(dim(e.what));
                            ui.label(RichText::new(support_label(e.support)).color(match e.support {
                                Support::None => theme::TEXT_FAINT,
                                Support::PainterPreset => theme::WARNING,
                                _ => theme::ACCENT,
                            }));
                            ui.label(RichText::new(e.note).small().color(theme::TEXT_FAINT));
                            ui.end_row();
                        }
                    });
                });
        });
    });
    ctx.data_mut(|d| d.insert_temp(id, query));
    state.recovery.prefs.keymap = preset;
    state.show_shortcuts = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_every_word_case_insensitively() {
        assert!(matches("", "Brush Smaller"));
        assert!(matches("brush small", "Brush Smaller [ "));
        assert!(matches("F5", "Perspective Camera F5"));
        assert!(!matches("brush eraser", "Brush Smaller"));
    }

    #[test]
    fn painter_keys_lists_documented_keys() {
        assert_eq!(painter_keys(subcraft_app::Action::PolygonFillTool), "4");
        assert_eq!(painter_keys(subcraft_app::Action::PenTool), "-");
    }
}
