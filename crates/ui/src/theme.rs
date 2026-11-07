//! Dark theme: layered charcoal surfaces, softened controls, generous spacing, one warm accent.
//! Inter for text, Phosphor for icons (both embedded, see `assets/fonts`).

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};
use egui_dock::Style as DockStyle;

// Surfaces, darkest to lightest. The greys carry a faint warm tint (hue about 25 degrees, 5% HSL\n// saturation, lightness unchanged from the old neutral greys) so the charcoal sits with the orange accent.
pub const BG_0: Color32 = Color32::from_rgb(26, 25, 24); // gaps between panels, status bar
pub const BG_1: Color32 = Color32::from_rgb(38, 36, 34); // panels
pub const BG_2: Color32 = Color32::from_rgb(43, 41, 39); // header bar, tab bar, cards
pub const BG_3: Color32 = Color32::from_rgb(51, 49, 47); // controls
pub const BG_4: Color32 = Color32::from_rgb(64, 60, 57); // hovered controls
/// Quiet alternating bands, below the contrast of hover and selection.
pub const ROW_BANDS: [Color32; 2] = [BG_1, Color32::from_rgb(48, 46, 44)];
pub const SECTION_BANDS: [Color32; 2] = [BG_2, Color32::from_rgb(47, 45, 43)];
pub const FIELD: Color32 = Color32::from_rgb(30, 29, 28); // text fields, slider rails

/// Stacked blocks in Properties (one channel, one mask effect): clearly lighter than the panel.
pub const CARD: Color32 = Color32::from_rgb(58, 55, 53);
pub const CARD_BORDER: Color32 = Color32::from_rgb(89, 84, 80);

/// Colour of the left accent bar of a channel card (muted, so the set reads as a family).
pub fn channel_accent(kind: subcraft_core::ChannelKind) -> Color32 {
    use subcraft_core::ChannelKind::*;
    match kind {
        BaseColor => Color32::from_rgb(214, 150, 70),
        Height => Color32::from_rgb(140, 150, 168),
        Roughness => Color32::from_rgb(120, 168, 196),
        Metallic => Color32::from_rgb(176, 180, 196),
        Normal => Color32::from_rgb(132, 120, 208),
        AmbientOcclusion => Color32::from_rgb(120, 168, 130),
        Emissive => Color32::from_rgb(236, 206, 90),
        _ => Color32::from_rgb(150, 150, 146),
    }
}

/// Slider rail / value-field fill: darker than the panel so the control reads as an inset track.
pub const RAIL: Color32 = Color32::from_rgb(30, 30, 30);
/// Soft separators and control outlines (low contrast on `BG_1`).
pub const BORDER_SOFT: Color32 = Color32::from_rgb(46, 46, 46);
pub const BORDER: Color32 = Color32::from_rgb(55, 52, 49);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(75, 70, 67);
