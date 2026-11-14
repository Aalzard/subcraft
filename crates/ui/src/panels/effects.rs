//! Effect stacks in the Layers panel and the selected effect's Properties.
//!
//! The selected layer shows the effects of the domain picked with its thumbnails (content or
//! mask) as nested rows, top of the stack first. Every edit is an app command.
use std::collections::BTreeMap;

use egui::{RichText, Ui};
use subcraft_app::{Command, LayerTarget, effect_stack::{DropSide, EffectAddress, EffectDrop, EffectSelection, reordered}};
use subcraft_core::{ChannelKind, ChannelValue, EffectId, LayerId};
use subcraft_doc::{
    BlendMode, ContentEffect, ContentEffectKind, FillChannel, FillContent, FillSource, Generator, GeneratorKind, Layer, Levels,
    MaskEffectKind, ProceduralKind, ProceduralParams,
};
use subcraft_image::TiledImage;

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, ACCENT, TEXT_DIM, TEXT_FAINT};
use crate::widgets::*;

/// One effect row's data, shared by both stacks.
struct Row {
    id: EffectId,
    name: String,
    visible: bool,
    icon: &'static str,
    /// Blend mode and opacity shown on the row (content: those of the active channel), for
    /// effects that have them there.
    blend: Option<(BlendMode, f32)>,
}

fn content_icon(kind: &ContentEffectKind) -> &'static str {
    match kind {
        ContentEffectKind::Paint { .. } => icons::PAINT_BRUSH,
        ContentEffectKind::Fill(_) => icons::PAINT_BUCKET,
        ContentEffectKind::Levels { .. } | ContentEffectKind::Filter { .. } => icons::SLIDERS,
        ContentEffectKind::Anchor => icons::CROSSHAIR,
        ContentEffectKind::Generator(_) => icons::MAGIC_WAND,
    }
}

fn mask_icon(kind: &MaskEffectKind) -> &'static str {
    match kind {
        MaskEffectKind::Paint { .. } => icons::PAINT_BRUSH,
        MaskEffectKind::Fill { .. } => icons::PAINT_BUCKET,
        MaskEffectKind::Generator(_) => icons::MAGIC_WAND,
        MaskEffectKind::Levels(_) | MaskEffectKind::Filter(_) => icons::SLIDERS,
        MaskEffectKind::Invert => icons::INTERSECT,
        MaskEffectKind::Image { .. } => icons::IMAGE,
        MaskEffectKind::IdColor(_) => icons::SELECTION,
        MaskEffectKind::Compare(_) => icons::STACK,
        MaskEffectKind::Anchor => icons::CROSSHAIR,
        MaskEffectKind::AnchorRef { .. } => icons::CURVE,
    }
}

/// A new Fill content effect: base color only, other channels off (enable them in Properties).
pub fn new_fill(channels: impl Iterator<Item = ChannelKind>) -> ContentEffectKind {
    let channels = channels
        .map(|k| {
            let source = FillSource::uniform(k.default_paint_value());
            (k, if k == ChannelKind::BaseColor { FillChannel::new(source) } else { FillChannel::disabled(source) })
        })
        .collect();
    ContentEffectKind::Fill(FillContent { projection: Default::default(), channels })
}

/// Content effects the Add menu offers for a layer, in Painter's order (paint, fill, levels,
/// filter, anchor point), each with its row icon.
fn add_content_menu(ui: &mut Ui, layer: &Layer, channels: &[ChannelKind], active: ChannelKind, cmds: &mut Vec<Command>) {
    let levels_channel = if active == ChannelKind::Normal { ChannelKind::Roughness } else { active };
    let options = [
        ("Add paint", ContentEffectKind::Paint { channels: BTreeMap::new() }),
        ("Add fill", new_fill(channels.iter().copied())),
        ("Add content generator", ContentEffectKind::Generator(subcraft_doc::Generator::new(subcraft_doc::GeneratorKind::Edges))),
        ("Add levels", ContentEffectKind::Levels { channel: levels_channel, levels: Levels::default() }),
    ];
    for (label, kind) in options {
        if layer.is_folder() && !kind.is_adjustment() { continue; }
        if ui.button(format!("{}  {label}", content_icon(&kind))).clicked() {
            cmds.push(Command::AddContentEffect { layer: layer.id, kind });
            ui.close();
        }
    }
    ui.menu_button(format!("{}  Add filter", icons::SLIDERS), |ui| {
        for filter in subcraft_doc::Filter::ALL {
            let channel = if filter.supports_channel(active) { Some(active) }
                else { channels.iter().copied().find(|k| filter.supports_channel(*k)) };
            if ui.add_enabled(channel.is_some(), egui::Button::new(filter.label())).clicked() {
                cmds.push(Command::AddContentEffect { layer: layer.id, kind: ContentEffectKind::Filter { channel: channel.unwrap(), filter } });
                ui.close();
            }
        }
    });
    if ui.button(format!("{}  Add anchor point", icons::CROSSHAIR)).on_hover_text("Captures this layer's content at this point, so layers above can read it").clicked() {
        cmds.push(Command::AddContentEffect { layer: layer.id, kind: ContentEffectKind::Anchor });
        ui.close();
    }
}

pub(crate) fn add_mask_filters(ui: &mut Ui, layer: LayerId, cmds: &mut Vec<Command>) {
    ui.menu_button(format!("{}  Add filter", icons::SLIDERS), |ui| {
        for filter in subcraft_doc::Filter::ALL.into_iter().filter(|f| f.supports_mask()) {
            if ui.button(filter.label()).clicked() {
                cmds.push(Command::AddMaskEffect { layer, kind: MaskEffectKind::Filter(filter) }); ui.close();
            }
        }
    });
}

/// Anchor Point / Anchor Reference entries of a mask's Add menu (ADR-0045).
pub(crate) fn add_mask_anchor_items(ui: &mut Ui, layer: LayerId, index: &subcraft_doc::AnchorIndex, cmds: &mut Vec<Command>) {
    if ui.button(format!("{}  Add anchor point", icons::CROSSHAIR)).on_hover_text("Captures the mask at this point, so other masks and generators can read it").clicked() {
        cmds.push(Command::AddMaskEffect { layer, kind: MaskEffectKind::Anchor });
        ui.close();
    }
    let candidates = index.candidates_above_mask(layer);
    let input = super::anchor::default_input(&candidates, ChannelKind::Height);
    let tip = if input.is_some() { "Grayscale from an anchor point below: a lower layer's channel or mask" } else { "Needs an anchor point below: add one to a lower layer first" };
    if ui.add_enabled(input.is_some(), egui::Button::new(format!("{}  Add anchor reference", icons::CURVE))).on_hover_text(tip).on_disabled_hover_text(tip).clicked()
        && let Some(input) = input
    {
        cmds.push(Command::AddMaskEffect { layer, kind: MaskEffectKind::AnchorRef { input, levels: None } });
        ui.close();
    }
}

/// Mask effects in Painter's order: generator, paint, fill, levels, compare mask, filter, color
/// selection, anchor point; SubCraft's own Invert and Anchor Reference come last.
fn add_mask_menu(ui: &mut Ui, layer: &Layer, resolution: u32, index: &subcraft_doc::AnchorIndex, cmds: &mut Vec<Command>) {
    let mut add = |ui: &mut Ui, label: &str, kind: MaskEffectKind| {
        if ui.button(format!("{}  {label}", mask_icon(&kind))).clicked() {
            cmds.push(Command::AddMaskEffect { layer: layer.id, kind });
            ui.close();
        }
    };
    add(ui, "Add generator", MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)));
    add(ui, "Add paint", MaskEffectKind::Paint { image: TiledImage::new(resolution, resolution, 2, [0.0; 4]) });
    add(ui, "Add fill", MaskEffectKind::Fill { value: 1.0, procedural: None, projection: Default::default() });
    add(ui, "Add levels", MaskEffectKind::Levels(Levels::default()));
    add(ui, "Add compare mask", MaskEffectKind::Compare(Default::default()));
    add_mask_filters(ui, layer.id, cmds);
    let mut add = |ui: &mut Ui, label: &str, kind: MaskEffectKind| {
        if ui.button(format!("{}  {label}", mask_icon(&kind))).clicked() {
            cmds.push(Command::AddMaskEffect { layer: layer.id, kind });
            ui.close();
        }
    };
    add(ui, "Add color selection", MaskEffectKind::IdColor(Default::default()));
    add(ui, "Add invert", MaskEffectKind::Invert);
    add_mask_anchor_items(ui, layer.id, index, cmds);
}

/// Painter's Add Effect menu for one of a layer's stacks: the Layers header button, the stack's
/// own header and the layer's right-click menu all show it. The mask menu needs a mask.
pub(crate) fn add_effect_menu(ui: &mut Ui, layer: &Layer, mask_domain: bool, channels: &[ChannelKind], active: ChannelKind, resolution: u32, session: &subcraft_app::Session, cmds: &mut Vec<Command>) {
    if mask_domain && layer.mask.is_some() {
        let index = subcraft_doc::AnchorIndex::build(&session.selected_texture_set().layers);
        add_mask_menu(ui, layer, resolution, &index, cmds)
    } else {
        add_content_menu(ui, layer, channels, active, cmds)
    }
}

/// Blend of a content effect on `active` as its row shows it: none for anchors, for adjustments
/// of another channel and for fills that leave the channel off.
fn content_row_blend(e: &ContentEffect, active: ChannelKind) -> Option<(BlendMode, f32)> {
    let shown = match &e.kind {
        ContentEffectKind::Anchor => false,
        ContentEffectKind::Levels { .. } | ContentEffectKind::Filter { .. } => e.applies_to(active),
        ContentEffectKind::Fill(fill) => fill.channels.get(&active).is_some_and(|c| c.enabled),
        ContentEffectKind::Paint { .. } => true,
        ContentEffectKind::Generator(_) => active != ChannelKind::Normal,
    };
    let blend = e.blend.get(&active)?;
    shown.then_some((blend.mode, blend.opacity))
}

/// The command that stores a new row blend mode and / or opacity (content: on `active`).
fn set_row_blend(layer: &Layer, mask: bool, id: EffectId, active: ChannelKind, mode: Option<BlendMode>, opacity: Option<f32>) -> Option<Command> {
    if mask {
        let mut e = (**layer.mask.as_ref()?.effects.iter().find(|e| e.id == id)?).clone();
        e.blend = mode.unwrap_or(e.blend);
        e.opacity = opacity.unwrap_or(e.opacity).clamp(0.0, 1.0);
        Some(Command::SetMaskEffect { layer: layer.id, effect: e })
    } else {
        let mut e = (**layer.content_effects.iter().find(|e| e.id == id)?).clone();
        let blend = e.blend.get_mut(&active)?;
        blend.mode = mode.unwrap_or(blend.mode);
        blend.opacity = opacity.unwrap_or(blend.opacity).clamp(0.0, 1.0);
        Some(Command::SetContentEffect { layer: layer.id, effect: e })
    }
}

/// Blend mode and opacity at the right of an effect row, like a layer row's.
#[allow(clippy::too_many_arguments)]
fn row_blend(ui: &mut Ui, layer: &Layer, mask: bool, id: EffectId, active: ChannelKind, mode: BlendMode, opacity: f32, cmds: &mut Vec<Command>) {
    ui.spacing_mut().interact_size.y = 18.0;
    ui.spacing_mut().button_padding = egui::vec2(3.0, 1.0);
    let inactive = &mut ui.visuals_mut().widgets.inactive;
    inactive.bg_fill = egui::Color32::TRANSPARENT;
    inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    inactive.bg_stroke = egui::Stroke::NONE;
    let what = if mask { "Effect".to_string() } else { format!("Effect {}", active.display_name()) };
    let modes: Vec<BlendMode> = BlendMode::available_for_effect(!mask && active == ChannelKind::Normal).collect();
    let combo = egui::ComboBox::from_id_salt("effect_row_blend")
        .width(60.0)
        .height(420.0)
        .selected_text(RichText::new(mode.short_label()).font(theme::font(10.5)).color(TEXT_DIM()))
        .show_ui(ui, |ui| super::layers::blend_mode_list(ui, modes.iter().copied(), mode));
    let picked = combo.inner.flatten();
    let stepped = super::layers::blend_step(ui, &combo.response, egui::Id::new(("effect_blend_focus", id)), &modes, mode, picked.is_some());
    if let Some(m) = picked.or(stepped) {
        cmds.extend(set_row_blend(layer, mask, id, active, Some(m), None));
    }
    combo.response.on_hover_text(format!("{what} blend mode: {}. Click, then Up / Down or the wheel steps through the modes.", mode.label()));
    let mut pct = opacity * 100.0;
    let drag = egui::DragValue::new(&mut pct).range(0.0..=100.0).speed(0.5).suffix("%").max_decimals(0);
    if ui.add_sized([40.0, 18.0], drag).on_hover_text(format!("{what} opacity (drag)")).changed() {
        cmds.extend(set_row_blend(layer, mask, id, active, None, Some(pct / 100.0)));
    }
}

/// The name being typed into an effect row.
#[derive(Clone)]
struct EffectRename {
    effect: EffectId,
    text: String,
    focus: bool,
}

fn rename_id() -> egui::Id {
    egui::Id::new("effect_inline_rename")
}

/// Delete (`remove`) or Ctrl+D on the selected effect(s). False when no effect is selected, so
/// the key falls through to the layer.
pub fn selection_action(state: &mut UiState, remove: bool, now: f64) -> bool {
    let (Some(layer), Some(effect)) = (state.session.selection().layer, state.session.selected_effect()) else { return false };
    let target = state.session.layer_target();
    let effects = state.effect_multi.targets((layer, target), Some(effect));
    state.effect_multi.clear();
    state.run(vec![if remove { Command::RemoveEffects { layer, target, effects } } else { Command::DuplicateEffects { layer, target, effects } }], now);
    true
}

/// Nested rows under the selected layer for its selected domain's effect stack.
#[allow(clippy::too_many_arguments)]
pub fn rows(
    ui: &mut Ui,
    layer: &Layer,
    depth: usize,
    target: LayerTarget,
    selected: Option<EffectId>,
    multi: &mut EffectSelection,
    channels: &[ChannelKind],
    active: ChannelKind,
    resolution: u32,
    previews: &mut crate::layer_previews::PreviewUi,
    session: &subcraft_app::Session,
    preview_size: f32,
    cmds: &mut Vec<Command>,
) {
    let mask_domain = target == LayerTarget::Mask && layer.mask.is_some();
    let rows: Vec<Row> = if mask_domain {
        let mask = layer.mask.as_ref().unwrap();
        mask.effects
            .iter()
            .map(|e| Row {
                id: e.id,
                name: e.name.clone(),
                visible: e.visible,
                icon: mask_icon(&e.kind),
                blend: (!matches!(e.kind, MaskEffectKind::Anchor)).then_some((e.blend, e.opacity)),
            })
            .collect()
    } else {
        layer
            .content_effects
            .iter()
            .map(|e| Row { id: e.id, name: e.name.clone(), visible: e.visible, icon: content_icon(&e.kind), blend: content_row_blend(e, active) })
            .collect()
    };
    let indent = depth as f32 * 16.0 + 54.0;
    let domain = if mask_domain { LayerTarget::Mask } else { LayerTarget::Content };
    let stack = (layer.id, domain);
    let ids: Vec<EffectId> = rows.iter().map(|r| r.id).collect();
    // Every selected effect of this stack: the session's one plus Ctrl / Shift click extras.
    let chosen = multi.targets(stack, selected);
    if !chosen.is_empty() && !ui.ctx().egui_wants_keyboard_input() {
        for (key, up) in [(egui::Key::ArrowUp, true), (egui::Key::ArrowDown, false)] {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, key)) {
                cmds.push(Command::NudgeEffects { layer: layer.id, target: domain, effects: chosen.clone(), up });
            }
        }
    }
    let mut rename = ui.ctx().data(|d| d.get_temp::<EffectRename>(rename_id())).filter(|r| ids.contains(&r.effect));
    // Some(None): cancelled; Some(Some(name)): confirmed.
    let mut rename_done: Option<Option<String>> = None;
    ui.push_id(("effects", layer.id, mask_domain), |ui| {
        let open_id = ui.make_persistent_id("expanded");
        let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(true);
        let header = ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.spacing_mut().item_spacing.x = 4.0;
            let caret = if open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
            if ui.add(egui::Button::new(RichText::new(caret).small().color(TEXT_FAINT())).frame(false)).clicked() { open = !open; }
            let title = if mask_domain { "Mask effects" } else { "Content effects" };
            if ui.add(egui::Button::new(RichText::new(title).small().color(TEXT_DIM())).frame(false)).clicked() { open = !open; }
            ui.label(RichText::new(rows.len().to_string()).small().color(TEXT_FAINT()));
            ui.menu_button(RichText::new(format!("{} Add", icons::PLUS)).small(), |ui| {
                add_effect_menu(ui, layer, mask_domain, channels, active, resolution, session, cmds)
            })
            .response
            .on_hover_text("Add an effect above the selected one, or on top of this stack");
        }).response;
        ui.data_mut(|d| d.insert_temp(open_id, open));
        if !open { return; }
        let count = rows.len();
        let alt = ui.input(|i| i.modifiers.alt);
        // Top of the stack first, like layers.
        for (index, row) in rows.iter().enumerate().rev() {
            let is_primary = selected == Some(row.id);
            let is_selected = chosen.contains(&row.id);
            let renaming = rename.as_ref().is_some_and(|r| r.effect == row.id);
            let resp = ui.push_id(row.id, |ui| {
                striped_list_row_height(ui, count - 1 - index, is_selected, (preview_size + 6.0).max(theme::EFFECT_ROW_HEIGHT), egui::Sense::click_and_drag(), |ui| {
                    ui.add_space(indent);
                    ui.spacing_mut().item_spacing.x = 5.0;
                    let (eye, tip) = if row.visible { (icons::EYE, "Hide effect") } else { (icons::EYE_SLASH, "Show effect") };
                    let eye = ui.add_sized([20.0, 22.0], egui::Button::new(RichText::new(eye).color(TEXT_FAINT())).frame(false)).on_hover_text(tip);
                    if super::layers::eye_slide(ui, &eye, row.visible) {
                        cmds.push(visibility(layer, mask_domain, row.id, !row.visible));
                    }
                    let thumb = previews.image(ui, session, subcraft_app::layer_previews::PreviewKey {
                        layer:layer.id,channel:active,mask:mask_domain,effect:Some(row.id),size:64
                    }, preview_size, is_selected).on_hover_text("Result through this effect, including the steps below it. Click to edit. Hidden effects show the bypassed result.");
                    if !row.visible {ui.painter().line_segment([thumb.rect.left_bottom(),thumb.rect.right_top()],egui::Stroke::new(1.0,TEXT_FAINT()));}
                    if thumb.clicked() {
                        multi.clear();
                        cmds.push(Command::SelectEffect {layer:layer.id,target:domain,effect:Some(row.id)});
                    }
                    let color = if is_selected { ACCENT() } else { TEXT_DIM() };
                    ui.label(RichText::new(row.icon).font(theme::font(13.0)).color(color));
                    // Blend and opacity sit at the right when the row is wide enough for them.
                    // Room for the blend dropdown, the opacity box and their spacing.
                    const CONTROLS: f32 = 128.0;
                    let controls = row.blend.filter(|_| !renaming && ui.available_width() >= CONTROLS + 48.0);
                    let name_width = (ui.available_width() - if controls.is_some() { CONTROLS + 8.0 } else { 0.0 }).max(20.0);
                    ui.allocate_ui_with_layout(egui::vec2(name_width, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_width(name_width);
                        if let Some(edit) = rename.as_mut().filter(|r| r.effect == row.id) {
                            let r = ui.add(egui::TextEdit::singleline(&mut edit.text).id_salt("inline_name").font(theme::font(11.5)).desired_width(name_width));
                            if edit.focus { r.request_focus(); edit.focus = false; }
                            else if ui.input(|i| i.key_pressed(egui::Key::Escape)) { rename_done = Some(None); }
                            else if r.lost_focus() || (r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                rename_done = Some(Some(edit.text.trim().to_string()));
                            }
                        } else {
                            let text = row_text(&row.name, is_selected || row.visible).font(theme::font(11.5));
                            ui.add(egui::Label::new(if row.visible { text } else { text.color(TEXT_FAINT()) }).truncate().selectable(false));
                        }
                    });
                    if let Some((mode, opacity)) = controls {
                        row_blend(ui, layer, mask_domain, row.id, active, mode, opacity, cmds);
                    }
                })
            })
            .inner;
            let address=EffectAddress {texture_set:session.selection().texture_set,layer:layer.id,domain,effect:row.id};
            if !renaming && !super::layers::eye_sliding(ui) { resp.dnd_set_drag_payload(address); }
            effect_drop(ui,&resp,address,&ids,&chosen,indent,cmds);
            shelf_effect_drop(ui, &resp, layer.id, domain, &ids, row.id, indent);
            let x = resp.rect.left() + indent - 2.0;
            let y = resp.rect.center().y;
            let bottom = if index == 0 { y } else { resp.rect.bottom() + 1.0 };
            ui.painter().line_segment([egui::pos2(x, resp.rect.top() - 1.0), egui::pos2(x, bottom)], egui::Stroke::new(1.0, theme::BORDER_STRONG()));
            ui.painter().line_segment([egui::pos2(x, y), egui::pos2(x + 7.0, y)], egui::Stroke::new(1.0, theme::BORDER_STRONG()));
            if resp.double_clicked() && !alt {
                // Double-click renames (Painter); the row ends up selected whatever the first click did.
                multi.clear();
                cmds.push(Command::SelectEffect { layer: layer.id, target: domain, effect: Some(row.id) });
                rename = Some(EffectRename { effect: row.id, text: row.name.clone(), focus: true });
            } else if resp.clicked() {
                let (toggle, range) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
                if toggle || range {
                    let primary = multi.click(stack, &ids, selected, row.id, toggle, range);
                    if primary != selected {
                        cmds.push(Command::SelectEffect { layer: layer.id, target: domain, effect: primary });
                    }
                } else {
                    // A plain click on the only selected effect returns to the layer.
                    let effect = if is_primary && chosen.len() == 1 { None } else { Some(row.id) };
                    multi.clear();
                    cmds.push(Command::SelectEffect { layer: layer.id, target: domain, effect });
                }
            }
            // The menu acts on the selection when the row is part of it, else on this row alone,
            // which then becomes the selection (new effects are added above it).
            if resp.secondary_clicked() && !is_selected {
                multi.clear();
                cmds.push(Command::SelectEffect { layer: layer.id, target: domain, effect: Some(row.id) });
            }
            let acted: Vec<EffectId> = if is_selected { chosen.clone() } else { vec![row.id] };
            resp.context_menu(|ui| {
                if actions(ui, layer, mask_domain, &acted, &ids, channels, active, resolution, session, cmds) {
                    rename = Some(EffectRename { effect: row.id, text: row.name.clone(), focus: true });
                }
            });
            if !renaming {
                resp.on_hover_text("Click to edit in Properties (click again to return to the layer). Ctrl / Shift click selects several. Double-click renames. Drag to reorder or onto another layer; hold Alt to copy. Alt+Up / Alt+Down move it. Right-click for more.");
            }
        }
        if rows.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(indent);
                ui.label(RichText::new("No effects yet").small().color(TEXT_FAINT()));
            });
        } else {
            let x = header.rect.left() + indent - 2.0;
            ui.painter().line_segment([egui::pos2(x, header.rect.center().y), egui::pos2(x, header.rect.bottom())], egui::Stroke::new(1.0, theme::BORDER_STRONG()));
        }
    });
    match rename_done {
        Some(done) => {
            if let (Some(name), Some(edit)) = (done, &rename)
                && !name.is_empty()
                && rows.iter().any(|r| r.id == edit.effect && r.name != name)
            {
                cmds.push(Command::RenameEffect { layer: layer.id, target: domain, effect: edit.effect, name });
            }
            ui.ctx().data_mut(|d| d.remove::<EffectRename>(rename_id()));
        }
        None => ui.ctx().data_mut(|d| {
            if let Some(edit) = rename { d.insert_temp(rename_id(), edit); } else { d.remove::<EffectRename>(rename_id()); }
        }),
    }
}

/// A Shelf item released between the effect rows of a stack: where it goes. Read by the Layers
/// panel, which has the library ([`shelf_drop_request_id`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShelfEffectDrop {
    pub layer: LayerId,
    pub domain: LayerTarget,
    /// The new effect goes right above this one; `None`: at the bottom of the stack.
    pub above: Option<EffectId>,
    pub item: crate::panels::shelf::ShelfDrag,
}

pub fn shelf_drop_request_id() -> egui::Id {
    egui::Id::new("shelf_effect_drop")
}

/// Where a drop on the row of `target` lands: right above it, or right above the effect below
/// it (`None` at the bottom). `ids` is the stack bottom to top.
pub fn shelf_drop_anchor(ids: &[EffectId], target: EffectId, side: DropSide) -> Option<EffectId> {
    match side {
        DropSide::Above => Some(target),
        DropSide::Below => ids.iter().position(|&i| i == target).and_then(|i| i.checked_sub(1)).map(|i| ids[i]),
    }
}

#[cfg(test)]
mod shelf_drop_tests {
    use super::*;
    use crate::panels::shelf::ShelfDrag;

    #[test]
    fn a_drop_above_or_below_a_row_names_the_effect_it_goes_above() {
        let ids = [EffectId::new(), EffectId::new(), EffectId::new()];
        assert_eq!(shelf_drop_anchor(&ids, ids[1], DropSide::Above), Some(ids[1]));
        assert_eq!(shelf_drop_anchor(&ids, ids[1], DropSide::Below), Some(ids[0]));
        assert_eq!(shelf_drop_anchor(&ids, ids[0], DropSide::Below), None, "below the bottom row: the bottom of the stack");
        assert_eq!(shelf_drop_anchor(&ids, ids[2], DropSide::Above), Some(ids[2]));
    }

    #[test]
    fn mask_stacks_take_alphas_images_and_smart_masks_and_content_stacks_take_filters() {
        assert!(shelf_item_fits(ShelfDrag::Alpha(0), true));
        assert!(!shelf_item_fits(ShelfDrag::Alpha(0), false));
        assert!(shelf_item_fits(ShelfDrag::SmartMask(0), true));
        assert!(shelf_item_fits(ShelfDrag::Texture(0), true));
        assert!(shelf_item_fits(ShelfDrag::Filter(0), false));
        assert!(!shelf_item_fits(ShelfDrag::Material(0), true));
        assert!(!shelf_item_fits(ShelfDrag::Material(0), false));
        if let Some(i) = subcraft_doc::Filter::ALL.iter().position(|f| !f.supports_mask()) {
            assert!(!shelf_item_fits(ShelfDrag::Filter(i), true));
        }
    }
}

/// Whether a Shelf item can become an effect of this kind of stack.
pub fn shelf_item_fits(item: crate::panels::shelf::ShelfDrag, mask: bool) -> bool {
    use crate::panels::shelf::ShelfDrag;
    match item {
        ShelfDrag::Filter(i) => subcraft_doc::Filter::ALL.get(i).is_some_and(|f| !mask || f.supports_mask()),
        ShelfDrag::Alpha(_) | ShelfDrag::Texture(_) | ShelfDrag::SmartMask(_) => mask,
        ShelfDrag::Material(_) | ShelfDrag::SmartMaterial(_) | ShelfDrag::PackMaterial(_) | ShelfDrag::Starter(_) => false,
    }
}

/// A Shelf item dragged over an effect row: an insert line, and on release a request.
fn shelf_effect_drop(ui: &Ui, response: &egui::Response, layer: LayerId, domain: LayerTarget, ids: &[EffectId], target: EffectId, indent: f32) {
    use crate::panels::shelf::ShelfDrag;
    let Some(item) = egui::DragAndDrop::payload::<ShelfDrag>(ui.ctx()).map(|p| *p) else { return };
    if !shelf_item_fits(item, domain == LayerTarget::Mask) || !response.contains_pointer() {
        return;
    }
    let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) else { return };
    let side = if pointer.y < response.rect.center().y { DropSide::Above } else { DropSide::Below };
    let y = if side == DropSide::Above { response.rect.top() } else { response.rect.bottom() };
    ui.painter().line_segment([egui::pos2(response.rect.left() + indent, y), egui::pos2(response.rect.right() - 4.0, y)], egui::Stroke::new(2.0, ACCENT()));
    if ui.input(|i| i.pointer.any_released()) {
        let drop = ShelfEffectDrop { layer, domain, above: shelf_drop_anchor(ids, target, side), item };
        ui.ctx().data_mut(|d| d.insert_temp(shelf_drop_request_id(), drop));
    }
}

/// The effects a drag of `source` carries: the whole selection when the grabbed row is part of
/// it, else that row alone.
fn dragged(source: EffectAddress, chosen: &[EffectId]) -> Vec<EffectId> {
    if chosen.contains(&source.effect) { chosen.to_vec() } else { vec![source.effect] }
}

/// Drop on an effect row of the stack the drag came from: reorder, or copy with Alt held.
fn effect_drop(ui:&Ui, response:&egui::Response, target:EffectAddress, ids:&[EffectId], chosen:&[EffectId], indent:f32, commands:&mut Vec<Command>) {
    let Some(pointer)=ui.input(|i|i.pointer.interact_pos()) else {return;};
    let side=if pointer.y<response.rect.center().y {DropSide::Above}else{DropSide::Below};
    let copy=ui.input(|i|i.modifiers.alt);
    let valid=|source:EffectAddress| {
        source.texture_set==target.texture_set && source.layer==target.layer && source.domain==target.domain && ids.contains(&source.effect)
            && (copy || reordered(ids,&dragged(source,chosen),target.effect,side).is_some())
    };
    if let Some(source)=response.dnd_hover_payload::<EffectAddress>() && valid(*source) {
        let y=if side==DropSide::Above {response.rect.top()}else{response.rect.bottom()};
        ui.painter().line_segment([egui::pos2(response.rect.left()+indent,y),egui::pos2(response.rect.right()-4.0,y)],egui::Stroke::new(2.0,ACCENT()));
    }
    // A wrong-type release must never consume a layer or shelf payload.
    if egui::DragAndDrop::has_payload_of_type::<EffectAddress>(ui.ctx())
        && let Some(source)=response.dnd_release_payload::<EffectAddress>() && valid(*source) {
        commands.push(Command::DropEffects {source:*source,effects:dragged(*source,chosen),to:EffectDrop::Beside {effect:target.effect,side},copy});
    }
}

/// Drop of dragged effects on a layer row: they go on top of that layer's stack of the same
/// kind (Alt: copies). Called for every layer row of the panel.
pub(crate) fn layer_row_drop(ui: &Ui, response: &egui::Response, layer: &Layer, session: &subcraft_app::Session, multi: &EffectSelection, cmds: &mut Vec<Command>) {
    if !egui::DragAndDrop::has_payload_of_type::<EffectAddress>(ui.ctx()) { return; }
    let copy = ui.input(|i| i.modifiers.alt);
    let valid = |source: EffectAddress| source.texture_set == session.selection().texture_set && session.selected_texture_set().find(source.layer).is_some();
    if let Some(source) = response.dnd_hover_payload::<EffectAddress>() && valid(*source) {
        let r = response.rect;
        ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.5, ACCENT()), egui::StrokeKind::Inside);
        let verb = if copy { "Copy" } else { "Move" };
        let place = match (source.domain, source.layer == layer.id) {
            (_, true) => "to the top of the stack".to_string(),
            (LayerTarget::Mask, false) => format!("to the mask of {}", layer.name),
            (LayerTarget::Content, false) => format!("to {}", layer.name),
        };
        let galley = ui.painter().layout_no_wrap(format!("{verb} {place}"), theme::font(11.0), theme::TEXT());
        let pill = egui::Rect::from_min_size(egui::pos2((r.right() - galley.size().x - 14.0).max(r.left()), r.top() + 2.0), galley.size() + egui::vec2(8.0, 4.0));
        ui.painter().rect_filled(pill, 3.0, theme::ACCENT_SOFT());
        ui.painter().galley(pill.min + egui::vec2(4.0, 2.0), galley, theme::TEXT());
    }
    if let Some(source) = response.dnd_release_payload::<EffectAddress>() && valid(*source) {
        let chosen = multi.targets((source.layer, source.domain), session.selected_effect());
        cmds.push(Command::DropEffects { source: *source, effects: dragged(*source, &chosen), to: EffectDrop::Layer(layer.id), copy });
    }
}

fn visibility(layer: &Layer, mask: bool, id: EffectId, visible: bool) -> Command {
    if mask {
        let e = layer.mask.as_ref().and_then(|m| m.effects.iter().find(|e| e.id == id)).expect("row comes from the stack");
        let mut e = (**e).clone();
        e.visible = visible;
        Command::SetMaskEffect { layer: layer.id, effect: e }
    } else {
        let e = layer.content_effects.iter().find(|e| e.id == id).expect("row comes from the stack");
        let mut e = (**e).clone();
        e.visible = visible;
        Command::SetContentEffect { layer: layer.id, effect: e }
    }
}

/// Right-click menu of an effect row: clipboard, duplicate, remove, move, add above, rename.
/// `effects` is what the entries act on (the selection, or just this row); `ids` is the stack
/// bottom to top. Returns true when Rename was picked.
#[allow(clippy::too_many_arguments)]
fn actions(ui: &mut Ui, layer: &Layer, mask: bool, effects: &[EffectId], ids: &[EffectId], channels: &[ChannelKind], active: ChannelKind, resolution: u32, session: &subcraft_app::Session, cmds: &mut Vec<Command>) -> bool {
    use crate::clipboard::{Request, request};
    let target = if mask { LayerTarget::Mask } else { LayerTarget::Content };
    let id = layer.id;
    let many = effects.len() > 1;
    let suffix = if many { format!(" {} effects", effects.len()) } else { String::new() };
    let key = |text: &'static str| crate::keys::native(text);
    if ui.add(egui::Button::new(format!("Cut{suffix}")).shortcut_text(key("Ctrl+X"))).clicked() {
        request(ui.ctx(), Request::CopyEffects { layer: id, target, effects: effects.to_vec(), cut: true });
        ui.close();
    }
    if ui.add(egui::Button::new(format!("{}  Copy{suffix}", icons::COPY)).shortcut_text(key("Ctrl+C"))).clicked() {
        request(ui.ctx(), Request::CopyEffects { layer: id, target, effects: effects.to_vec(), cut: false });
        ui.close();
    }
    let have_clip = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("effect_clip_present"))).unwrap_or(false);
    if ui.add_enabled(have_clip, egui::Button::new("Paste").shortcut_text(key("Ctrl+V"))).on_hover_text("Adds the copied effect(s) above the selected effect of the matching stack").clicked() {
        request(ui.ctx(), Request::PasteEffects { layer: id });
        ui.close();
    }
    if ui.add(egui::Button::new(format!("Duplicate{suffix}")).shortcut_text(key("Ctrl+D"))).clicked() {
        cmds.push(Command::DuplicateEffects { layer: id, target, effects: effects.to_vec() });
        ui.close();
    }
    if ui.add(egui::Button::new(format!("{}  Remove{suffix}", icons::TRASH)).shortcut_text("Del")).clicked() {
        cmds.push(Command::RemoveEffects { layer: id, target, effects: effects.to_vec() });
        ui.close();
    }
    ui.separator();
    for (label, icon, keys, up) in [("Move up", icons::ARROW_UP, "Alt+Up", true), ("Move down", icons::ARROW_DOWN, "Alt+Down", false)] {
        let can = subcraft_app::effect_stack::nudged(ids, effects, up).is_some();
        if ui.add_enabled(can, egui::Button::new(format!("{icon}  {label}")).shortcut_text(key(keys))).clicked() {
            cmds.push(Command::NudgeEffects { layer: id, target, effects: effects.to_vec(), up });
            ui.close();
        }
    }
    ui.separator();
    ui.menu_button(format!("{}  Add effect above", icons::PLUS), |ui| add_effect_menu(ui, layer, mask, channels, active, resolution, session, cmds));
    let mut rename = false;
    if ui.add_enabled(!many, egui::Button::new("Rename")).clicked() {
        rename = true;
        ui.close();
    }
    rename
}

/// The channels a filter processes after a click on one of its channel chips (`states`: every
/// channel it supports and whether it is on). Alt (`solo`) keeps only the clicked one; the last
/// channel cannot be switched off.
pub(crate) fn filter_channels_after_toggle(states: &[(ChannelKind, bool)], kind: ChannelKind, solo: bool) -> Vec<ChannelKind> {
    let mut on: Vec<ChannelKind> = states.iter().filter(|s| s.1).map(|s| s.0).collect();
    if solo {
        return vec![kind];
    }
    if on.contains(&kind) {
        if on.len() > 1 {
            on.retain(|k| *k != kind);
        }
    } else {
        on.push(kind);
    }
    on
}

/// Properties of the selected effect. Returns false when there is none to show.
pub fn properties(state: &mut UiState, ui: &mut Ui) -> bool {
    let Some(effect) = state.session.selected_effect() else { return false };
    let Some(layer) = state.session.selected_layer().cloned() else { return false };
    let target = state.session.layer_target();
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    egui::ScrollArea::vertical().id_salt(("effect_properties", effect)).auto_shrink([false, false]).show(ui, |ui| {
        panel(ui, |ui| {
            if target == LayerTarget::Mask {
                let Some(e) = layer.mask.as_ref().and_then(|m| m.effects.iter().find(|e| e.id == effect)).cloned() else { return };
                header(ui, &layer, &e.name, target, &mut cmds);
                if matches!(e.kind, MaskEffectKind::Paint { .. }) {
                    hint(ui, "The brush paints into this effect (white reveals, black hides). Paint value is in Brush › Material.");
                }
                super::properties::effect_editor(ui, state, layer.id, &e, &mut cmds);
            } else {
                let Some(e) = layer.content_effects.iter().find(|e| e.id == effect).cloned() else { return };
                header(ui, &layer, &e.name, target, &mut cmds);
                content_editor(ui, state, &layer, &e, &mut cmds);
            }
        });
    });
    state.run(cmds, now);
    true
}

fn header(ui: &mut Ui, layer: &Layer, name: &str, target: LayerTarget, cmds: &mut Vec<Command>) {
    let domain = if target == LayerTarget::Mask { "Mask effect" } else { "Effect" };
    ui.label(RichText::new(format!("{} · {domain}: {name}", layer.name)).font(theme::font_strong(14.0)).color(theme::TEXT()));
    let back = if target == LayerTarget::Mask { "Back to mask" } else { "Back to layer" };
    if secondary_button(ui, back).clicked() {
        cmds.push(Command::SelectEffect { layer: layer.id, target, effect: None });
    }
    ui.add_space(4.0);
}

fn content_editor(ui: &mut Ui, state: &mut UiState, layer: &Layer, e: &ContentEffect, cmds: &mut Vec<Command>) {
    let mut eff = e.clone();
    let default_name = eff.name == eff.kind.label();
    let mut changed = false;
    let channels: Vec<ChannelKind> = state.session.selected_texture_set().channel_kinds().collect();
    section(ui, "effect_general", "Effect", true, |ui| {
        changed |= ui.checkbox(&mut eff.visible, "Visible").changed();
        prop_row(ui, "Name", |ui| {
            // The text being typed lives in egui memory while the field has focus: rebuilt from the
            // effect every frame, each typed character was gone by the next frame and the field
            // could not be edited at all.
            let name_id = egui::Id::new(("content_effect_name", eff.id));
            let mut name: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_else(|| eff.name.clone());
            let r = ui.text_edit_singleline(&mut name);
            if r.lost_focus() && !name.trim().is_empty() && name.trim() != eff.name {
                eff.name = name.trim().to_string();
                changed = true;
            }
            if r.has_focus() { ui.data_mut(|d| d.insert_temp(name_id, name.clone())); } else { ui.data_mut(|d| d.remove_temp::<String>(name_id)); }
        });
        match &mut eff.kind {
            ContentEffectKind::Paint { channels: painted } => {
                hint(ui, "The brush paints into this effect while it is selected, on paint and fill layers alike. Erasing removes only this effect's paint.");
                ui.horizontal_wrapped(|ui| {
                    if painted.is_empty() {
                        ui.label(RichText::new("Nothing painted yet.").small().color(TEXT_FAINT()));
                    }
                    for k in painted.keys() {
                        badge(ui, k.display_name(), TEXT_DIM());
                    }
                });
            }
            ContentEffectKind::Fill(fill) => {
                hint(ui, "A material on top of the layer's content. Enable the channels it should change.");
                changed |= super::properties::projection_editor(ui, &mut fill.projection);
                changed |= super::properties::projection_options(ui, &mut fill.projection);
                let states: Vec<_> = channels.iter().map(|k| (*k, fill.channels.get(k).is_some_and(|c| c.enabled))).collect();
                if let Some((channel, solo)) = super::properties::channel_toggles(ui, &states) {
                    cmds.push(Command::ToggleFillChannel { layer: layer.id, effect: Some(eff.id), channel, solo });
                }
                for kind in &channels {
                    let mut ch = fill.channels.get(kind).cloned().unwrap_or(FillChannel::disabled(FillSource::uniform(kind.default_paint_value())));
                    if !ch.enabled { continue; }
                    let before = ch.clone();
                    ui.push_id(("effect_fill", *kind), |ui| {
                        ui.label(RichText::new(kind.display_name()).color(theme::TEXT()));
                        if ch.enabled {
                            let current = match ch.source {
                                FillSource::Procedural { .. } => 1,
                                // Fill effects offer Uniform / Procedural only (anchors: fill layers).
                                _ => 0,
                            };
                            let mut sel = current;
                            prop_row(ui, "Source", |ui| {
                                egui::ComboBox::from_id_salt("source").selected_text(["Uniform", "Procedural"][current]).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut sel, 0, "Uniform");
                                    ui.selectable_value(&mut sel, 1, "Procedural");
                                });
                            });
                            if sel != current {
                                ch.source = if sel == 0 {
                                    FillSource::uniform(kind.default_paint_value())
                                } else {
                                    FillSource::Procedural { params: ProceduralParams::new(ProceduralKind::Clouds), low: ChannelValue::scalar(0.0), high: ChannelValue::scalar(1.0) }
                                };
                            }
                            // Mesh-map and image sources stay on fill layers for now.
                            if matches!(ch.source, FillSource::Uniform { .. } | FillSource::Procedural { .. }) {
                                super::properties::source_controls(ui, state, *kind, &mut ch);
                            }
                        }
                    });
                    if ch != before {
                        fill.channels.insert(*kind, ch);
                        changed = true;
                    }
                }
            }
            ContentEffectKind::Levels { channel, levels } => {
                hint(ui, "Remaps one channel of the content below. Transparent areas stay transparent.");
                prop_row(ui, "Channel", |ui| {
                    egui::ComboBox::from_id_salt("levels_channel").selected_text(channel.display_name()).show_ui(ui, |ui| {
                        for k in channels.iter().filter(|k| **k != ChannelKind::Normal) {
                            changed |= ui.selectable_value(channel, *k, k.display_name()).changed();
                        }
                    });
                });
                let key = subcraft_app::layer_previews::PreviewKey { layer: layer.id, channel: *channel, mask: false, effect: Some(eff.id), size: 0 };
                changed |= super::properties::levels_editor(ui, state, key, levels, channel.components() >= 3);
            }
            ContentEffectKind::Generator(g) => {
                hint(ui, "The generator's gray result goes into every channel but Normal; set each channel's blending below.");
                changed |= super::properties::content_generator_controls(ui, state, g);
            }
            ContentEffectKind::Anchor => {
                let index = subcraft_doc::AnchorIndex::build(&state.session.selected_texture_set().layers);
                super::anchor::anchor_info(ui, &index, eff.id, false);
            }
            ContentEffectKind::Filter { channel, filter } => {
                hint(ui, "Adjusts the preceding effect stage. Blur filters soften coverage; others preserve it.");
                changed |= super::filter::editor(ui, filter, Some(*channel));
                if super::properties::filter_image_row(ui, state, filter, &mut changed) {
                    crate::files::pick(ui.ctx(), crate::files::Purpose::FilterImage { layer: layer.id, effect: eff.id, mask: false }, crate::files::IMAGES, false, None);
                }
                // Painter's per-channel toggles: every channel the filter processes.
                let states: Vec<(ChannelKind, bool)> = channels
                    .iter()
                    .filter(|k| filter.supports_channel(**k))
                    .map(|k| (*k, *k == *channel || eff.extra_channels.contains(k)))
                    .collect();
                ui.label(RichText::new("Channels").small().color(TEXT_DIM()));
                if let Some((kind, solo)) = super::properties::channel_toggles(ui, &states) {
                    let on = filter_channels_after_toggle(&states, kind, solo);
                    if !on.contains(channel) {
                        *channel = on[0];
                    }
                    eff.extra_channels = on.into_iter().filter(|k| k != channel).collect();
                    changed = true;
                }
            }
        }
    });
    if !matches!(eff.kind, ContentEffectKind::Anchor) { section(ui, "effect_blend", "Blending per channel", true, |ui| {
        hint(ui, "How this effect combines with the content below it, per channel.");
        for kind in &channels {
            if matches!(eff.kind, ContentEffectKind::Filter { .. } | ContentEffectKind::Levels { .. }) && !eff.applies_to(*kind) { continue; }
            if let ContentEffectKind::Fill(fill) = &eff.kind && !fill.channels.get(kind).is_some_and(|c| c.enabled) { continue; }
            let Some(blend) = eff.blend.get_mut(kind) else { continue };
            ui.push_id(("effect_blend", *kind), |ui| {
                prop_row(ui, kind.display_name(), |ui| {
                    egui::ComboBox::from_id_salt("mode").width(96.0).selected_text(blend.mode.label()).show_ui(ui, |ui| {
                        for mode in BlendMode::available_for_effect(*kind == ChannelKind::Normal) {
                            changed |= ui.selectable_value(&mut blend.mode, mode, mode.label()).changed();
                        }
                    });
                    // Fit the slider and its value box in the remaining row width.
                    ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(40.0);
                    changed |= ui.add(egui::Slider::new(&mut blend.opacity, 0.0..=1.0).show_value(true)).changed();
                });
            });
        }
    }); }
    ui.horizontal_wrapped(|ui| {
        if secondary_button(ui, format!("{}  Duplicate", icons::COPY)).clicked() {
            cmds.push(Command::DuplicateContentEffect { layer: layer.id, effect: e.id });
        }
        if secondary_button(ui, format!("{}  Delete", icons::TRASH)).clicked() {
            cmds.push(Command::RemoveContentEffect { layer: layer.id, effect: e.id });
        }
    });
    if changed {
        if default_name && eff.name == e.name {
            eff.name = eff.kind.label();
        }
        cmds.push(Command::SetContentEffect { layer: layer.id, effect: eff });
    }
}

#[cfg(test)]
mod filter_channel_tests {
    use super::*;
    use ChannelKind::{BaseColor as C, Metallic as M, Roughness as R};

    #[test]
    fn channel_chips_toggle_solo_and_never_leave_a_filter_without_a_channel() {
        let states = [(C, true), (R, false), (M, true)];
        assert_eq!(filter_channels_after_toggle(&states, R, false), [C, M, R], "switch one on");
        assert_eq!(filter_channels_after_toggle(&states, C, false), [M], "switch one off");
        assert_eq!(filter_channels_after_toggle(&states, R, true), [R], "Alt: only this one");
        let one = [(C, false), (R, true), (M, false)];
        assert_eq!(filter_channels_after_toggle(&one, R, false), [R], "the last channel stays on");
    }
}
