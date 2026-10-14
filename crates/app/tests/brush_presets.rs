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
