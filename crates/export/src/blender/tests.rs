use std::collections::BTreeMap;

use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;

use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("subcraft-blender-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Parse the OBJ written by `welded_obj`: (positions, uv count, normal count, faces as corner
/// triples of (v, vt, vn) 1-based indices, usemtl names in order).
fn parse(obj: &str) -> (usize, usize, usize, Vec<[(usize, usize, usize); 3]>, Vec<String>) {
    let (mut v, mut vt, mut vn, mut faces, mut mats) = (0, 0, 0, Vec::new(), Vec::new());
    for line in obj.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => v += 1,
            Some("vt") => vt += 1,
            Some("vn") => vn += 1,
            Some("usemtl") => mats.push(it.next().unwrap().to_string()),
            Some("f") => {
                let c: Vec<(usize, usize, usize)> = it
                    .map(|c| {
                        let p: Vec<usize> = c.split('/').map(|x| x.parse().unwrap()).collect();
                        (p[0], p[1], p[2])
                    })
                    .collect();
                faces.push([c[0], c[1], c[2]]);
            }
            _ => {}
        }
    }
    (v, vt, vn, faces, mats)
}

/// A cube in SubCraft is split at every UV seam and hard edge; Blender must get one shared vertex
/// per position so the surface is closed for sculpting, with every UV and normal kept.
#[test]
fn welded_obj_shares_positions_and_keeps_uvs_and_normals() {
    let mesh = primitives::cube(2.0);
    let (obj, mtl) = welded_obj(&mesh, "Crate", "crate.mtl");
    let (v, vt, vn, faces, mats) = parse(&obj);
    // The test cube has 4×4 quads per face: 6·5·5 face corners, minus the 12 edges' 5 shared
    // points counted twice, plus the 8 corners counted three times = 98 distinct positions.
    assert!(mesh.positions.len() > 98, "the test needs a cube split at its seams: {}", mesh.positions.len());
    assert_eq!(v, 6 * 25 - 12 * 5 + 8);
    assert_eq!(vt, mesh.uvs.len());
    assert_eq!(vn, mesh.normals.len());
    assert_eq!(faces.len(), mesh.triangle_count());
    assert!(obj.lines().filter(|l| l.starts_with("o ")).count() == 1, "one Blender object");
    // Every triangle reads the same position, UV (flipped to OBJ's bottom-left origin) and
    // normal as in SubCraft.
    let pos: Vec<Vec<f32>> = obj.lines().filter(|l| l.starts_with("v ")).map(|l| l[2..].split(' ').map(|x| x.parse().unwrap()).collect()).collect();
    for (t, f) in faces.iter().enumerate() {
        for (k, &(pv, uv, n)) in f.iter().enumerate() {
            let i = mesh.tri(t)[k] as usize;
            assert_eq!(uv, i + 1);
            assert_eq!(n, i + 1);
            let p = &pos[pv - 1];
            assert_eq!([p[0], p[1], p[2]], mesh.positions[i].to_array());
        }
    }
    // Each edge of a closed welded cube is shared by exactly two triangles.
    let mut edges: BTreeMap<(usize, usize), u32> = BTreeMap::new();
    for f in &faces {
        for k in 0..3 {
            let (a, b) = (f[k].0, f[(k + 1) % 3].0);
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    assert!(edges.values().all(|&n| n == 2), "{edges:?}");
    assert!(!mats.is_empty() && mats.iter().all(|m| mtl.contains(&format!("newmtl {m}"))));
}

fn set_channels(kinds: &[ChannelKind]) -> BTreeMap<ChannelKind, TiledImage> {
    kinds.iter().map(|&k| (k, TiledImage::new(32, 32, if k.is_color() || k == ChannelKind::Normal { 3 } else { 1 }, [0.5, 0.5, 0.5, 1.0]))).collect()
}

/// The default template gives one input per Principled BSDF socket with the right colour space:
/// colour maps sRGB, data maps Non-Color, OpenGL normals; maps the project lacks are not written.
#[test]
fn bridge_writes_mesh_textures_and_a_manifest_with_roles_and_colour_spaces() {
    use ChannelKind::*;
    let mesh = primitives::cube(1.0);
    let ch = set_channels(&[BaseColor, Metallic, Roughness, Normal, Height]);
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let settings = ExportSettings { template: template(), size: Some(32), ..Default::default() };
    let dir = temp_dir("full");
    let files = write_bridge(&mesh, "Crate", &sets, &settings, true, &dir).unwrap();
    assert!(files.mesh.is_file() && files.script.is_file() && files.manifest.is_file());
    assert!(dir.join(RETURN_DIR).is_dir());
    assert_eq!(files.return_file, dir.join(RETURN_DIR).join(RETURN_FILE));
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&files.manifest).unwrap()).unwrap();
    assert_eq!(json["version"], MANIFEST_VERSION);
    assert_eq!(json["include_textures"], true);
    let mats = json["materials"].as_array().unwrap();
    assert_eq!(mats.len(), 1);
    let tex = mats[0]["textures"].as_array().unwrap();
    let find = |role: &str| tex.iter().find(|t| t["role"] == role).unwrap_or_else(|| panic!("{role} missing: {tex:?}"));
    assert_eq!(find("base_color")["srgb"], true);
    for role in ["metallic", "roughness", "normal"] {
        assert_eq!(find(role)["srgb"], false, "{role} is data");
        assert!(Path::new(find(role)["path"].as_str().unwrap()).is_file());
    }
    assert_eq!(find("normal")["directx"], false);
    // No emissive / opacity / AO in this project: not written, not listed.
    assert!(tex.iter().all(|t| !matches!(t["role"].as_str(), Some("emission" | "alpha" | "ambient_occlusion"))), "{tex:?}");
    assert_eq!(files.textures.len(), 4);
    assert!(SCRIPT.contains("def main()") && std::fs::read_to_string(&files.script).unwrap() == SCRIPT);
    let _ = std::fs::remove_dir_all(&dir);
}

/// "Include textures" off: mesh only, materials named but without textures.
#[test]
fn mesh_only_bridge_writes_no_textures() {
    let mesh = primitives::cube(1.0);
    let ch = set_channels(&[ChannelKind::BaseColor]);
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let dir = temp_dir("mesh-only");
    let files = write_bridge(&mesh, "Crate", &sets, &ExportSettings { template: template(), ..Default::default() }, false, &dir).unwrap();
    assert!(files.textures.is_empty());
    assert!(!dir.join("textures").exists());
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&files.manifest).unwrap()).unwrap();
    assert_eq!(json["include_textures"], false);
    assert!(json["materials"][0]["textures"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Other templates are wired too: Unreal's packed ORM gives three components (AO, roughness,
/// metallic) of one Non-Color file, and its DirectX normal is flagged for the green flip.
#[test]
fn packed_and_directx_templates_map_to_components() {
    let settings = ExportSettings::default();
    let unreal = ExportTemplate::unreal();
    let orm = unreal.maps.iter().find(|m| m.components.len() == 3 && matches!(m.components[1], Source::Channel { kind: ChannelKind::Roughness, .. })).expect("ORM map");
    let got = inputs_of_map(orm, &settings);
    assert_eq!(got, vec![(Role::AmbientOcclusion, Some(0), false, false), (Role::Roughness, Some(1), false, false), (Role::Metallic, Some(2), false, false)]);
    let normal = unreal.maps.iter().find(|m| matches!(m.components[0], Source::Normal { .. })).unwrap();
    assert_eq!(inputs_of_map(normal, &settings), vec![(Role::Normal, None, false, true)]);
    // Forcing OpenGL in the export settings wins, as in the written file.
    assert_eq!(inputs_of_map(normal, &ExportSettings { normal_directx: Some(false), ..Default::default() }), vec![(Role::Normal, None, false, false)]);
    // Smoothness (inverted roughness) is roughness with an invert node.
    let smooth = OutputMap::new("S", FileFormat::Png8, vec![Source::Channel { kind: ChannelKind::Roughness, component: 0, invert: true }], false);
    assert_eq!(inputs_of_map(&smooth, &settings), vec![(Role::Roughness, None, true, false)]);
    // Base colour with opacity in alpha: colour plus alpha from component 3.
    let rgba = ExportTemplate::pbr_metal_rough_opacity();
    let bc = &rgba.maps[0];
    assert_eq!(inputs_of_map(bc, &settings), vec![(Role::BaseColor, None, false, false), (Role::Alpha, Some(3), false, false)]);
}

/// The script Blender runs is valid Python that only reads `bridge.json` (no network, no
/// installs) and declares the panel and operator the way back relies on.
#[test]
fn script_declares_the_send_back_operator() {
    for needle in ["subcraft.send_back", "Send to SubCraft", "obj_export", "os.replace", "Non-Color", "ShaderNodeNormalMap", "MULTIRES"] {
        assert!(SCRIPT.contains(needle), "{needle}");
    }
    for forbidden in ["urllib", "subprocess", "pip", "socket"] {
        assert!(!SCRIPT.contains(&format!("import {forbidden}")), "{forbidden}");
    }
}
