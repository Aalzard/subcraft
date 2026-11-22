//! "NORMAL MAP BRUSH" panel: shown at the top of the brush settings while a normal-map (or
//! height) stamp is the active brush. Edits go straight into the session's brush state; the
//! stamp maths and the first-stroke channel switch-on live in `app` / `paint`.

use std::hash::{Hash, Hasher};

use egui::{RichText, Ui};
use subcraft_paint::StampKind;

use crate::state::UiState;
use crate::theme;
use crate::widgets::{check_row_reset, hint, prop_row_reset, segmented_response, slider_row_own_parser_reset, slider_row_reset};

/// Preview size in points.
const PREVIEW: [u32; 2] = [264, 88];

/// The live preview texture and the settings it was rendered for.
#[derive(Default)]
pub struct PreviewCache {
    key: u64,
    texture: Option<egui::TextureHandle>,
}

fn preview_key(state: &UiState) -> Option<u64> {
    let (stamp, _) = state.session.brush_stamp()?;
    let s = &state.session.brush.settings;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    stamp.resource.hash(&mut h);
    (stamp.kind as u8).hash(&mut h);
    for v in [stamp.strength, s.hardness, s.spacing, s.angle_deg] {
        v.to_bits().hash(&mut h);
    }
    stamp.flip_green.hash(&mut h);
    Some(h.finish())
}

fn refresh_preview(state: &mut UiState, ctx: &egui::Context) {
    let Some(key) = preview_key(state) else { return };
    if state.stamp_preview.key == key && state.stamp_preview.texture.is_some() {
        return;
    }
    let Some((stamp, image)) = state.session.brush_stamp() else { return };
    let s = &state.session.brush.settings;
    let rgba = subcraft_paint::image_stamp::render_preview(&image, stamp, s.hardness, s.spacing, s.angle_deg.to_radians(), PREVIEW[0], PREVIEW[1]);
    let color = egui::ColorImage::from_rgba_unmultiplied([PREVIEW[0] as usize, PREVIEW[1] as usize], &rgba);
    state.stamp_preview = PreviewCache { key, texture: Some(ctx.load_texture("normal-brush-preview", color, egui::TextureOptions::LINEAR)) };
}

/// Draws the panel; returns without drawing when no stamp is active.
pub fn show(state: &mut UiState, ui: &mut Ui) {
    let Some((stamp, _)) = state.session.brush_stamp().map(|(s, i)| (s.clone(), i)) else { return };
    refresh_preview(state, ui.ctx());
    let height = stamp.kind == StampKind::Height;
    egui::Frame::new()
        .fill(theme::BG_2())
        .stroke(egui::Stroke::new(1.0, theme::ACCENT()))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(RichText::new(if height { "HEIGHT BRUSH" } else { "NORMAL MAP BRUSH" }).strong().small().color(theme::ACCENT()));
            ui.label(RichText::new(&stamp.name).color(theme::TEXT()));
            if let Some(t) = &state.stamp_preview.texture {
                ui.add_space(4.0);
                let w = ui.available_width().min(PREVIEW[0] as f32).max(1.0);
                ui.add(egui::Image::new((t.id(), egui::vec2(w, w * PREVIEW[1] as f32 / PREVIEW[0] as f32))).corner_radius(3.0));
                match state.asset_preview.pending_brush().filter(|(n, _)| n.strip_prefix("Starter: ").unwrap_or(n) == stamp.name).map(|(_, p)| p) {
                    Some(p) => super::brush_header::loading_row(ui, p),
                    None => hint(ui, if height { "Lit preview: one stamp and an example stroke" } else { "Lit preview: one stamp and an example stroke (light from the upper left)" }),
                }
            }
            ui.add_space(4.0);
            let mut st = stamp.clone();
            let mut changed = false;
            let ds = subcraft_paint::ImageStamp::default();
            {
                // Defaults: a new brush's size and angles, and what picking a stamp sets.
                let b = &mut state.session.brush.settings;
                let d = subcraft_paint::BrushSettings::default();
                slider_row_reset(ui, "Size", &mut b.size, d.size, |v| egui::Slider::new(v, 0.5..=60.0).logarithmic(true).suffix("%"));
                changed |= slider_row_reset(ui, if height { "Height" } else { "Strength" }, &mut st.strength, ds.strength, |v| egui::Slider::new(v, 0.0..=4.0)).changed();
                slider_row_reset(ui, "Rotation", &mut b.angle_deg, d.angle_deg, |v| egui::Slider::new(v, -180.0..=180.0).suffix("°"));
                slider_row_reset(ui, "Random rotation", &mut b.angle_jitter_deg, d.angle_jitter_deg, |v| egui::Slider::new(v, 0.0..=180.0).suffix("°"));
                slider_row_reset(ui, "Spacing", &mut b.spacing, subcraft_app::stamp_brush::STAMP_SPACING, |v| egui::Slider::new(v, 0.05..=3.0).logarithmic(true))
                    .on_hover_text("Distance between stamps in brush diameters. 1 = one stamp per diameter; small values average the overlapping stamps");
                slider_row_own_parser_reset(ui, "Edge softness", &mut b.hardness, subcraft_app::stamp_brush::STAMP_HARDNESS, |v| egui::Slider::new(v, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", (1.0 - v) * 100.0)).custom_parser(|s| crate::widgets::parse_number(s.trim().trim_end_matches('%')).map(|v| 1.0 - v / 100.0)))
                    .on_hover_text("Fades the stamp out toward its rim, so no square border is left");
            }
            if !height {
                changed |= check_row_reset(ui, "", "Flip green (OpenGL ↔ DirectX)", &mut st.flip_green, ds.flip_green).changed();
            }
            changed |= prop_row_reset(ui, "Mode", &mut st.tiled, ds.tiled, |ui, v| segmented_response(ui, v, &[(false, "Single stamp"), (true, "Tiled")])).changed();
            if changed {
                state.session.brush.material.stamp = Some(st);
            }
            ui.add_space(4.0);
            if ui.button("Back to normal brush").on_hover_text("Restore the previous channels, spacing and edge softness").clicked() {
                state.session.clear_brush_stamp();
            }
            hint(ui, if height { "Paint on a layer: its Height channel is switched on by the first stroke." } else { "Paint on a layer: its Normal channel is switched on by the first stroke." });
        });
    ui.add_space(6.0);
}
