//! Layer stack: tree with visibility, selection, creation/reorder buttons and per-channel blending.
//! Folders show their hierarchy with guide lines; mask generators are listed under their layer.

use crate::i18n::tr;
use std::sync::Arc;

use egui::{RichText, Ui};
use subcraft_app::{Command, NewLayer, LayerTarget, layer_previews::PreviewKey};
use subcraft_core::{ChannelKind, LayerId, TextureSetId};
use subcraft_app::layer_tree::{DropTarget, drop_destination};
use subcraft_app::multi_select::MultiSelect;
use subcraft_core::MeshMapKind;
use subcraft_doc::{BlendMode, GeneratorKind, Layer, LayerContent, MaskEffectKind};

use crate::icons;
use crate::panels::shelf::{self, ShelfDrag};
use crate::state::UiState;
use crate::theme::{self, ACCENT, FOLDER, FOLDER_BAND, PAD, TEXT_DIM, TEXT_FAINT, TREE_LINE};
use crate::widgets::{badge, divider, empty_state, fill_width, icon_button, striped_list_row_height, prop_row, row_text, tree_row};

/// Line under a thumbnail whose stack has at least one effect (Painter draws it red).
const EFFECT_LINE: egui::Color32 = egui::Color32::from_rgb(214, 86, 78);
/// Horizontal indent per tree level.
const INDENT: f32 = 16.0;
/// Height of the generator rows listed under a layer.
const GENERATOR_ROW_HEIGHT: f32 = 24.0;

/// Generators offered by the toolbar and the row menu.
const GENERATORS: [(&str, GeneratorKind); 6] = [
    ("Edge wear", GeneratorKind::Edges),
    ("Dirt", GeneratorKind::Cavity),
    ("Ambient occlusion", GeneratorKind::AmbientOcclusion),
    ("Crevices (concave)", GeneratorKind::Concave),
    ("Directional", GeneratorKind::Directional),
    ("Position gradient", GeneratorKind::PositionGradient),
];
const MESH_MAP_GENERATORS: [MeshMapKind; 3] = [MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature, MeshMapKind::Thickness];

fn generator_menu(ui: &mut Ui, picked: &mut Option<GeneratorKind>) {
    for (label, kind) in GENERATORS {
        if ui.button(format!("{}  {label}", icons::MAGIC_WAND)).clicked() { *picked = Some(kind); ui.close(); }
    }
    ui.menu_button(tr("Mesh map"), |ui| {
        for m in MESH_MAP_GENERATORS {
            if ui.button(m.display_name()).clicked() { *picked = Some(GeneratorKind::MeshMap(m)); ui.close(); }
        }
    });
}

/// Painter's Create Mask menu: the header button and the layer's right-click menu.
fn mask_menu(ui: &mut Ui, layer: &Layer, action: &mut Option<(LayerId, subcraft_app::part_workflow::LayerAction)>) {
    use subcraft_app::part_workflow::LayerAction;
    // As in Painter, adding a mask again replaces the existing one and its effects (undoable).
    let tip = if layer.mask.is_some() { "Replaces this layer's mask and its effects" } else { "" };
    for (label, a) in [("Add white mask", LayerAction::WhiteMask), ("Add black mask", LayerAction::BlackMask)] {
        let r = ui.button(label);
        if (if tip.is_empty() { r } else { r.on_hover_text(tip) }).clicked() {
            *action = Some((layer.id, a));
            ui.close();
        }
    }
    if ui.button(tr("Add bitmap mask")).on_hover_text(tr("Pick a black & white image, or a logo with transparency (a black mask is created if the layer has none)")).clicked() {
        crate::files::pick(ui.ctx(), crate::files::Purpose::AddMaskImage { layer: layer.id }, crate::files::IMAGES, false, None);
        ui.close();
    }
    if ui.button(tr("Add mask with color selection")).on_hover_text(tr("A black mask with a color selection effect: pick ID colors in Properties")).clicked() {
        *action = Some((layer.id, LayerAction::IdColorMask));
        ui.close();
    }
    if ui.button(tr("Add mask with height combination")).on_hover_text(tr("A mask showing this layer where its height rises above the layers below; its Height blending becomes Normal")).clicked() {
        *action = Some((layer.id, LayerAction::HeightCombinationMask));
        ui.close();
    }
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

/// The layers a drag carries: the whole multi-selection when the grabbed row is part of it,
/// else that row alone.
fn dragged_layers(drag: &LayerDrag, multi: &MultiSelect) -> Vec<LayerId> {
    if multi.is_multi() && multi.contains(drag.layer) { multi.ids.clone() } else { vec![drag.layer] }
}

/// Whether dropping `drag` on `target` would do something. `copy`: Ctrl held (duplicate).
fn can_drop(ts: &subcraft_doc::TextureSet, drag: &LayerDrag, multi: &MultiSelect, target: DropTarget, copy: bool) -> bool {
    if drag.texture_set != ts.id || ts.find(drag.layer).is_none() { return false; }
    let anchor = match target {
        DropTarget::Above(id) | DropTarget::Below(id) | DropTarget::Inside(id) => Some(id),
        DropTarget::RootTop | DropTarget::RootBottom => None,
    };
    if let DropTarget::Inside(id) = target && !ts.find(id).is_some_and(|l| l.is_folder()) { return false; }
    let layers = dragged_layers(drag, multi);
    if copy { return true; }
    if layers.len() == 1 { return drop_destination(ts, drag.layer, target).is_some(); }
    !anchor.is_some_and(|a| layers.iter().any(|&l| ts.is_descendant_or_self(a, l)))
}

fn layer_drop(ui: &Ui, resp: &egui::Response, ts: &subcraft_doc::TextureSet, target: DropTarget, multi: &MultiSelect, cmds: &mut Vec<Command>) {
    // Ctrl + drag duplicates (Painter); several selected rows travel together.
    let copy = ui.input(|i| i.modifiers.command);
    if let Some(drag) = resp.dnd_hover_payload::<LayerDrag>() && can_drop(ts, &drag, multi, target, copy) {
        drop_marker(ui, resp.rect, target);
        if copy {
            ui.painter().text(resp.rect.right_center() - egui::vec2(6.0, 0.0), egui::Align2::RIGHT_CENTER, "Duplicate here", theme::font(10.0), ACCENT);
        }
    }
    // Taking a payload of the wrong type clears it too. Preserve shelf/material dragging.
    if egui::DragAndDrop::has_payload_of_type::<LayerDrag>(ui.ctx())
        && let Some(drag) = resp.dnd_release_payload::<LayerDrag>()
        && can_drop(ts, &drag, multi, target, copy)
    {
        let layers = dragged_layers(&drag, multi);
        cmds.push(if copy || layers.len() > 1 {
            Command::DropLayers { texture_set: drag.texture_set, layers, target, copy }
        } else {
            Command::DropLayer { texture_set: drag.texture_set, layer: drag.layer, target }
        });
    }
}

fn root_drop(ui: &mut Ui, ts: &subcraft_doc::TextureSet, target: DropTarget, multi: &MultiSelect, cmds: &mut Vec<Command>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    let label = if target == DropTarget::RootTop { "Top level — top" } else { "Top level — bottom" };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, theme::font(10.0), TEXT_FAINT);
    layer_drop(ui, &resp, ts, target, multi, cmds);
    resp
}

/// The state an eye slide is giving to the rows it passes over.
#[derive(Clone, Copy)]
struct EyeSlide(bool);

/// Painter's eye column: pressing an eye toggles it, and sliding over other eyes with the
/// button held gives them the same state. True when this eye must toggle now.
pub(crate) fn eye_slide(ui: &Ui, eye: &egui::Response, visible: bool) -> bool {
    let id = egui::Id::new("eye_slide");
    let (pressed, down, pos) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.interact_pos()));
    let over = pos.is_some_and(|p| eye.rect.contains(p));
    if !down {
        ui.data_mut(|d| d.remove::<EyeSlide>(id));
        // Pressed and released within one frame (a quick pen tap, a slow frame): still a click.
        return pressed && over && eye.contains_pointer();
    }
    if pressed {
        if over && eye.contains_pointer() {
            ui.data_mut(|d| d.insert_temp(id, EyeSlide(!visible)));
            return true;
        }
        return false;
    }
    over && ui.data(|d| d.get_temp::<EyeSlide>(id)).is_some_and(|slide| slide.0 != visible)
}

/// An eye slide is in progress: rows must not start a drag of their own meanwhile.
pub(crate) fn eye_sliding(ui: &Ui) -> bool {
    ui.data(|d| d.get_temp::<EyeSlide>(egui::Id::new("eye_slide")).is_some())
}

/// Modes of a blending dropdown with a separator between Painter's groups. Returns the pick.
pub(crate) fn blend_mode_list(ui: &mut Ui, modes: impl Iterator<Item = BlendMode>, current: BlendMode) -> Option<BlendMode> {
    let mut picked = None;
    let mut group = None;
    for m in modes {
        if group.is_some_and(|g| g != m.group()) { ui.separator(); }
        group = Some(m.group());
        if ui.selectable_label(current == m, m.label()).clicked() { picked = Some(m); }
    }
    picked
}

/// A blending dropdown that was clicked keeps the focus (Painter): Arrow Up / Down and the
/// mouse wheel over it then step through its modes, until Esc or a click elsewhere. `key`
/// tells the dropdowns apart; `picked`: a mode was chosen from the list this frame.
pub(crate) fn blend_step(ui: &Ui, combo: &egui::Response, key: egui::Id, modes: &[BlendMode], current: BlendMode, picked: bool) -> Option<BlendMode> {
    let id = egui::Id::new("blend_dropdown_focus");
    let focused = ui.data(|d| d.get_temp::<egui::Id>(id)) == Some(key);
    let (pressed, pos, escape) = ui.input(|i| (i.pointer.any_pressed(), i.pointer.interact_pos(), i.key_pressed(egui::Key::Escape)));
    if combo.clicked() || picked {
        ui.data_mut(|d| d.insert_temp(id, key));
    } else if focused && (escape || (pressed && !pos.is_some_and(|p| combo.rect.contains(p)))) {
        ui.data_mut(|d| d.remove::<egui::Id>(id));
        return None;
    } else if !focused {
        return None;
    }
    ui.painter().rect_stroke(combo.rect, 2.0, egui::Stroke::new(1.0, ACCENT), egui::StrokeKind::Outside);
    if ui.ctx().egui_wants_keyboard_input() { return None; }
    let mut step = 0i32;
    ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) { step -= 1; }
        if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) { step += 1; }
    });
    if combo.hovered() {
        let wheel: f32 = ui.input(|i| i.events.iter().map(|e| match e { egui::Event::MouseWheel { delta, .. } => delta.y, _ => 0.0 }).sum());
        if wheel != 0.0 { step += if wheel > 0.0 { -1 } else { 1 }; }
        // The wheel belongs to the dropdown while it has the focus: the list must not scroll.
        ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
    }
    if step == 0 { return None; }
    let at = modes.iter().position(|m| *m == current)? as i32;
    let next = modes[(at + step).clamp(0, modes.len() as i32 - 1) as usize];
    (next != current).then_some(next)
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
    let have_content_clip = state.clips.content.is_some();
    let blend_clip = state.clips.blending.clone();
    let mut copy_blending: Option<LayerId> = None;
    let multi_ids = state.multi.ids.clone();
    let pad = egui::Margin::symmetric(PAD, 8);
    let rename_id = egui::Id::new(("layer_inline_rename", state.session.selection().texture_set));
    let mut rename = ui.ctx().data_mut(|data| data.get_temp::<InlineRename>(rename_id));
    if rename.as_ref().is_some_and(|edit| state.session.selected_texture_set().find(edit.layer).is_none()) { rename = None; }
    // F2 renames unless the active key set uses it (Painter: F2 = 3D view only; double-click renames).
    if !ui.ctx().egui_wants_keyboard_input() && state.recovery.prefs.keys().action_for(subcraft_app::Shortcut::key(subcraft_app::Key::F2)).is_none() && ui.input(|i| i.key_pressed(egui::Key::F2))
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
                ui.label(RichText::new(tr("Editing in PhotoCraft")).strong())
                    .on_hover_text(format!("Saves in PhotoCraft come back as one undo step each.
Files: {}", ex.dir().display()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = ui.small_button(tr("Stop")).on_hover_text(tr("Stop bringing PhotoCraft saves back (PhotoCraft stays open)")).clicked();
                });
            });
        });
        if stop { state.stop_photocraft(now); }
    }

    let channels = state.session.channels();
    if !channels.contains(&state.active_channel) {
        state.active_channel = channels.first().copied().unwrap_or(ChannelKind::BaseColor);
    }
    // Toolbar in Painter's order: Add Effect, Create Mask, paint layer, fill layer, folder, then
    // SubCraft's own (text, import, move, duplicate) and Delete last. It wraps in a narrow dock.
    egui::Frame::new().inner_margin(pad).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            let selected_effect = state.session.selected_effect();
            let mask_stack = state.session.layer_target() == LayerTarget::Mask;
            ui.add_enabled_ui(sel.is_some(), |ui| {
                let layer = state.session.selected_layer();
                ui.menu_button(RichText::new(icons::SPARKLE).font(theme::font(16.0)), |ui| {
                    if let Some(l) = layer {
                        let resolution = state.session.selected_texture_set().resolution;
                        super::effects::add_effect_menu(ui, l, mask_stack, &channels, state.active_channel, resolution, &state.session, &mut cmds);
                    }
                })
                .response
                .on_hover_text(if mask_stack { "Add effect to the selected layer's mask" } else { "Add effect to the selected layer" });
                ui.menu_button(RichText::new(icons::MASK).font(theme::font(16.0)), |ui| {
                    if let Some(l) = layer { mask_menu(ui, l, &mut layer_action); }
                })
                .response
                .on_hover_text(tr("Create mask"));
            });
            if icon_button(ui, icons::PAINT_BRUSH, crate::keys::native("New paint layer (Ctrl+Shift+N)")).clicked() {
                cmds.push(Command::AddLayer(NewLayer::Paint));
            }
            if icon_button(ui, icons::PAINT_BUCKET, crate::keys::native("New fill layer (Ctrl+Shift+F)")).clicked() {
                cmds.push(Command::AddLayer(NewLayer::Fill));
            }
            if icon_button(ui, icons::FOLDER, "New folder (inside the selected folder when it is empty)").clicked() {
                cmds.push(Command::AddLayer(NewLayer::Folder));
            }
            if icon_button(ui, "Aa", crate::keys::native("New text layer (Ctrl+Shift+T): type text, pick font and colour, then click the model to place it")).clicked() {
                state.text.open_new();
            }
            if icon_button(ui, icons::IMAGE, "Import texture set as a fill layer…").clicked() {
                import_texture_set_dialog(ui.ctx());
            }
            ui.add_enabled_ui(sel.is_some(), |ui| {
                let id = sel.unwrap_or_default();
                if icon_button(ui, icons::ARROW_UP, "Move up").clicked() {
                    cmds.push(Command::MoveUp(id));
                }
                if icon_button(ui, icons::ARROW_DOWN, "Move down").clicked() {
                    cmds.push(Command::MoveDown(id));
                }
                if icon_button(ui, icons::COPY, crate::keys::native(if multi_selected { "Duplicate selected layers (Ctrl+D)" } else { "Duplicate (Ctrl+D)" })).clicked() {
                    cmds.push(if multi_selected { Command::DuplicateLayers(state.multi.ids.clone()) } else { Command::DuplicateLayer(id) });
                }
                // Painter's trash removes what is selected: the effect when one is, else the layer(s).
                let tip = if selected_effect.is_some() { "Delete the selected effect(s) (Del)" } else if multi_selected { "Delete selected layers (Del)" } else { "Delete (Del)" };
                if icon_button(ui, icons::TRASH, tip).clicked() {
                    cmds.push(match selected_effect {
                        Some(effect) => {
                            let target = if mask_stack { LayerTarget::Mask } else { LayerTarget::Content };
                            let effects = state.effect_multi.targets((id, target), Some(effect));
                            state.effect_multi.clear();
                            Command::RemoveEffects { layer: id, target, effects }
                        }
                        None if multi_selected => Command::DeleteLayers(state.multi.ids.clone()),
                        None => Command::DeleteLayer(id),
                    });
                }
            });
        });
        ui.add_space(4.0);
        prop_row(ui, tr("Channel"), |ui| {
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
    let mut drop_request: Option<(subcraft_core::LayerId, ShelfDrag, bool)> = None;
    let mut open_text: Option<(subcraft_core::LayerId, crate::text_panel::Window)> = None;
    let mut grouped = false;
    let alt = ui.input(|i| i.modifiers.alt);
    let mut open: Vec<OpenFolder> = Vec::new();
    let mut list = Vec::new();
    rows(&ts.layers, 0, &mut list);
    // Anchor references that cannot be evaluated are flagged on their layer's row (ADR-0045).
    let anchor_index = subcraft_doc::anchor::has_references(&ts.layers).then(|| subcraft_doc::AnchorIndex::build(&ts.layers));
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
        let top_zone = root_drop(ui, ts, DropTarget::RootTop, &state.multi, &mut cmds);
        // Colour tags of the open folders around the current row: (folder depth, colour).
        let mut tags: Vec<(usize, subcraft_doc::LayerColor)> = Vec::new();
        if let Some(item) = top_zone.dnd_hover_payload::<ShelfDrag>() {
            ui.painter().rect_stroke(top_zone.rect, 0.0, egui::Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
            ui.painter().text(top_zone.rect.center(), egui::Align2::CENTER_CENTER, shelf::drop_label(*item, alt, Some("the top layer"), false), theme::font(11.0), ACCENT);
        }
        if let Some(item) = top_zone.dnd_release_payload::<ShelfDrag>() {
            // Above everything: aim at the top layer, so the item lands above it.
            if let Some((_, top)) = list.iter().find(|(d, _)| *d == 0) { drop_request = Some((top.id, *item, alt)); }
            else { drop_request = Some((LayerId::default(), *item, alt)); }
        }
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
            let mut over_mask: Option<String> = None;
            let is_text = matches!(&l.content, LayerContent::Paint(p) if p.text.is_some());
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
                let (eye, tip) = if l.visible { (icons::EYE, "Hide (hold and slide over other eyes to hide them too)") } else { (icons::EYE_SLASH, "Show (hold and slide over other eyes to show them too)") };
                let eye = icon_button(ui, eye, tip);
                if eye_slide(ui, &eye, l.visible) {
                    cmds.push(Command::SetVisible { layer: l.id, visible: !l.visible });
                }
                let thumb = state.layer_previews.image(ui, &state.session, PreviewKey { layer: l.id, channel: active, mask: false, effect: None, size: 64 }, thumb_size,
                    selected && selected_target == LayerTarget::Content).on_hover_text(format!("{:?} content · {} preview · click to edit", l.kind(), active.display_name()));
                if thumb.clicked() { cmds.push(Command::SelectLayerTarget { layer: l.id, target: LayerTarget::Content }); }
                // Painter's line under each thumbnail: grey without effects, coloured with at least one.
                let effect_line = |ui: &Ui, rect: egui::Rect, has: bool| {
                    ui.painter().hline(rect.x_range(), rect.bottom() + 3.0, egui::Stroke::new(2.0, if has { EFFECT_LINE } else { theme::BORDER_STRONG }));
                };
                effect_line(ui, thumb.rect, !l.content_effects.is_empty());
                if let Some(mask) = &l.mask {
                    let thumb = state.layer_previews.image(ui, &state.session, PreviewKey { layer: l.id, channel: ChannelKind::BaseColor, mask: true, effect: None, size: 64 }, thumb_size,
                        selected && selected_target == LayerTarget::Mask).on_hover_text(tr("Mask · click to edit · Alt+click: UV preview · Shift+click: enable/disable"));
                    if let Some(item) = thumb.dnd_hover_payload::<ShelfDrag>() {
                        ui.painter().rect_stroke(thumb.rect, 0.0, egui::Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
                        over_mask = Some(shelf::drop_label(*item, alt, Some(&l.name), true));
                    }
                    if egui::DragAndDrop::has_payload_of_type::<ShelfDrag>(ui.ctx())
                        && let Some(item) = thumb.dnd_release_payload::<ShelfDrag>() { drop_request = Some((l.id, *item, true)); }
                    if !mask.enabled { ui.painter().line_segment([thumb.rect.left_bottom(), thumb.rect.right_top()], egui::Stroke::new(1.5, theme::WARNING)); }
                    effect_line(ui, thumb.rect, !mask.effects.is_empty());
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
                    let problem = anchor_index.as_ref().and_then(|i| i.problems().find(|r| r.site.layer() == l.id)).map(|r| r.status);
                    if let Some(status) = problem {
                        ui.add(egui::Label::new(RichText::new(format!("{}  {detail}  ·  anchor input ignored", icons::WARNING)).font(theme::font(10.0)).color(theme::WARNING)).truncate())
                            .on_hover_text(status.message());
                    } else {
                    ui.add(egui::Label::new(RichText::new(detail).font(theme::font(10.0)).color(TEXT_FAINT)).truncate())
                        .on_hover_text(format!("{} content effects · {} mask effects", l.content_effects.len(), l.mask.as_ref().map_or(0, |m| m.effects.len())));
                    }
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
                    let modes: Vec<BlendMode> = BlendMode::available(active == ChannelKind::Normal, l.is_folder()).collect();
                    let combo = egui::ComboBox::from_id_salt(("row_blend", l.id))
                        .width(74.0)
                        .height(420.0)
                        .selected_text(RichText::new(b.mode.short_label()).font(theme::font(10.5)).color(theme::TEXT_DIM))
                        .show_ui(ui, |ui| blend_mode_list(ui, modes.iter().copied(), b.mode));
                    let picked = combo.inner.flatten();
                    let stepped = blend_step(ui, &combo.response, egui::Id::new(("row_blend_focus", l.id)), &modes, b.mode, picked.is_some());
                    if let Some(mode) = picked.or(stepped) {
                        cmds.push(Command::SetBlendMode { layer: l.id, channel: active, mode });
                    }
                    // Its own popup id: a context menu on the combo would share (and close) the list's.
                    egui::Popup::context_menu(&combo.response).id(combo.response.id.with("apply_to_all")).show(|ui| {
                        if ui.button("Apply to all channels").on_hover_text(format!("Gives every channel of this layer the {channel} blend mode")).clicked() {
                            cmds.push(Command::SetBlendAllChannels { layer: l.id, from: active, mode: true, opacity: false });
                            ui.close();
                        }
                    });
                    combo.response.on_hover_text(format!("{channel} blend mode: {}. Click, then Up / Down or the wheel steps through the modes. Right-click: apply to all channels.", b.mode.label()));
                    let mut pct = b.opacity * 100.0;
                    let opacity = egui::DragValue::new(&mut pct).range(0.0..=100.0).speed(0.5).suffix("%").max_decimals(0);
                    let opacity = ui.add_sized([40.0, 18.0], opacity);
                    if opacity.changed() {
                        cmds.push(Command::SetBlendOpacity { layer: l.id, channel: active, opacity: pct / 100.0 });
                    }
                    opacity.context_menu(|ui| {
                        if ui.button("Apply to all channels").on_hover_text(format!("Gives every channel of this layer the {channel} opacity")).clicked() {
                            cmds.push(Command::SetBlendAllChannels { layer: l.id, from: active, mode: false, opacity: true });
                            ui.close();
                        }
                    });
                    opacity.on_hover_text(format!("{channel} opacity (drag). Right-click: apply to all channels."));
                });
                };
                match folder {
                    Some(_) => tree_row(ui, row_height, Some(FOLDER_BAND), row_selected, egui::Sense::click_and_drag(), content),
                    None => striped_list_row_height(ui, index, row_selected, row_height, egui::Sense::click_and_drag(), content),
                }
            }).inner;
            // Colour tag: the row's own, or dimmed the one of a folder around it.
            while tags.last().is_some_and(|(d, _)| *d >= *depth) { tags.pop(); }
            let inherited = tags.last().map(|(_, c)| *c);
            if let Some(tag) = l.color.or(inherited) {
                let [r, g, b] = tag.rgb();
                let color = egui::Color32::from_rgb(r, g, b);
                let stripe = egui::Rect::from_min_max(egui::pos2(resp.rect.right() - 4.0, resp.rect.top()), resp.rect.right_bottom());
                ui.painter().rect_filled(stripe, 0.0, if l.color.is_some() { color } else { color.gamma_multiply(0.4) });
                if folder.is_some() { tags.push((*depth, tag)); }
            }
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
            if rename.as_ref().is_none_or(|edit| edit.layer != l.id) && !eye_sliding(ui) {
                resp.dnd_set_drag_payload(LayerDrag { texture_set: ts.id, layer: l.id });
            }
            if resp.clicked() {
                let mods = ui.input(|i| subcraft_app::multi_select::ClickMods { toggle: i.modifiers.command, range: i.modifiers.shift });
                state.multi.click(&order, l.id, mods);
                cmds.push(Command::SelectLayer(Some(state.multi.primary_after_click(l.id))));
            }
            if resp.double_clicked() {
                // Text layers open their editor; every other layer renames inline (F2 renames text too).
                if is_text { cmds.push(Command::SelectLayer(Some(l.id))); open_text = Some((l.id, crate::text_panel::Window::Edit)); }
                else { rename = Some(InlineRename { layer: l.id, text: l.name.clone(), focus: true }); }
            }
            if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
                layer_drop(ui, &resp, ts, row_target(resp.rect, pointer, l), &state.multi, &mut cmds);
            }
            super::effects::layer_row_drop(ui, &resp, l, &state.session, &state.effect_multi, &mut cmds);
            if egui::DragAndDrop::has_payload_of_type::<ShelfDrag>(ui.ctx())
                && let Some(item) = resp.dnd_release_payload::<ShelfDrag>() {
                // A drop on a layer row: alphas go into its mask, everything else per Alt.
                drop_request = Some((l.id, *item, alt || matches!(*item, ShelfDrag::Alpha(_))));
            }
            if let Some(item) = resp.dnd_hover_payload::<ShelfDrag>() {
                let r = resp.rect;
                ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
                let text = over_mask.clone().unwrap_or_else(|| shelf::drop_label(*item, alt, Some(&l.name), false));
                let galley = ui.painter().layout_no_wrap(text, theme::font(11.0), theme::TEXT);
                let pill = egui::Rect::from_min_size(egui::pos2(r.right() - galley.size().x - 14.0, r.top() + 2.0), galley.size() + egui::vec2(8.0, 4.0));
                ui.painter().rect_filled(pill, 3.0, theme::ACCENT_SOFT);
                ui.painter().galley(pill.min + egui::vec2(4.0, 2.0), galley, theme::TEXT);
            }
            let has_effects = l.mask.as_ref().is_some_and(|m| !m.effects.is_empty());
            // Painter's order: effects, masks, clipboard, grouping, smart presets; SubCraft's own
            // entries (rename, move, new layers, text) come last.
            resp.context_menu(|ui| {
                use subcraft_app::part_workflow::LayerAction;
                let mask_stack = selected && selected_target == LayerTarget::Mask;
                let row_channels: Vec<ChannelKind> = ts.channel_kinds().collect();
                super::effects::add_effect_menu(ui, l, mask_stack, &row_channels, active, ts.resolution, &state.session, &mut cmds);
                if !mask_stack {
                    ui.menu_button(format!("{}  Add generator", icons::MAGIC_WAND), |ui| {
                        let mut picked = None;
                        generator_menu(ui, &mut picked);
                        if let Some(kind) = picked { layer_action = Some((l.id, LayerAction::Generator(kind))); }
                    })
                    .response
                    .on_hover_text(tr("Adds the generator to this layer's mask (a black mask is created if it has none)"));
                }
                ui.separator();
                mask_menu(ui, l, &mut layer_action);
                if let Some(mask) = &l.mask {
                    ui.menu_button(tr("Mask actions"), |ui| super::mask::actions(ui, l.id, mask.enabled, can_paste_mask, &mut cmds));
                } else if ui.add_enabled(can_paste_mask, egui::Button::new("Paste mask")).clicked() {
                    cmds.push(Command::PasteMask(l.id)); ui.close();
                }
                ui.separator();
                let in_multi = multi_selected && multi_ids.contains(&l.id);
                let n = if in_multi { multi_ids.len() } else { 1 };
                let suffix = if n > 1 { format!(" {n} layers") } else { " layer".to_string() };
                // Acting on a row outside the multi-selection selects it first.
                let aim = |cmds: &mut Vec<Command>| if !in_multi { cmds.push(Command::SelectLayer(Some(l.id))); };
                if ui.add(egui::Button::new(format!("Cut{suffix}")).shortcut_text(crate::keys::native("Ctrl+X"))).clicked() {
                    aim(&mut cmds); crate::clipboard::request(ui.ctx(), crate::clipboard::Request::CopyLayers { cut: true }); ui.close();
                }
                if ui.add(egui::Button::new(format!("{}  Copy{suffix}", icons::COPY)).shortcut_text(crate::keys::native("Ctrl+C"))).clicked() {
                    aim(&mut cmds); crate::clipboard::request(ui.ctx(), crate::clipboard::Request::CopyLayers { cut: false }); ui.close();
                }
                if ui.add_enabled(have_layer_clip, egui::Button::new("Paste layer").shortcut_text(crate::keys::native("Ctrl+V"))).on_hover_text(tr("Pastes above this layer")).clicked() {
                    aim(&mut cmds); crate::clipboard::request(ui.ctx(), crate::clipboard::Request::PasteLayers); ui.close();
                }
                if ui.add(egui::Button::new(format!("Duplicate{suffix}")).shortcut_text(crate::keys::native("Ctrl+D"))).clicked() {
                    cmds.push(if in_multi { Command::DuplicateLayers(multi_ids.clone()) } else { Command::DuplicateLayer(l.id) });
                    ui.close();
                }
                if ui.add(egui::Button::new(format!("{}  Remove{suffix}", icons::TRASH)).shortcut_text("Del")).clicked() {
                    cmds.push(if in_multi { Command::DeleteLayers(multi_ids.clone()) } else { Command::DeleteLayer(l.id) });
                    ui.close();
                }
                ui.separator();
                // "Group layers" puts the whole selection into one new folder.
                if ui.add(egui::Button::new(format!("{}  Group layers", icons::FOLDER)).shortcut_text(crate::keys::native("Ctrl+G"))).clicked() {
                    cmds.push(Command::GroupLayers(if in_multi { multi_ids.clone() } else { vec![l.id] }));
                    grouped = true;
                    ui.close();
                }
                let targets = if in_multi { multi_ids.clone() } else { vec![l.id] };
                ui.menu_button("Blending options", |ui| {
                    if ui.button("Copy").on_hover_text("Copies this layer's blend mode and opacity of every channel").clicked() {
                        copy_blending = Some(l.id);
                        ui.close();
                    }
                    if ui.add_enabled(blend_clip.is_some(), egui::Button::new("Paste")).on_disabled_hover_text("Copy blending options first").clicked()
                        && let Some(blend) = blend_clip.clone()
                    {
                        cmds.push(Command::PasteBlending { layers: targets.clone(), blend });
                        ui.close();
                    }
                });
                if folder.is_none() {
                    if ui.add(egui::Button::new("Copy layer content").shortcut_text(crate::keys::native("Ctrl+Shift+C"))).on_hover_text("Copies the paint or fill and the content effects, not the mask").clicked() {
                        crate::clipboard::request(ui.ctx(), crate::clipboard::Request::CopyContent { layer: Some(l.id) });
                        ui.close();
                    }
                    if ui.add_enabled(have_content_clip, egui::Button::new("Paste layer content").shortcut_text(crate::keys::native("Ctrl+Shift+V"))).on_hover_text("Replaces this layer's content and content effects; its name, mask and blending stay").clicked() {
                        crate::clipboard::request(ui.ctx(), crate::clipboard::Request::PasteContent { layer: Some(l.id) });
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(format!("{}  Create smart material", icons::SAVE)).clicked() {
                    save_request = Some((l.id, false));
                    ui.close();
                }
                if ui.add_enabled(has_effects, egui::Button::new(format!("{}  Create smart mask", icons::MASK))).on_disabled_hover_text(tr("Needs a mask with at least one effect")).clicked() {
                    save_request = Some((l.id, true));
                    ui.close();
                }
                ui.menu_button("Set color", |ui| {
                    if ui.selectable_label(l.color.is_none(), "None").clicked() {
                        cmds.push(Command::SetLayerColor { layers: targets.clone(), color: None });
                        ui.close();
                    }
                    for tag in subcraft_doc::LayerColor::ALL {
                        let [r, g, b] = tag.rgb();
                        ui.horizontal(|ui| {
                            let (swatch, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                            ui.painter().rect_filled(swatch, 2.0, egui::Color32::from_rgb(r, g, b));
                            if ui.selectable_label(l.color == Some(tag), tag.label()).clicked() {
                                cmds.push(Command::SetLayerColor { layers: targets.clone(), color: Some(tag) });
                                ui.close();
                            }
                        });
                    }
                });
                ui.separator();
                if ui.add(egui::Button::new("Rename").shortcut_text("F2")).clicked() {
                    rename = Some(InlineRename { layer: l.id, text: l.name.clone(), focus: true });
                    cmds.push(Command::SelectLayer(Some(l.id))); ui.close();
                }
                ui.menu_button(tr("Move into folder"), |ui| {
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
                if ts.location(l.id).is_some_and(|loc| loc.parent.is_some()) && ui.button(tr("Move to top level")).clicked() {
                    cmds.push(Command::DropLayer { texture_set: ts.id, layer: l.id, target: DropTarget::RootTop }); ui.close();
                }
                for (label, action) in [("Add paint layer", LayerAction::AddPaint), ("Add fill layer", LayerAction::AddFill)] {
                    if ui.button(label).clicked() { layer_action = Some((l.id, action)); ui.close(); }
                }
                if ui.button(tr("Add text layer…")).clicked() { open_text = Some((l.id, crate::text_panel::Window::New)); ui.close(); }
                if is_text && ui.button(tr("Edit text…")).clicked() { open_text = Some((l.id, crate::text_panel::Window::Edit)); ui.close(); }
                if in_multi {
                    if ui.button(format!("Hide{suffix}")).clicked() { cmds.push(Command::SetLayersVisible { layers: multi_ids.clone(), visible: false }); ui.close(); }
                    if ui.button(format!("Show{suffix}")).clicked() { cmds.push(Command::SetLayersVisible { layers: multi_ids.clone(), visible: true }); ui.close(); }
                }
            });
            resp.on_hover_text(tr("Drag to reorder. Drop in a folder's center to move inside it; drop at Top level to move out. Double-click renames (text layers: opens the text editor); F2 renames."));
            // The selected layer's mask stack lists its generators itself.
            if let Some(mask) = &l.mask && !(selected && selected_target == LayerTarget::Mask) {
                generator_rows(ui, l, mask, *depth, &mut cmds);
            }
            if selected {
                let channels: Vec<ChannelKind> = ts.channel_kinds().collect();
                super::effects::rows(ui, l, *depth, selected_target, selected_effect, &mut state.effect_multi, &channels, active, ts.resolution, &mut state.layer_previews, &state.session, (preview_size * 0.55).round().clamp(18.0, 28.0), &mut cmds);
            }
        }
        while let Some(f) = open.pop() { close_folder(ui, f); }
        let _ = root_drop(ui, ts, DropTarget::RootBottom, &state.multi, &mut cmds);
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
    // Copies made by a Ctrl + drag become the selection.
    let duplicated = cmds.iter().any(|c| matches!(c, Command::DropLayers { copy: true, .. }));
    if let Some(layer) = copy_blending
        && let Some(l) = ts.find(layer)
    {
        state.clips.blending = Some(l.blend.clone());
        state.set_status("Copied blending options", now);
    }
    state.run(cmds, now);
    // The grouped layers are inside the new folder, which is now the selection.
    if grouped || duplicated { state.multi.clear(); }
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
    if let Some((layer, item, alt)) = drop_request {
        if layer != LayerId::default() { state.run(vec![Command::SelectLayer(Some(layer))], now); }
        shelf::apply_shelf_item_with(state, item, alt, now);
    }
    if let Some((layer, window)) = open_text {
        state.run(vec![Command::SelectLayer(Some(layer))], now);
        match window { crate::text_panel::Window::New => state.text.open_new(), crate::text_panel::Window::Edit => state.text.open_edit() }
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
