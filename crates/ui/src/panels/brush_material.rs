//! Brush image widgets dispatch tool commands; import and paint behavior live in app/paint.
use super::{
    properties::{image_row, projection_editor},
    shelf::ShelfDrag,
};
use crate::{
    files::{self, Purpose},
    state::UiState,
    theme,
    widgets::{channel_value, hint, prop_row, quiet_button, segmented},
};
use egui::{RichText, Ui};
use subcraft_app::Command;
use subcraft_paint::{BrushChannel, NormalPaintBlend};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let mut material = state.session.brush.material.clone();
    let original = material.clone();
    let mut dropped = None;
    let mut starter_dropped = None;
    // While a texture is being dragged, channels that are switched off still accept it (the app
    // switches the channel on when it receives the image).
    let dragging_texture = egui::DragAndDrop::payload::<ShelfDrag>(ui.ctx()).is_some_and(|d| matches!(*d, ShelfDrag::Texture(_) | ShelfDrag::Starter(_)));
    if quiet_button(ui, crate::icons::PAINT_BUCKET, "Paint whole material…")
        .on_hover_text("Choose named texture maps for the brush; only imported channels paint")
        .clicked()
    {
        files::pick(
            ui.ctx(),
            Purpose::BrushTextureSet,
            files::IMAGES,
            true,
            Some("Brush material maps"),
        );
    }
    hint(
        ui,
        "Choose images or drop Shelf › Textures onto a channel. Images replace flat values.",
    );
    ui.add_space(4.0);
    let states: Vec<_> = state.session.channels().into_iter().map(|k| (k, material.channels.get(&k).is_some_and(|c| c.enabled))).collect();
    if let Some((channel, solo)) = super::properties::channel_toggles(ui, &states) {
        let _ = state.session.execute(Command::ToggleBrushChannel { channel, solo });
        material = state.session.brush.material.clone();
    }
    for (index, kind) in state.session.channels().into_iter().enumerate() {
        ui.push_id(kind, |ui| {
            let entry = material.channels.entry(kind).or_insert(BrushChannel {
                enabled: false,
                value: kind.default_paint_value(),
            });
            let mut pick = false;
            let enabled = entry.enabled;
            if !enabled && !dragging_texture { return; }
            let has_image = material.textures.contains_key(&kind);
            let row = egui::Frame::new()
                .fill(theme::ROW_BANDS[index % 2])
                .inner_margin(egui::Margin::same(8))
                .corner_radius(2)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(kind.display_name());
                        if !enabled {
                            ui.weak("drop the image here to use it");
                        } else if !has_image {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    pick = quiet_button(ui, crate::icons::IMAGE, "Image…")
                                        .on_hover_text(
                                            "Choose image… or drop a texture from the shelf",
                                        )
                                        .clicked();
                                },
                            );
                        }
                    });
                    if !enabled {
                    } else if let Some(&id) = material.textures.get(&kind) {
                        pick = image_row(ui, state, id);
                        if quiet_button(ui, crate::icons::X, "Clear image").clicked() {
                            material.textures.remove(&kind);
                        }
                    } else if entry.enabled {
                        channel_value(ui, kind, &mut entry.value);
                    }
                    if pick {
                        files::pick(
                            ui.ctx(),
                            Purpose::BrushChannel { channel: kind },
                            files::IMAGES,
                            false,
                            None,
                        );
                    }
                });
            if row
                .response
                .dnd_hover_payload::<ShelfDrag>()
                .is_some_and(|d| matches!(*d, ShelfDrag::Texture(_) | ShelfDrag::Starter(_)))
            {
                ui.painter().rect_stroke(
                    row.response.rect,
                    0.0,
                    egui::Stroke::new(1.5, theme::ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
            if let Some(d) = row.response.dnd_release_payload::<ShelfDrag>() {
                match *d {
                    ShelfDrag::Texture(i) => {
                        dropped = state.textures.items.get(i).map(|item| (kind, item.path.clone()));
                    }
                    ShelfDrag::Starter(i) => starter_dropped = Some((i, kind)),
                    _ => {}
                }
            }
        });
    }
    if material.textures.keys().any(|kind| material.channels.get(kind).is_some_and(|c| c.enabled)) {
        ui.label(RichText::new("Image projection").strong());
        projection_editor(ui, &mut material.projection);
        ui.checkbox(&mut material.camera_projection, "Project from the view")
            .on_hover_text("Like a slide projector: the image is cast from the camera the stroke started in, whatever the model's UVs. Scale, rotation and offset place it on screen.");
        ui.checkbox(&mut material.texture_tiling, "Tile images")
            .on_hover_text("Off: paint only inside image coordinates 0–1");
        hint(
            ui,
            "UV uses the model's UVs; planar / tri-planar use the same object frame as fill layers.",
        );
    }
    if material.channels.get(&subcraft_core::ChannelKind::Normal).is_some_and(|c| c.enabled) {
    prop_row(ui, "Normal blend", |ui| {
        segmented(
            ui,
            &mut material.normal_blend,
            &[
                (NormalPaintBlend::Replace, "Replace"),
                (NormalPaintBlend::Detail, "Detail"),
            ],
        );
    });
    hint(
        ui,
        "Detail combines with normals already painted on this layer. Alt+click nrm to paint only normals.",
    );
    }
    let now = ui.input(|i| i.time);
    if material != original
        && let Err(e) = state.session.execute(Command::SetBrushMaterial(material))
    {
        state.set_status(e.to_string(), now);
    }
    if let Some((index, channel)) = starter_dropped {
        state.asset_preview.queue_brush(&state.session, index, channel);
    }
    if let Some((channel, path)) = dropped {
        match state.session.set_brush_channel_image(channel, &path) {
            Ok(()) => {
                state.set_status(
                    format!("Brush {} image assigned", channel.display_name()),
                    now,
                );
            }
            Err(e) => {
                state.set_status(e, now);
            }
        }
    }
}
