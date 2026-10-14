//! Layer stack: tree with visibility, selection, creation/reorder buttons and per-channel blending.

use std::sync::Arc;

use egui::{RichText, Ui};
use subcraft_app::{Command, NewLayer};
use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, Layer, LayerKind};

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, ACCENT, PAD, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{badge, divider, empty_state, fill_width, icon_button, list_row, prop_row, row_text, slider_row};

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
                import_texture_set_dialog(state, now);
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
        if list.is_empty() {
            empty_state(ui, icons::STACK, "No layers yet. Add one above.");
        }
        for (depth, l) in &list {
            let selected = sel == Some(l.id);
            let b = l.blend_for(active);
            let resp = list_row(ui, selected, |ui| {
                ui.add_space(*depth as f32 * 16.0);
                let (eye, tip) = if l.visible { (icons::EYE, "Hide") } else { (icons::EYE_SLASH, "Show") };
                if icon_button(ui, eye, tip).clicked() {
                    cmds.push(Command::SetVisible { layer: l.id, visible: !l.visible });
                }
                let icon = match l.kind() {
                    LayerKind::Paint => icons::PAINT_BRUSH,
                    LayerKind::Fill => icons::PAINT_BUCKET,
                    LayerKind::Folder => icons::FOLDER_OPEN,
                };
                let icon_color = if selected { ACCENT } else { TEXT_DIM };
                ui.label(RichText::new(icon).font(theme::font(14.0)).color(icon_color)).on_hover_text(format!("{:?} layer", l.kind()));
                let mut text = row_text(&l.name, selected || l.visible);
                if !l.visible {
                    text = text.color(TEXT_FAINT);
                }
                ui.add(egui::Label::new(text).truncate().selectable(false));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} {:.0}", b.mode.short_label(), b.opacity * 100.0)).font(theme::font(11.0)).color(TEXT_FAINT));
                    if l.mask.is_some() {
                        badge(ui, "mask", ACCENT).on_hover_text("Has a mask");
                    }
                });
            });
            if resp.clicked() {
                cmds.push(Command::SelectLayer(Some(l.id)));
            }
            if let Some(item) = resp.dnd_release_payload::<crate::panels::shelf::ShelfDrag>() {
                drop_request = Some((l.id, *item));
            }
            if resp.dnd_hover_payload::<crate::panels::shelf::ShelfDrag>().is_some() {
                // Materials land above the row; smart masks go onto the row's layer.
                let r = resp.rect;
                ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
            }
            let has_effects = l.mask.as_ref().is_some_and(|m| !m.effects.is_empty());
            resp.context_menu(|ui| {
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
        }
    });
    state.run(cmds, now);
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
pub fn import_texture_set_dialog(state: &mut UiState, now: f64) {
    let Some(files) = rfd::FileDialog::new()
        .set_title("Import texture set: select the maps (base color, roughness, normal, …)")
        .add_filter("Images", &subcraft_image::IMAGE_EXTENSIONS)
        .pick_files()
    else {
        return;
    };
    match state.session.import_texture_set(&files) {
        Ok(r) => {
            for (file, why) in &r.skipped {
                log::info!("import texture set: skipped {file}: {why}");
            }
            state.set_status(r.summary(), now);
        }
        Err(e) => state.set_status(format!("Import texture set: {e}"), now),
    }
}
