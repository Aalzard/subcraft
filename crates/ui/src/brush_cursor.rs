//! The brush cursor in the 3D view, in Painter's three brush preview styles (contextual toolbar):
//! the brush outline, a crosshair, or a full preview of the tip's falloff. The geometry is pure
//! and unit-tested; [`draw`] paints it with the egui painter.

use egui::{Color32, Pos2, Stroke};
use serde::{Deserialize, Serialize};

/// How the brush is shown under the cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushPreview {
    /// A circle the size of the brush (SubCraft's cursor before this setting existed).
    #[default]
    Outline,
    /// A small cross at the brush center, without the circle.
    Crosshair,
    /// The tip's falloff (hardness) as a translucent disc, inside the outline.
    Full,
}

impl BrushPreview {
    pub const ALL: [BrushPreview; 3] = [BrushPreview::Full, BrushPreview::Outline, BrushPreview::Crosshair];

    pub fn label(self) -> &'static str {
        match self {
            BrushPreview::Full => "Full preview",
            BrushPreview::Outline => "Brush outline",
            BrushPreview::Crosshair => "Crosshair",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            BrushPreview::Full => "The brush outline filled with the tip's falloff (hardness)",
            BrushPreview::Outline => "A circle the size of the brush",
            BrushPreview::Crosshair => "A small cross at the brush center, for precise placement",
        }
    }
}

/// Length of each crosshair arm, in points.
pub const CROSSHAIR_ARM: f32 = 7.0;
/// Rings the full preview is drawn with.
pub const PREVIEW_RINGS: usize = 8;
/// Opacity of the full preview where the tip paints at full strength.
pub const PREVIEW_ALPHA: f32 = 0.35;

/// What a cursor style draws.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorParts {
    /// The brush circle.
    pub outline: bool,
    /// Crosshair arm length, when the style has one.
    pub crosshair: Option<f32>,
    /// Full preview: rings as (radius of the ring's middle, width, strength 0..1), outside in.
    pub rings: Vec<(f32, f32, f32)>,
}

/// The parts of the cursor for a brush of `radius` points and the tip's `hardness`.
pub fn parts(style: BrushPreview, radius: f32, hardness: f32) -> CursorParts {
    let radius = radius.max(0.0);
    match style {
        BrushPreview::Outline => CursorParts { outline: true, crosshair: None, rings: Vec::new() },
        BrushPreview::Crosshair => CursorParts { outline: false, crosshair: Some(CROSSHAIR_ARM), rings: Vec::new() },
        BrushPreview::Full => {
            let width = radius / PREVIEW_RINGS as f32;
            let rings = (0..PREVIEW_RINGS)
                .rev()
                .map(|k| {
                    let d = (k as f32 + 0.5) / PREVIEW_RINGS as f32;
                    (d * radius, width, subcraft_paint::falloff(d, hardness))
                })
                .collect();
            CursorParts { outline: true, crosshair: None, rings }
        }
    }
}

/// Draw the cursor at `at`; `color` is the tool's cursor colour.
pub fn draw(painter: &egui::Painter, at: Pos2, radius: f32, hardness: f32, color: Color32, style: BrushPreview) {
    let p = parts(style, radius, hardness);
    for (r, w, strength) in p.rings {
        if strength > 0.0 && r > 0.0 {
            // Rings slightly wider than their band so neighbours meet without gaps.
            painter.circle_stroke(at, r, Stroke::new(w + 0.5, color.gamma_multiply(strength * PREVIEW_ALPHA)));
        }
    }
    if p.outline {
        painter.circle_stroke(at, radius, Stroke::new(1.5, Color32::from_black_alpha(160)));
        painter.circle_stroke(at, radius, Stroke::new(1.0, color));
    }
    if let Some(arm) = p.crosshair {
        for (w, c) in [(3.0, Color32::from_black_alpha(160)), (1.2, color)] {
            painter.line_segment([at - egui::vec2(arm, 0.0), at + egui::vec2(arm, 0.0)], Stroke::new(w, c));
            painter.line_segment([at - egui::vec2(0.0, arm), at + egui::vec2(0.0, arm)], Stroke::new(w, c));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_is_the_default_and_draws_only_the_circle() {
        assert_eq!(BrushPreview::default(), BrushPreview::Outline);
        assert_eq!(parts(BrushPreview::Outline, 30.0, 0.5), CursorParts { outline: true, crosshair: None, rings: vec![] });
    }

    #[test]
    fn crosshair_draws_a_cross_and_no_circle_whatever_the_size() {
        for radius in [0.0, 4.0, 300.0] {
            let p = parts(BrushPreview::Crosshair, radius, 0.5);
            assert!(!p.outline && p.rings.is_empty());
            assert_eq!(p.crosshair, Some(CROSSHAIR_ARM));
        }
    }

    #[test]
    fn full_preview_fills_the_circle_with_the_tip_falloff() {
        let p = parts(BrushPreview::Full, 40.0, 0.5);
        assert!(p.outline && p.crosshair.is_none());
        assert_eq!(p.rings.len(), PREVIEW_RINGS);
        // The rings tile the disc: outside in, each band `width` wide, the outer one reaching the rim.
        let (outer_r, width, _) = p.rings[0];
        assert!((outer_r + width / 2.0 - 40.0).abs() < 1e-4, "the outer ring ends at the rim");
        assert!(p.rings.windows(2).all(|w| (w[0].0 - w[1].0 - width).abs() < 1e-4));
        // Strength follows the tip: full inside the hardness radius, fading towards the rim.
        let strengths: Vec<f32> = p.rings.iter().map(|r| r.2).collect();
        assert!(strengths.windows(2).all(|w| w[1] >= w[0]), "stronger towards the center: {strengths:?}");
        assert_eq!(*strengths.last().unwrap(), 1.0);
        assert!(strengths[0] < 0.2, "faint at the rim with hardness 0.5: {strengths:?}");
        // A hard tip is solid to the rim.
        assert!(parts(BrushPreview::Full, 40.0, 1.0).rings.iter().all(|r| r.2 == 1.0));
    }

    #[test]
    fn preferences_keep_the_style_and_older_files_get_the_outline() {
        let mut prefs = crate::viewport_bar::ViewportPrefs::default();
        assert_eq!(prefs.brush_preview, BrushPreview::Outline);
        assert_eq!(prefs.pressure_curve, subcraft_paint::PressureCurve::Linear);
        prefs.brush_preview = BrushPreview::Crosshair;
        prefs.pressure_curve = subcraft_paint::PressureCurve::EaseInOut;
        let json = serde_json::to_string(&prefs).unwrap();
        assert!(json.contains("\"brush_preview\":\"crosshair\"") && json.contains("\"pressure_curve\":\"ease_in_out\""), "{json}");
        let back: crate::viewport_bar::ViewportPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!((back.brush_preview, back.pressure_curve), (BrushPreview::Crosshair, subcraft_paint::PressureCurve::EaseInOut));
        let old: crate::viewport_bar::ViewportPrefs = serde_json::from_str("{\"symmetry_guides\":false}").unwrap();
        assert_eq!((old.brush_preview, old.pressure_curve), (BrushPreview::Outline, subcraft_paint::PressureCurve::Linear));
    }
}
