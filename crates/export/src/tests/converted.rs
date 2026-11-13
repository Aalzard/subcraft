//! Painter's converted maps and the predefined / default templates built on them.
use super::*;

fn decode(name: &str, bytes: &[u8]) -> subcraft_image::Bitmap {
    subcraft_image::io::decode_bitmap(bytes, Some(name)).unwrap()
}

fn render(sets: &[ExportTextureSet], template: ExportTemplate) -> Vec<(String, subcraft_image::Bitmap)> {
    let mesh = primitives::plane(2.0, 1);
    let s = ExportSettings { template, padding: Padding::None, height_to_normal: 0.0, ..Default::default() };
    let mut out = Vec::new();
    render_maps(&mesh, "M", sets, &s, |n, b| {
        out.push((n.to_string(), decode(n, b)));
        Ok(())
    })
    .unwrap();
    out
}

fn user(maps: Vec<OutputMap>) -> ExportTemplate {
    ExportTemplate { name: "Mine".into(), maps, id: Some(preset::new_id()), ..Default::default() }
}

fn material() -> BTreeMap<ChannelKind, TiledImage> {
    let mut m = BTreeMap::new();
    m.insert(ChannelKind::BaseColor, TiledImage::new(RES, RES, 4, [0.8, 0.4, 0.2, 1.0]));
    m.insert(ChannelKind::Metallic, TiledImage::new(RES, RES, 1, [0.25; 4]));
    m.insert(ChannelKind::Roughness, TiledImage::new(RES, RES, 1, [0.3; 4]));
    m.insert(ChannelKind::Specular, TiledImage::new(RES, RES, 1, [0.5; 4]));
    m.insert(ChannelKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.6; 4]));
    m.insert(ChannelKind::Normal, TiledImage::new(RES, RES, 4, [0.5, 0.5, 1.0, 1.0]));
    m
}

fn conv_map(map: Converted, format: FileFormat) -> OutputMap {
    let comps = (0..map.components() as u8).map(|component| Source::Converted { map, component }).collect();
    OutputMap::new(&map.label().replace(['/', ' '], ""), format, comps, true)
}

#[test]
fn converted_maps_follow_their_formulas() {
    let ch = material();
    let mut baked = BTreeMap::new();
    baked.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.5; 4]));
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: Some(&baked), mixing: subcraft_doc::MixingSettings::LEGACY }];
    // EXR keeps the linear values exactly (and colour maps are not sRGB-encoded in float files).
    let maps = Converted::ALL.iter().map(|m| conv_map(*m, FileFormat::Exr32)).collect();
    let files = render(&sets, user(maps));
    assert_eq!(files.len(), Converted::ALL.len());
    let px = |map: Converted| files.iter().find(|f| f.0.contains(&format!("_{}.", map.label().replace(['/', ' '], "")))).unwrap().1.texel(7, 7, false);
    let base = [0.8f32, 0.4, 0.2];
    let (metal, f0) = (0.25f32, 0.04f32);
    for c in 0..3 {
        assert!((px(Converted::Diffuse)[c] - base[c] * (1.0 - metal)).abs() < 1e-6, "diffuse");
        assert!((px(Converted::Specular)[c] - (f0 + (base[c] - f0) * metal)).abs() < 1e-6, "specular");
        assert!((px(Converted::Reflection)[c] - (1.0 + (base[c] - 1.0) * metal)).abs() < 1e-6, "reflection");
    }
    assert!((px(Converted::Glossiness)[0] - 0.7).abs() < 1e-6);
    assert!((px(Converted::GlossinessSquared)[0] - 0.49).abs() < 1e-6);
    assert!((px(Converted::MixedAo)[0] - 0.3).abs() < 1e-6, "AO channel x baked AO");
    assert!((px(Converted::F0)[0] - 0.04).abs() < 1e-6);
    assert!((px(Converted::InverseIor)[0] - 1.0 / 1.5).abs() < 1e-5, "specular level 0.5 = IOR 1.5");
}

#[test]
fn mixed_ao_uses_whichever_ao_exists_and_colour_maps_are_srgb_in_8_bit() {
    let mut ch = material();
    ch.remove(&ChannelKind::AmbientOcclusion);
    let mut baked = BTreeMap::new();
    baked.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.5; 4]));
    let only_baked = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: Some(&baked), mixing: subcraft_doc::MixingSettings::LEGACY }];
    let ao = |sets: &[ExportTextureSet]| render(sets, user(vec![conv_map(Converted::MixedAo, FileFormat::Exr32)]))[0].1.texel(3, 3, false)[0];
    assert!((ao(&only_baked) - 0.5).abs() < 1e-6, "baked only");
    let full = material();
    let only_painted = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &full, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    assert!((ao(&only_painted) - 0.6).abs() < 1e-6, "channel only");
    // QA 2026-11: with AO mixing Multiply (the default) the bake was applied twice (0.6 x 0.5 x 0.5).
    let both = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &full, mesh_maps: Some(&baked), mixing: subcraft_doc::MixingSettings::default() }];
    assert_eq!(both[0].mixing.ao, subcraft_doc::AoMixing::Multiply);
    assert!((ao(&both) - 0.3).abs() < 1e-6, "channel x bake, once: {}", ao(&both));
    let diffuse = render(&only_painted, user(vec![conv_map(Converted::Diffuse, FileFormat::Png8)]));
    let d = diffuse[0].1.texel(3, 3, false);
    assert!((d[0] - linear_to_srgb(0.6)).abs() < 1.0 / 255.0, "8-bit colour maps are sRGB: {d:?}");
    // A texture set with none of a converted map's inputs doesn't get the file.
    let mut only_height = BTreeMap::new();
    only_height.insert(ChannelKind::Height, TiledImage::new(RES, RES, 1, [0.0; 4]));
    let none = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &only_height, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    assert!(render(&none, user(vec![conv_map(Converted::Glossiness, FileFormat::Png8)])).is_empty());
}

#[test]
fn every_builtin_template_is_valid_and_uniquely_named() {
    let all = ExportTemplate::builtin();
    let mut names = std::collections::HashSet::new();
    for t in &all {
        assert!(t.problems().is_empty(), "{}: {:?}", t.name, t.problems());
        assert!(t.is_builtin(), "{}", t.name);
        assert!(names.insert(t.name.clone()), "duplicate name {}", t.name);
    }
    assert_eq!(all[0].name, "PBR Metallic Roughness", "the first nine keep their order");
    assert_eq!(all[8].name, "2D View");
    for name in [
        "Blender (Principled BSDF)",
        "Arnold (AiStandard)",
        "Corona",
        "PBR Specular Glossiness (converted from Metallic Roughness)",
        "Unity HD Render Pipeline (Specular)",
        "Unity Universal Render Pipeline (Specular)",
        "Vray Next (Specular Glossiness)",
        "Mesh Maps",
        "Document channels + Normal + AO (No Alpha)",
        "Roblox (MaterialVariant)",
    ] {
        assert!(names.contains(name), "{name}");
    }
    assert!(all.len() >= 30, "{}", all.len());
}

#[test]
fn painter_templates_export_a_metal_roughness_material() {
    let ch = material();
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    for t in crate::builtin_presets::painter().into_iter().filter(|t| t.name != "Mesh Maps") {
        let files = render(&sets, t.clone());
        assert!(!files.is_empty(), "{} writes files", t.name);
        for (name, b) in &files {
            assert_eq!((b.width, b.height), (RES, RES), "{} {name}", t.name);
        }
    }
    // Spec / gloss conversion: the diffuse of a 25% metal is darker than its base color.
    let files = render(&sets, ExportTemplate::builtin().into_iter().find(|t| t.name == "Vray Next (Specular Glossiness)").unwrap());
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert!(names.contains(&"M_P_Diffuse.png") && names.contains(&"M_P_Reflection.png") && names.contains(&"M_P_Glossiness.png"), "{names:?}");
}

#[test]
fn document_channels_and_mesh_maps_write_only_what_exists() {
    let mut ch = BTreeMap::new();
    ch.insert(ChannelKind::BaseColor, TiledImage::new(RES, RES, 4, [0.5, 0.5, 0.5, 1.0]));
    ch.insert(ChannelKind::User1, TiledImage::new(RES, RES, 1, [0.2; 4]));
    let mut baked = BTreeMap::new();
    baked.insert(MeshMapKind::Curvature, TiledImage::new(RES, RES, 1, [0.5; 4]));
    baked.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.7; 4]));
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: Some(&baked), mixing: subcraft_doc::MixingSettings::LEGACY }];
    let find = |name: &str| ExportTemplate::builtin().into_iter().find(|t| t.name == name).unwrap();
    let names = |t| render(&sets, t).into_iter().map(|f| f.0).collect::<Vec<_>>();
    assert_eq!(names(find("Document channels + Normal + AO (No Alpha)")), ["M_P_BaseColor.png", "M_P_User1.png", "M_P_AO.png"]);
    let mesh = names(find("Mesh Maps"));
    assert_eq!(mesh.len(), 2, "only the baked maps: {mesh:?}");
    assert!(mesh.iter().any(|n| n.contains("Curvature")) && mesh.iter().any(|n| n.contains("Ambient")), "{mesh:?}");
}

#[test]
fn udim_legacy_templates_name_tiles_explicitly() {
    let ch = material();
    let sets = [ExportTextureSet { name: "Body.1001", material_slot: 0, udim: Some(1001), channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let t = ExportTemplate::builtin().into_iter().find(|t| t.name == "Vray UDIM Legacy (Metallic Roughness)").unwrap();
    let files = render(&sets, t);
    assert!(files.iter().any(|f| f.0 == "M_Body_BaseColor.1001.png"), "{:?}", files.iter().map(|f| &f.0).collect::<Vec<_>>());
}
