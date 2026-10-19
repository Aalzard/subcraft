//! Effect stacks in the Layers panel and the selected effect's Properties.
//!
//! The selected layer shows the effects of the domain picked with its thumbnails (content or
//! mask) as nested rows, top of the stack first. Every edit is an app command.
use std::collections::BTreeMap;

use egui::{RichText, Ui};
use subcraft_app::{Command, LayerTarget};
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
}

fn content_icon(kind: &ContentEffectKind) -> &'static str {
    match kind {
        ContentEffectKind::Paint { .. } => icons::PAINT_BRUSH,
        ContentEffectKind::Fill(_) => icons::PAINT_BUCKET,
        ContentEffectKind::Levels { .. } | ContentEffectKind::Filter { .. } => icons::SLIDERS,
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

/// Content effects the Add menu offers for a layer.
fn add_content_menu(ui: &mut Ui, layer: &Layer, channels: &[ChannelKind], active: ChannelKind, cmds: &mut Vec<Command>) {
    let levels_channel = if active == ChannelKind::Normal { ChannelKind::Roughness } else { active };
    let options = [
        ("Paint", ContentEffectKind::Paint { channels: BTreeMap::new() }),
        ("Fill", new_fill(channels.iter().copied())),
        ("Levels", ContentEffectKind::Levels { channel: levels_channel, levels: Levels::default() }),
    ];
    for (label, kind) in options {
        if layer.is_folder() && !kind.is_adjustment() { continue; }
        if ui.button(label).clicked() {
            cmds.push(Command::AddContentEffect { layer: layer.id, kind });
            ui.close();
        }
    }
    ui.menu_button("Filter", |ui| {
        for filter in subcraft_doc::Filter::ALL {
            let channel = if filter.supports_channel(active) { Some(active) }
                else { channels.iter().copied().find(|k| filter.supports_channel(*k)) };
            if ui.add_enabled(channel.is_some(), egui::Button::new(filter.label())).clicked() {
                cmds.push(Command::AddContentEffect { layer: layer.id, kind: ContentEffectKind::Filter { channel: channel.unwrap(), filter } });
                ui.close();
            }
        }
    });
}

pub(crate) fn add_mask_filters(ui: &mut Ui, layer: LayerId, cmds: &mut Vec<Command>) {
    ui.menu_button("Filter", |ui| {
        for filter in subcraft_doc::Filter::ALL.into_iter().filter(|f| f.supports_mask()) {
            if ui.button(filter.label()).clicked() {
                cmds.push(Command::AddMaskEffect { layer, kind: MaskEffectKind::Filter(filter) }); ui.close();
            }
        }
    });
}

fn add_mask_menu(ui: &mut Ui, layer: &Layer, resolution: u32, cmds: &mut Vec<Command>) {
    add_mask_filters(ui, layer.id, cmds);
    let options = [
        ("Paint", MaskEffectKind::Paint { image: TiledImage::new(resolution, resolution, 2, [0.0; 4]) }),
        ("Fill", MaskEffectKind::Fill { value: 1.0, procedural: None, projection: Default::default() }),
        ("Generator", MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges))),
        ("Levels", MaskEffectKind::Levels(Levels::default())),
        ("Invert", MaskEffectKind::Invert),
    ];
    for (label, kind) in options {
        if ui.button(label).clicked() {
            cmds.push(Command::AddMaskEffect { layer: layer.id, kind });
            ui.close();
        }
    }
}

/// Nested rows under the selected layer for its selected domain's effect stack.
#[allow(clippy::too_many_arguments)]
pub fn rows(
    ui: &mut Ui,
    layer: &Layer,
    depth: usize,
    target: LayerTarget,
    selected: Option<EffectId>,
    channels: &[ChannelKind],
    active: ChannelKind,
    resolution: u32,
    cmds: &mut Vec<Command>,
) {
    let mask_domain = target == LayerTarget::Mask && layer.mask.is_some();
    let rows: Vec<Row> = if mask_domain {
        let mask = layer.mask.as_ref().unwrap();
        mask.effects.iter().map(|e| Row { id: e.id, name: e.name.clone(), visible: e.visible, icon: mask_icon(&e.kind) }).collect()
    } else {
        layer.content_effects.iter().map(|e| Row { id: e.id, name: e.name.clone(), visible: e.visible, icon: content_icon(&e.kind) }).collect()
    };
    let indent = depth as f32 * 16.0 + 54.0;
    let domain = if mask_domain { LayerTarget::Mask } else { LayerTarget::Content };
    ui.push_id(("effects", layer.id, mask_domain), |ui| {
        let open_id = ui.make_persistent_id("expanded");
        let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(true);
        let header = ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.spacing_mut().item_spacing.x = 4.0;
            let caret = if open { icons::CARET_DOWN } else { icons::CARET_RIGHT };
            if ui.add(egui::Button::new(RichText::new(caret).small().color(TEXT_FAINT)).frame(false)).clicked() { open = !open; }
            let title = if mask_domain { "Mask effects" } else { "Content effects" };
            if ui.add(egui::Button::new(RichText::new(title).small().color(TEXT_DIM)).frame(false)).clicked() { open = !open; }
            ui.label(RichText::new(rows.len().to_string()).small().color(TEXT_FAINT));
            ui.menu_button(RichText::new(format!("{} Add", icons::PLUS)).small(), |ui| {
                if mask_domain { add_mask_menu(ui, layer, resolution, cmds) } else { add_content_menu(ui, layer, channels, active, cmds) }
            })
            .response
            .on_hover_text("Add an effect on top of this stack");
        }).response;
        ui.data_mut(|d| d.insert_temp(open_id, open));
        if !open { return; }
        let count = rows.len();
        // Top of the stack first, like layers.
        for (index, row) in rows.iter().enumerate().rev() {
            let is_selected = selected == Some(row.id);
            let resp = ui.push_id(row.id, |ui| {
                striped_list_row_height(ui, count - 1 - index, is_selected, theme::EFFECT_ROW_HEIGHT, egui::Sense::click(), |ui| {
                    ui.add_space(indent);
                    ui.spacing_mut().item_spacing.x = 5.0;
                    let (eye, tip) = if row.visible { (icons::EYE, "Hide effect") } else { (icons::EYE_SLASH, "Show effect") };
                    if ui.add_sized([20.0, 22.0], egui::Button::new(RichText::new(eye).color(TEXT_FAINT)).frame(false)).on_hover_text(tip).clicked() {
                        cmds.push(visibility(layer, mask_domain, row.id, !row.visible));
                    }
                    let color = if is_selected { ACCENT } else { TEXT_DIM };
                    ui.label(RichText::new(row.icon).font(theme::font(13.0)).color(color));
                    let text = row_text(&row.name, is_selected || row.visible).font(theme::font(11.5));
                    ui.add(egui::Label::new(if row.visible { text } else { text.color(TEXT_FAINT) }).truncate());
                })
            })
            .inner;
            let x = resp.rect.left() + indent - 2.0;
            let y = resp.rect.center().y;
            let bottom = if index == 0 { y } else { resp.rect.bottom() + 1.0 };
            ui.painter().line_segment([egui::pos2(x, resp.rect.top() - 1.0), egui::pos2(x, bottom)], egui::Stroke::new(1.0, theme::BORDER_STRONG));
            ui.painter().line_segment([egui::pos2(x, y), egui::pos2(x + 7.0, y)], egui::Stroke::new(1.0, theme::BORDER_STRONG));
            if resp.clicked() {
                let effect = if is_selected { None } else { Some(row.id) };
                cmds.push(Command::SelectEffect { layer: layer.id, target: domain, effect });
            }
            resp.context_menu(|ui| actions(ui, layer.id, mask_domain, row.id, index, count, cmds));
            resp.on_hover_text("Click to edit in Properties (click again to return to the layer). Right-click: order, duplicate, delete.");
        }
        if rows.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(indent);
                ui.label(RichText::new("No effects yet").small().color(TEXT_FAINT));
            });
        } else {
            let x = header.rect.left() + indent - 2.0;
            ui.painter().line_segment([egui::pos2(x, header.rect.center().y), egui::pos2(x, header.rect.bottom())], egui::Stroke::new(1.0, theme::BORDER_STRONG));
        }
    });
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

/// Order / duplicate / delete for one effect (`index` is bottom-to-top).
fn actions(ui: &mut Ui, layer: LayerId, mask: bool, effect: EffectId, index: usize, count: usize, cmds: &mut Vec<Command>) {
    let mut push = |c: Command, ui: &mut Ui| {
        cmds.push(c);
        ui.close();
    };
    if ui.add_enabled(index + 1 < count, egui::Button::new(format!("{}  Move up", icons::ARROW_UP))).clicked() {
        let index = index + 1;
        push(if mask { Command::MoveMaskEffect { layer, effect, index } } else { Command::MoveContentEffect { layer, effect, index } }, ui);
    }
    if ui.add_enabled(index > 0, egui::Button::new(format!("{}  Move down", icons::ARROW_DOWN))).clicked() {
        let index = index - 1;
        push(if mask { Command::MoveMaskEffect { layer, effect, index } } else { Command::MoveContentEffect { layer, effect, index } }, ui);
    }
    if ui.button(format!("{}  Duplicate", icons::COPY)).clicked() {
        push(if mask { Command::DuplicateMaskEffect { layer, effect } } else { Command::DuplicateContentEffect { layer, effect } }, ui);
    }
    if ui.button(format!("{}  Delete", icons::TRASH)).clicked() {
        push(if mask { Command::RemoveMaskEffect { layer, effect } } else { Command::RemoveContentEffect { layer, effect } }, ui);
    }
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
    ui.label(RichText::new(format!("{} · {domain}: {name}", layer.name)).font(theme::font_strong(14.0)).color(theme::TEXT));
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
            let mut name = eff.name.clone();
            let r = ui.text_edit_singleline(&mut name);
            if r.lost_focus() && !name.trim().is_empty() && name != eff.name {
                eff.name = name.trim().to_string();
                changed = true;
            }
        });
        match &mut eff.kind {
            ContentEffectKind::Paint { channels: painted } => {
                hint(ui, "The brush paints into this effect while it is selected, on paint and fill layers alike. Erasing removes only this effect's paint.");
                ui.horizontal_wrapped(|ui| {
                    if painted.is_empty() {
                        ui.label(RichText::new("Nothing painted yet.").small().color(TEXT_FAINT));
                    }
                    for k in painted.keys() {
                        badge(ui, k.display_name(), TEXT_DIM);
                    }
                });
            }
            ContentEffectKind::Fill(fill) => {
                hint(ui, "A material on top of the layer's content. Enable the channels it should change.");
                changed |= super::properties::projection_editor(ui, &mut fill.projection);
                let states: Vec<_> = channels.iter().map(|k| (*k, fill.channels.get(k).is_some_and(|c| c.enabled))).collect();
                if let Some((channel, solo)) = super::properties::channel_toggles(ui, &states) {
                    cmds.push(Command::ToggleFillChannel { layer: layer.id, effect: Some(eff.id), channel, solo });
                }
                for kind in &channels {
                    let mut ch = fill.channels.get(kind).cloned().unwrap_or(FillChannel::disabled(FillSource::uniform(kind.default_paint_value())));
                    if !ch.enabled { continue; }
                    let before = ch.clone();
                    ui.push_id(("effect_fill", *kind), |ui| {
                        ui.label(RichText::new(kind.display_name()).color(theme::TEXT));
                        if ch.enabled {
                            let current = match ch.source {
                                FillSource::Procedural { .. } => 1,
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
                changed |= slider_row(ui, "In low", egui::Slider::new(&mut levels.in_low, 0.0..=1.0)).changed();
                changed |= slider_row(ui, "In high", egui::Slider::new(&mut levels.in_high, 0.0..=1.0)).changed();
                changed |= slider_row(ui, "Gamma", egui::Slider::new(&mut levels.gamma, 0.1..=4.0)).changed();
                changed |= slider_row(ui, "Out low", egui::Slider::new(&mut levels.out_low, 0.0..=1.0)).changed();
                changed |= slider_row(ui, "Out high", egui::Slider::new(&mut levels.out_high, 0.0..=1.0)).changed();
            }
            ContentEffectKind::Filter { channel, filter } => {
                hint(ui, "Adjusts the previous effect stage. Paint coverage and other channels are preserved.");
                prop_row(ui, "Channel", |ui| {
                    egui::ComboBox::from_id_salt("filter_channel").selected_text(channel.display_name()).show_ui(ui, |ui| {
                        for k in channels.iter().filter(|k| filter.supports_channel(**k)) {
                            changed |= ui.selectable_value(channel, *k, k.display_name()).changed();
                        }
                    });
                });
                changed |= super::filter::editor(ui, filter, Some(*channel));
            }
        }
    });
    section(ui, "effect_blend", "Blending per channel", true, |ui| {
        hint(ui, "How this effect combines with the content below it, per channel.");
        for kind in &channels {
            if let ContentEffectKind::Filter { channel, .. } | ContentEffectKind::Levels { channel, .. } = &eff.kind
                && channel != kind { continue; }
            if let ContentEffectKind::Fill(fill) = &eff.kind && !fill.channels.get(kind).is_some_and(|c| c.enabled) { continue; }
            let Some(blend) = eff.blend.get_mut(kind) else { continue };
            ui.push_id(("effect_blend", *kind), |ui| {
                prop_row(ui, kind.display_name(), |ui| {
                    egui::ComboBox::from_id_salt("mode").width(96.0).selected_text(blend.mode.label()).show_ui(ui, |ui| {
                        for mode in BlendMode::available(*kind == ChannelKind::Normal, false) {
                            changed |= ui.selectable_value(&mut blend.mode, mode, mode.label()).changed();
                        }
                    });
                    // Fit the slider and its value box in the remaining row width.
                    ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(40.0);
                    changed |= ui.add(egui::Slider::new(&mut blend.opacity, 0.0..=1.0).show_value(true)).changed();
                });
            });
        }
    });
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
