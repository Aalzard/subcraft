//! Paths panel (Painter): a small panel floating in the 3D view while the Pen tool is active,
//! listing the texture set's pen paths. SubCraft keeps each path on its own layer, so the
//! panel's actions are layer actions: select, rename (double-click), show / hide, duplicate,
//! delete, and in the right-click menu copy / paste of a path's properties or of its points.

use egui::{Color32, RichText, Ui};
use subcraft_app::Command;
use subcraft_core::LayerId;
use subcraft_doc::{LayerContent, PenAnchor, PenPath};

use crate::icons;
use crate::state::{Tool, UiState};
use crate::theme;

/// What was copied from a path: its style (everything but the points) or its points.
#[derive(Clone, Debug)]
pub enum PathClip {
    Properties(PenPath),
    Position(Vec<PenAnchor>, bool),
}

/// `target` with the style of `source`: width, hardness, opacity, values, type and stamp.
pub fn with_properties(target: &PenPath, source: &PenPath) -> PenPath {
    PenPath { anchors: target.anchors.clone(), closed: target.closed, ..source.clone() }
}

/// `target` with the points (fresh ids) and open / closed state of `anchors`.
pub fn with_position(target: &PenPath, anchors: &[PenAnchor], closed: bool) -> PenPath {
    let anchors = anchors.iter().map(|a| PenAnchor { id: subcraft_core::AnchorId::new(), ..a.clone() }).collect();
    PenPath { anchors, closed, ..target.clone() }
}

/// The pen-path layers of the selected texture set, top of the stack first: id, name, visible.
pub fn path_layers(state: &UiState) -> Vec<(LayerId, String, bool)> {
    let mut out = Vec::new();
    state.session.selected_texture_set().walk(|l, _| {
        if let LayerContent::Paint(p) = &l.content && p.path.is_some() {
            out.push((l.id, l.name.clone(), l.visible));
        }
    });
    out.reverse();
    out
}

fn path_of(state: &UiState, layer: LayerId) -> Option<PenPath> {
    match &state.session.selected_texture_set().find(layer)?.content {
        LayerContent::Paint(p) => p.path.clone(),
        _ => None,
    }
}

/// Replace a path layer's path (selects the layer; one undo step once drawn).
fn apply(state: &mut UiState, layer: LayerId, path: PenPath, now: f64) {
    let _ = state.session.execute(Command::SelectLayer(Some(layer)));
    let result = state.pen.set_path(&state.session, path).and_then(|()| state.pen.release(&state.session));
    if let Err(e) = result {
        state.set_status(e.to_string(), now);
    }
}

fn clip_id() -> egui::Id {
    egui::Id::new("paths_panel_clip")
}

fn rename_id() -> egui::Id {
    egui::Id::new("paths_panel_rename")
}

/// Draw the panel at the top left of the 3D view `rect` (Pen tool only).
pub fn show(state: &mut UiState, ui: &Ui, rect: egui::Rect, now: f64) {
    if state.tool != Tool::Pen {
        return;
    }
    let layers = path_layers(state);
    let selected = state.session.selection().layer;
    let mut actions: Vec<(LayerId, Action)> = Vec::new();
    egui::Area::new(ui.id().with("paths_panel"))
        .fixed_pos(rect.left_top() + egui::vec2(10.0, 44.0))
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(190.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Paths").font(theme::font_strong(12.0)).color(theme::TEXT));
                    if ui.small_button(format!("{} New", icons::PLUS)).on_hover_text("Start a new path on the next click").clicked() {
                        state.pen.new_path();
                    }
                });
                if layers.is_empty() {
                    ui.label(RichText::new("No path yet: click the model to start one").small().color(theme::TEXT_DIM));
                }
                let mut renaming: Option<(LayerId, String)> = ui.data(|d| d.get_temp(rename_id()));
                let clip: Option<PathClip> = ui.data(|d| d.get_temp(clip_id()));
                for (id, name, visible) in &layers {
                    ui.horizontal(|ui| {
                        let eye = if *visible { icons::EYE } else { icons::EYE_SLASH };
                        if ui.small_button(eye).on_hover_text("Show / hide the path").clicked() {
                            actions.push((*id, Action::Visible(!*visible)));
                        }
                        let editing = renaming.as_ref().is_some_and(|(r, _)| r == id);
                        if editing {
                            let mut finished = None;
                            if let Some((_, text)) = renaming.as_mut() {
                                let r = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                                r.request_focus();
                                if r.lost_focus() {
                                    finished = Some(ui.input(|i| !i.key_pressed(egui::Key::Escape)).then(|| text.trim().to_string()));
                                }
                            }
                            if let Some(name) = finished {
                                if let Some(name) = name {
                                    actions.push((*id, Action::Rename(name)));
                                }
                                renaming = None;
                            }
                        } else {
                            {
                                let label = ui.selectable_label(selected == Some(*id), RichText::new(name).color(if *visible { theme::TEXT } else { theme::TEXT_DIM }));
                                if label.double_clicked() {
                                    renaming = Some((*id, name.clone()));
                                } else if label.clicked() {
                                    actions.push((*id, Action::Select));
                                }
                                label.context_menu(|ui| {
                                    if ui.button("Rename").clicked() { renaming = Some((*id, name.clone())); ui.close(); }
                                    if ui.button("Duplicate").clicked() { actions.push((*id, Action::Duplicate)); ui.close(); }
                                    if ui.button("Delete").clicked() { actions.push((*id, Action::Delete)); ui.close(); }
                                    if ui.button(if *visible { "Hide" } else { "Show" }).clicked() { actions.push((*id, Action::Visible(!*visible))); ui.close(); }
                                    ui.separator();
                                    if ui.button("Copy properties").clicked() { actions.push((*id, Action::CopyProperties)); ui.close(); }
                                    if ui.add_enabled(matches!(clip, Some(PathClip::Properties(_))), egui::Button::new("Paste properties")).clicked() { actions.push((*id, Action::PasteProperties)); ui.close(); }
                                    if ui.button("Copy position").clicked() { actions.push((*id, Action::CopyPosition)); ui.close(); }
                                    if ui.add_enabled(matches!(clip, Some(PathClip::Position(..))), egui::Button::new("Paste position")).clicked() { actions.push((*id, Action::PastePosition)); ui.close(); }
                                });
                            }
                        }
                    });
                }
                ui.data_mut(|d| match renaming {
                    Some(r) => {
                        d.insert_temp(rename_id(), r);
                    }
                    None => {
                        d.remove::<(LayerId, String)>(rename_id());
                    }
                });
                ui.label(RichText::new("Double-click to rename · right-click for more").small().color(Color32::from_gray(140)));
            });
        });
    for (layer, action) in actions {
        perform(state, ui, layer, action, now);
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Action {
    Select,
    Rename(String),
    Visible(bool),
    Duplicate,
    Delete,
    CopyProperties,
    PasteProperties,
    CopyPosition,
    PastePosition,
}

fn perform(state: &mut UiState, ui: &Ui, layer: LayerId, action: Action, now: f64) {
    let run = |state: &mut UiState, cmd: Command| {
        if let Err(e) = state.session.execute(cmd) {
            state.set_status(e.to_string(), now);
        }
    };
    match action {
        Action::Select => run(state, Command::SelectLayer(Some(layer))),
        Action::Rename(name) if !name.is_empty() => run(state, Command::Rename { layer, name }),
        Action::Rename(_) => {}
        Action::Visible(visible) => run(state, Command::SetVisible { layer, visible }),
        Action::Duplicate => run(state, Command::DuplicateLayer(layer)),
        Action::Delete => run(state, Command::DeleteLayer(layer)),
        Action::CopyProperties | Action::CopyPosition => {
            if let Some(path) = path_of(state, layer) {
                let clip = if action == Action::CopyProperties { PathClip::Properties(path) } else { PathClip::Position(path.anchors.clone(), path.closed) };
                ui.data_mut(|d| d.insert_temp(clip_id(), clip));
            }
        }
        Action::PasteProperties | Action::PastePosition => {
            let (Some(target), Some(clip)) = (path_of(state, layer), ui.data(|d| d.get_temp::<PathClip>(clip_id()))) else { return };
            let path = match clip {
                PathClip::Properties(source) => with_properties(&target, &source),
                PathClip::Position(anchors, closed) => with_position(&target, &anchors, closed),
            };
            apply(state, layer, path, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;
    use subcraft_core::{ChannelKind, ChannelValue};

    fn path(points: &[(f32, f32)], radius: f32) -> PenPath {
        PenPath {
            anchors: points.iter().map(|&(x, y)| PenAnchor::new(Vec2::new(x, y))).collect(),
            closed: false,
            radius,
            hardness: 0.5,
            opacity: 1.0,
            values: [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))].into(),
            kind: Default::default(),
            stamp: None, ribbon: None,
        }
    }

    #[test]
    fn pasting_properties_keeps_the_points_and_pasting_position_keeps_the_style() {
        let a = path(&[(0.1, 0.1), (0.2, 0.2)], 0.01);
        let mut b = path(&[(0.5, 0.5), (0.6, 0.6), (0.7, 0.5)], 0.05);
        b.kind = subcraft_doc::PathKind::Filled;
        let styled = with_properties(&a, &b);
        assert_eq!(styled.anchors, a.anchors);
        assert_eq!((styled.radius, styled.kind), (0.05, subcraft_doc::PathKind::Filled));
        let moved = with_position(&a, &b.anchors, true);
        assert_eq!(moved.radius, 0.01);
        assert!(moved.closed && moved.anchors.len() == 3);
        assert!(moved.anchors.iter().zip(&b.anchors).all(|(m, s)| m.position == s.position && m.id != s.id), "fresh anchor ids");
        assert!(moved.validate().is_ok());
    }
}
