//! Layer stack: tree with visibility, selection, creation/reorder buttons and per-channel blending.
//! Folders show their hierarchy with guide lines; mask generators are listed under their layer.

use std::sync::Arc;

use egui::{RichText, Ui};
use subcraft_app::{Command, NewLayer, LayerTarget, layer_previews::PreviewKey};
use subcraft_core::{ChannelKind, LayerId, TextureSetId};
use subcraft_app::layer_tree::{DropTarget, drop_destination};
use subcraft_core::MeshMapKind;
use subcraft_doc::{BlendMode, GeneratorKind, Layer, LayerContent, MaskEffectKind};

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, ACCENT, FOLDER, FOLDER_BAND, PAD, TEXT_DIM, TEXT_FAINT, TREE_LINE};
use crate::widgets::{badge, divider, empty_state, fill_width, icon_button, striped_list_row_height, prop_row, row_text, tree_row};

/// Horizontal indent per tree level.
const INDENT: f32 = 16.0;
/// Height of the generator rows listed under a layer.
const GENERATOR_ROW_HEIGHT: f32 = 24.0;

/// Generators offered by the toolbar and the row menu.
const GENERATORS: [(&str, GeneratorKind); 5] = [
    ("Edge wear", GeneratorKind::Edges),
    ("Dirt (cavity)", GeneratorKind::Cavity),
    ("Crevices (concave)", GeneratorKind::Concave),
    ("Directional", GeneratorKind::Directional),
    ("Position gradient", GeneratorKind::PositionGradient),
];
const MESH_MAP_GENERATORS: [MeshMapKind; 3] = [MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature, MeshMapKind::Thickness];

fn generator_menu(ui: &mut Ui, picked: &mut Option<GeneratorKind>) {
    for (label, kind) in GENERATORS {
        if ui.button(format!("{}  {label}", icons::MAGIC_WAND)).clicked() { *picked = Some(kind); ui.close(); }
    }
    ui.menu_button("Mesh map", |ui| {
        for m in MESH_MAP_GENERATORS {
            if ui.button(m.display_name()).clicked() { *picked = Some(GeneratorKind::MeshMap(m)); ui.close(); }
        }
    });
}

/// Guide x of a tree level: the center of its disclosure caret (rows pad their content by 8).
fn guide_x(row_left: f32, depth: usize) -> f32 {
    row_left + 8.0 + depth as f32 * INDENT + 7.0
}

/// Expanded folder whose vertical guide is still being drawn.
struct OpenFolder { depth: usize, x: f32, top: f32, last_child: Option<f32> }

fn close_folder(ui: &Ui, folder: OpenFolder) {
    if let Some(bottom) = folder.last_child {
        ui.painter().line_segment([egui::pos2(folder.x, folder.top), egui::pos2(folder.x, bottom)], egui::Stroke::new(1.0, TREE_LINE));
    }
}

fn descendants(layer: &Layer) -> usize {
    match &layer.content {
        LayerContent::Group(g) => g.children.iter().map(|c| 1 + descendants(c)).sum(),
        _ => 0,
    }
}

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
    let multi_selected = state.multi.is_multi();
    let have_layer_clip = state.clips.layers.is_some();
    let multi_ids = state.multi.ids.clone();
    let pad = egui::Margin::symmetric(PAD, 8);
    let rename_id = egui::Id::new(("layer_inline_rename", state.session.selection().texture_set));
    let mut rename = ui.ctx().data_mut(|data| data.get_temp::<InlineRename>(rename_id));
    if rename.as_ref().is_some_and(|edit| state.session.selected_texture_set().find(edit.layer).is_none()) { rename = None; }
    state.layers_hovered = ui.rect_contains_pointer(ui.max_rect());
    if std::mem::take(&mut state.rename_request)
        && let Some(layer) = state.session.selected_layer()
    { rename = Some(InlineRename { layer: layer.id, text: layer.name.clone(), focus: true }); }
    let mut finish_rename = None;
    let mut layer_action = None;
    let cancel_rename = rename.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape));

    // Edit in PhotoCraft is running for this texture set: say so and offer to stop.
    if let Some(ex) = &state.photocraft
        && ex.texture_set() == state.session.selection().texture_set
    {
        let mut stop = false;
        egui::Frame::new().fill(theme::ACCENT_SOFT).inner_margin(egui::Margin::symmetric(PAD, 6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::EXPORT).color(ACCENT));
                ui.label(RichText::new("Editing in PhotoCraft").strong())
                    .on_hover_text(format!("Saves in PhotoCraft come back as one undo step each.
Files: {}", ex.dir().display()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = ui.small_button("Stop").on_hover_text("Stop bringing PhotoCraft saves back (PhotoCraft stays open)").clicked();
                });
            });
        });
        if stop { state.stop_photocraft(now); }
    }

    // Toolbar: create on the left, arrange on the right.
    egui::Frame::new().inner_margin(pad).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if icon_button(ui, icons::PAINT_BRUSH, crate::keys::native("New paint layer (Ctrl+Shift+N)")).clicked() {
                cmds.push(Command::AddLayer(NewLayer::Paint));
            }
            if icon_button(ui, icons::PAINT_BUCKET, crate::keys::native("New fill layer (Ctrl+Shift+F)")).clicked() {
                cmds.push(Command::AddLayer(NewLayer::Fill));
            }
            if icon_button(ui, icons::FOLDER, crate::keys::native("New folder (Ctrl+G)")).clicked() {
                cmds.push(Command::AddLayer(NewLayer::Folder));
            }
            ui.add_enabled_ui(sel.is_some(), |ui| {
                let mut picked = None;
                ui.menu_button(RichText::new(icons::MAGIC_WAND).font(theme::font(16.0)), |ui| generator_menu(ui, &mut picked))
                    .response
                    .on_hover_text("Add a generator to the selected layer's mask (adds a black mask if it has none)");
                if let (Some(kind), Some(id)) = (picked, sel) { layer_action = Some((id, subcraft_app::part_workflow::LayerAction::Generator(kind))); }
            });
            if icon_button(ui, icons::IMAGE, "Import texture set as a fill layer…").clicked() {
                import_texture_set_dialog(ui.ctx());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(sel.is_some(), |ui| {
                    let id = sel.unwrap_or_default();
                    if icon_button(ui, icons::TRASH, if multi_selected { "Delete selected layers (Del)" } else { "Delete (Del)" }).clicked() {
                        cmds.push(if multi_selected { Command::DeleteLayers(state.multi.ids.clone()) } else { Command::DeleteLayer(id) });
                    }
                    if icon_button(ui, icons::COPY, crate::keys::native(if multi_selected { "Duplicate selected layers (Ctrl+D)" } else { "Duplicate (Ctrl+D)" })).clicked() {
                        cmds.push(if multi_selected { Command::DuplicateLayers(state.multi.ids.clone()) } else { Command::DuplicateLayer(id) });
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

    // Blend mode and opacity of the active channel sit on the right of every layer row.
    let active = state.active_channel;
    let selected_target = state.session.layer_target();
    let selected_effect = state.session.selected_effect();
    let can_paste_mask = state.session.can_paste_mask();

    let ts = state.session.selected_texture_set();
    let mut save_request: Option<(subcraft_core::LayerId, bool)> = None;
    let mut drop_request: Option<(subcraft_core::LayerId, crate::panels::shelf::ShelfDrag)> = None;
    let mut open: Vec<OpenFolder> = Vec::new();
    let mut list = Vec::new();
    rows(&ts.layers, 0, &mut list);
    let order: Vec<LayerId> = list.iter().map(|(_, l)| l.id).collect();
    state.multi.retain_existing(&order);
    // Default min height (64 px) would let rows spill past the panel when it is short; the list
    // must only use the space that is left (it scrolls).
    let rows_height = (ui.available_height() - 44.0).max(0.0);
    let preview_size = state.recovery.prefs.layer_preview_size.clamp(24, 64) as f32;
    egui::ScrollArea::vertical()
        .id_salt("layer_rows")
        .auto_shrink([false, false])
        .min_scrolled_height(0.0)
        .max_height(rows_height)
        .show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        fill_width(ui);
        if (egui::DragAndDrop::has_payload_of_type::<LayerDrag>(ui.ctx())
            || egui::DragAndDrop::has_payload_of_type::<subcraft_app::effect_stack::EffectAddress>(ui.ctx()))
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
        for (index, (depth, l)) in list.iter().enumerate() {
            // Reserve room for names and the blend/opacity column even in a narrow dock.
            let thumb_size = preview_size.min(((ui.available_width() - 176.0 - *depth as f32 * INDENT) / 2.0).clamp(24.0, 64.0));
            let row_height = (thumb_size + 12.0).max(44.0);
            let selected = sel == Some(l.id);
            let row_selected = selected || (state.multi.is_multi() && state.multi.contains(l.id));
            let b = l.blend_for(active);
            let folder = match &l.content { LayerContent::Group(g) => Some(g.expanded), _ => None };
            let resp = ui.push_id(l.id, |ui| {
                let content = |ui: &mut Ui| {
                ui.add_space(*depth as f32 * INDENT);
                if let LayerContent::Group(group) = &l.content {
                    let icon = if group.expanded { icons::CARET_DOWN } else { icons::CARET_RIGHT };
                    if ui.add_sized([14.0, 26.0], egui::Button::new(icon).frame(false)).on_hover_text(if group.expanded { "Collapse folder" } else { "Expand folder" }).clicked() {
                        cmds.push(Command::SetFolderExpanded { layer: l.id, expanded: !group.expanded });
                    }
                } else { ui.add_space(14.0); }
                let (eye, tip) = if l.visible { (icons::EYE, "Hide") } else { (icons::EYE_SLASH, "Show") };
                if icon_button(ui, eye, tip).clicked() {
                    cmds.push(Command::SetVisible { layer: l.id, visible: !l.visible });
                }
                let thumb = state.layer_previews.image(ui, &state.session, PreviewKey { layer: l.id, channel: active, mask: false, effect: None, size: 64 }, thumb_size,
                    selected && selected_target == LayerTarget::Content).on_hover_text(format!("{:?} content · {} preview · click to edit", l.kind(), active.display_name()));
                if thumb.clicked() { cmds.push(Command::SelectLayerTarget { layer: l.id, target: LayerTarget::Content }); }
                if let Some(mask) = &l.mask {
                    let thumb = state.layer_previews.image(ui, &state.session, PreviewKey { layer: l.id, channel: ChannelKind::BaseColor, mask: true, effect: None, size: 64 }, thumb_size,
                        selected && selected_target == LayerTarget::Mask).on_hover_text("Mask · click to edit · Alt+click: UV preview · Shift+click: enable/disable");
                    if !mask.enabled { ui.painter().line_segment([thumb.rect.left_bottom(), thumb.rect.right_top()], egui::Stroke::new(1.5, theme::WARNING)); }
                    if thumb.clicked() {
                        if ui.input(|i| i.modifiers.shift) { cmds.push(Command::SetMaskEnabled { layer: l.id, enabled: !mask.enabled }); }
                        else {
                            cmds.push(Command::SelectLayerTarget { layer: l.id, target: LayerTarget::Mask });
                            if state.tool == crate::state::Tool::Pen { state.tool = crate::state::Tool::Brush; }
                            if ui.input(|i| i.modifiers.alt) { state.layer_previews.inspect = Some(l.id); }
                        }
                    }
                    thumb.context_menu(|ui| super::mask::actions(ui, l.id, mask.enabled, can_paste_mask, &mut cmds));
                }
                if let Some(expanded) = folder {
                    let icon = if expanded { icons::FOLDER_OPEN } else { icons::FOLDER };
                    let color = if l.visible { FOLDER } else { TEXT_FAINT };
                    ui.label(RichText::new(icon).font(theme::font(16.0)).color(color));
                    let count = descendants(l);
                    badge(ui, &count.to_string(), color).on_hover_text(format!("{count} layer(s) inside"));
                }
                let mut text = row_text(&l.name, selected || l.visible);
                if folder.is_some() { text = text.strong(); }
                if !l.visible {
                    text = text.color(TEXT_FAINT);
                }
                let name_width = (ui.available_width() - 82.0).max(20.0);
                ui.allocate_ui_with_layout(egui::vec2(name_width, 38.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    if let Some(edit) = rename.as_mut().filter(|edit| edit.layer == l.id) {
                        let r = ui.add(egui::TextEdit::singleline(&mut edit.text).id_salt("inline_name").desired_width(name_width));
                        if edit.focus { r.request_focus(); edit.focus = false; }
                        else if r.lost_focus() || (r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            finish_rename = Some((l.id, edit.text.trim().to_string()));
                        }
                    } else { ui.add(egui::Label::new(text).truncate().selectable(false)); }
                    let effects = l.content_effects.len() + l.mask.as_ref().map_or(0, |m| m.effects.len());
                    let kind = format!("{:?}", l.kind());
                    let detail = if effects == 0 { kind } else { format!("{kind}  ·  {effects} fx") };
                    ui.add(egui::Label::new(RichText::new(detail).font(theme::font(10.0)).color(TEXT_FAINT)).truncate())
                        .on_hover_text(format!("{} content effects · {} mask effects", l.content_effects.len(), l.mask.as_ref().map_or(0, |m| m.effects.len())));
                });
                ui.allocate_ui_with_layout(egui::vec2(76.0, 40.0), egui::Layout::top_down(egui::Align::Max), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.spacing_mut().interact_size.y = 18.0;
                    ui.spacing_mut().button_padding = egui::vec2(3.0, 1.0);
                    let inactive = &mut ui.visuals_mut().widgets.inactive;
                    inactive.bg_fill = egui::Color32::TRANSPARENT;
                    inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                    inactive.bg_stroke = egui::Stroke::NONE;
                    let channel = active.display_name();
                    egui::ComboBox::from_id_salt(("row_blend", l.id))
                        .width(74.0)
                        .height(420.0)
                        .selected_text(RichText::new(b.mode.short_label()).font(theme::font(10.5)).color(theme::TEXT_DIM))
                        .show_ui(ui, |ui| {
                            for m in BlendMode::available(active == ChannelKind::Normal, l.is_folder()) {
                                if ui.selectable_label(b.mode == m, m.label()).clicked() {
                                    cmds.push(Command::SetBlendMode { layer: l.id, channel: active, mode: m });
                                }
                            }
                        })
                        .response
                        .on_hover_text(format!("{channel} blend mode: {}", b.mode.label()));
                    let mut pct = b.opacity * 100.0;
                    let opacity = egui::DragValue::new(&mut pct).range(0.0..=100.0).speed(0.5).suffix("%").max_decimals(0);
                    if ui.add_sized([40.0, 18.0], opacity).on_hover_text(format!("{channel} opacity (drag)")).changed() {
                        cmds.push(Command::SetBlendOpacity { layer: l.id, channel: active, opacity: pct / 100.0 });
                    }
                });
                };
                match folder {
                    Some(_) => tree_row(ui, row_height, Some(FOLDER_BAND), row_selected, egui::Sense::click_and_drag(), content),
                    None => striped_list_row_height(ui, index, row_selected, row_height, egui::Sense::click_and_drag(), content),
                }
            }).inner;
            // Hierarchy guides: close folders this row is outside of, tick to the parent's guide.
            while open.last().is_some_and(|f| f.depth >= *depth) { close_folder(ui, open.pop().unwrap()); }
            let center = resp.rect.center().y;
            if let Some(parent) = open.last_mut() {
                parent.last_child = Some(center);
                ui.painter().line_segment([egui::pos2(parent.x, center), egui::pos2(parent.x + 10.0, center)], egui::Stroke::new(1.0, TREE_LINE));
            }
            if folder == Some(true) {
                open.push(OpenFolder { depth: *depth, x: guide_x(resp.rect.left(), *depth), top: center + 8.0, last_child: None });
            }
            if rename.as_ref().is_none_or(|edit| edit.layer != l.id) {
                resp.dnd_set_drag_payload(LayerDrag { texture_set: ts.id, layer: l.id });
            }
            if resp.clicked() {
                let mods = ui.input(|i| subcraft_app::multi_select::ClickMods { toggle: i.modifiers.command, range: i.modifiers.shift });
                state.multi.click(&order, l.id, mods);
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
                    // A big stack has many folders: scroll them so the submenu stays small and never covers this menu.
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for (folder, label) in folders {
                            if ui.button(label).clicked() { cmds.push(Command::DropLayer { texture_set: ts.id, layer: l.id, target: DropTarget::Inside(folder) }); ui.close(); }
                        }
                    });
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
                if let Some(mask) = &l.mask {
                    ui.menu_button("Mask actions", |ui| super::mask::actions(ui, l.id, mask.enabled, can_paste_mask, &mut cmds));
                } else if ui.add_enabled(can_paste_mask, egui::Button::new("Paste mask")).clicked() {
                    cmds.push(Command::PasteMask(l.id)); ui.close();
                }
                ui.menu_button("Add generator", |ui| {
                    let mut picked = None;
                    generator_menu(ui, &mut picked);
                    if let Some(kind) = picked { layer_action = Some((l.id, LayerAction::Generator(kind))); }
                });
                for (label, action) in [("Add fill mask effect", LayerAction::MaskFill), ("Add paint mask effect", LayerAction::MaskPaint), ("Add ID color mask", LayerAction::IdColorMask)] {
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
                let in_multi = multi_selected && multi_ids.contains(&l.id);
                let n = if in_multi { multi_ids.len() } else { 1 };
                let suffix = if n > 1 { format!(" {n} layers") } else { String::new() };
                // Acting on a row outside the multi-selection selects it first.
                let aim = |cmds: &mut Vec<Command>| if !in_multi { cmds.push(Command::SelectLayer(Some(l.id))); };
                if ui.add(egui::Button::new(format!("{}  Copy{suffix}", icons::COPY)).shortcut_text(crate::keys::native("Ctrl+C"))).clicked() {
                    aim(&mut cmds); crate::clipboard::request(ui.ctx(), crate::clipboard::Request::CopyLayers { cut: false }); ui.close();
                }
                if ui.add(egui::Button::new(format!("Cut{suffix}")).shortcut_text(crate::keys::native("Ctrl+X"))).clicked() {
                    aim(&mut cmds); crate::clipboard::request(ui.ctx(), crate::clipboard::Request::CopyLayers { cut: true }); ui.close();
                }
                if ui.add_enabled(have_layer_clip, egui::Button::new("Paste above").shortcut_text(crate::keys::native("Ctrl+V"))).clicked() {
                    aim(&mut cmds); crate::clipboard::request(ui.ctx(), crate::clipboard::Request::PasteLayers); ui.close();
                }
                if ui.button(format!("Duplicate{suffix}")).clicked() {
                    cmds.push(if in_multi { Command::DuplicateLayers(multi_ids.clone()) } else { Command::DuplicateLayer(l.id) });
                    ui.close();
                }
                if in_multi {
                    if ui.button(format!("Hide{suffix}")).clicked() { cmds.push(Command::SetLayersVisible { layers: multi_ids.clone(), visible: false }); ui.close(); }
                    if ui.button(format!("Show{suffix}")).clicked() { cmds.push(Command::SetLayersVisible { layers: multi_ids.clone(), visible: true }); ui.close(); }
                }
                if ui.button(format!("{}  Delete{suffix}", icons::TRASH)).clicked() {
                    cmds.push(if in_multi { Command::DeleteLayers(multi_ids.clone()) } else { Command::DeleteLayer(l.id) });
                    ui.close();
                }
            });
            resp.on_hover_text("Drag to reorder. Drop in a folder's center to move inside it; drop at Top level to move out. Double-click or F2 to rename.");
            // The selected layer's mask stack lists its generators itself.
            if let Some(mask) = &l.mask && !(selected && selected_target == LayerTarget::Mask) {
                generator_rows(ui, l, mask, *depth, &mut cmds);
            }
            if selected {
                let channels: Vec<ChannelKind> = ts.channel_kinds().collect();
                super::effects::rows(ui, l, *depth, selected_target, selected_effect, &channels, active, ts.resolution, &mut state.layer_previews, &state.session, (preview_size * 0.55).round().clamp(18.0, 28.0), &mut cmds);
            }
        }
        while let Some(f) = open.pop() { close_folder(ui, f); }
        root_drop(ui, ts, DropTarget::RootBottom, &mut cmds);
    });
    crate::widgets::preview_size_footer(ui, &mut state.recovery.prefs.layer_preview_size, 24..=64);
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
        use subcraft_app::part_workflow::LayerAction;
        let mask_added = matches!(action, LayerAction::WhiteMask | LayerAction::BlackMask);
        if let Err(e) = state.session.layer_action(layer, action) { state.set_status(format!("Layer: {e}"), now); }
        else if mask_added { state.run(vec![Command::SelectLayerTarget { layer, target: LayerTarget::Mask }], now); }
        else if matches!(action, LayerAction::Generator(_)) {
            // Open the new generator in Properties.
            let effect = state.session.selected_texture_set().find(layer).and_then(|l| l.mask.as_ref()).and_then(|m| m.effects.last()).map(|e| e.id);
            if effect.is_some() { state.run(vec![Command::SelectEffect { layer, target: LayerTarget::Mask, effect }], now); }
        }
    }
    if let Some((layer, item)) = drop_request {
        state.run(vec![Command::SelectLayer(Some(layer))], now);
        crate::panels::shelf::apply_shelf_item(state, item, now);
    }
    if let Some((layer, mask)) = save_request {
        save_to_library(state, layer, mask, now);
    }
}

/// Compact rows under a layer for the generators in its mask, top of the stack first. Clicking
/// one opens it in Properties; the eye toggles it.
fn generator_rows(ui: &mut Ui, layer: &Layer, mask: &subcraft_doc::Mask, depth: usize, cmds: &mut Vec<Command>) {
    let indent = depth as f32 * INDENT + 30.0;
    for e in mask.effects.iter().rev() {
        let MaskEffectKind::Generator(g) = &e.kind else { continue };
        let lit = e.visible && mask.enabled && layer.visible;
        let resp = ui.push_id(("generator_row", e.id), |ui| tree_row(ui, GENERATOR_ROW_HEIGHT, None, false, egui::Sense::click(), |ui| {
            ui.add_space(indent);
            let (eye, tip) = if e.visible { (icons::EYE, "Hide generator") } else { (icons::EYE_SLASH, "Show generator") };
            if ui.add(egui::Button::new(RichText::new(eye).font(theme::font(13.0)).color(TEXT_DIM)).frame(false)).on_hover_text(tip).clicked() {
                let mut e = (**e).clone();
                e.visible = !e.visible;
                cmds.push(Command::SetMaskEffect { layer: layer.id, effect: e });
            }
            ui.label(RichText::new(icons::MAGIC_WAND).font(theme::font(13.0)).color(if lit { ACCENT } else { TEXT_FAINT }));
            ui.add(egui::Label::new(RichText::new(&e.name).font(theme::font(11.5)).color(if lit { TEXT_DIM } else { TEXT_FAINT })).truncate().selectable(false));
            let kind = g.kind.label();
            if !e.name.contains(&kind) {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(kind).font(theme::font(10.0)).color(TEXT_FAINT));
                });
            }
        })).inner;
        if resp.clicked() {
            cmds.push(Command::SelectEffect { layer: layer.id, target: LayerTarget::Mask, effect: Some(e.id) });
        }
        resp.on_hover_text(format!("Generator in {}'s mask · click to edit in Properties", layer.name));
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
