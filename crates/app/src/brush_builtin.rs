//! Built-in brush presets: ready-made behaviours (size, spacing, jitter, pressure) on the
//! built-in procedural tips. They carry no material, so the current paint colour is kept.

use subcraft_paint::{Alignment, BrushSettings, StrokeOptions};

use crate::brush_preset::{BrushPreset, FORMAT};

fn preset(name: &str, tip: Option<&str>, settings: BrushSettings, lazy_distance_px: f32) -> BrushPreset {
    BrushPreset {
        format: FORMAT.into(),
        version: 2,
        name: name.into(),
        tip: tip.map(str::to_string),
        settings,
        options: StrokeOptions { lazy_distance_px, ..Default::default() },
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
        preset("Pencil", None, BrushSettings { size: 1.2, hardness: 0.8, flow: 0.6, spacing: 0.04, pressure_size: 0.6, pressure_flow: 0.5, ..d() }, 4.0),
    ]
}
