//! "BRUSH" header at the top of the brush settings: the active brush's name and a preview of its
//! tip (one stamp and an example stroke), like Substance Painter's brush properties showing the
//! selected alpha. Stamp brushes (normal / height) have their own block in `stamp_brush`.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{RichText, Ui};

use crate::state::UiState;
use crate::theme;
use crate::widgets::hint;

/// Preview size in points (same as the stamp brush preview).
pub const PREVIEW: [u32; 2] = [264, 88];

/// The rendered tip preview and the brush state it shows.
#[derive(Default)]
pub struct PreviewCache {
    key: u64,
    texture: Option<egui::TextureHandle>,
}

fn preview_key(state: &UiState) -> u64 {
    let b = &state.session.brush;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.alpha.as_ref().map(|a| Arc::as_ptr(a) as usize).hash(&mut h);
    for v in [b.settings.hardness, b.settings.spacing, b.settings.angle_deg, b.settings.roundness, b.settings.flow] {
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

/// Re-render the preview when the tip or its shape settings changed. Returns the texture.
pub fn refresh(state: &mut UiState, ctx: &egui::Context) -> egui::TextureId {
    let key = preview_key(state);
    if state.tip_preview.key != key || state.tip_preview.texture.is_none() {
        let b = &state.session.brush;
        let rgba = subcraft_paint::tip_preview::render_tip_preview(b.alpha.as_deref(), &b.settings, PREVIEW[0], PREVIEW[1]);
        let image = egui::ColorImage::from_rgba_unmultiplied([PREVIEW[0] as usize, PREVIEW[1] as usize], &rgba);
        state.tip_preview = PreviewCache { key, texture: Some(ctx.load_texture("brush-tip-preview", image, egui::TextureOptions::LINEAR)) };
    }
    state.tip_preview.texture.as_ref().expect("rendered above").id()
}

/// Title of the header: the Shelf preset that set the brush, else the tip's name.
pub fn title(state: &UiState) -> String {
    let tip = &state.session.brush.alpha_name;
    match &state.selected_brush {
        Some((name, t)) if t == tip => name.clone(),
        _ => tip.strip_prefix("Starter: ").unwrap_or(tip).to_string(),
    }
}

/// Draws the header; nothing while a stamp brush is active (its own block shows it).
pub fn show(state: &mut UiState, ui: &mut Ui) {
    if state.session.brush.material.stamp.is_some() {
        return;
    }
    let texture = refresh(state, ui.ctx());
    let title = title(state);
    let tip = state.session.brush.alpha_name.clone();
    let pending = state.asset_preview.pending_brush().map(|(n, p)| (n == tip, p)).filter(|(same, _)| *same).map(|(_, p)| p);
    egui::Frame::new()
        .fill(theme::BG_2)
        .stroke(egui::Stroke::new(1.0, theme::ACCENT))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(RichText::new("BRUSH").strong().small().color(theme::ACCENT));
            ui.label(RichText::new(&title).color(theme::TEXT));
            let tip_name = tip.strip_prefix("Starter: ").unwrap_or(&tip);
            if tip_name != title {
                ui.label(RichText::new(format!("Tip: {tip_name}")).small().color(theme::TEXT_DIM));
            }
            ui.add_space(4.0);
            ui.add(egui::Image::new((texture, egui::vec2(PREVIEW[0] as f32, PREVIEW[1] as f32))).corner_radius(3.0));
            if let Some(p) = pending {
                loading_row(ui, p);
            } else {
                hint(ui, "Tip preview: one stamp and an example stroke. Change the brush in Shelf › Brushes");
            }
        });
    ui.add_space(6.0);
}

/// "Loading full resolution…" with a spinner, while a thumbnail stands in for the image.
pub fn loading_row(ui: &mut Ui, progress: f32) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(12.0));
        let pct = if progress > 0.0 { format!(" {:.0}%", progress * 100.0) } else { String::new() };
        ui.label(RichText::new(format!("Loading full resolution…{pct} (painting uses the preview meanwhile)")).small().color(theme::TEXT_DIM));
    });
}
