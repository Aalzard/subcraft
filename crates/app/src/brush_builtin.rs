//! Built-in brush presets: ready-made behaviours (size, spacing, jitter, pressure) on the
//! built-in procedural tips. They carry no material, so the current paint colour is kept.

use subcraft_paint::{Alignment, BrushSettings, StrokeOptions};

use crate::brush_preset::{BrushPreset, FORMAT};

fn preset(name: &str, tip: Option<&str>, settings: BrushSettings, lazy_distance_px: f32) -> BrushPreset {
    preset_with(name, tip, settings, StrokeOptions { lazy_distance_px, ..Default::default() })
}

/// Brushes meant for the Smudge tool (they do nothing on an empty layer).
pub const SMUDGE_PRESETS: [&str; 3] = ["Soft Blend", "Wet Smear", "Paint Smear"];

fn preset_with(name: &str, tip: Option<&str>, settings: BrushSettings, options: StrokeOptions) -> BrushPreset {
    BrushPreset {
        format: FORMAT.into(),
        version: 2,
        name: name.into(),
        tip: tip.map(str::to_string),
        settings,
        options,
        material: Default::default(),
        images: Vec::new(),
        user_file: None,
    }
}

/// One line for the Shelf tooltip of a built-in brush.
pub fn describe(name: &str) -> Option<&'static str> {
    Some(match name {
        "Hard Round" => "Crisp round brush for clean shapes and line work",
        "Soft Airbrush" => "Soft, low-flow build-up; press harder for more paint",
        "Chalk" => "Dusty, grainy edge that breaks up as it drags",
        "Ink Pen" => "Hard, tight line that thins with a light touch; lazy mouse smooths it",
        "Charcoal" => "Rough, smudgy strokes with pressure-sensitive darkness",
        "Scatter Dots" => "Loose dots of varied size scattered around the stroke",
        "Hatch Lines" => "Short diagonal lines for hatching and shading",
        "Splatter" => "Random blobs thrown around the cursor",
        "Dry Brush" => "Streaky bristle marks that follow the stroke direction",
        "Texture Stamp" => "One grunge stamp per click, rotated at random",
        "Spray Paint" => "Fine speckle spray that builds up slowly",
        "Chisel Marker" => "Flat 45-degree chisel tip for calligraphy and blocking",
        "Smooth Ink" => "Ink line with weighted-average smoothing and a rope; the line catches up to the pen when you lift",
        "Steady Liner" => "Long rope plus smoothing for smooth, confident curves",
        "Dust Scatter" => "Fine specks scattered around the stroke, like dust or sand",
        "Drips" => "Blobs that run down the screen as you paint, for leaks and grime",
        "Rust Flakes" => "Chunky grunge flecks spread around the path with random size and rotation",
        "Sparkle Spray" => "Wide, sparse spray of tiny dots with random opacity",
        "Soft Blend" => "Smudge tool: gentle, short blend that softens edges between paint",
        "Wet Smear" => "Smudge tool: drags paint a long way like wet paint",
        "Paint Smear" => "Smudge tool: smears and mixes your current paint colour into what it drags",
        "Pencil" => "Thin, slightly soft line; light pressure gives a faint trace",
        _ => return None,
    })
}

pub fn builtin_brush_presets() -> Vec<BrushPreset> {
    let d = BrushSettings::default;
    vec![
        preset("Hard Round", None, BrushSettings { size: 4.0, hardness: 1.0, spacing: 0.05, ..d() }, 0.0),
        preset("Soft Airbrush", None, BrushSettings { size: 14.0, hardness: 0.0, flow: 0.2, spacing: 0.06, pressure_flow: 0.8, pressure_size: 0.0, ..d() }, 0.0),
        preset("Chalk", Some("Grunge"), BrushSettings { size: 7.0, hardness: 0.9, flow: 0.8, spacing: 0.15, angle_jitter_deg: 180.0, size_jitter: 0.2, ..d() }, 0.0),
        preset("Ink Pen", None, BrushSettings { size: 1.6, hardness: 1.0, spacing: 0.03, pressure_size: 0.9, ..d() }, 12.0),
        preset("Charcoal", Some("Grunge"), BrushSettings { size: 9.0, hardness: 0.6, flow: 0.55, spacing: 0.12, angle_jitter_deg: 30.0, flow_jitter: 0.3, pressure_flow: 0.6, ..d() }, 0.0),
        preset("Scatter Dots", None, BrushSettings { size: 2.5, hardness: 0.9, spacing: 1.5, size_jitter: 0.6, position_jitter: 1.5, pressure_size: 0.0, ..d() }, 0.0),
        preset("Hatch Lines", Some("Line"), BrushSettings { size: 6.0, spacing: 0.45, angle_deg: 45.0, angle_jitter_deg: 5.0, pressure_size: 0.0, ..d() }, 0.0),
        preset("Splatter", Some("Splatter"), BrushSettings { size: 10.0, spacing: 1.2, size_jitter: 0.5, angle_jitter_deg: 180.0, position_jitter: 0.8, pressure_size: 0.0, ..d() }, 0.0),
        preset("Dry Brush", Some("Scratches"), BrushSettings { size: 9.0, flow: 0.7, spacing: 0.08, follow_path: true, flow_jitter: 0.5, angle_jitter_deg: 4.0, ..d() }, 0.0),
        preset("Texture Stamp", Some("Grunge"), BrushSettings { size: 14.0, hardness: 1.0, spacing: 1.0, angle_jitter_deg: 180.0, pressure_size: 0.0, ..d() }, 0.0),
        preset("Spray Paint", Some("Spray"), BrushSettings { size: 12.0, flow: 0.5, spacing: 0.05, angle_jitter_deg: 180.0, pressure_size: 0.0, ..d() }, 0.0),
        preset("Chisel Marker", Some("Square"), BrushSettings { size: 5.0, spacing: 0.05, angle_deg: 45.0, roundness: 0.35, alignment: Alignment::TangentWrap, pressure_size: 0.0, ..d() }, 0.0),
        preset_with("Smooth Ink", None, BrushSettings { size: 1.8, hardness: 1.0, spacing: 0.03, pressure_size: 0.9, ..d() },
            StrokeOptions { lazy_distance_px: 6.0, smoothing: 0.7, catch_up: true, ..Default::default() }),
        preset_with("Steady Liner", None, BrushSettings { size: 2.4, hardness: 0.95, spacing: 0.03, pressure_size: 0.5, ..d() },
            StrokeOptions { lazy_distance_px: 30.0, smoothing: 0.4, catch_up: true, ..Default::default() }),
        preset("Dust Scatter", None, BrushSettings { size: 5.0, hardness: 0.6, flow: 0.7, spacing: 0.8, scatter_count: 14, scatter_radius: 1.4,
            scatter_size_random: 0.8, scatter_opacity_random: 0.5, pressure_size: 0.0, ..d() }, 0.0),
        preset("Drips", None, BrushSettings { size: 2.5, hardness: 0.9, spacing: 1.2, scatter_count: 3, scatter_radius: 0.6, scatter_size_random: 0.5,
            scatter_opacity_random: 0.2, scatter_fall: 6.0, pressure_size: 0.0, ..d() }, 0.0),
        preset("Rust Flakes", Some("Grunge"), BrushSettings { size: 6.0, hardness: 0.9, spacing: 0.9, scatter_count: 8, scatter_radius: 1.2, scatter_size_random: 0.7,
            scatter_rotation_deg: 180.0, scatter_opacity_random: 0.4, pressure_size: 0.0, ..d() }, 0.0),
        preset("Sparkle Spray", Some("Spray"), BrushSettings { size: 14.0, flow: 0.6, spacing: 0.6, scatter_count: 24, scatter_radius: 2.0, scatter_size_random: 0.9,
            scatter_opacity_random: 0.7, pressure_size: 0.0, ..d() }, 0.0),
        preset("Soft Blend", None, BrushSettings { size: 10.0, hardness: 0.0, flow: 0.6, spacing: 0.08, smudge_strength: 0.6, smudge_carry: 0.3, smudge_falloff: 0.6, ..d() }, 0.0),
        preset("Wet Smear", None, BrushSettings { size: 8.0, hardness: 0.7, spacing: 0.05, smudge_carry: 0.96, ..d() }, 0.0),
        preset("Paint Smear", None, BrushSettings { size: 8.0, hardness: 0.6, spacing: 0.06, smudge_carry: 0.9, smudge_material: 0.25, ..d() }, 0.0),
        preset("Pencil", None, BrushSettings { size: 1.2, hardness: 0.8, flow: 0.6, spacing: 0.04, pressure_size: 0.6, pressure_flow: 0.5, ..d() }, 4.0),
    ]
}
