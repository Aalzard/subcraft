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
    /// The material sphere and the material values it shows.
    material_key: u64,
    material: Option<egui::TextureHandle>,
    /// Turn of the preview's light around the sphere (radians), set by dragging it.
    pub light_yaw: f32,
}

/// Side of the material preview sphere in texels (and points).
pub const SPHERE: usize = 64;

/// The values the material sphere shows: enabled channels' values, neutral otherwise.
fn sphere_texel(m: &subcraft_paint::BrushMaterial) -> subcraft_materials::uv_material::UvTexel {
    use subcraft_core::ChannelKind as K;
    let d = subcraft_materials::uv_material::UvTexel::default();
    let value = |k: K| m.channels.get(&k).filter(|c| c.enabled).map(|c| c.value.0);
    subcraft_materials::uv_material::UvTexel {
        base: value(K::BaseColor).unwrap_or(d.base),
        roughness: value(K::Roughness).map_or(d.roughness, |v| v[0]),
        metallic: value(K::Metallic).map_or(d.metallic, |v| v[0]),
        ..d
    }
}

/// Painter's Material Preview: a sphere lit by the studio light of the 2D view's Material
/// display ([`subcraft_materials::uv_material::shade`]) with the brush material's base colour,
/// roughness and metallic (images in slots are not shown). Antialiased edge, sRGB.
pub fn sphere_image(m: &subcraft_paint::BrushMaterial, light_yaw: f32) -> egui::ColorImage {
    let t = sphere_texel(m);
    let light = subcraft_materials::uv_material::light_turned(light_yaw);
    let mut rgba = Vec::with_capacity(SPHERE * SPHERE * 4);
    for py in 0..SPHERE {
        for px in 0..SPHERE {
            let x = (px as f32 + 0.5) / SPHERE as f32 * 2.0 - 1.0;
            let y = (py as f32 + 0.5) / SPHERE as f32 * 2.0 - 1.0;
            let r2 = x * x + y * y;
            let coverage = ((1.0 - r2.sqrt()) * SPHERE as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            // The sphere's normal facing the viewer (screen y down, normal y up).
            let z = (1.0 - r2.min(1.0)).sqrt();
            let normal = [x * 0.5 + 0.5, -y * 0.5 + 0.5, z * 0.5 + 0.5];
            let c = subcraft_materials::uv_material::shade_lit(subcraft_materials::uv_material::UvTexel { normal, ..t }, light);
            let srgb = subcraft_core::color::linear3_to_srgb(c.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }));
            rgba.extend(srgb.map(|v| (v * 255.0).round() as u8));
            rgba.push((coverage * 255.0).round() as u8);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([SPHERE, SPHERE], &rgba)
}

fn material_key(m: &subcraft_paint::BrushMaterial, light_yaw: f32) -> u64 {
    let t = sphere_texel(m);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in t.base.into_iter().chain([t.roughness, t.metallic, light_yaw]) {
        v.to_bits().hash(&mut h);
    }
    h.finish()
}

/// Re-render the material sphere when the brush material's values changed.
pub fn refresh_material(state: &mut UiState, ctx: &egui::Context) -> egui::TextureId {
    let yaw = state.tip_preview.light_yaw;
    let key = material_key(&state.session.brush.material, yaw);
    if state.tip_preview.material_key != key || state.tip_preview.material.is_none() {
        let image = sphere_image(&state.session.brush.material, yaw);
        state.tip_preview.material = Some(ctx.load_texture("brush-material-preview", image, egui::TextureOptions::LINEAR));
        state.tip_preview.material_key = key;
    }
    state.tip_preview.material.as_ref().expect("rendered above").id()
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
        state.tip_preview.texture = Some(ctx.load_texture("brush-tip-preview", image, egui::TextureOptions::LINEAR));
        state.tip_preview.key = key;
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
    let sphere = refresh_material(state, ui.ctx());
    let title = title(state);
    let tip = state.session.brush.alpha_name.clone();
    let pending = state.asset_preview.pending_brush().map(|(n, p)| (n == tip, p)).filter(|(same, _)| *same).map(|(_, p)| p);
    egui::Frame::new()
        .fill(theme::BG_2())
        .stroke(egui::Stroke::new(1.0, theme::ACCENT()))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(RichText::new("BRUSH").strong().small().color(theme::ACCENT()));
            ui.label(RichText::new(&title).color(theme::TEXT()));
            let tip_name = tip.strip_prefix("Starter: ").unwrap_or(&tip);
            if tip_name != title {
                ui.label(RichText::new(format!("Tip: {tip_name}")).small().color(theme::TEXT_DIM()));
            }
            ui.add_space(4.0);
            // Shrinks with a narrow panel (never widens the card past it).
            let w = ui.available_width().min(PREVIEW[0] as f32).max(1.0);
            ui.add(egui::Image::new((texture, egui::vec2(w, w * PREVIEW[1] as f32 / PREVIEW[0] as f32))).corner_radius(3.0));
            // Painter's Material Preview: the paint material on a lit sphere.
            ui.horizontal(|ui| {
                let r = ui.add(egui::Image::new((sphere, egui::vec2(SPHERE as f32, SPHERE as f32))).sense(egui::Sense::drag()))
                    .on_hover_text("The brush material (base colour, roughness, metallic) on a lit sphere · drag to turn the light");
                if r.dragged() {
                    state.tip_preview.light_yaw = (state.tip_preview.light_yaw + r.drag_delta().x * 0.03).rem_euclid(std::f32::consts::TAU);
                }
                ui.label(RichText::new("Material").small().color(theme::TEXT_DIM()));
            });
            if let Some(p) = pending {
                loading_row(ui, p);
            } else {
                hint(ui, "Tip preview: one stamp and an example stroke. Change the brush in Shelf › Brushes");
            }
        });
    ui.add_space(6.0);
}

#[cfg(test)]
mod material_preview_tests {
    use super::*;
    use subcraft_core::{ChannelKind, ChannelValue};

    #[test]
    fn the_sphere_shows_the_material_lit() {
        let red = subcraft_paint::BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(0.8, 0.05, 0.05)), (ChannelKind::Roughness, ChannelValue::scalar(0.6))]);
        let img = sphere_image(&red, 0.0);
        assert_eq!(img.size, [SPHERE, SPHERE]);
        let at = |x: usize, y: usize| img.pixels[y * SPHERE + x];
        assert_eq!(at(0, 0).a(), 0, "outside the sphere is transparent");
        let centre = at(SPHERE / 2, SPHERE / 2);
        assert!(centre.a() == 255 && centre.r() > 2 * centre.g(), "red in the middle: {centre:?}");
        // Lit from the upper left: the upper-left side is brighter than the lower right.
        let (ul, lr) = (at(SPHERE / 4, SPHERE / 4), at(3 * SPHERE / 4, 3 * SPHERE / 4));
        assert!(ul.r() > lr.r(), "{ul:?} vs {lr:?}");
        // A disabled channel shows the neutral value; the key follows the values.
        let mut off = red.clone();
        off.disable(ChannelKind::BaseColor);
        assert_eq!(sphere_texel(&off).base, subcraft_materials::uv_material::UvTexel::default().base);
        assert_ne!(material_key(&red, 0.0), material_key(&off, 0.0));
        // Turning the light a quarter: the key changes and the bright side moves to the right.
        assert_ne!(material_key(&red, 0.0), material_key(&red, 1.0));
        let turned = sphere_image(&red, std::f32::consts::FRAC_PI_2);
        let at2 = |x: usize, y: usize| turned.pixels[y * SPHERE + x];
        assert!(at2(3 * SPHERE / 4, SPHERE / 2).r() > at2(SPHERE / 4, SPHERE / 2).r());
    }
}

/// "Loading full resolution…" with a spinner, while a thumbnail stands in for the image.
pub fn loading_row(ui: &mut Ui, progress: f32) {
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::Spinner::new().size(12.0));
        let pct = if progress > 0.0 { format!(" {:.0}%", progress * 100.0) } else { String::new() };
        ui.label(RichText::new(format!("Loading full resolution…{pct} (painting uses the preview meanwhile)")).small().color(theme::TEXT_DIM()));
    });
}
