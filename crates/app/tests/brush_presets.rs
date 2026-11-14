//! Brush presets in the user library.

use std::path::PathBuf;

use subcraft_app::Session;
use subcraft_app::assets::{UserAssetKind, UserLibrary, alphas};
use subcraft_app::brush_preset::{load_brush_presets, save_brush_preset};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_mesh::primitives;
use subcraft_paint::Alignment;

fn temp_lib(name: &str) -> (PathBuf, UserLibrary) {
    let d = std::env::temp_dir().join(format!("subcraft-brushes-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    (d.clone(), UserLibrary::new(d))
}

#[test]
fn preset_roundtrip_restores_the_whole_brush() {
    let (dir, lib) = temp_lib("roundtrip");
    let tips = alphas::builtin();
    let mut a = Session::new(primitives::cube(1.0), 128);
    let splatter = tips.iter().find(|t| t.name == "Splatter").unwrap();
    a.set_brush_alpha(splatter);
    a.brush.settings.size = 12.5;
    a.brush.settings.flow = 0.35;
    a.brush.settings.alignment = Alignment::Camera;
    a.brush.settings.size_jitter = 0.4;
    a.brush.options.symmetry = [true, false, false];
    a.brush.options.radial = subcraft_paint::RadialSymmetry { enabled: true, axis: 0, count: 6, center_offset: [0.0, 0.25, 0.0], flip: false };
    a.brush.options.lazy_distance_px = 30.0;
    a.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.1, 0.6, 0.2));
    let saved = save_brush_preset(&lib, &a.brush_preset("Green Splatter")).unwrap();
    assert_eq!(saved.name, "Green Splatter");
    assert_eq!(lib.list(UserAssetKind::BrushPreset).len(), 1);

    let (loaded, errors) = load_brush_presets(&lib);
    assert!(errors.is_empty(), "{errors:?}");
    let mut b = Session::new(primitives::cube(1.0), 128);
    assert!(b.apply_brush_preset(&loaded[0], &tips).is_none(), "tip found");
    assert_eq!(b.brush.settings, a.brush.settings);
    assert_eq!(b.brush.options, a.brush.options);
    assert_eq!(b.brush.material, a.brush.material);
    assert_eq!(b.brush.alpha_name, "Splatter");
    assert!(b.brush.alpha.is_some());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn missing_tip_falls_back_to_round_with_a_message() {
    let (dir, lib) = temp_lib("missing");
    let tips = alphas::builtin();
    let mut a = Session::new(primitives::cube(1.0), 128);
    let mut preset = a.brush_preset("Gone");
    preset.tip = Some("Deleted Tip".into());
    let saved = save_brush_preset(&lib, &preset).unwrap();
    let msg = a.apply_brush_preset(&saved, &tips).expect("reports the missing tip");
    assert!(msg.contains("Deleted Tip"));
    assert_eq!(a.brush.alpha_name, "Round");
    assert!(a.brush.alpha.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// QA 2026-11: a preset's settings and stroke options were applied without the checks of
/// SetBrushSettings / SetStrokeOptions (an infinite radial centre, smoothing past its limit).
#[test]
fn preset_values_pass_the_same_checks_as_the_brush_commands() {
    let tips = alphas::builtin();
    let a = Session::new(primitives::cube(1.0), 128);
    let mut preset = a.brush_preset("Odd");
    preset.options.smoothing = 5.0;
    preset.options.radial.count = 10_000;
    preset.settings.size = 1e9;
    let mut b = Session::new(primitives::cube(1.0), 128);
    assert!(b.apply_brush_preset(&preset, &tips).is_none());
    assert_eq!(b.brush.options.smoothing, subcraft_paint::MAX_SMOOTHING, "clamped like SetStrokeOptions");
    assert_eq!(b.brush.options.radial.count, subcraft_paint::MAX_SYMMETRY_COPIES);
    assert_eq!(b.brush.settings.size, 100.0, "clamped like SetBrushSettings");
    let mut broken = a.brush_preset("Broken");
    broken.options.radial.center_offset = [f32::INFINITY, 0.0, 0.0];
    broken.settings.size = 3.0;
    let before = (b.brush.settings, b.brush.options);
    let message = b.apply_brush_preset(&broken, &tips).expect("refused with a message");
    assert!(message.contains("radial") && message.contains("unchanged"), "{message}");
    assert_eq!((b.brush.settings, b.brush.options), before, "the brush is unchanged");
    let mut nan = a.brush_preset("NaN");
    nan.settings.flow = f32::NAN;
    assert!(b.apply_brush_preset(&nan, &tips).is_some_and(|m| m.contains("finite")));
    assert_eq!((b.brush.settings, b.brush.options), before);
}

#[test]
fn old_presets_without_new_fields_still_load() {
    let (dir, lib) = temp_lib("compat");
    let folder = lib.folder(UserAssetKind::BrushPreset);
    std::fs::create_dir_all(&folder).unwrap();
    // A minimal file: missing settings fields, options and material use defaults.
    std::fs::write(folder.join("Minimal.scbrush"), r#"{"format":"subcraft-brush","version":1,"name":"x","tip":null,"settings":{"size":20.0}}"#).unwrap();
    std::fs::write(folder.join("Other.scbrush"), r#"{"format":"something-else","version":1,"name":"x","tip":null,"settings":{}}"#).unwrap();
    std::fs::write(folder.join("Broken.scbrush"), "{").unwrap();
    let (loaded, errors) = load_brush_presets(&lib);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "Minimal", "shelf name comes from the file name");
    assert_eq!(loaded[0].settings.size, 20.0);
    assert_eq!(loaded[0].settings.flow, subcraft_paint::BrushSettings::default().flow);
    assert_eq!(errors.len(), 2, "{errors:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn stabilizer_and_scatter_settings_roundtrip_and_old_presets_load() {
    let (dir, lib) = temp_lib("stab-scatter");
    let tips = alphas::builtin();
    let mut a = Session::new(primitives::cube(1.0), 128);
    a.brush.options.smoothing = 0.7;
    a.brush.options.catch_up = true;
    a.brush.options.lazy_distance_px = 18.0;
    a.brush.settings.scatter_count = 9;
    a.brush.settings.scatter_radius = 1.75;
    a.brush.settings.scatter_size_random = 0.4;
    a.brush.settings.scatter_rotation_deg = 90.0;
    a.brush.settings.scatter_opacity_random = 0.2;
    a.brush.settings.scatter_fall = 3.0;
    save_brush_preset(&lib, &a.brush_preset("Dust")).unwrap();
    let (loaded, errors) = load_brush_presets(&lib);
    assert!(errors.is_empty(), "{errors:?}");
    let mut b = Session::new(primitives::cube(1.0), 128);
    assert!(b.apply_brush_preset(&loaded[0], &tips).is_none());
    assert_eq!(b.brush.settings, a.brush.settings);
    assert_eq!(b.brush.options, a.brush.options);
    // A preset written before these fields existed still loads, with them off.
    let old = r#"{"format":"subcraft-brush","version":2,"name":"Old","tip":null,"settings":{"size":3.0},"options":{"lazy_distance_px":4.0}}"#;
    let p: subcraft_app::brush_preset::BrushPreset = serde_json::from_str(old).unwrap();
    assert_eq!(p.settings.scatter_count, 0);
    assert_eq!(p.options.smoothing, 0.0);
    assert!(!p.options.catch_up);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn presets_saved_before_radial_symmetry_load_with_radial_off() {
    let (dir, lib) = temp_lib("pre-radial");
    std::fs::create_dir_all(lib.folder(UserAssetKind::BrushPreset)).unwrap();
    let path = lib.folder(UserAssetKind::BrushPreset).join("Old.scbrush");
    // A version-2 preset written before radial symmetry existed: `options` has no `radial` key.
    let mut json: serde_json::Value = serde_json::to_value(Session::new(primitives::cube(1.0), 64).brush_preset("Old")).unwrap();
    json["options"] = serde_json::json!({ "lazy_distance_px": 10.0, "symmetry": [false, true, false] });
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let preset = subcraft_app::brush_preset::load_brush_preset(&path).unwrap();
    assert_eq!(preset.options.symmetry, [false, true, false]);
    assert_eq!(preset.options.radial, subcraft_paint::RadialSymmetry::default());
    assert!(!preset.options.radial.enabled);
    assert_eq!(preset.options.copies(), 2);
    std::fs::remove_dir_all(&dir).ok();
}
