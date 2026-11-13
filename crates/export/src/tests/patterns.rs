//! File name patterns: Painter's keywords, sub-folders, optional parts, and per-map file types
//! of one texture set.
use super::*;

fn vars<'a>(udim: Option<u32>, color_space: &'a str) -> PatternVars<'a> {
    PatternVars { mesh: "Crate", project: "My Project", texture_set: "Body", scene_material: "M_Body", udim, suffix: "BaseColor", color_space, ..Default::default() }
}

#[test]
fn keywords_subfolders_and_optional_parts() {
    let v = vars(None, "sRGB");
    assert_eq!(expand_pattern_with("$project_$sceneMaterial_$suffix_$colorSpace", &v), "My_Project_M_Body_BaseColor_sRGB");
    assert_eq!(expand_pattern_with("$textureSet/$mesh_$suffix", &v), "Body/Crate_BaseColor", "/ makes a sub-folder");
    assert_eq!(expand_pattern_with("/../$textureSet//x/", &v), "Body/x", "no empty, leading or .. folders");
    assert_eq!(expand_pattern_with(r"a\b", &v), "a_b", "a backslash is not a separator");
    // Optional parts: written only when their tokens have values, without the parentheses.
    assert_eq!(expand_pattern_with("$textureSet_$suffix(.$udim)", &v), "Body_BaseColor");
    assert_eq!(expand_pattern_with("$textureSet_$suffix(.$udim)", &vars(Some(1002), "")), "Body_BaseColor.1002");
    assert_eq!(expand_pattern_with("$suffix(_$colorSpace)", &vars(None, "")), "BaseColor");
    assert_eq!(expand_pattern_with("$suffix(_$colorSpace)", &v), "BaseColor_sRGB");
    assert_eq!(expand_pattern_with("$suffix(v2)", &v), "BaseColorv2", "a part without tokens is kept");
    assert_eq!(expand_pattern_with("$suffix(open", &v), "BaseColor_open", "an unclosed ( is a plain character");
    // The classic four-token form still expands as before.
    assert_eq!(expand_pattern("$mesh_$textureSet_$suffix.$udim", "Crate", "Body", None, "N"), "Crate_Body_N");
    // Every keyword is offered by the $ menu.
    for tok in ["$mesh", "$project", "$textureSet", "$sceneMaterial", "$udim", "$suffix", "$colorSpace"] {
        assert!(PATTERN_TOKENS.contains(&tok), "{tok}");
    }
}

#[test]
fn color_space_names_follow_the_encoding() {
    let color = OutputMap::new("C", FileFormat::Png8, (0..3).map(|c| Source::ch(ChannelKind::BaseColor, c)).collect(), true);
    let data = OutputMap::new("R", FileFormat::Png8, vec![Source::ch(ChannelKind::Roughness, 0)], true);
    assert_eq!(color_space(&color, FileFormat::Png8), "sRGB");
    assert_eq!(color_space(&color, FileFormat::Exr16), "Linear");
    assert_eq!(color_space(&data, FileFormat::Png8), "Raw");
    let unencoded = OutputMap { srgb: false, ..color };
    assert_eq!(color_space(&unencoded, FileFormat::Png8), "Raw");
}

#[test]
fn exports_fill_project_material_and_colour_space_and_write_sub_folders() {
    let mesh = primitives::plane(2.0, 1);
    let ch = distinct_channels();
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let map = |suffix: &str, pattern: &str, comps: Vec<Source>, srgb| OutputMap { pattern: Some(pattern.into()), ..OutputMap::new(suffix, FileFormat::Png8, comps, srgb) };
    let template = ExportTemplate {
        name: "Mine".into(),
        maps: vec![
            map("BaseColor", "$project/$sceneMaterial_$suffix_$colorSpace", (0..3).map(|c| Source::ch(ChannelKind::BaseColor, c)).collect(), true),
            map("Rough", "$project/maps/$textureSet_$suffix(_$colorSpace)", vec![Source::ch(ChannelKind::Roughness, 0)], false),
        ],
        id: Some(preset::new_id()),
        ..Default::default()
    };
    let mut s = ExportSettings { template, project_name: Some("Hero".into()), ..Default::default() };
    s.scene_materials.insert("Body".into(), "lambert2".into());
    let plan: Vec<String> = plan_all("M", &sets, &s).into_iter().map(|f| f.path).collect();
    assert_eq!(plan, ["Hero/lambert2_BaseColor_sRGB.png", "Hero/maps/Body_Rough_Raw.png"]);
    let dir = tmp("patterns");
    let files = export(&mesh, "M", &sets, &s, &dir).unwrap();
    assert_eq!(files.len(), 2);
    assert!(dir.join("Hero").join("maps").join("Body_Rough_Raw.png").is_file(), "sub-folders are created");
    std::fs::remove_dir_all(dir).unwrap();
    // Without a project name, $project is the mesh name passed to the export.
    s.project_name = None;
    assert!(plan_all("M", &sets, &s)[0].path.starts_with("M/"));
}

#[test]
fn a_texture_set_can_write_one_map_in_its_own_file_type() {
    let ch = distinct_channels();
    let sets = [
        ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY },
        ExportTextureSet { name: "Trim", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY },
    ];
    let mut s = ExportSettings::default();
    let mut o = subcraft_doc::ExportOverrides::default();
    o.maps.insert("Normal".into(), subcraft_doc::MapFormat { format: FileFormat::Exr16, dither: false });
    s.set_overrides.insert("Trim".into(), o);
    let plan: Vec<String> = plan_all("M", &sets, &s).into_iter().map(|f| f.path).collect();
    assert!(plan.contains(&"M_Trim_Normal.exr".to_string()) && plan.contains(&"M_Body_Normal.png".to_string()), "{plan:?}");
    assert!(plan.contains(&"M_Trim_BaseColor.png".to_string()), "other maps keep the template's type");
    // The per-map type wins over the set's and the window's file type.
    s.format_override = Some(FileFormat::Tga8);
    let plan: Vec<String> = plan_all("M", &sets, &s).into_iter().map(|f| f.path).collect();
    assert!(plan.contains(&"M_Trim_Normal.exr".to_string()) && plan.contains(&"M_Trim_BaseColor.tga".to_string()), "{plan:?}");
}

#[test]
fn the_folder_template_writes_one_folder_per_texture_set() {
    let ch = distinct_channels();
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let t = ExportTemplate::builtin().into_iter().find(|t| t.name == "PBR Metallic Roughness (Folder or PSD Groups)").unwrap();
    let s = ExportSettings { template: t, ..Default::default() };
    let plan = plan_all("M", &sets, &s);
    assert!(!plan.is_empty() && plan.iter().all(|f| f.path.starts_with("Body/M_Body_")), "{plan:?}");
}
