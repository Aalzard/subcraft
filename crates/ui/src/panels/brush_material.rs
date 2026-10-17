//! Brush image widgets dispatch tool commands; import and paint behavior live in app/paint.
use super::{
    properties::{image_row, projection_editor},
    shelf::ShelfDrag,
};
use crate::{
    files::{self, Purpose},
    state::UiState,
    theme,
    widgets::{channel_value, hint, prop_row, segmented},
};
use egui::{RichText, Ui};
use subcraft_app::Command;
use subcraft_paint::{BrushChannel, NormalPaintBlend};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let mut material = state.session.brush.material.clone();
    let original = material.clone();
    let mut dropped = None;
    if ui
        .button("Paint whole material…")
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
    for kind in state.session.channels() {
        ui.push_id(kind, |ui| {
            let entry = material.channels.entry(kind).or_insert(BrushChannel {
                enabled: false,
                value: kind.default_paint_value(),
            });
            let row = ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut entry.enabled, kind.display_name());
                    if !material.textures.contains_key(&kind) {
                        ui.add_enabled_ui(entry.enabled, |ui| {
                            channel_value(ui, kind, &mut entry.value);
                        });
                    }
                });
                let pick = if let Some(&id) = material.textures.get(&kind) {
                    let pick = image_row(ui, state, id);
                    if ui.small_button("Clear image").clicked() {
                        material.textures.remove(&kind);
                    }
                    pick
                } else {
                    ui.small_button("Choose image…").clicked()
                };
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
                .is_some_and(|d| matches!(*d, ShelfDrag::Texture(_)))
            {
                ui.painter().rect_stroke(
                    row.response.rect,
                    0.0,
                    egui::Stroke::new(1.5, theme::ACCENT),
                    egui::StrokeKind::Inside,
                );
            }
            if let Some(d) = row.response.dnd_release_payload::<ShelfDrag>()
                && let ShelfDrag::Texture(i) = *d
            {
                dropped = state
                    .textures
                    .items
                    .get(i)
                    .map(|item| (kind, item.path.clone()));
            }
        });
    }
    if !material.textures.is_empty() {
        ui.label(RichText::new("Image projection").strong());
        projection_editor(ui, &mut material.projection);
        ui.checkbox(&mut material.texture_tiling, "Tile images")
            .on_hover_text("Off: paint only inside image coordinates 0–1");
        hint(
            ui,
            "UV uses the model's UVs; planar / tri-planar use the same object frame as fill layers.",
        );
    }
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
        "Detail combines with normals already painted on this layer. Uncheck other channels to paint only normals.",
    );
    let now = ui.input(|i| i.time);
    if material != original
        && let Err(e) = state.session.execute(Command::SetBrushMaterial(material))
    {
        state.set_status(e.to_string(), now);
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
