//! Issue #145: Base Color with alpha and separate Opacity maps. Numeric checks of the written
//! alpha in PNG, TGA and EXR, of the effective opacity (Opacity × Base Color coverage), of the
//! colour bleed under transparent texels, and of padding leaving alpha edges inside islands alone.

use super::*;

/// A transparent set: Base Color filled with alpha 0, red (alpha 1) on the left half, a
/// half-covered green column at x = 70. Opacity 0.5 on the top half when `opacity`.
fn transparent_channels(opacity: bool) -> BTreeMap<ChannelKind, TiledImage> {
    let mut m = BTreeMap::new();
    let mut bc = TiledImage::new(RES, RES, 4, [0.5, 0.5, 0.5, 0.0]);
    for y in 0..RES {
        for x in 0..RES / 2 {
            bc.set(x, y, [1.0, 0.0, 0.0, 1.0]);
        }
        bc.set(70, y, [0.0, 1.0, 0.0, 0.5]);
    }
    m.insert(ChannelKind::BaseColor, bc);
    m.insert(ChannelKind::Roughness, TiledImage::new(RES, RES, 1, [0.25; 4]));
    if opacity {
        let mut o = TiledImage::new(RES, RES, 1, [1.0; 4]);
        for y in 0..RES / 2 {
            for x in 0..RES {
                o.set(x, y, [0.5; 4]);
            }
        }
        m.insert(ChannelKind::Opacity, o);
    }
    m
}

fn file<'a>(files: &'a [PathBuf], suffix: &str) -> &'a PathBuf {
    files.iter().find(|p| p.file_stem().unwrap().to_str().unwrap().ends_with(suffix)).unwrap_or_else(|| panic!("{suffix} in {files:?}"))
}

fn near(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn opacity_preset_writes_rgba_base_color_and_a_grey_opacity_map() {
    let mesh = primitives::plane(2.0, 1);
    let ch = transparent_channels(true);
    let sets = [ExportTextureSet { name: "Glass", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let settings = ExportSettings { template: ExportTemplate::pbr_metal_rough_opacity(), padding: Padding::None, ..Default::default() };
    let dir = tmp("alpha-png");
    let files = export(&mesh, "M", &sets, &settings, &dir).unwrap();
    let bc = file(&files, "_BaseColor");
    let b = load_bitmap(bc).unwrap();
    assert_eq!(b.channels, 4, "Base Color is RGBA");
    let q = 1.0 / 255.0;
    // Bottom-left: red, opaque. Top-left: opacity 0.5. Right: unpainted, transparent.
    let p = px(bc, 10, 100);
    assert!(near(p[0], 1.0, q) && near(p[3], 1.0, q), "{p:?}");
    assert!(near(px(bc, 10, 10)[3], 0.5, q), "Opacity scales alpha");
    assert_eq!(px(bc, 110, 100)[3], 0.0, "unpainted texels are transparent");
    // Half-covered green: alpha 0.5 (bottom), 0.25 (top: × opacity 0.5).
    assert!(near(px(bc, 70, 100)[3], 0.5, q) && near(px(bc, 70, 10)[3], 0.25, q));
    let op = file(&files, "_Opacity");
    let o = load_bitmap(op).unwrap();
    assert_eq!(o.channels, 1, "Opacity is one grey channel");
    assert!(near(o.texel(10, 100, false)[0], 1.0, q) && near(o.texel(10, 10, false)[0], 0.5, q) && o.texel(110, 100, false)[0] == 0.0);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn rgba_alpha_survives_tga_and_exr() {
    let mesh = primitives::plane(2.0, 1);
    let ch = transparent_channels(true);
    let sets = [ExportTextureSet { name: "Glass", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    for (format, tol) in [(FileFormat::Tga8, 1.0 / 255.0), (FileFormat::Png16, 1.0 / 65535.0), (FileFormat::Exr32, 1e-3)] {
        let settings = ExportSettings { template: ExportTemplate::pbr_metal_rough_opacity(), padding: Padding::None, format_override: Some(format), ..Default::default() };
        let dir = tmp(&format!("alpha-roundtrip-{}", format.extension()));
        let files = export(&mesh, "M", &sets, &settings, &dir).unwrap();
        let bc = file(&files, "_BaseColor");
        assert_eq!(bc.extension().unwrap(), format.extension());
        let b = load_bitmap(bc).unwrap();
        assert_eq!(b.channels, 4, "{format:?}");
        let a = |x, y| b.texel(x, y, false)[3];
        assert!(near(a(10, 100), 1.0, tol) && near(a(10, 10), 0.5, tol) && near(a(70, 10), 0.25, tol) && near(a(110, 100), 0.0, tol), "{format:?}");
        if format == FileFormat::Exr32 {
            assert!(near(b.texel(10, 100, false)[0], 1.0, tol), "EXR colour stays linear");
        }
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn transparent_texels_take_the_nearest_visible_colour_and_keep_alpha_zero() {
    let mesh = primitives::plane(2.0, 1);
    let ch = transparent_channels(false);
    let sets = [ExportTextureSet { name: "Decal", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let settings = ExportSettings { template: ExportTemplate::godot_alpha(), padding: Padding::None, ..Default::default() };
    let dir = tmp("alpha-bleed");
    let files = export(&mesh, "M", &sets, &settings, &dir).unwrap();
    let bc = file(&files, "_BaseColor");
    // Just right of the red half: red colour, alpha 0 (no grey fringe when filtered).
    let p = px(bc, 66, 50);
    assert_eq!(p[3], 0.0);
    assert!(p[0] > 0.99 && p[1] < 0.01 && p[2] < 0.01, "{p:?}");
    // Right of the green column: green.
    let p = px(bc, 75, 50);
    assert!(p[1] > 0.99 && p[0] < 0.01 && p[3] == 0.0, "{p:?}");
    // Visible texels are untouched.
    assert_eq!(px(bc, 10, 50), [1.0, 0.0, 0.0, 1.0]);
    assert!(!files.iter().any(|f| f.to_string_lossy().contains("Opacity")), "the alpha presets for engines have no separate map");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn opaque_sets_export_alpha_one_and_no_opacity_map() {
    let mesh = primitives::plane(2.0, 1);
    let ch = channels();
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let settings = ExportSettings { template: ExportTemplate::pbr_metal_rough_opacity(), ..Default::default() };
    let names: Vec<String> = plan("M", &sets, &settings).into_iter().map(|(_, p)| p).collect();
    assert!(!names.iter().any(|n| n.contains("Opacity")), "nothing to write: {names:?}");
    let dir = tmp("alpha-opaque");
    let files = export(&mesh, "M", &sets, &settings, &dir).unwrap();
    let bc = file(&files, "_BaseColor");
    assert_eq!(load_bitmap(bc).unwrap().channels, 4);
    assert_eq!(px(bc, 40, 40)[3], 1.0);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn padding_keeps_alpha_edges_inside_islands() {
    // The crate has gutters between its islands; padding must only write gutter texels.
    let mesh = primitives::sample_crate();
    let ch = transparent_channels(false);
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let covered = coverage(&mesh, 0, RES);
    assert!(covered.iter().any(|c| !c) && covered.iter().any(|c| *c));
    let source = ch[&ChannelKind::BaseColor].to_dense();
    for padding in [Padding::Infinite, Padding::Texels(4)] {
        let settings = ExportSettings { template: ExportTemplate::pbr_metal_rough_opacity(), padding, ..Default::default() };
        let dir = tmp("alpha-pad");
        let files = export(&mesh, "M", &sets, &settings, &dir).unwrap();
        let b = load_bitmap(file(&files, "_BaseColor")).unwrap();
        let mut transparent_inside = 0;
        for y in 0..RES {
            for x in 0..RES {
                let i = (y * RES + x) as usize;
                if !covered[i] {
                    continue;
                }
                let want = source[i * 4 + 3];
                let got = b.texel(x, y, false)[3];
                assert!(near(got, want, 1.0 / 255.0), "{padding:?} ({x}, {y}): alpha {got}, want {want}");
                transparent_inside += (want == 0.0) as usize;
            }
        }
        assert!(transparent_inside > 0, "the test covers transparent texels inside islands");
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn base_color_component_a_is_the_stack_coverage() {
    // Painter's "With Alpha" templates read the A of a document channel.
    let ch = transparent_channels(true);
    let set = ExportTextureSet { name: "S", material_slot: 0, udim: None, channels: &ch, mesh_maps: None };
    let map = OutputMap::new("C", FileFormat::Png8, vec![Source::ch(ChannelKind::BaseColor, 3), Source::ch(ChannelKind::Emissive, 3)], false);
    let mut dense = Dense { set: &set, size: RES, cache: HashMap::new(), normal: None, view: None, mesh_maps: HashMap::new(), alpha: None };
    let data = evaluate_map(&map, &mut dense, &ExportSettings::default(), FileFormat::Png8);
    let at = |x: u32, y: u32, c: usize| data[((y * RES + x) as usize) * 2 + c];
    assert_eq!((at(10, 10, 0), at(110, 10, 0)), (1.0, 0.0), "coverage, not opacity");
    assert_eq!(at(110, 10, 1), 1.0, "a channel the set lacks is fully covered");
}

/// QA 2026-11: in an sRGB map, the A component of a colour channel (its coverage) was
/// sRGB-encoded like the colour: half coverage came out as 0.735 (188 in 8 bits), not 0.5.
#[test]
fn coverage_in_an_srgb_map_is_not_colour_encoded() {
    let ch = transparent_channels(false);
    let set = ExportTextureSet { name: "S", material_slot: 0, udim: None, channels: &ch, mesh_maps: None };
    let map = OutputMap::new("C", FileFormat::Png8, vec![Source::ch(ChannelKind::BaseColor, 1), Source::ch(ChannelKind::BaseColor, 3)], true);
    let mut dense = Dense { set: &set, size: RES, cache: HashMap::new(), normal: None, view: None, mesh_maps: HashMap::new(), alpha: None };
    let data = evaluate_map(&map, &mut dense, &ExportSettings::default(), FileFormat::Png8);
    let at = |x: u32, y: u32, c: usize| data[((y * RES + x) as usize) * 2 + c];
    assert!(near(at(70, 10, 1), 0.5, 1e-6), "half coverage stays 0.5, got {}", at(70, 10, 1));
    assert!(near(at(110, 10, 0), linear_to_srgb(0.5), 1e-6), "the colour component is still encoded");
}

#[test]
fn alpha_presets_are_builtin_and_valid() {
    let names: Vec<String> = ExportTemplate::builtin().into_iter().map(|t| t.name).collect();
    for want in ["PBR Metallic Roughness + Opacity", "Unreal Engine (ORM packed) + Opacity", "Unity HDRP (Mask Map) + Alpha", "Unity URP (Metallic + Occlusion) + Alpha", "Godot 4 (ORM) + Alpha", "glTF 2.0 + Alpha"] {
        assert!(names.iter().any(|n| n == want), "{want}");
    }
    // The alpha variants (Painter's templates come after them).
    for t in ExportTemplate::builtin().into_iter().filter(|t| t.name.ends_with("+ Opacity") || t.name.ends_with("+ Alpha")) {
        let bc = t.maps.iter().find(|m| m.srgb && m.components.len() == 4).unwrap_or_else(|| panic!("{}: RGBA base colour", t.name));
        assert_eq!(bc.components[3], Source::Alpha, "{}", t.name);
        assert!(bc.format != FileFormat::Jpeg && t.problems().is_empty(), "{}", t.name);
        // Survives the user-preset JSON format.
        let copy = preset::duplicate(&t, "Copy");
        assert_eq!(preset::from_json(&preset::to_json(&copy), "x").unwrap().presets.remove(0), copy);
    }
    assert_eq!(serde_json::to_string(&Source::Alpha).unwrap(), r#"{"type":"alpha"}"#);
}
