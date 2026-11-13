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

/// What each brush preview style shows, for tooltips.
pub fn tip(style: BrushCursor) -> &'static str {
    match style {
        BrushCursor::Full => "The brush outline filled with the tip's falloff (hardness)",
        BrushCursor::Outline => "A circle the size of the brush",
        BrushCursor::Crosshair => "A small cross at the brush center, for precise placement",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
