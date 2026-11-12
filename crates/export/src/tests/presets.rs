//! User presets: channel packing, bit depth, per-map padding / size / normal format, file name
//! patterns, the JSON file and Painter preset import.
use super::*;
use subcraft_image::{BitDepth, BitmapData, Container};

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.5 / 255.0
}

fn set<'a>(ch: &'a BTreeMap<ChannelKind, TiledImage>, baked: Option<&'a BTreeMap<MeshMapKind, TiledImage>>) -> ExportTextureSet<'a> {
    ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: ch, mesh_maps: baked }
}

fn user(maps: Vec<OutputMap>) -> ExportTemplate {
    ExportTemplate { name: "Mine".into(), maps, id: Some(preset::new_id()), ..Default::default() }
}

/// Render `template` and return `(file name, bytes)` per file.
fn render(template: ExportTemplate, sets: &[ExportTextureSet], tweak: impl FnOnce(&mut ExportSettings)) -> Vec<(String, Vec<u8>)> {
    let mesh = primitives::plane(2.0, 1);
    let mut s = ExportSettings { template, ..Default::default() };
    tweak(&mut s);
    let mut out = Vec::new();
    render_maps(&mesh, "M", sets, &s, |n, b| {
        out.push((n.to_string(), b.to_vec()));
        Ok(())
    })
    .unwrap();
    out
}

fn decode(name: &str, bytes: &[u8]) -> subcraft_image::Bitmap {
    subcraft_image::io::decode_bitmap(bytes, Some(name)).unwrap()
}

#[test]
fn custom_packing_reads_each_slot_from_its_source() {
    let ch = distinct_channels();
    let mut baked = BTreeMap::new();
    baked.insert(MeshMapKind::Curvature, TiledImage::new(RES, RES, 1, [0.35; 4]));
    let map = OutputMap::new(
        "Packed",
        FileFormat::Png8,
        vec![
            Source::ch(ChannelKind::AmbientOcclusion, 0),
            Source::Channel { kind: ChannelKind::Roughness, component: 0, invert: true },
            Source::MeshMap { kind: MeshMapKind::Curvature, component: 0, invert: false },
            Source::Constant { value: 0.4 },
        ],
        false,
    );
    let files = render(user(vec![map]), &[set(&ch, Some(&baked))], |_| {});
    assert_eq!(files.len(), 1);
    let b = decode(&files[0].0, &files[0].1);
    assert_eq!(b.channels, 4);
    let p = b.texel(9, 9, false);
    assert!(near(p[0], 0.6) && near(p[1], 0.7) && near(p[2], 0.35) && near(p[3], 0.4), "AO / 1-rough / curvature / 0.4: {p:?}");
}

#[test]
fn grayscale_conversion_uses_rec709_luminance_and_srgb_for_color() {
    let mut ch = distinct_channels();
    ch.insert(ChannelKind::BaseColor, TiledImage::new(RES, RES, 4, [0.8, 0.2, 0.1, 1.0]));
    let luma = 0.2126 * 0.8 + 0.7152 * 0.2 + 0.0722 * 0.1;
    let gray = OutputMap::new("Gray", FileFormat::Png8, vec![Source::Gray { kind: ChannelKind::BaseColor, invert: false }], true);
    let linear = OutputMap::new("GrayLinear", FileFormat::Png8, vec![Source::Gray { kind: ChannelKind::BaseColor, invert: true }], false);
    let rough = OutputMap::new("GrayRough", FileFormat::Png8, vec![Source::Gray { kind: ChannelKind::Roughness, invert: false }], true);
    let files = render(user(vec![gray, linear, rough]), &[set(&ch, None)], |_| {});
    let v = |i: usize| decode(&files[i].0, &files[i].1).texel(5, 5, false)[0];
    assert!(near(v(0), linear_to_srgb(luma)), "{} vs {}", v(0), linear_to_srgb(luma));
    assert!(near(v(1), 1.0 - luma), "inverted, linear: {}", v(1));
    assert!(near(v(2), 0.3), "data channels are never color-transformed: {}", v(2));
}

#[test]
fn bit_depths_store_the_requested_precision() {
    let mut ch = distinct_channels();
    // A value 8-bit can't represent, 16-bit nearly can, float exactly.
    let v = 0.123_456_7f32;
    ch.insert(ChannelKind::Roughness, TiledImage::new(RES, RES, 1, [v; 4]));
    let gray = vec![Source::ch(ChannelKind::Roughness, 0)];
    let maps: Vec<OutputMap> = [FileFormat::Png8, FileFormat::Png16, FileFormat::Tiff8, FileFormat::Tiff16, FileFormat::Tiff32, FileFormat::Exr32, FileFormat::Tga8]
        .iter()
        .enumerate()
        .map(|(i, f)| OutputMap::new(&format!("R{i}"), *f, gray.clone(), false))
        .collect();
    let files = render(user(maps), &[set(&ch, None)], |_| {});
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names, ["M_P_R0.png", "M_P_R1.png", "M_P_R2.tif", "M_P_R3.tif", "M_P_R4.tif", "M_P_R5.exr", "M_P_R6.tga"]);
    for (name, bytes) in &files {
        let b = decode(name, bytes);
        let got = b.texel(3, 3, false)[0];
        match &b.data {
            BitmapData::U8(_) => assert_eq!((got * 255.0).round(), (v * 255.0).round(), "{name}"),
            BitmapData::U16(_) => assert_eq!((got * 65535.0).round(), (v * 65535.0).round(), "{name}"),
            BitmapData::F32(_) => assert_eq!(got, v, "{name} is exact"),
            #[allow(unreachable_patterns)]
            _ => panic!("{name}: unexpected storage"),
        }
    }
    let depth = |n: &str| match decode(n, &files.iter().find(|f| f.0 == n).unwrap().1).data {
        BitmapData::U8(_) => 8,
        BitmapData::U16(_) => 16,
        _ => 32,
    };
    assert_eq!((depth("M_P_R2.tif"), depth("M_P_R3.tif"), depth("M_P_R4.tif")), (8, 16, 32));
}

#[test]
fn format_parts_round_trip_and_clamp_to_what_the_container_allows() {
    for f in FileFormat::ALL {
        assert_eq!(FileFormat::from_parts(f.container(), f.depth()), f);
        assert!(f.container().depths().contains(&f.depth()));
    }
    assert_eq!(FileFormat::from_parts(Container::Tga, BitDepth::B16), FileFormat::Tga8);
    assert_eq!(FileFormat::from_parts(Container::Png, BitDepth::F32), FileFormat::Png16);
    assert_eq!(FileFormat::from_parts(Container::Exr, BitDepth::B8), FileFormat::Exr16);
    assert_eq!(FileFormat::from_parts(Container::Jpeg, BitDepth::B16), FileFormat::Jpeg);
}

#[test]
fn per_map_padding_overrides_template_and_window() {
    // A plane whose UVs fill only the middle half: wide gutters all around.
    let mut mesh = primitives::plane(2.0, 1);
    mesh.uvs.iter_mut().for_each(|uv| *uv = *uv * 0.5 + glam::Vec2::splat(0.25));
    let cov = coverage(&mesh, 0, RES);
    let mut rough = TiledImage::new(RES, RES, 1, [0.0; 4]);
    for (i, &c) in cov.iter().enumerate() {
        if c {
            rough.set(i as u32 % RES, i as u32 / RES, [1.0; 4]);
        }
    }
    let mut ch = BTreeMap::new();
    ch.insert(ChannelKind::Roughness, rough);
    let sets = [ExportTextureSet { name: "C", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    // A gutter texel exactly 2 texels from the nearest island, and one far from all islands.
    let dist = |i: usize| {
        let (x, y) = ((i as u32 % RES) as i32, (i as u32 / RES) as i32);
        (0..cov.len()).filter(|j| cov[*j]).map(|j| ((j as u32 % RES) as i32 - x).abs().max(((j as u32 / RES) as i32 - y).abs())).min().unwrap()
    };
    let near_gutter = (0..cov.len()).find(|i| !cov[*i] && dist(*i) == 2).unwrap();
    let far_gutter = (0..cov.len()).filter(|i| !cov[*i]).max_by_key(|i| dist(*i)).unwrap();
    assert!(dist(far_gutter) > 8, "{}", dist(far_gutter));
    let src = vec![Source::ch(ChannelKind::Roughness, 0)];
    let map = |suffix: &str, padding: Option<Padding>| OutputMap { padding, ..OutputMap::new(suffix, FileFormat::Png8, src.clone(), false) };
    let mut t = user(vec![map("Inherit", None), map("None", Some(Padding::None)), map("Two", Some(Padding::Texels(2))), map("Inf", Some(Padding::Infinite))]);
    t.padding = Some(Padding::Texels(1));
    let mut files = Vec::new();
    let s = ExportSettings { template: t, padding: Padding::Infinite, ..Default::default() };
    render_maps(&mesh, "M", &sets, &s, |n, b| {
        files.push((n.to_string(), decode(n, b)));
        Ok(())
    })
    .unwrap();
    let at = |name: &str, i: usize| files.iter().find(|f| f.0.contains(name)).unwrap().1.texel(i as u32 % RES, i as u32 / RES, false)[0];
    assert_eq!((at("Inherit", near_gutter), at("Inherit", far_gutter)), (0.0, 0.0), "template's 1 texel wins over the window's Infinite");
    assert_eq!((at("None", near_gutter), at("None", far_gutter)), (0.0, 0.0));
    assert_eq!((at("Two", near_gutter), at("Two", far_gutter)), (1.0, 0.0), "2 texels reach the near gutter only");
    assert_eq!((at("Inf", near_gutter), at("Inf", far_gutter)), (1.0, 1.0));
}

#[test]
fn size_override_resamples_after_padding() {
    let mut ch = distinct_channels();
    let mut grad = TiledImage::new(RES, RES, 1, [0.0; 4]);
    for y in 0..RES {
        for x in 0..RES {
            grad.set(x, y, [if x < RES / 2 { 0.0 } else { 1.0 }; 4]);
        }
    }
    ch.insert(ChannelKind::Roughness, grad);
    let src = vec![Source::ch(ChannelKind::Roughness, 0)];
    let small = OutputMap { size: Some(32), ..OutputMap::new("Small", FileFormat::Png8, src.clone(), false) };
    let inherit = OutputMap::new("Inherit", FileFormat::Png8, src.clone(), false);
    let mut t = user(vec![small, inherit]);
    t.size = Some(256);
    let files = render(t, &[set(&ch, None)], |_| {});
    let (a, b) = (decode(&files[0].0, &files[0].1), decode(&files[1].0, &files[1].1));
    assert_eq!((a.width, b.width), (32, 256), "map size wins over the preset size");
    assert_eq!(a.texel(15, 5, false)[0], 0.0);
    assert_eq!(a.texel(16, 5, false)[0], 1.0, "box filter keeps the edge sharp at a 4x reduction");
    // 2x upscale: bilinear between texel centres.
    assert_eq!(b.texel(10, 5, false)[0], 0.0);
    assert_eq!(b.texel(200, 5, false)[0], 1.0);
    let mid = b.texel(127, 5, false)[0];
    assert!(mid > 0.1 && mid < 0.4, "bilinear ramp at the edge: {mid}");
}

#[test]
fn resample_box_averages_and_keeps_constants() {
    let data: Vec<f32> = (0..16).map(|i| i as f32).collect();
    let out = resample(&data, 1, 4, 2);
    assert_eq!(out, [(0.0 + 1.0 + 4.0 + 5.0) / 4.0, (2.0 + 3.0 + 6.0 + 7.0) / 4.0, (8.0 + 9.0 + 12.0 + 13.0) / 4.0, (10.0 + 11.0 + 14.0 + 15.0) / 4.0]);
    let flat = vec![0.25f32; 3 * 6 * 6];
    assert!(resample(&flat, 3, 6, 4).iter().all(|v| (v - 0.25).abs() < 1e-6));
    assert!(resample(&flat, 3, 6, 9).iter().all(|v| (v - 0.25).abs() < 1e-6));
}

#[test]
fn normal_format_precedence_window_then_map_then_preset_then_source() {
    let ch = distinct_channels(); // normal (0, 0.6, 0.8): green 0.8 in OpenGL, 0.2 in DirectX
    let n = |suffix: &str, dx: Option<bool>| OutputMap { normal_directx: dx, ..OutputMap::new(suffix, FileFormat::Png8, (0..3).map(|c| Source::Normal { component: c, directx: false }).collect(), false) };
    let mut t = user(vec![n("Src", None), n("MapGL", Some(false))]);
    t.normal_directx = Some(true);
    let green = |files: &[(String, Vec<u8>)], s: &str| {
        let f = files.iter().find(|f| f.0.contains(s)).unwrap();
        decode(&f.0, &f.1).texel(5, 5, false)[1]
    };
    let files = render(t.clone(), &[set(&ch, None)], |s| s.height_to_normal = 0.0);
    assert!(near(green(&files, "Src"), 0.2), "preset DirectX beats the source's OpenGL");
    assert!(near(green(&files, "MapGL"), 0.8), "map OpenGL beats the preset");
    let files = render(t, &[set(&ch, None)], |s| {
        s.height_to_normal = 0.0;
        s.normal_directx = Some(false);
    });
    assert!(near(green(&files, "Src"), 0.8) && near(green(&files, "MapGL"), 0.8), "the window's choice wins");
}

#[test]
fn file_name_patterns_expand_tokens() {
    assert_eq!(expand_pattern("$mesh_$textureSet_$suffix", "My Crate", "Body Set", None, "BaseColor"), "My_Crate_Body_Set_BaseColor");
    assert_eq!(expand_pattern("T_$textureSet_$udim_D", "m", "Hull", Some(1003), "D"), "T_Hull_1003_D");
    assert_eq!(expand_pattern("T_$textureSet_$udim_D", "m", "Hull", None, "D"), "T_Hull_D", "empty $udim drops its separator");
    assert_eq!(expand_pattern("$textureSet.$udim", "m", "Hull", None, "x"), "Hull");
    assert_eq!(expand_pattern("a/b\\c:..d", "m", "s", None, "x"), "a_b_c_.d", "no path separators or ..");
    assert_eq!(expand_pattern("$unknown", "m", "s", None, "x"), "unknown");
    assert_eq!(expand_pattern("", "m", "s", None, "x"), "");
}

#[test]
fn patterns_name_files_and_handle_udim_tiles() {
    let ch = distinct_channels();
    let base = OutputMap { pattern: Some("T_$textureSet_$suffix".into()), ..OutputMap::new("D", FileFormat::Tga8, (0..3).map(|c| Source::ch(ChannelKind::BaseColor, c)).collect(), true) };
    let tiled = OutputMap { pattern: Some("$mesh_$textureSet_$udim_N".into()), ..OutputMap::new("N", FileFormat::Png8, (0..3).map(|c| Source::Normal { component: c, directx: false }).collect(), false) };
    let classic = OutputMap::new("R", FileFormat::Png8, vec![Source::ch(ChannelKind::Roughness, 0)], false);
    let t = user(vec![base, tiled, classic]);
    let tile = ExportTextureSet { name: "Hull.1002", material_slot: 0, udim: Some(1002), channels: &ch, mesh_maps: None };
    let plain = set(&ch, None);
    let s = ExportSettings { template: t.clone(), ..Default::default() };
    let names: Vec<String> = plan("Ship", &[plain, tile], &s).into_iter().map(|p| p.1).collect();
    assert_eq!(names, ["T_P_D.tga", "Ship_P_N.png", "Ship_P_R.png", "T_Hull_D.1002.tga", "Ship_Hull_1002_N.png", "Ship_Hull_R.1002.png"]);
    let s = ExportSettings { template: t, udim_folders: true, ..Default::default() };
    let tile = ExportTextureSet { name: "Hull.1002", material_slot: 0, udim: Some(1002), channels: &ch, mesh_maps: None };
    let names: Vec<String> = plan("Ship", &[tile], &s).into_iter().map(|p| p.1).collect();
    assert_eq!(names, ["1002/T_Hull_D.tga", "1002/Ship_Hull_1002_N.png", "1002/Ship_Hull_R.png"]);
}

#[test]
fn explicit_classic_pattern_names_files_like_the_builtins() {
    let ch = distinct_channels();
    let mut t = preset::duplicate(&ExportTemplate::unreal(), "UE copy");
    for m in &mut t.maps {
        m.pattern = Some("$mesh_$textureSet_$suffix".into());
    }
    let sets = [ExportTextureSet { name: "Body Set", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let a = plan("My Crate", &sets, &ExportSettings { template: t, ..Default::default() });
    let b = plan("My Crate", &sets, &ExportSettings { template: ExportTemplate::unreal(), ..Default::default() });
    assert_eq!(a, b);
}

#[test]
fn problems_catch_duplicate_and_unsafe_suffixes() {
    assert!(ExportTemplate::builtin().iter().all(|t| t.problems().is_empty()));
    let r = OutputMap::new("R", FileFormat::Png8, vec![Source::Height], false);
    let mut t = user(vec![r.clone(), r.clone()]);
    assert_eq!(t.problems().len(), 1);
    t.maps[1].suffix = "a/b".into();
    t.maps[0].components.clear();
    t.name = " ".into();
    assert_eq!(t.problems().len(), 3, "{:?}", t.problems());
}

#[test]
fn preset_json_round_trips_and_rejects_newer_versions() {
    let mut t = preset::duplicate(&ExportTemplate::unity_hdrp(), "HDRP 16");
    t.padding = Some(Padding::Texels(4));
    t.size = Some(1024);
    t.normal_directx = Some(true);
    t.maps[0].pattern = Some("$textureSet_BC".into());
    t.maps[1].format = FileFormat::Tiff16;
    t.maps[2].components[2] = Source::MeshMap { kind: MeshMapKind::Thickness, component: 0, invert: true };
    t.maps[3].components[0] = Source::Gray { kind: ChannelKind::Emissive, invert: false };
    let json = preset::to_json(&t);
    assert!(json.contains("\"format\": \"subcraft.export-preset\"") && json.contains("\"version\": 1"));
    let back = preset::from_json(&json, "x").unwrap();
    assert_eq!(back.presets, [t]);
    let newer = json.replace("\"version\": 1", "\"version\": 99");
    assert!(matches!(preset::from_json(&newer, "x"), Err(preset::PresetError::Version(99))));
    assert!(preset::from_json("{\"hello\": 1}", "x").is_err());
    assert!(preset::from_json("not json", "x").is_err());
}

#[test]
fn library_saves_loads_and_deletes_by_id() {
    let dir = tmp("preset-lib");
    let lib = preset::PresetLibrary::new(&dir);
    assert!(lib.load().0.is_empty(), "missing folder = no presets");
    let a = preset::duplicate(&ExportTemplate::godot(), "Zeta");
    let b = preset::duplicate(&ExportTemplate::gltf(), "alpha");
    lib.save(&a).unwrap();
    lib.save(&b).unwrap();
    assert!(lib.save(&ExportTemplate::godot()).is_err(), "built-ins are read-only");
    std::fs::write(dir.join("broken.json"), "{").unwrap();
    let (presets, errors) = lib.load();
    assert_eq!(presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["alpha", "Zeta"]);
    assert_eq!(presets[1], a);
    assert_eq!(errors.len(), 1);
    let mut renamed = a.clone();
    renamed.name = "Renamed".into();
    lib.save(&renamed).unwrap();
    lib.delete(b.id.as_deref().unwrap()).unwrap();
    lib.delete("does-not-exist").unwrap();
    let (presets, _) = lib.load();
    assert_eq!(presets, [renamed]);
    assert_eq!(preset::unique_name("alpha", &["alpha", "alpha (2)"]), "alpha (3)");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Painter's export preset JSON (the export-config / `.spexp` layout).
const PAINTER: &str = r#"{
  "exportPresets": [{
    "name": "Studio ORM",
    "maps": [
      { "fileName": "$mesh_$textureSet_BaseColor",
        "channels": [
          { "destChannel": "R", "srcChannel": "R", "srcMapType": "documentMap", "srcMapName": "basecolor" },
          { "destChannel": "G", "srcChannel": "G", "srcMapType": "documentMap", "srcMapName": "basecolor" },
          { "destChannel": "B", "srcChannel": "B", "srcMapType": "documentMap", "srcMapName": "basecolor" },
          { "destChannel": "A", "srcChannel": "L", "srcMapType": "documentMap", "srcMapName": "opacity" } ],
        "parameters": { "fileFormat": "png", "bitDepth": "8" } },
      { "fileName": "$mesh_$textureSet_ORM",
        "channels": [
          { "destChannel": "R", "srcChannel": "R", "srcMapType": "meshMap", "srcMapName": "ambient_occlusion" },
          { "destChannel": "G", "srcChannel": "R", "srcMapType": "documentMap", "srcMapName": "roughness" },
          { "destChannel": "B", "srcChannel": "R", "srcMapType": "documentMap", "srcMapName": "metallic" } ],
        "parameters": { "fileFormat": "tga", "bitDepth": "16", "paddingAlgorithm": "passthrough" } },
      { "fileName": "$textureSet_Normal.$udim",
        "channels": [
          { "destChannel": "R", "srcChannel": "R", "srcMapType": "virtualMap", "srcMapName": "Normal_DirectX" },
          { "destChannel": "G", "srcChannel": "G", "srcMapType": "virtualMap", "srcMapName": "Normal_DirectX" },
          { "destChannel": "B", "srcChannel": "B", "srcMapType": "virtualMap", "srcMapName": "Normal_DirectX" } ],
        "parameters": { "fileFormat": "exr", "bitDepth": "16f", "sizeLog2": 9, "paddingAlgorithm": "color", "dilationDistance": 8 } },
      { "fileName": "$mesh_$textureSet_Height_$colorSpace",
        "channels": [ { "destChannel": "L", "srcChannel": "L", "srcMapType": "documentMap", "srcMapName": "height" } ],
        "parameters": { "fileFormat": "tif", "bitDepth": "32f" } },
      { "fileName": "$mesh_$textureSet_Gloss",
        "channels": [ { "destChannel": "L", "srcChannel": "L", "srcMapType": "virtualMap", "srcMapName": "Glossiness" },
                      { "destChannel": "A", "srcChannel": "L", "srcMapType": "virtualMap", "srcMapName": "f0" } ] }
    ]
  }]
}"#;

#[test]
fn painter_presets_convert_maps_sources_and_parameters() {
    let imported = preset::from_json(PAINTER, "file").unwrap();
    assert_eq!(imported.presets.len(), 1);
    let p = &imported.presets[0];
    assert_eq!(p.name, "Studio ORM");
    assert!(p.id.is_some() && p.problems().is_empty(), "{:?}", p.problems());
    let suffixes: Vec<&str> = p.maps.iter().map(|m| m.suffix.as_str()).collect();
    assert_eq!(suffixes, ["BaseColor", "ORM", "Normal", "Height", "Gloss"]);
    let bc = &p.maps[0];
    assert!(bc.srgb && bc.components.len() == 4 && bc.format == FileFormat::Png8);
    assert_eq!(bc.components[3], Source::Alpha, "Painter's opacity is the effective alpha");
    let orm = &p.maps[1];
    assert_eq!(orm.components[0], Source::MeshMap { kind: MeshMapKind::AmbientOcclusion, component: 0, invert: false });
    assert_eq!(orm.components[1], Source::ch(ChannelKind::Roughness, 0));
    assert_eq!((orm.format, orm.padding), (FileFormat::Tga8, Some(Padding::None)), "TGA has no 16-bit");
    let n = &p.maps[2];
    assert_eq!(n.components[1], Source::Normal { component: 1, directx: true });
    assert_eq!((n.format, n.size, n.padding), (FileFormat::Exr16, Some(512), Some(Padding::Background(8))));
    assert_eq!(n.pattern.as_deref(), Some("$textureSet_Normal.$udim"));
    let h = &p.maps[3];
    assert_eq!((h.components.as_slice(), h.format), ([Source::Height].as_slice(), FileFormat::Tiff32));
    assert_eq!(h.pattern.as_deref(), Some("$mesh_$textureSet_Height_"));
    let g = &p.maps[4];
    assert_eq!(g.components, [Source::Channel { kind: ChannelKind::Roughness, component: 0, invert: true }, Source::Constant { value: 0.0 }]);
    assert!(imported.warnings.iter().any(|w| w.contains("f0")), "{:?}", imported.warnings);
    assert!(imported.warnings.iter().any(|w| w.contains("$colorSpace")));
    assert!(imported.warnings.iter().any(|w| w.contains("16-bit")));
    // And it exports.
    let ch = distinct_channels();
    let files = render(p.clone(), &[set(&ch, None)], |_| {});
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names, ["M_P_BaseColor.png", "M_P_ORM.tga", "P_Normal.exr", "M_P_Height.tif", "M_P_Gloss.png"]);
    let orm = decode(&files[1].0, &files[1].1).texel(4, 4, false);
    assert!(near(orm[0], 1.0) && near(orm[1], 0.3) && near(orm[2], 0.8), "unbaked AO mesh map reads white: {orm:?}");
}

#[test]
fn single_painter_preset_object_uses_the_file_name() {
    let one = r#"{ "maps": [ { "fileName": "$mesh_Rough", "channels": [ { "destChannel": "L", "srcChannel": "R", "srcMapType": "documentMap", "srcMapName": "roughness" } ] } ] }"#;
    let p = preset::from_json(one, "my preset").unwrap().presets.remove(0);
    assert_eq!((p.name.as_str(), p.maps[0].suffix.as_str(), p.maps[0].components.len()), ("my preset", "Rough", 1));
}
