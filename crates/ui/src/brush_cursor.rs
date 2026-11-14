//! Brush cursor helpers for Painter's brush preview styles ([`crate::viewport_bar::BrushCursor`]):
//! the full preview's falloff rings (pure and unit-tested; drawn by the 3D view's `brush_mark`)
//! and the tooltips of the toolbar choices.

use crate::viewport_bar::BrushCursor;

/// Rings the full preview is drawn with.
pub const PREVIEW_RINGS: usize = 8;
/// Opacity of the full preview where the tip paints at full strength.
pub const PREVIEW_ALPHA: f32 = 0.35;

/// Full preview of a brush of `radius` points with the tip's `hardness`: rings as (radius of the
/// ring's middle, width, strength 0..1), outside in, tiling the disc.
pub fn preview_rings(radius: f32, hardness: f32) -> Vec<(f32, f32, f32)> {
    let radius = if radius.is_finite() { radius.max(0.0) } else { 0.0 };
    let width = radius / PREVIEW_RINGS as f32;
    (0..PREVIEW_RINGS)
        .rev()
        .map(|k| {
            let d = (k as f32 + 0.5) / PREVIEW_RINGS as f32;
            (d * radius, width, subcraft_paint::falloff(d, hardness))
        })
        .collect()
}

/// Side in texels of the full preview's footprint image.
pub const FOOTPRINT_SIDE: usize = 96;

/// What the cached footprint image was drawn from: the alpha's identity and the tip's hardness,
/// angle and roundness (bits).
pub type FootprintKey = (usize, u32, u32, u32);

pub fn footprint_key(alpha: Option<&std::sync::Arc<subcraft_image::Bitmap>>, s: &subcraft_paint::BrushSettings) -> FootprintKey {
    (alpha.map_or(0, |a| std::sync::Arc::as_ptr(a) as usize), s.hardness.to_bits(), s.angle_deg.to_bits(), s.roundness.to_bits())
}

/// Painter's full brush preview: the tip's footprint (its alpha shape, angle and roundness, or
/// the round tip's falloff) as white with coverage in alpha, drawn by the painting kernel's own
/// [`subcraft_paint::stamp::tip_footprint`]. The image spans `reach` brush radii each way from
/// its centre (see [`footprint_reach`]); screen y points down, as in the brush panel preview.
pub fn footprint_image(alpha: Option<&subcraft_image::Bitmap>, s: &subcraft_paint::BrushSettings) -> egui::ColorImage {
    let reach = footprint_reach(alpha.is_some());
    let side = FOOTPRINT_SIDE;
    let angle = s.angle_deg.to_radians();
    let mut rgba = Vec::with_capacity(side * side * 4);
    for py in 0..side {
        for px in 0..side {
            let local = glam::Vec2::new(((px as f32 + 0.5) / side as f32 * 2.0 - 1.0) * reach, ((py as f32 + 0.5) / side as f32 * 2.0 - 1.0) * reach);
            let a = subcraft_paint::stamp::tip_footprint(alpha, s.hardness, angle, s.roundness, local);
            rgba.extend_from_slice(&[255, 255, 255, (a.clamp(0.0, 1.0) * PREVIEW_ALPHA * 255.0).round() as u8]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([side, side], &rgba)
}

/// Brush radii the footprint image covers from its centre (an alpha tip reaches its corners).
pub fn footprint_reach(has_alpha: bool) -> f32 {
    subcraft_paint::stamp::reach(has_alpha)
}

/// What each brush preview style shows, for tooltips.
pub fn tip(style: BrushCursor) -> &'static str {
    match style {
        BrushCursor::Full => "The brush outline filled with the tip itself: its alpha shape, angle and roundness, or the round tip's falloff",
        BrushCursor::Outline => "A circle the size of the brush",
        BrushCursor::Crosshair => "A small cross at the brush center, for precise placement",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_full_preview_draws_the_tip_footprint() {
        let alpha_at = |img: &egui::ColorImage, x: usize, y: usize| img.pixels[y * FOOTPRINT_SIDE + x].a();
        let c = FOOTPRINT_SIDE / 2;
        // Round hard tip: covered at the centre, empty in the corner, a disc (left = top).
        let round = footprint_image(None, &subcraft_paint::BrushSettings { hardness: 1.0, ..Default::default() });
        assert!(alpha_at(&round, c, c) > 80 && alpha_at(&round, 0, 0) == 0);
        assert_eq!(alpha_at(&round, 2, c), alpha_at(&round, c, 2));
        // A squashed tip (roundness 0.3): wide along x, thin along y.
        let flat = footprint_image(None, &subcraft_paint::BrushSettings { hardness: 1.0, roundness: 0.3, ..Default::default() });
        assert!(alpha_at(&flat, c - 30, c) > 0 && alpha_at(&flat, c, c - 30) == 0);
        // An alpha tip: its own shape, here the left half only.
        let half = subcraft_image::Bitmap::new_u8(4, 4, 1, (0..16).map(|i| if i % 4 < 2 { 255 } else { 0 }).collect());
        let img = footprint_image(Some(&half), &subcraft_paint::BrushSettings::default());
        let reach = footprint_reach(true);
        let at = |u: f32| (((u / reach) * 0.5 + 0.5) * FOOTPRINT_SIDE as f32) as usize;
        assert!(alpha_at(&img, at(-0.5), c) > 0 && alpha_at(&img, at(0.6), c) == 0, "left half covered, right half empty");
        // The cache key follows what changes the picture.
        let s = subcraft_paint::BrushSettings::default();
        assert_ne!(footprint_key(None, &s), footprint_key(None, &subcraft_paint::BrushSettings { angle_deg: 30.0, ..s.clone() }));
    }

    #[test]
    fn full_preview_fills_the_circle_with_the_tip_falloff() {
        let rings = preview_rings(40.0, 0.5);
        assert_eq!(rings.len(), PREVIEW_RINGS);
        // The rings tile the disc: outside in, each band `width` wide, the outer one reaching the rim.
        let (outer_r, width, _) = rings[0];
        assert!((outer_r + width / 2.0 - 40.0).abs() < 1e-4, "the outer ring ends at the rim");
        assert!(rings.windows(2).all(|w| (w[0].0 - w[1].0 - width).abs() < 1e-4));
        // Strength follows the tip: full inside the hardness radius, fading towards the rim.
        let strengths: Vec<f32> = rings.iter().map(|r| r.2).collect();
        assert!(strengths.windows(2).all(|w| w[1] >= w[0]), "stronger towards the center: {strengths:?}");
        assert_eq!(*strengths.last().unwrap(), 1.0);
        assert!(strengths[0] < 0.2, "faint at the rim with hardness 0.5: {strengths:?}");
        // A hard tip is solid to the rim; a broken size draws nothing visible.
        assert!(preview_rings(40.0, 1.0).iter().all(|r| r.2 == 1.0));
        assert!(preview_rings(f32::NAN, 0.5).iter().all(|r| r.0 == 0.0 && r.1 == 0.0));
    }

    #[test]
    fn every_style_is_listed_once_with_its_own_tooltip() {
        let tips: std::collections::BTreeSet<_> = BrushCursor::ALL.into_iter().map(tip).collect();
        assert_eq!(tips.len(), 3);
        assert_eq!(BrushCursor::default(), BrushCursor::Outline, "the cursor as before stays the default");
    }

    #[test]
    fn preferences_keep_the_style_and_curve_and_older_files_get_the_defaults() {
        let mut prefs = crate::viewport_bar::ViewportPrefs::default();
        assert_eq!((prefs.brush_cursor, prefs.pressure_curve), (BrushCursor::Outline, subcraft_paint::PressureCurve::Linear));
        prefs.brush_cursor = BrushCursor::Full;
        prefs.pressure_curve = subcraft_paint::PressureCurve::EaseInOut;
        let json = serde_json::to_string(&prefs).unwrap();
        assert!(json.contains("\"pressure_curve\":\"ease_in_out\""), "{json}");
        let back: crate::viewport_bar::ViewportPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!((back.brush_cursor, back.pressure_curve), (BrushCursor::Full, subcraft_paint::PressureCurve::EaseInOut));
        let old: crate::viewport_bar::ViewportPrefs = serde_json::from_str("{\"symmetry_guides\":false}").unwrap();
        assert_eq!((old.brush_cursor, old.pressure_curve), (BrushCursor::Outline, subcraft_paint::PressureCurve::Linear));
    }
}
