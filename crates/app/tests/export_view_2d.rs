//! File > Export 2D view: the written image equals the 2D panel's display of the selected
//! texture set (gray for scalar channels, sRGB for color channels) with the export settings.
use subcraft_app::Session;
use subcraft_core::ChannelKind;
use subcraft_core::color::linear_to_srgb;
use subcraft_export::{ExportSettings, Padding, View2d};
use subcraft_image::FileFormat;

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-view2d-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn check(kind: ChannelKind, format: FileFormat, tolerance: f32) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let settings = ExportSettings { padding: Padding::None, format_override: Some(format), ..Default::default() };
    let dir = temp(&format!("{kind:?}-{format:?}"));
    let files = s.export_view_2d(&dir, View2d::Channel(kind), &settings).unwrap();
    assert_eq!(files.len(), 1, "one image for the selected texture set");
    let ts = s.selected_texture_set().id;
    let img = s.composite(ts).unwrap().channels[&kind].clone();
    let out = subcraft_image::load_bitmap(&files[0]).unwrap();
    assert_eq!((out.width, out.height), (img.width(), img.height()));
    for (x, y) in [(0, 0), (13, 7), (31, 40), (63, 63)] {
        let v = img.get(x, y);
        let p = out.texel(x, y, false);
        for c in 0..3 {
            let raw = if img.comps() == 1 { v[0] } else { v[c] }.clamp(0.0, 1.0);
            let shown = if kind.is_color() { linear_to_srgb(raw) } else { raw };
            assert!((p[c] - shown).abs() <= tolerance, "{kind:?} {format:?} ({x},{y}) c{c}: {} vs {shown}", p[c]);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn exported_pixels_equal_the_displayed_channel_8_and_16_bit() {
    check(ChannelKind::BaseColor, FileFormat::Png8, 0.5 / 255.0 + 1e-4);
    check(ChannelKind::Roughness, FileFormat::Png8, 0.5 / 255.0 + 1e-4);
    check(ChannelKind::BaseColor, FileFormat::Png16, 0.5 / 65535.0 + 1e-4);
    check(ChannelKind::Roughness, FileFormat::Png16, 0.5 / 65535.0 + 1e-4);
}

#[test]
fn missing_view_source_writes_nothing_and_regular_export_is_unchanged() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let settings = ExportSettings::default();
    let baked_ao = View2d::MeshMap(subcraft_core::MeshMapKind::AmbientOcclusion);
    assert!(s.export_view_2d(&temp("none"), baked_ao, &settings).unwrap().is_empty(), "no bake yet");
    // The 2D View map never appears in other templates' plans.
    assert!(s.export_plan(&settings).iter().all(|(_, p)| !p.contains("View2D")));
    let _ = std::fs::remove_dir_all(temp("none"));
}
