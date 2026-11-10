//! Scene exporters: write, then parse the result with the importers / a JSON reader.

use super::*;
use crate::scene::{SceneFormat, export_scene, glb_bytes};
use subcraft_mesh::import_mesh;

fn sets_for<'a>(ch: &'a BTreeMap<ChannelKind, TiledImage>) -> Vec<ExportTextureSet<'a>> {
    vec![
        ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: ch, mesh_maps: None },
        ExportTextureSet { name: "Trim", material_slot: 1, udim: None, channels: ch, mesh_maps: None },
    ]
}

fn assert_same_geometry(a: &subcraft_mesh::Mesh, b: &subcraft_mesh::Mesh) {
    assert_eq!(a.triangle_count(), b.triangle_count());
    assert_eq!(a.materials.len(), b.materials.len());
    let ba = a.bounds;
    let bb = b.bounds;
    assert!((ba.min - bb.min).length() < 1e-4 && (ba.max - bb.max).length() < 1e-4, "bounds {ba:?} vs {bb:?}");
    let area = |m: &subcraft_mesh::Mesh| (0..m.triangle_count()).map(|t| { let [p, q, r] = m.tri_positions(t); (q - p).cross(r - p).length() * 0.5 }).sum::<f32>();
    assert!((area(a) - area(b)).abs() < 1e-3 * area(a), "surface area");
    let uv_sum = |m: &subcraft_mesh::Mesh| (0..m.triangle_count()).map(|t| m.tri_uvs(t).iter().map(|u| u.x + 2.0 * u.y).sum::<f32>()).sum::<f32>();
    assert!((uv_sum(a) - uv_sum(b)).abs() < 1e-2, "uv {} vs {}", uv_sum(a), uv_sum(b));
}

fn roundtrip(format: SceneFormat) -> (subcraft_mesh::Mesh, subcraft_mesh::Mesh, PathBuf, Vec<PathBuf>) {
    let mesh = primitives::sample_crate();
    let ch = channels();
    let dir = tmp(&format!("scene-{}", format.extension()));
    let files = export_scene(&mesh, "Crate", &sets_for(&ch), &ExportSettings::default(), format, &dir).unwrap();
    let back = import_mesh(&files[0]).unwrap().mesh;
    (mesh, back, dir, files)
}

#[test]
fn obj_round_trips_geometry_uvs_and_lists_separate_maps() {
    let (mesh, back, dir, _) = roundtrip(SceneFormat::ObjMtl);
    assert_same_geometry(&mesh, &back);
    let mtl = std::fs::read_to_string(dir.join("Crate.mtl")).unwrap();
    for key in ["newmtl ", "map_Kd Crate_Body_BaseColor.png", "map_Pr Crate_Body_Roughness.png", "map_Pm Crate_Body_Metallic.png", "norm Crate_Body_Normal.png"] {
        assert!(mtl.contains(key), "{key} in\n{mtl}");
    }
    for f in ["Crate_Body_BaseColor.png", "Crate_Trim_Roughness.png"] {
        assert!(dir.join(f).exists(), "{f}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn glb_round_trips_and_material_points_at_packed_textures() {
    let (mesh, back, dir, files) = roundtrip(SceneFormat::Glb);
    assert_eq!(files.len(), 1, "GLB is self-contained");
    assert_same_geometry(&mesh, &back);
    let (doc, buffers, images) = gltf::import(&files[0]).unwrap();
    assert_eq!(doc.materials().count(), mesh.materials.len());
    for m in doc.materials() {
        let pbr = m.pbr_metallic_roughness();
        assert!(pbr.base_color_texture().is_some() && pbr.metallic_roughness_texture().is_some());
        assert!(m.normal_texture().is_some() && m.occlusion_texture().is_some());
        assert_eq!(pbr.metallic_roughness_texture().unwrap().texture().index(), m.occlusion_texture().unwrap().texture().index());
    }
    assert!(!buffers.is_empty() && images.len() >= 3);
    // Packed ORM texture: R occlusion (default 1), G roughness 0.25, B metallic 1.
    let m = doc.materials().next().unwrap();
    let img = &images[m.pbr_metallic_roughness().metallic_roughness_texture().unwrap().texture().source().index()];
    let (w, c) = (img.width as usize, if img.format == gltf::image::Format::R8G8B8A8 { 4 } else { 3 });
    let p = &img.pixels[(5 * w + 5) * c..(5 * w + 5) * c + 3];
    assert_eq!(p[0], 255);
    assert!((p[1] as i32 - 64).abs() <= 1 && p[2] == 255, "{p:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn gltf_separate_files_validate_and_reference_existing_images() {
    let (mesh, back, dir, files) = roundtrip(SceneFormat::Gltf);
    assert_same_geometry(&mesh, &back);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&files[0]).unwrap()).unwrap();
    assert_eq!(json["asset"]["version"], "2.0");
    let bin_len = std::fs::metadata(dir.join("Crate.bin")).unwrap().len();
    assert_eq!(json["buffers"][0]["byteLength"].as_u64().unwrap(), bin_len);
    for img in json["images"].as_array().unwrap() {
        assert!(dir.join(img["uri"].as_str().unwrap()).exists());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn usda_round_trips_and_declares_preview_surface_networks() {
    let (mesh, back, dir, files) = roundtrip(SceneFormat::Usda);
    assert_same_geometry(&mesh, &back);
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.starts_with("#usda 1.0"));
    for key in ["UsdPreviewSurface", "UsdUVTexture", "inputs:roughness.connect", "ORM.outputs:g", "inputs:scale = (2, 2, 2, 1)", "upAxis = \"Y\""] {
        assert!(text.contains(key), "{key}");
    }
    assert_eq!(back.materials.iter().map(|m| m.name.clone()).collect::<Vec<_>>().len(), mesh.materials.len());
    std::fs::remove_dir_all(dir).unwrap();
}

/// Material names that are not USD identifiers (a leading digit, accents, spaces) or that
/// repeat used to produce a `.usda` that USD refuses or that merges two materials into one.
#[test]
fn usda_prim_names_are_valid_and_distinct_for_awkward_material_names() {
    let mut mesh = primitives::sample_crate();
    mesh.name = "7 crates".into();
    mesh.materials[0].name = "1_Default".into();
    mesh.materials[1].name = "Métal A".into();
    let ch = channels();
    let sets = vec![
        ExportTextureSet { name: "1_Default", material_slot: 0, udim: None, channels: &ch, mesh_maps: None },
        ExportTextureSet { name: "Métal A", material_slot: 1, udim: None, channels: &ch, mesh_maps: None },
    ];
    let dir = tmp("scene-usda-names");
    let files = export_scene(&mesh, "7 crates", &sets, &ExportSettings::default(), SceneFormat::Usda, &dir).unwrap();
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("def Material \"_1_Default\"") && text.contains("def Material \"M_tal_A\""), "{text}");
    assert!(text.contains("def Xform \"_7_crates\""));
    let back = import_mesh(&files[0]).unwrap().mesh;
    assert_eq!(back.triangle_count(), mesh.triangle_count());
    assert_eq!(back.materials.len(), 2, "{:?}", back.materials);
    std::fs::remove_dir_all(&dir).unwrap();

    // Two slots with the same name stay two materials.
    let mut same = primitives::sample_crate();
    same.materials[0].name = "Material".into();
    same.materials[1].name = "Material".into();
    let text = crate::scene::usda_text(&same, &[Default::default(), Default::default()]);
    assert!(text.contains("def Material \"Material\"") && text.contains("def Material \"Material_2\""), "{text}");
    assert!(text.contains("</Crate/Materials/Material_2>") || text.contains("/Materials/Material_2>"), "{text}");
}

#[test]
fn glb_without_textures_is_still_valid() {
    let mesh = primitives::plane(2.0, 1);
    let bytes = glb_bytes(&mesh, &[]);
    let g = gltf::Gltf::from_slice(&bytes).unwrap();
    assert_eq!(g.meshes().count(), 1);
    assert_eq!(bytes.len() % 4, 0);
}

fn packed_pixel(template: ExportTemplate, file: &str) -> [f32; 4] {
    let mesh = primitives::plane(2.0, 1);
    let ch = channels();
    let dir = tmp(&format!("pack-{file}"));
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    export(&mesh, "M", &sets, &ExportSettings { template, ..Default::default() }, &dir).unwrap();
    let p = px(&dir.join(file), 10, 10);
    std::fs::remove_dir_all(dir).unwrap();
    p
}

#[test]
fn preset_channel_packing_values() {
    // channels(): roughness 0.25, metallic 1.0, no AO channel (defaults to 1).
    let hdrp = packed_pixel(ExportTemplate::unity_hdrp(), "M_Body_MaskMap.png");
    assert!((hdrp[0] - 1.0).abs() < 1e-3 && (hdrp[1] - 1.0).abs() < 1e-3 && hdrp[2].abs() < 1e-3 && (hdrp[3] - 0.75).abs() < 1e-3, "HDRP mask map R=metal G=AO B=0 A=smoothness: {hdrp:?}");
    let urp = packed_pixel(ExportTemplate::unity_urp(), "M_Body_MetallicSmoothness.png");
    assert!((urp[0] - 1.0).abs() < 1e-3 && (urp[3] - 0.75).abs() < 1e-3, "{urp:?}");
    let ue = packed_pixel(ExportTemplate::unreal(), "M_Body_OcclusionRoughnessMetallic.png");
    assert!((ue[0] - 1.0).abs() < 1e-3 && (ue[1] - 0.25).abs() < 1e-3 && (ue[2] - 1.0).abs() < 1e-3, "{ue:?}");
    let godot = packed_pixel(ExportTemplate::godot(), "M_Body_ORM.png");
    assert!((godot[1] - 0.25).abs() < 1e-3 && (godot[2] - 1.0).abs() < 1e-3, "{godot:?}");
    let rb_r = packed_pixel(ExportTemplate::roblox(), "M_Body_RoughnessMap.png");
    let rb_m = packed_pixel(ExportTemplate::roblox(), "M_Body_MetalnessMap.png");
    assert!((rb_r[0] - 0.25).abs() < 1e-3 && (rb_m[0] - 1.0).abs() < 1e-3);
    let sk = packed_pixel(ExportTemplate::sketchfab(), "M_Body_Roughness.png");
    assert!((sk[0] - 0.25).abs() < 1e-3);
}
