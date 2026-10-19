//! Presentation of app-produced UV previews. Widgets never evaluate document content.
use crate::theme;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui};
use std::collections::HashMap;
use std::sync::Arc;
use subcraft_app::{
    Session,
    layer_previews::{CACHE_LIMIT, LayerPreview, LayerPreviews, PreviewKey},
};
use subcraft_core::{ChannelKind, LayerId};

struct Texture {
    source: Arc<LayerPreview>,
    handle: egui::TextureHandle,
    used: u64,
}
#[derive(Default)]
pub struct PreviewUi {
    cache: LayerPreviews,
    textures: HashMap<PreviewKey, Texture>,
    clock: u64,
    pub inspect: Option<LayerId>,
}

impl PreviewUi {
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.cache.poll();
        if self.cache.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    pub fn image(
        &mut self,
        ui: &mut Ui,
        session: &Session,
        key: PreviewKey,
        side: f32,
        selected: bool,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                if key.effect.is_some() {
                    "Effect thumbnail"
                } else if key.mask {
                    "Mask thumbnail"
                } else {
                    "Content thumbnail"
                },
            )
        });
        if !ui.is_rect_visible(rect) {
            return response;
        }
        self.clock += 1;
        // Transparency is visible against a quiet checker, including empty paint layers.
        let painter = ui.painter_at(rect);
        let square = side / 4.0;
        for y in 0..4 {
            for x in 0..4 {
                let tile = Rect::from_min_size(
                    rect.min + egui::vec2(x as f32 * square, y as f32 * square),
                    egui::Vec2::splat(square),
                );
                painter.rect_filled(
                    tile,
                    0,
                    if (x + y) % 2 == 0 {
                        theme::BG_2
                    } else {
                        theme::BG_4
                    },
                );
            }
        }
        if let Some(image) = self.cache.request(session, key) {
            let pixels =
                || egui::ColorImage::from_rgba_unmultiplied([image.size as usize; 2], &image.rgba);
            if let Some(texture) = self.textures.get_mut(&key) {
                if !Arc::ptr_eq(&texture.source, &image) {
                    texture.handle.set(pixels(), egui::TextureOptions::LINEAR);
                    texture.source = image;
                }
                texture.used = self.clock;
            } else {
                if self.textures.len() >= CACHE_LIMIT {
                    let oldest = *self.textures.iter().min_by_key(|(_, t)| t.used).unwrap().0;
                    self.textures.remove(&oldest);
                }
                let handle = ui.ctx().load_texture(
                    format!(
                        "layer-{}-{:?}-{}-{}-{:?}",
                        key.layer, key.channel, key.mask, key.size, key.effect
                    ),
                    pixels(),
                    egui::TextureOptions::LINEAR,
                );
                self.textures.insert(
                    key,
                    Texture {
                        source: image,
                        handle,
                        used: self.clock,
                    },
                );
            }
            painter.image(
                self.textures[&key].handle.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        painter.rect_stroke(
            rect,
            1,
            Stroke::new(
                if selected { 2.0 } else { 1.0 },
                if selected {
                    theme::ACCENT
                } else {
                    theme::BORDER_STRONG
                },
            ),
            egui::StrokeKind::Inside,
        );
        if self.cache.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        response
    }
}

pub fn inspection_window(state: &mut crate::state::UiState, ctx: &egui::Context) {
    let Some(layer) = state.layer_previews.inspect else {
        return;
    };
    let found = state
        .session
        .document()
        .texture_sets
        .iter()
        .find_map(|ts| ts.find(layer));
    let Some(found) = found.filter(|l| l.mask.is_some()) else {
        state.layer_previews.inspect = None;
        return;
    };
    let title = format!("Mask preview · {}", found.name);
    let mut open = true;
    egui::Window::new(title).id(egui::Id::new("mask_uv_preview")).open(&mut open).resizable(true).show(ctx, |ui| {
        ui.label(egui::RichText::new("Evaluated mask · UV space").color(theme::TEXT_DIM));
        let side = ui.available_width().clamp(192.0, 512.0);
        state.layer_previews.image(ui, &state.session, PreviewKey { layer, channel: ChannelKind::BaseColor, mask: true, effect: None, size: 256 }, side, false);
        crate::widgets::hint(ui, "Includes painting and effects. A disabled mask still shows its authored result here.");
    });
    if !open {
        state.layer_previews.inspect = None;
    }
}
