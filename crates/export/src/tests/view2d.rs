//! "2D View" export: the file equals what the 2D panel displays.
use super::*;

fn view_settings(view: View2d, padding: Padding, format: Option<FileFormat>) -> ExportSettings {
    ExportSettings { template: ExportTemplate::view_2d(), view_2d: Some(view), padding, format_override: format, ..Default::default() }
}

fn run(ch: &BTreeMap<ChannelKind, TiledImage>, maps: Option<&BTreeMap<MeshMapKind, TiledImage>>, s: &ExportSettings, name: &str) -> PathBuf {
    let mesh = primitives::plane(2.0, 1);
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: ch, mesh_maps: maps }];
    let dir = tmp(name);
    let files = export(&mesh, "M", &sets, s, &dir).unwrap();
    assert_eq!(files.len(), 1, "exactly one file: {files:?}");
    assert_eq!(files[0].file_stem().unwrap(), "M_Body_View2D");
    files[0].clone()
}

#[test]
fn color_view_matches_the_displayed_srgb_pixels() {
    let ch = channels(); // base color (0.5, 0.25, 0) linear
    let f = run(&ch, None, &view_settings(View2d::Channel(ChannelKind::BaseColor), Padding::None, None), "v-color");
    let p = px(&f, 10, 10);
    for (got, lin) in p.iter().zip([0.5f32, 0.25, 0.0]) {
        assert!((got - linear_to_srgb(lin)).abs() < 1.0 / 255.0, "{got} vs {}", linear_to_srgb(lin));
    }
}

#[test]
fn data_and_single_component_views_are_stored_values_shown_as_gray() {
    let ch = channels(); // roughness 0.25
    let f = run(&ch, None, &view_settings(View2d::Channel(ChannelKind::Roughness), Padding::None, None), "v-gray");
    let p = px(&f, 3, 3);
    for c in 0..3 {
        assert!((p[c] - 0.25).abs() < 1.0 / 255.0);
    }
    // normal is a data channel: not color-transformed
    let f = run(&ch, None, &view_settings(View2d::Channel(ChannelKind::Normal), Padding::None, None), "v-normal");
    let p = px(&f, 3, 3);
    assert!((p[0] - 0.5).abs() < 1.0 / 255.0 && (p[2] - 1.0).abs() < 1.0 / 255.0);
}

#[test]
fn sixteen_bit_keeps_more_precision_than_eight() {
    let mut ch = channels();
    ch.insert(ChannelKind::Roughness, TiledImage::new(RES, RES, 1, [0.3337; 4]));
    let v = view_settings(View2d::Channel(ChannelKind::Roughness), Padding::None, Some(FileFormat::Png16));
    let p16 = px(&run(&ch, None, &v, "v-16"), 5, 5)[0];
    let p8 = px(&run(&ch, None, &view_settings(View2d::Channel(ChannelKind::Roughness), Padding::None, Some(FileFormat::Png8)), "v-8"), 5, 5)[0];
    assert!((p16 - 0.3337).abs() < 1.0 / 65535.0 * 2.0, "{p16}");
    assert!((p8 - 0.3337).abs() > 1.0 / 65535.0 * 2.0 && (p8 - 0.3337).abs() <= 0.5 / 255.0 + 1e-4, "{p8}");
}

#[test]
fn exr_view_stays_linear() {
    let ch = channels();
    let f = run(&ch, None, &view_settings(View2d::Channel(ChannelKind::BaseColor), Padding::None, Some(FileFormat::Exr32)), "v-exr");
    assert_eq!(f.extension().unwrap(), "exr");
    let p = px(&f, 4, 4);
    assert!((p[0] - 0.5).abs() < 1e-4 && (p[1] - 0.25).abs() < 1e-4);
}

#[test]
fn padding_fills_the_gutter_with_the_displayed_island_color() {
    // Islands are (0.5, 0.25, 0); gutter texels start black so padding is visible.
    let mesh = primitives::sample_crate();
    let cov = coverage(&mesh, 0, RES);
    let mut ch = channels();
    let mut base = TiledImage::new(RES, RES, 4, [0.0, 0.0, 0.0, 1.0]);
    for (i, _) in cov.iter().enumerate().filter(|(_, c)| **c) {
        base.set(i as u32 % RES, i as u32 / RES, [0.5, 0.25, 0.0, 1.0]);
    }
    ch.insert(ChannelKind::BaseColor, base);
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let hole = cov.iter().position(|c| !c).expect("crate UVs leave some gutter");
    let (hx, hy) = (hole as u32 % RES, hole as u32 / RES);
    let hole_pixel = |padding: Padding, name: &str| {
        let dir = tmp(name);
        let files = export(&mesh, "M", &sets, &view_settings(View2d::Channel(ChannelKind::BaseColor), padding, None), &dir).unwrap();
        let p = px(&files[0], hx, hy);
        std::fs::remove_dir_all(dir).unwrap();
        p
    };
    assert!(hole_pixel(Padding::None, "v-nopad")[0] < 1.0 / 255.0, "unpadded gutter stays as displayed");
    let padded = hole_pixel(Padding::Infinite, "v-pad");
    assert!((padded[0] - linear_to_srgb(0.5)).abs() < 1.0 / 255.0 && (padded[1] - linear_to_srgb(0.25)).abs() < 1.0 / 255.0, "gutter {padded:?}");
}

#[test]
fn baked_mesh_map_view_exports_and_skips_when_missing() {
    let mesh = primitives::plane(2.0, 1);
    let ch = channels();
    let mut maps = BTreeMap::new();
    maps.insert(MeshMapKind::AmbientOcclusion, TiledImage::new(RES, RES, 1, [0.4; 4]));
    let f = run(&ch, Some(&maps), &view_settings(View2d::MeshMap(MeshMapKind::AmbientOcclusion), Padding::None, None), "v-mm");
    assert!((px(&f, 2, 2)[0] - 0.4).abs() < 1.0 / 255.0);
    // no baked map → nothing is written (and no panic)
    let sets = [ExportTextureSet { name: "Body", material_slot: 0, udim: None, channels: &ch, mesh_maps: None }];
    let s = view_settings(View2d::MeshMap(MeshMapKind::AmbientOcclusion), Padding::None, None);
    assert!(plan("M", &sets, &s).is_empty());
    assert!(export(&mesh, "M", &sets, &ExportSettings { view_2d: None, ..s }, &tmp("v-none")).unwrap().is_empty());
}

#[test]
fn view_2d_is_a_builtin_preset() {
    assert!(ExportTemplate::builtin().iter().any(|t| t.name == "2D View"));
    let json = serde_json::to_string(&ExportTemplate::view_2d()).unwrap();
    assert_eq!(serde_json::from_str::<ExportTemplate>(&json).unwrap(), ExportTemplate::view_2d());
}
