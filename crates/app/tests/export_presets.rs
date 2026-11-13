//! Export preset library: create / duplicate / rename / delete / import / export, persisted in
//! the user library, and a user preset driving a real session export.

use subcraft_app::Session;
use subcraft_app::assets::UserLibrary;
use subcraft_app::export_presets::ExportPresets;
use subcraft_export::{ExportSettings, Padding, Source};
use subcraft_image::FileFormat;

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-presets-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn presets_persist_in_the_user_library() {
    let root = temp("lib");
    let lib = UserLibrary::new(&root);
    let mut presets = ExportPresets::load(Some(&lib));
    assert_eq!(presets.user().len(), 0);
    assert_eq!(presets.builtin().len(), subcraft_export::ExportTemplate::builtin().len());

    let new = presets.create().unwrap();
    assert_eq!(new.name, "New preset");
    assert_eq!(presets.create().unwrap().name, "New preset (2)");
    let copy = presets.duplicate("Godot 4 (ORM)").unwrap();
    assert_eq!(copy.name, "Godot 4 (ORM) copy");
    assert_eq!(copy.maps, subcraft_export::ExportTemplate::godot().maps);

    // Built-ins are read-only.
    assert!(presets.update(subcraft_export::ExportTemplate::godot()).is_err());
    assert!(presets.delete("Godot 4 (ORM)").is_err());
    assert!(presets.rename("Godot 4 (ORM)", "x").is_err());

    let id = copy.id.clone().unwrap();
    let renamed = presets.rename(&id, "Godot 16-bit").unwrap();
    assert_eq!(renamed.name, "Godot 16-bit");
    assert_eq!(presets.rename(&id, "New preset").unwrap().name, "New preset (3)", "names stay unique");
    let mut edited = presets.find(&id).unwrap().clone();
    edited.maps[1].format = FileFormat::Png16;
    edited.padding = Some(Padding::Texels(4));
    presets.update(edited.clone()).unwrap();
    let mut bad = edited.clone();
    bad.maps[1].suffix = bad.maps[0].suffix.clone();
    assert!(presets.update(bad).is_err(), "invalid presets are not saved");

    // A fresh load reads the same presets back.
    let again = ExportPresets::load(Some(&lib));
    assert_eq!(again.user().len(), 3);
    assert_eq!(again.find(&id), Some(&edited));
    assert!(root.join("export-presets").join(format!("{id}.json")).is_file());

    presets.delete(new.id.as_deref().unwrap()).unwrap();
    assert_eq!(ExportPresets::load(Some(&lib)).user().len(), 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn export_and_import_round_trip_and_read_painter_presets() {
    let root = temp("io");
    let lib = UserLibrary::new(root.join("lib"));
    let mut presets = ExportPresets::load(Some(&lib));
    std::fs::create_dir_all(&root).unwrap();
    // Built-ins can be exported too; importing makes an editable copy with a fresh id.
    let file = root.join("ue.json");
    presets.export_to("Unreal Engine (ORM packed)", &file).unwrap();
    let (added, warnings) = presets.import(&file).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(added[0].name, "Unreal Engine (ORM packed) (2)");
    assert!(!added[0].is_builtin());
    assert_eq!(added[0].maps, subcraft_export::ExportTemplate::unreal().maps);

    let spexp = root.join("Studio.spexp");
    std::fs::write(&spexp, r#"{"exportPresets":[{"name":"Studio","maps":[{"fileName":"$mesh_$textureSet_ORM","channels":[
        {"destChannel":"R","srcChannel":"R","srcMapType":"documentMap","srcMapName":"ambientOcclusion"},
        {"destChannel":"G","srcChannel":"R","srcMapType":"documentMap","srcMapName":"roughness"},
        {"destChannel":"B","srcChannel":"R","srcMapType":"documentMap","srcMapName":"metallic"}],
        "parameters":{"fileFormat":"png","bitDepth":"16"}}]}]}"#).unwrap();
    let (added, _) = presets.import(&spexp).unwrap();
    assert_eq!(added[0].name, "Studio");
    assert_eq!(added[0].maps[0].format, FileFormat::Png16);
    assert!(ExportPresets::load(Some(&lib)).find(added[0].key()).is_some(), "imported presets are saved");
    assert!(presets.import(&root.join("missing.json")).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn without_a_library_presets_live_for_the_session() {
    let mut presets = ExportPresets::load(None);
    let p = presets.create().unwrap();
    assert!(presets.find(p.key()).is_some());
}

#[test]
fn a_user_preset_drives_a_session_export() {
    let dir = temp("session");
    let mut presets = ExportPresets::load(None);
    let mut p = presets.duplicate("PBR Metallic Roughness").unwrap();
    p.maps.retain(|m| m.suffix == "BaseColor" || m.suffix == "Roughness");
    p.maps[0].pattern = Some("T_$textureSet_D".into());
    p.maps[0].format = FileFormat::Tga8;
    p.maps[1].components = vec![Source::Gray { kind: subcraft_core::ChannelKind::BaseColor, invert: true }];
    p.maps[1].format = FileFormat::Tiff16;
    p.maps[1].size = Some(64);
    let p = presets.update(p).unwrap();
    let mut session = Session::sample();
    let files = session.export_textures(&dir, &ExportSettings { template: p, ..Default::default() }).unwrap();
    let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert!(!names.is_empty() && names.iter().all(|n| n.starts_with("T_") && n.ends_with("_D.tga") || n.ends_with("_Roughness.tif")), "{names:?}");
    let tif = files.iter().find(|f| f.extension().is_some_and(|e| e == "tif")).unwrap();
    let b = subcraft_image::load_bitmap(tif).unwrap();
    assert_eq!(b.width, 64, "size override");
    assert!(matches!(b.data, subcraft_image::BitmapData::U16(_)), "16-bit TIFF");
    std::fs::remove_dir_all(dir).unwrap();
}
