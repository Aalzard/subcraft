mod more;
use std::collections::BTreeMap;

use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_image::{TiledImage, load_bitmap};
use subcraft_mesh::primitives;

use super::*;

const RES: u32 = 128;

#[test]
fn archive_contains_all_ten_maps_identical_to_directory_export() {
    use std::io::Read;
    let mesh=primitives::sample_crate();let ch=channels();
    let sets=[ExportTextureSet {name:"Body",material_slot:0,udim:None,channels:&ch,mesh_maps:None},ExportTextureSet {name:"Trim",material_slot:1,udim:None,channels:&ch,mesh_maps:None}];
    let settings=ExportSettings::default();let dir=tmp("archive");let files=export(&mesh,"Test Model",&sets,&settings,&dir).unwrap();
    let result=archive(&mesh,"Test Model",&sets,&settings).unwrap();assert_eq!(result.files,10);assert_eq!(result.name,"Test_Model-textures.zip");
    let mut zip=zip::ZipArchive::new(std::io::Cursor::new(result.bytes)).unwrap();assert_eq!(zip.len(),files.len());
    for p in files {let mut entry=zip.by_name(p.file_name().unwrap().to_str().unwrap()).unwrap();assert_eq!(entry.compression(),zip::CompressionMethod::Stored);let mut bytes=Vec::new();entry.read_to_end(&mut bytes).unwrap();assert_eq!(bytes,std::fs::read(p).unwrap());let b=subcraft_image::io::decode_bitmap(&bytes,Some("map.png")).unwrap();assert_eq!((b.width,b.height),(RES,RES));}
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn archive_uses_packed_templates_and_format_overrides() {
    let mesh=primitives::plane(2.0,1);let ch=channels();let sets=[ExportTextureSet {name:"Body",material_slot:0,udim:None,channels:&ch,mesh_maps:None}];
    let settings=ExportSettings {template:ExportTemplate::unreal(),format_override:Some(FileFormat::Png16),..Default::default()};
    let result=archive(&mesh,"M",&sets,&settings).unwrap();let mut zip=zip::ZipArchive::new(std::io::Cursor::new(result.bytes)).unwrap();assert_eq!(result.files,plan("M",&sets,&settings).len());
    let mut entry=zip.by_name("M_Body_OcclusionRoughnessMetallic.png").unwrap();let mut bytes=Vec::new();std::io::Read::read_to_end(&mut entry,&mut bytes).unwrap();let b=subcraft_image::io::decode_bitmap(&bytes,Some("map.png")).unwrap();let p=b.texel(10,10,false);assert!((p[1]-0.25).abs()<1e-4);assert!((p[2]-1.0).abs()<1e-4);
}

#[test]
fn archive_rejects_colliding_and_unsafe_output_names() {
    let mesh=primitives::plane(2.0,1);let ch=channels();let set=||ExportTextureSet {name:"Same",material_slot:0,udim:None,channels:&ch,mesh_maps:None};
    assert!(archive(&mesh,"M",&[set(),set()],&ExportSettings::default()).is_err());
    let mut settings=ExportSettings::default();settings.template.maps[0].suffix="../outside".into();assert!(archive(&mesh,"M",&[set()],&settings).is_err());
}

fn channels() -> BTreeMap<ChannelKind, TiledImage> {
    let mut m = BTreeMap::new();
    m.insert(ChannelKind::BaseColor, TiledImage::new(RES, RES, 4, [0.5, 0.25, 0.0, 1.0]));
    m.insert(ChannelKind::Roughness, TiledImage::new(RES, RES, 1, [0.25; 4]));
    m.insert(ChannelKind::Metallic, TiledImage::new(RES, RES, 1, [1.0; 4]));
    m.insert(ChannelKind::Normal, TiledImage::new(RES, RES, 4, [0.5, 0.5, 1.0, 1.0]));
    m.insert(ChannelKind::Height, TiledImage::new(RES, RES, 1, [0.0; 4]));
    m
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-export-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn px(path: &Path, x: u32, y: u32) -> [f32; 4] {
    load_bitmap(path).unwrap().texel(x, y, false)
}

#[test]
fn plan_skips_maps_without_channels() {
    let ch = channels();
    let sets = [ExportTextureSet { name: "Body Set", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let files = plan("My Crate", &sets, &ExportSettings::default());
    let names: Vec<&str> = files.iter().map(|(_, f)| f.as_str()).collect();
    assert_eq!(
        names,
        [
            "My_Crate_Body_Set_BaseColor.png",
            "My_Crate_Body_Set_Roughness.png",
            "My_Crate_Body_Set_Metallic.png",
            "My_Crate_Body_Set_Normal.png",
            "My_Crate_Body_Set_Height.png"
        ],
        "no Emissive / AO channel in the set → those maps are skipped"
    );
}

#[test]
fn metal_rough_values_and_srgb_encoding() {
    let mesh = primitives::plane(2.0, 1);
    let ch = channels();
    let sets = [ExportTextureSet { name: "Plane", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let dir = tmp("pbr");
    let files = export(&mesh, "Mesh", &sets, &ExportSettings::default(), &dir).unwrap();
    assert_eq!(files.len(), 5);
    let base = px(&dir.join("Mesh_Plane_BaseColor.png"), 10, 10);
    assert!((base[0] - linear_to_srgb(0.5)).abs() < 1.0 / 255.0, "base color is sRGB-encoded: {base:?}");
    assert!((base[1] - linear_to_srgb(0.25)).abs() < 1.0 / 255.0);
    let rough = px(&dir.join("Mesh_Plane_Roughness.png"), 10, 10);
    assert!((rough[0] - 0.25).abs() < 1.0 / 255.0, "data channels are not color-transformed: {rough:?}");
    let height = px(&dir.join("Mesh_Plane_Height.png"), 3, 3);
    assert!((height[0] - 0.5).abs() < 1e-3, "height 0 maps to mid-gray in integer formats");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn packed_maps_and_normal_formats() {
    let mesh = primitives::plane(2.0, 1);
    let mut ch = channels();
    let tilted = glam::Vec3::new(0.0, 0.6, 0.8);
    ch.insert(ChannelKind::Normal, TiledImage::new(RES, RES, 4, [0.5, tilted.y * 0.5 + 0.5, tilted.z * 0.5 + 0.5, 1.0]));
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let dir = tmp("packed");
    let ue = ExportSettings { template: ExportTemplate::unreal(), ..ExportSettings::default() };
    export(&mesh, "M", &sets, &ue, &dir).unwrap();
    let orm = px(&dir.join("M_P_OcclusionRoughnessMetallic.png"), 5, 5);
    assert!((orm[0] - 1.0).abs() < 1e-3 && (orm[1] - 0.25).abs() < 1.0 / 255.0 && (orm[2] - 1.0).abs() < 1e-3, "{orm:?}");
    let dx = px(&dir.join("M_P_Normal.png"), 5, 5);
    assert!((dx[1] - (1.0 - 0.8)).abs() < 2.0 / 255.0, "DirectX flips green: {dx:?}");

    let unity = ExportSettings { template: ExportTemplate::unity_hdrp(), ..ExportSettings::default() };
    export(&mesh, "M", &sets, &unity, &dir).unwrap();
    let mask = px(&dir.join("M_P_MaskMap.png"), 5, 5);
    assert!((mask[0] - 1.0).abs() < 1e-3 && mask[2] == 0.0 && (mask[3] - 0.75).abs() < 1.0 / 255.0, "smoothness = 1 - roughness: {mask:?}");
    let gl = ExportSettings { template: ExportTemplate::gltf(), ..ExportSettings::default() };
    export(&mesh, "M", &sets, &gl, &dir).unwrap();
    let n = px(&dir.join("M_P_Normal.png"), 5, 5);
    assert!((n[1] - 0.8).abs() < 2.0 / 255.0, "OpenGL keeps green: {n:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn height_slopes_tilt_the_exported_normal() {
    let mesh = primitives::plane(2.0, 1);
    let mut ch = channels();
    let mut h = TiledImage::new(RES, RES, 1, [0.0; 4]);
    for y in 0..RES {
        for x in 0..RES {
            h.set(x, y, [x as f32 / 8.0; 4]); // rises steeply toward +u
        }
    }
    ch.insert(ChannelKind::Height, h);
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let dir = tmp("height");
    export(&mesh, "M", &sets, &ExportSettings::default(), &dir).unwrap();
    let n = px(&dir.join("M_P_Normal.png"), 64, 64);
    assert!(n[0] < 0.45, "slope up in +u tilts the normal toward -u: {n:?}");
    let off = ExportSettings { height_to_normal: 0.0, ..ExportSettings::default() };
    export(&mesh, "M", &sets, &off, &dir).unwrap();
    let n = px(&dir.join("M_P_Normal.png"), 64, 64);
    assert!((n[0] - 0.5).abs() < 2.0 / 255.0, "height-to-normal disabled: {n:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn padding_fills_the_gutter_from_islands() {
    // Cube atlas: islands with gutters. Roughness is 1 on covered texels, 0 in the gutter.
    let mesh = primitives::cube(2.0);
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
    let gutter = cov.iter().position(|c| !c).unwrap() as u32;
    let (gx, gy) = (gutter % RES, gutter / RES);
    let dir = tmp("pad");
    let only_rough = ExportTemplate { name: "R".into(), maps: vec![ExportTemplate::pbr_metal_rough().maps[1].clone()] };
    for (padding, expect) in [(Padding::None, 0.0), (Padding::Infinite, 1.0)] {
        let s = ExportSettings { template: only_rough.clone(), padding, ..ExportSettings::default() };
        export(&mesh, "M", &sets, &s, &dir).unwrap();
        let v = px(&dir.join("M_C_Roughness.png"), gx, gy)[0];
        assert_eq!(v, expect, "{padding:?} gutter texel ({gx},{gy})");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn baked_ao_fills_the_occlusion_slot() {
    let mesh = primitives::plane(2.0, 1);
    let ch = channels();
    let mut baked = BTreeMap::new();
    baked.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(64, 64, 1, [0.4; 4]));
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: Some(&baked) }];
    let files = plan("M", &sets, &ExportSettings::default());
    assert!(files.iter().any(|(_, f)| f == "M_P_AO.png"), "AO map exported from the bake: {files:?}");
    let dir = tmp("ao");
    let gl = ExportSettings { template: ExportTemplate::gltf(), ..ExportSettings::default() };
    export(&mesh, "M", &sets, &gl, &dir).unwrap();
    let orm = px(&dir.join("M_P_OcclusionRoughnessMetallic.png"), 70, 70);
    assert!((orm[0] - 0.4).abs() < 1.0 / 255.0, "baked AO (resampled 64 -> 128) in R: {orm:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every channel with its own value, so a packing mistake shows up as a wrong number.
fn distinct_channels() -> BTreeMap<ChannelKind, TiledImage> {
    let mut m = channels();
    m.insert(ChannelKind::Roughness, TiledImage::new(RES, RES, 1, [0.3; 4]));
    m.insert(ChannelKind::Metallic, TiledImage::new(RES, RES, 1, [0.8; 4]));
    m.insert(ChannelKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.6; 4]));
    m.insert(ChannelKind::Emissive, TiledImage::new(RES, RES, 4, [0.2, 0.4, 0.6, 1.0]));
    m.insert(ChannelKind::Height, TiledImage::new(RES, RES, 1, [0.5; 4]));
    let tilted = glam::Vec3::new(0.0, 0.6, 0.8);
    m.insert(ChannelKind::Normal, TiledImage::new(RES, RES, 4, [0.5, tilted.y * 0.5 + 0.5, tilted.z * 0.5 + 0.5, 1.0]));
    m
}

fn near(a: f32, b: f32) -> bool { (a - b).abs() <= 1.5 / 255.0 }

fn export_with(template: ExportTemplate, name: &str) -> (PathBuf, Vec<String>) {
    let mesh = primitives::plane(2.0, 1);
    let ch = distinct_channels();
    let sets = [ExportTextureSet { name: "P", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let dir = tmp(name);
    let files = export(&mesh, "M", &sets, &ExportSettings { template, ..ExportSettings::default() }, &dir).unwrap();
    let names = files.iter().map(|f| f.file_name().unwrap().to_str().unwrap().to_string()).collect();
    (dir, names)
}

#[test]
fn unreal_orm_packs_occlusion_roughness_metallic_with_directx_normals() {
    let (dir, names) = export_with(ExportTemplate::unreal(), "ue");
    assert_eq!(names, ["M_P_BaseColor.png", "M_P_Normal.png", "M_P_OcclusionRoughnessMetallic.png", "M_P_Emissive.png"]);
    let orm = px(&dir.join("M_P_OcclusionRoughnessMetallic.png"), 9, 9);
    assert!(near(orm[0], 0.6) && near(orm[1], 0.3) && near(orm[2], 0.8), "R occlusion, G roughness, B metallic: {orm:?}");
    let n = px(&dir.join("M_P_Normal.png"), 9, 9);
    assert!(near(n[1], 1.0 - 0.8), "DirectX normals flip green: {n:?}");
    let emissive = px(&dir.join("M_P_Emissive.png"), 9, 9);
    assert!(near(emissive[2], linear_to_srgb(0.6)), "emission is sRGB: {emissive:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unity_hdrp_and_urp_pack_their_masks_as_the_engines_read_them() {
    let (dir, _) = export_with(ExportTemplate::unity_hdrp(), "hdrp");
    let mask = px(&dir.join("M_P_MaskMap.png"), 9, 9);
    assert!(near(mask[0], 0.8) && near(mask[1], 0.6) && mask[2] == 0.0 && near(mask[3], 0.7), "R metallic, G occlusion, B detail, A smoothness: {mask:?}");
    std::fs::remove_dir_all(dir).unwrap();

    let (dir, names) = export_with(ExportTemplate::unity_urp(), "urp");
    assert_eq!(names, ["M_P_BaseColor.png", "M_P_Normal.png", "M_P_MetallicSmoothness.png", "M_P_Occlusion.png", "M_P_Emissive.png", "M_P_Height.png"]);
    let ms = px(&dir.join("M_P_MetallicSmoothness.png"), 9, 9);
    assert!(near(ms[0], 0.8) && near(ms[1], 0.8) && near(ms[2], 0.8) && near(ms[3], 0.7), "metallic in RGB, smoothness (1 - roughness) in A: {ms:?}");
    let occlusion = px(&dir.join("M_P_Occlusion.png"), 9, 9);
    assert!(near(occlusion[0], 0.6) && near(occlusion[1], 0.6) && near(occlusion[2], 0.6), "occlusion is gray: {occlusion:?}");
    let n = px(&dir.join("M_P_Normal.png"), 9, 9);
    assert!(near(n[1], 0.8), "Unity uses OpenGL normals: {n:?}");
    let base = px(&dir.join("M_P_BaseColor.png"), 9, 9);
    assert!(near(base[0], linear_to_srgb(0.5)), "{base:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn godot_and_gltf_pack_orm_with_opengl_normals() {
    let (dir, names) = export_with(ExportTemplate::godot(), "godot");
    assert_eq!(names, ["M_P_BaseColor.png", "M_P_Normal.png", "M_P_ORM.png", "M_P_Emissive.png", "M_P_Height.png"]);
    let orm = px(&dir.join("M_P_ORM.png"), 9, 9);
    assert!(near(orm[0], 0.6) && near(orm[1], 0.3) && near(orm[2], 0.8), "R occlusion, G roughness, B metallic: {orm:?}");
    let n = px(&dir.join("M_P_Normal.png"), 9, 9);
    assert!(near(n[1], 0.8), "Godot uses OpenGL normals: {n:?}");
    let height = px(&dir.join("M_P_Height.png"), 9, 9);
    assert!((height[0] - 0.75).abs() < 0.01, "height 0.5 maps to 0.75 in integer formats: {height:?}");
    std::fs::remove_dir_all(dir).unwrap();

    let (dir, _) = export_with(ExportTemplate::gltf(), "gltf");
    let orm = px(&dir.join("M_P_OcclusionRoughnessMetallic.png"), 9, 9);
    assert!(near(orm[0], 0.6) && near(orm[1], 0.3) && near(orm[2], 0.8), "{orm:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn builtin_templates_are_listed_once_with_unique_names_and_safe_suffixes() {
    let all = ExportTemplate::builtin();
    assert_eq!(all.len(), 6);
    let mut names: Vec<_> = all.iter().map(|t| t.name.clone()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), all.len());
    for t in &all {
        let mut suffixes: Vec<_> = t.maps.iter().map(|m| m.suffix.clone()).collect();
        assert!(suffixes.iter().all(|s| sanitize(s) == *s), "{}: suffixes must be file safe", t.name);
        suffixes.sort();
        suffixes.dedup();
        assert_eq!(suffixes.len(), t.maps.len(), "{}: suffixes are unique", t.name);
    }
}
