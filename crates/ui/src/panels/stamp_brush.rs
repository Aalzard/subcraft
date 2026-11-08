//! "NORMAL MAP BRUSH" panel: shown at the top of the brush settings while a normal-map (or
//! height) stamp is the active brush. Edits go straight into the session's brush state; the
//! stamp maths and the first-stroke channel switch-on live in `app` / `paint`.

use std::hash::{Hash, Hasher};

use egui::{RichText, Ui};
use subcraft_paint::StampKind;

use crate::state::UiState;
use crate::theme;
use crate::widgets::{hint, prop_row, segmented, slider_row};

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
        .fill(theme::BG_2)
        .stroke(egui::Stroke::new(1.0, theme::ACCENT))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(RichText::new(if height { "HEIGHT BRUSH" } else { "NORMAL MAP BRUSH" }).strong().small().color(theme::ACCENT));
            ui.label(RichText::new(&stamp.name).color(theme::TEXT));
            if let Some(t) = &state.stamp_preview.texture {
                ui.add_space(4.0);
                ui.add(egui::Image::new((t.id(), egui::vec2(PREVIEW[0] as f32, PREVIEW[1] as f32))).corner_radius(3.0));
                match state.asset_preview.pending_brush().filter(|(n, _)| n.strip_prefix("Starter: ").unwrap_or(n) == stamp.name).map(|(_, p)| p) {
                    Some(p) => super::brush_header::loading_row(ui, p),
                    None => hint(ui, if height { "Lit preview: one stamp and an example stroke" } else { "Lit preview: one stamp and an example stroke (light from the upper left)" }),
                }
            }
            ui.add_space(4.0);
            let mut st = stamp.clone();
            let mut changed = false;
            {
                let b = &mut state.session.brush.settings;
                slider_row(ui, "Size", egui::Slider::new(&mut b.size, 0.5..=60.0).logarithmic(true).suffix("%"));
                changed |= slider_row(ui, if height { "Height" } else { "Strength" }, egui::Slider::new(&mut st.strength, 0.0..=4.0)).changed();
                slider_row(ui, "Rotation", egui::Slider::new(&mut b.angle_deg, -180.0..=180.0).suffix("°"));
                slider_row(ui, "Random rotation", egui::Slider::new(&mut b.angle_jitter_deg, 0.0..=180.0).suffix("°"));
                slider_row(ui, "Spacing", egui::Slider::new(&mut b.spacing, 0.05..=3.0).logarithmic(true))
                    .on_hover_text("Distance between stamps in brush diameters. 1 = one stamp per diameter; small values average the overlapping stamps");
                slider_row(ui, "Edge softness", egui::Slider::new(&mut b.hardness, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", (1.0 - v) * 100.0)).custom_parser(|s| s.trim_end_matches('%').parse::<f64>().ok().map(|v| 1.0 - v / 100.0)))
                    .on_hover_text("Fades the stamp out toward its rim, so no square border is left");
            }
            if !height {
                prop_row(ui, "", |ui| changed |= ui.checkbox(&mut st.flip_green, "Flip green (OpenGL ↔ DirectX)").changed());
            }
            prop_row(ui, "Mode", |ui| {
                changed |= segmented(ui, &mut st.tiled, &[(false, "Single stamp"), (true, "Tiled")]);
            });
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
