use std::collections::BTreeMap;

use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_image::{TiledImage, load_bitmap};
use subcraft_mesh::primitives;

use super::*;

const RES: u32 = 128;

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
    let sets = [ExportTextureSet { name: "Body Set", material_slot: 0, channels: &ch, mesh_maps: None }];
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
    let sets = [ExportTextureSet { name: "Plane", material_slot: 0, channels: &ch, mesh_maps: None }];
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
    let sets = [ExportTextureSet { name: "P", material_slot: 0, channels: &ch, mesh_maps: None }];
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
    let sets = [ExportTextureSet { name: "P", material_slot: 0, channels: &ch, mesh_maps: None }];
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
    let sets = [ExportTextureSet { name: "C", material_slot: 0, channels: &ch, mesh_maps: None }];
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
    let sets = [ExportTextureSet { name: "P", material_slot: 0, channels: &ch, mesh_maps: Some(&baked) }];
    let files = plan("M", &sets, &ExportSettings::default());
    assert!(files.iter().any(|(_, f)| f == "M_P_AO.png"), "AO map exported from the bake: {files:?}");
    let dir = tmp("ao");
    let gl = ExportSettings { template: ExportTemplate::gltf(), ..ExportSettings::default() };
    export(&mesh, "M", &sets, &gl, &dir).unwrap();
    let orm = px(&dir.join("M_P_OcclusionRoughnessMetallic.png"), 70, 70);
    assert!((orm[0] - 0.4).abs() < 1.0 / 255.0, "baked AO (resampled 64 -> 128) in R: {orm:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}
