//! Layer stack: tree with visibility, selection, creation/reorder buttons and per-channel blending.

use std::sync::Arc;

use egui::{RichText, Ui};
use subcraft_app::{Command, NewLayer};
use subcraft_core::{ChannelKind, LayerId, TextureSetId};
use subcraft_app::layer_tree::{DropTarget, drop_destination};
use subcraft_doc::{BlendMode, Layer, LayerKind};

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, ACCENT, PAD, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{badge, divider, empty_state, fill_width, icon_button, list_row_sense, prop_row, row_text, slider_row};

// Folder drop zones and inline rename adapted from CalangoTech's Studio fork (bd6c064).
#[derive(Clone, Copy)]
struct LayerDrag { texture_set: TextureSetId, layer: LayerId }

#[derive(Clone)]
struct InlineRename { layer: LayerId, text: String, focus: bool }

fn row_target(rect: egui::Rect, pointer: egui::Pos2, layer: &Layer) -> DropTarget {
    let relative = (pointer.y - rect.top()) / rect.height();
    if layer.is_folder() && (0.25..=0.75).contains(&relative) {
        DropTarget::Inside(layer.id)
    } else if relative < 0.5 { DropTarget::Above(layer.id) } else { DropTarget::Below(layer.id) }
}

fn drop_marker(ui: &Ui, rect: egui::Rect, target: DropTarget) {
    let stroke = egui::Stroke::new(2.0, ACCENT);
    match target {
        DropTarget::Above(_) | DropTarget::RootTop => { ui.painter().line_segment([rect.left_top(), rect.right_top()], stroke); }
        DropTarget::Below(_) | DropTarget::RootBottom => { ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], stroke); }
        DropTarget::Inside(_) => { ui.painter().rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside); }
    }
}

fn layer_drop(ui: &Ui, resp: &egui::Response, ts: &subcraft_doc::TextureSet, target: DropTarget, cmds: &mut Vec<Command>) {
    if let Some(drag) = resp.dnd_hover_payload::<LayerDrag>()
        && drag.texture_set == ts.id && drop_destination(ts, drag.layer, target).is_some()
    { drop_marker(ui, resp.rect, target); }
    // Taking a payload of the wrong type clears it too. Preserve shelf/material dragging.
    if egui::DragAndDrop::has_payload_of_type::<LayerDrag>(ui.ctx())
        && let Some(drag) = resp.dnd_release_payload::<LayerDrag>()
        && drag.texture_set == ts.id && drop_destination(ts, drag.layer, target).is_some()
    { cmds.push(Command::DropLayer { texture_set: drag.texture_set, layer: drag.layer, target }); }
}

fn root_drop(ui: &mut Ui, ts: &subcraft_doc::TextureSet, target: DropTarget, cmds: &mut Vec<Command>) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    let label = if target == DropTarget::RootTop { "Top level — top" } else { "Top level — bottom" };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, theme::font(10.0), TEXT_FAINT);
    layer_drop(ui, &resp, ts, target, cmds);
}

fn rows<'a>(list: &'a [Arc<Layer>], depth: usize, out: &mut Vec<(usize, &'a Layer)>) {
    for l in list.iter().rev() {
        out.push((depth, l));
        if let subcraft_doc::LayerContent::Group(g) = &l.content
            && g.expanded
        {
            rows(&g.children, depth + 1, out);
        }
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    let sel = state.session.selection().layer;
    let pad = egui::Margin::symmetric(PAD, 8);
    let rename_id = egui::Id::new(("layer_inline_rename", state.session.selection().texture_set));
    let mut rename = ui.ctx().data_mut(|data| data.get_temp::<InlineRename>(rename_id));
    if rename.as_ref().is_some_and(|edit| state.session.selected_texture_set().find(edit.layer).is_none()) { rename = None; }
    if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::F2))
        && let Some(layer) = state.session.selected_layer()
    { rename = Some(InlineRename { layer: layer.id, text: layer.name.clone(), focus: true }); }
    let mut finish_rename = None;
    let cancel_rename = rename.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape));

    // Toolbar: create on the left, arrange on the right.
    egui::Frame::new().inner_margin(pad).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if icon_button(ui, icons::PAINT_BRUSH, "New paint layer (Ctrl+Shift+N)").clicked() {
                cmds.push(Command::AddLayer(NewLayer::Paint));
            }
            if icon_button(ui, icons::PAINT_BUCKET, "New fill layer (Ctrl+Shift+F)").clicked() {
                cmds.push(Command::AddLayer(NewLayer::Fill));
            }
            if icon_button(ui, icons::FOLDER, "New folder (Ctrl+G)").clicked() {
                cmds.push(Command::AddLayer(NewLayer::Folder));
            }
            if icon_button(ui, icons::IMAGE, "Import texture set as a fill layer…").clicked() {
                import_texture_set_dialog(ui.ctx());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(sel.is_some(), |ui| {
                    let id = sel.unwrap_or_default();
                    if icon_button(ui, icons::TRASH, "Delete (Del)").clicked() {
                        cmds.push(Command::DeleteLayer(id));
                    }
                    if icon_button(ui, icons::COPY, "Duplicate (Ctrl+D)").clicked() {
                        cmds.push(Command::DuplicateLayer(id));
                    }
                    if icon_button(ui, icons::ARROW_DOWN, "Move down").clicked() {
                        cmds.push(Command::MoveDown(id));
                    }
                    if icon_button(ui, icons::ARROW_UP, "Move up").clicked() {
                        cmds.push(Command::MoveUp(id));
                    }
                });
            });
        });
        let channels = state.session.channels();
        if !channels.contains(&state.active_channel) {
            state.active_channel = channels.first().copied().unwrap_or(ChannelKind::BaseColor);
        }
        ui.add_space(4.0);
        prop_row(ui, "Channel", |ui| {
            egui::ComboBox::from_id_salt("active_channel").selected_text(state.active_channel.display_name()).show_ui(ui, |ui| {
                for k in &channels {
                    ui.selectable_value(&mut state.active_channel, *k, k.display_name());
                }
            });
        });
    });
    divider(ui);

    // Blend controls of the selected layer, pinned to the bottom (one compact row when the panel
    // is short, so at least two layer rows stay visible).
    let compact = ui.available_height() < 200.0;
    let active = state.active_channel;
    if let Some(l) = state.session.selected_layer() {
        let id = l.id;
        let folder = l.is_folder();
        let mut b = l.blend_for(active);
        egui::Panel::bottom("layer_blend")
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(theme::BG_2).inner_margin(egui::Margin::symmetric(PAD, 10)))
            .show(ui, |ui| {
                let blend_combo = |ui: &mut Ui, cmds: &mut Vec<Command>, width: f32| {
                    egui::ComboBox::from_id_salt("blend_mode").width(width).selected_text(b.mode.label()).show_ui(ui, |ui| {
                        for m in BlendMode::available(active == ChannelKind::Normal, folder) {
                            if ui.selectable_label(b.mode == m, m.label()).clicked() {
                                cmds.push(Command::SetBlendMode { layer: id, channel: active, mode: m });
                            }
                        }
                    });
                };
                if compact {
                    // Short panel: one row so the layer list keeps room.
                    ui.horizontal(|ui| {
                        blend_combo(ui, &mut cmds, (ui.available_width() - 70.0).max(60.0));
                        let mut pct = b.opacity * 100.0;
                        let r = ui.add(egui::DragValue::new(&mut pct).range(0.0..=100.0).speed(0.5).suffix(" %").max_decimals(0)).on_hover_text("Opacity");
                        if r.changed() {
                            cmds.push(Command::SetBlendOpacity { layer: id, channel: active, opacity: pct / 100.0 });
                        }
                    });
                } else {
                    prop_row(ui, "Blend", |ui| blend_combo(ui, &mut cmds, ui.available_width()));
                    if slider_row(ui, "Opacity", egui::Slider::new(&mut b.opacity, 0.0..=1.0).max_decimals(2)).changed() {
                        cmds.push(Command::SetBlendOpacity { layer: id, channel: active, opacity: b.opacity });
                    }
                }
            });
    }

    let ts = state.session.selected_texture_set();
    let mut save_request: Option<(subcraft_core::LayerId, bool)> = None;
    let mut drop_request: Option<(subcraft_core::LayerId, crate::panels::shelf::ShelfDrag)> = None;
    let mut layer_action = None;
    let mut list = Vec::new();
    rows(&ts.layers, 0, &mut list);
    // Default min height (64 px) would let rows spill over the pinned blend controls when the
    // panel is short; the list must only use the space that is left (it scrolls).
    let rows_height = ui.available_height().max(0.0);
    egui::ScrollArea::vertical()
        .id_salt("layer_rows")
        .auto_shrink([false, false])
        .min_scrolled_height(0.0)
        .max_height(rows_height)
        .show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        fill_width(ui);
        if egui::DragAndDrop::has_payload_of_type::<LayerDrag>(ui.ctx())
            && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
        {
            let visible = ui.clip_rect();
            if visible.contains(pointer) {
                let speed = if pointer.y < visible.top() + 24.0 { 300.0 } else if pointer.y > visible.bottom() - 24.0 { -300.0 } else { 0.0 };
                ui.scroll_with_delta(egui::vec2(0.0, speed * ui.input(|i| i.stable_dt)));
                if speed != 0.0 { ui.ctx().request_repaint(); }
            }
        }
        root_drop(ui, ts, DropTarget::RootTop, &mut cmds);
        if list.is_empty() {
            empty_state(ui, icons::STACK, "No layers yet. Add one above.");
        }
        for (depth, l) in &list {
            let selected = sel == Some(l.id);
            let b = l.blend_for(active);
            let resp = ui.push_id(l.id, |ui| list_row_sense(ui, selected, egui::Sense::click_and_drag(), |ui| {
                ui.add_space(*depth as f32 * 16.0);
                if let subcraft_doc::LayerContent::Group(group) = &l.content {
                    let icon = if group.expanded { icons::CARET_DOWN } else { icons::CARET_RIGHT };
                    if icon_button(ui, icon, if group.expanded { "Collapse folder" } else { "Expand folder" }).clicked() {
                        cmds.push(Command::SetFolderExpanded { layer: l.id, expanded: !group.expanded });
                    }
                } else { ui.add_space(26.0); }
                let (eye, tip) = if l.visible { (icons::EYE, "Hide") } else { (icons::EYE_SLASH, "Show") };
                if icon_button(ui, eye, tip).clicked() {
                    cmds.push(Command::SetVisible { layer: l.id, visible: !l.visible });
                }
                let icon = match l.kind() {
                    LayerKind::Paint => icons::PAINT_BRUSH,
                    LayerKind::Fill => icons::PAINT_BUCKET,
                    LayerKind::Folder => if matches!(&l.content, subcraft_doc::LayerContent::Group(g) if g.expanded) { icons::FOLDER_OPEN } else { icons::FOLDER },
                };
                let icon_color = if selected { ACCENT } else { TEXT_DIM };
                ui.label(RichText::new(icon).font(theme::font(14.0)).color(icon_color)).on_hover_text(format!("{:?} layer", l.kind()));
                let mut text = row_text(&l.name, selected || l.visible);
                if !l.visible {
                    text = text.color(TEXT_FAINT);
                }
                if let Some(edit) = rename.as_mut().filter(|edit| edit.layer == l.id) {
                    let r = ui.add(egui::TextEdit::singleline(&mut edit.text).id_salt("inline_name").desired_width((ui.available_width() - 65.0).max(40.0)));
                    if edit.focus { r.request_focus(); edit.focus = false; }
                    else if r.lost_focus() || (r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        finish_rename = Some((l.id, edit.text.trim().to_string()));
                    }
                } else { ui.add(egui::Label::new(text).truncate().selectable(false)); }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} {:.0}", b.mode.short_label(), b.opacity * 100.0)).font(theme::font(11.0)).color(TEXT_FAINT));
                    if l.mask.is_some() {
                        badge(ui, "mask", ACCENT).on_hover_text("Has a mask");
                    }
                });
            })).inner;
            if rename.as_ref().is_none_or(|edit| edit.layer != l.id) {
                resp.dnd_set_drag_payload(LayerDrag { texture_set: ts.id, layer: l.id });
            }
            if resp.clicked() {
                cmds.push(Command::SelectLayer(Some(l.id)));
            }
            if resp.double_clicked() { rename = Some(InlineRename { layer: l.id, text: l.name.clone(), focus: true }); }
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                layer_drop(ui, &resp, ts, row_target(resp.rect, pointer, l), &mut cmds);
            }
            if egui::DragAndDrop::has_payload_of_type::<crate::panels::shelf::ShelfDrag>(ui.ctx())
                && let Some(item) = resp.dnd_release_payload::<crate::panels::shelf::ShelfDrag>() {
                drop_request = Some((l.id, *item));
            }
            if resp.dnd_hover_payload::<crate::panels::shelf::ShelfDrag>().is_some() {
                // Materials land above the row; smart masks go onto the row's layer.
                let r = resp.rect;
                ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
            }
            let has_effects = l.mask.as_ref().is_some_and(|m| !m.effects.is_empty());
            resp.context_menu(|ui| {
                if ui.add(egui::Button::new("Rename").shortcut_text("F2")).clicked() {
                    rename = Some(InlineRename { layer: l.id, text: l.name.clone(), focus: true });
                    cmds.push(Command::SelectLayer(Some(l.id))); ui.close();
                }
                if ui.button("Group in new folder").clicked() { cmds.push(Command::GroupLayer(l.id)); ui.close(); }
                ui.menu_button("Move into folder", |ui| {
                    let mut folders = Vec::new();
                    ts.walk(|folder, depth| {
                        if folder.is_folder() && drop_destination(ts, l.id, DropTarget::Inside(folder.id)).is_some() {
                            folders.push((folder.id, format!("{}{}", "  ".repeat(depth), folder.name)));
                        }
                    });
                    if folders.is_empty() { ui.weak("No available folders"); }
                    for (folder, label) in folders {
                        if ui.button(label).clicked() { cmds.push(Command::DropLayer { texture_set: ts.id, layer: l.id, target: DropTarget::Inside(folder) }); ui.close(); }
                    }
                });
                if ts.location(l.id).is_some_and(|loc| loc.parent.is_some()) && ui.button("Move to top level").clicked() {
                    cmds.push(Command::DropLayer { texture_set: ts.id, layer: l.id, target: DropTarget::RootTop }); ui.close();
                }
                ui.separator();
                use subcraft_app::part_workflow::LayerAction;
                for (label, action) in [("Add paint layer", LayerAction::AddPaint), ("Add fill layer", LayerAction::AddFill)] {
                    if ui.button(label).clicked() { layer_action = Some((l.id, action)); ui.close(); }
                }
                ui.menu_button("Add mask", |ui| {
                    for (label, action) in [("White mask", LayerAction::WhiteMask), ("Black mask", LayerAction::BlackMask)] {
                        if ui.button(label).clicked() { layer_action = Some((l.id, action)); ui.close(); }
                    }
                });
                ui.menu_button("Add generator", |ui| {
                    for (label, kind) in [("Edge wear", subcraft_doc::GeneratorKind::Edges), ("Dirt", subcraft_doc::GeneratorKind::Cavity), ("Directional", subcraft_doc::GeneratorKind::Directional), ("Position gradient", subcraft_doc::GeneratorKind::PositionGradient)] {
                        if ui.button(label).clicked() { layer_action = Some((l.id, LayerAction::Generator(kind))); ui.close(); }
                    }
                });
                for (label, action) in [("Add fill mask effect", LayerAction::MaskFill), ("Add paint mask effect", LayerAction::MaskPaint)] {
                    if ui.button(label).clicked() { layer_action = Some((l.id, action)); ui.close(); }
                }
                ui.separator();
                if ui.button(format!("{}  Save as smart material", icons::SAVE)).clicked() {
                    save_request = Some((l.id, false));
                    ui.close();
                }
                if has_effects && ui.button(format!("{}  Save mask as smart mask", icons::MASK)).clicked() {
                    save_request = Some((l.id, true));
                    ui.close();
                }
                ui.separator();
                if ui.button(format!("{}  Duplicate", icons::COPY)).clicked() {
                    cmds.push(Command::DuplicateLayer(l.id));
                    ui.close();
                }
                if ui.button(format!("{}  Delete", icons::TRASH)).clicked() {
                    cmds.push(Command::DeleteLayer(l.id));
                    ui.close();
                }
            });
            resp.on_hover_text("Drag to reorder. Drop in a folder's center to move inside it; drop at Top level to move out. Double-click or F2 to rename.");
        }
        root_drop(ui, ts, DropTarget::RootBottom, &mut cmds);
    });
    if cancel_rename { rename = None; }
    else if let Some((layer, name)) = finish_rename {
        if !name.is_empty() && ts.find(layer).is_some_and(|l| l.name != name) { cmds.push(Command::Rename { layer, name }); }
        rename = None; state.rename = None;
    }
    ui.ctx().data_mut(|data| {
        if let Some(edit) = rename { data.insert_temp(rename_id, edit); }
        else { data.remove::<InlineRename>(rename_id); }
    });
    state.run(cmds, now);
    if let Some((layer, action)) = layer_action {
        if let Err(e) = state.session.layer_action(layer, action) { state.set_status(format!("Layer: {e}"), now); }
    }
    if let Some((layer, item)) = drop_request {
        state.run(vec![Command::SelectLayer(Some(layer))], now);
        crate::panels::shelf::apply_shelf_item(state, item, now);
    }
    if let Some((layer, mask)) = save_request {
        save_to_library(state, layer, mask, now);
    }
}

/// Save a layer as a smart material (or its mask as a smart mask) into the user library and
/// show it in the shelf.
pub fn save_to_library(state: &mut UiState, layer: subcraft_core::LayerId, mask: bool, now: f64) {
    let Some(lib) = state.user_library.clone() else {
        state.set_status("No library folder (Documents not found; set SUBCRAFT_LIBRARY)", now);
        return;
    };
    let result = if mask {
        state.session.save_smart_mask(layer, &lib).map(|m| {
            let msg = format!("Saved smart mask \"{}\" to the library", m.name);
            state.library.smart_masks.push(m);
            state.shelf.tab = crate::panels::shelf::ShelfTab::SmartMasks;
            msg
        })
    } else {
        state.session.save_smart_material(layer, &lib).map(|m| {
            let msg = format!("Saved smart material \"{}\" to the library", m.name);
            state.library.smart_materials.push(m);
            state.shelf.tab = crate::panels::shelf::ShelfTab::SmartMaterials;
            msg
        })
    };
    match result {
        Ok(msg) => state.set_status(msg, now),
        Err(e) => state.set_status(format!("Save to library: {e}"), now),
    }
}

/// Pick texture files (base color, roughness, normal…) and import them as one fill layer.
pub fn import_texture_set_dialog(ctx: &egui::Context) {
    crate::files::pick(
        ctx,
        crate::files::Purpose::TextureSet,
        crate::files::IMAGES,
        true,
        Some("Import texture set: select the maps (base color, roughness, normal, …)"),
    );
}
