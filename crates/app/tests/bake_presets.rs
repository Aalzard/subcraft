//! Bake presets: settings saved under a name, reused in another project without its files.
use subcraft_app::bake_presets::{self, BakePreset, BakePresets};
use subcraft_core::{MeshMapKind, ObjectId, TextureSetId};
use subcraft_doc::BakeConfig;

fn temp() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("bake-presets-{}", ObjectId::new()))
}

fn project_config() -> BakeConfig {
    BakeConfig {
        high_poly: "/art/crate_high.fbx".into(),
        extra_high: vec!["/art/bolts_high.fbx".into()],
        cage_file: "/art/cage.obj".into(),
        skip_sets: vec![TextureSetId::new()],
        vertex_color_set: 1,
        size: Some(2048),
        maps: vec![MeshMapKind::Normal, MeshMapKind::AmbientOcclusion],
        ao_samples: 128,
        antialiasing: 4,
        dilation: 8,
        ..BakeConfig::default()
    }
}

#[test]
fn a_saved_preset_keeps_the_settings_but_not_the_files_and_survives_a_reload() {
    let dir = temp();
    let mut presets = BakePresets::load_dir(Some(dir.clone()));
    let saved = presets.save("Crate AO", &project_config()).unwrap();
    assert!(saved.config.high_poly.is_empty() && saved.config.extra_high.is_empty() && saved.config.cage_file.is_empty());
    assert!(saved.config.skip_sets.is_empty() && saved.config.vertex_color_set == 0);
    assert_eq!((saved.config.ao_samples, saved.config.antialiasing, saved.config.size), (128, 4, Some(2048)));

    let reloaded = BakePresets::load_dir(Some(dir.clone()));
    assert!(reloaded.load_errors.is_empty(), "{:?}", reloaded.load_errors);
    let p = reloaded.find(saved.key()).expect("saved preset is listed after a reload");
    assert_eq!(p, &saved);
    // Saving the same name again replaces it instead of adding a second file.
    let mut again = reloaded.clone();
    again.save("crate ao", &BakeConfig { ao_samples: 16, ..project_config() }).unwrap();
    assert_eq!(again.user().len(), 1);
    assert_eq!(BakePresets::load_dir(Some(dir.clone())).user()[0].config.ao_samples, 16);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn applying_a_preset_keeps_the_projects_files_and_texture_sets() {
    let other = BakeConfig { high_poly: "/other/high.obj".into(), cage_file: "/other/cage.obj".into(), skip_sets: vec![TextureSetId::new()], ..BakeConfig::default() };
    let preset = BakePreset::new("P", &project_config());
    let applied = preset.apply_to(&other);
    assert_eq!(applied.high_poly, other.high_poly);
    assert_eq!(applied.cage_file, other.cage_file);
    assert_eq!(applied.skip_sets, other.skip_sets);
    assert_eq!(applied.maps, project_config().maps);
    assert_eq!(applied.ao_samples, 128);
    assert!(preset.matches(&applied), "the applied settings are recognised as the preset");
    assert!(!preset.matches(&other));
}

#[test]
fn a_preset_saved_from_a_built_ins_settings_shows_under_its_own_name() {
    let mut presets = BakePresets::load_dir(None);
    let high = presets.find("High quality").unwrap().config.clone();
    assert_eq!(presets.matching(&high).unwrap().name, "High quality");
    presets.save("Mine", &high).unwrap();
    assert_eq!(presets.matching(&high).unwrap().name, "Mine");
}

#[test]
fn built_ins_come_first_cannot_be_deleted_or_shadowed_and_files_are_versioned() {
    let dir = temp();
    let mut presets = BakePresets::load_dir(Some(dir.clone()));
    let names: Vec<String> = presets.all().map(|p| p.name.clone()).collect();
    assert_eq!(names, ["Fast preview", "Standard", "High quality"]);
    assert!(presets.delete("Standard").is_err());
    let copy = presets.save("Standard", &project_config()).unwrap();
    assert_eq!(copy.name, "Standard 2", "a built-in's name gets a number");
    presets.delete(copy.key()).unwrap();
    assert!(presets.user().is_empty() && std::fs::read_dir(&dir).unwrap().next().is_none(), "the file is gone too");

    // Files from a newer SubCraft are refused with a clear message; broken files are listed.
    std::fs::write(dir.join("new.json"), bake_presets::to_json(&BakePreset::new("N", &project_config())).replace("\"version\": 1", "\"version\": 9")).unwrap();
    std::fs::write(dir.join("broken.json"), "{").unwrap();
    let loaded = BakePresets::load_dir(Some(dir.clone()));
    assert!(loaded.user().is_empty());
    assert_eq!(loaded.load_errors.len(), 2);
    assert!(loaded.load_errors.iter().any(|e| e.contains("newer SubCraft")), "{:?}", loaded.load_errors);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn without_a_library_folder_presets_live_for_the_session() {
    let mut presets = BakePresets::load_dir(None);
    let p = presets.save("Mine", &project_config()).unwrap();
    assert!(presets.find(p.key()).is_some());
    presets.delete(p.key()).unwrap();
    assert!(presets.user().is_empty());
}
