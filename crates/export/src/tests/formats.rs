//! Painter's export settings: padding modes, the Size menu, 8-bit dithering and the extra file
//! types (BMP, HDR, WebP, half-float EXR, Netpbm).
use super::*;
use subcraft_image::BitmapData;

fn decode(name: &str, bytes: &[u8]) -> subcraft_image::Bitmap {
    subcraft_image::io::decode_bitmap(bytes, Some(name)).unwrap()
}

fn render(mesh: &Mesh, sets: &[ExportTextureSet], s: &ExportSettings) -> Vec<(String, subcraft_image::Bitmap)> {
    let mut out = Vec::new();
    render_maps(mesh, "M", sets, s, |n, b| {
        out.push((n.to_string(), decode(n, b)));
        Ok(())
    })
    .unwrap();
    out
}

fn user(maps: Vec<OutputMap>) -> ExportTemplate {
    ExportTemplate { name: "Mine".into(), maps, id: Some(preset::new_id()), ..Default::default() }
}

/// A plane whose UVs fill only the middle half (wide gutters), its coverage, and the Chebyshev
/// distance from each texel to the nearest covered one.
fn island() -> (Mesh, Vec<bool>, Vec<i32>) {
    let mut mesh = primitives::plane(2.0, 1);
    mesh.uvs.iter_mut().for_each(|uv| *uv = *uv * 0.5 + glam::Vec2::splat(0.25));
    let cov = coverage(&mesh, 0, RES);
    let covered: Vec<(i32, i32)> = (0..cov.len()).filter(|j| cov[*j]).map(|j| ((j as u32 % RES) as i32, (j as u32 / RES) as i32)).collect();
    let dist = (0..cov.len())
        .map(|i| {
            let (x, y) = ((i as u32 % RES) as i32, (i as u32 / RES) as i32);
            covered.iter().map(|(cx, cy)| (cx - x).abs().max((cy - y).abs())).min().unwrap()
        })
        .collect();
    (mesh, cov, dist)
}

#[test]
fn background_padding_dilates_then_fills_with_each_maps_default() {
    let (mesh, cov, dist) = island();
    let mut ch = BTreeMap::new();
    // Inside the island: roughness 1, a tilted normal.
    let mut rough = TiledImage::new(RES, RES, 1, [0.0; 4]);
    let mut normal = TiledImage::new(RES, RES, 3, [0.0; 4]);
    for (i, &c) in cov.iter().enumerate() {
        if c {
            rough.set(i as u32 % RES, i as u32 / RES, [1.0; 4]);
            normal.set(i as u32 % RES, i as u32 / RES, [0.8, 0.5, 0.6, 0.0]);
        }
    }
    ch.insert(ChannelKind::Roughness, rough);
    ch.insert(ChannelKind::Normal, normal);
    let sets = [ExportTextureSet { name: "C", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let maps = vec![
        OutputMap::new("Rough", FileFormat::Png8, vec![Source::ch(ChannelKind::Roughness, 0)], false),
        OutputMap::new("Normal", FileFormat::Png8, (0..3).map(|c| Source::Normal { component: c, directx: false }).collect(), false),
        OutputMap::new("Gloss", FileFormat::Png8, vec![Source::Channel { kind: ChannelKind::Roughness, component: 0, invert: true }], false),
    ];
    let s = ExportSettings { template: user(maps), padding: Padding::Background(2), height_to_normal: 0.0, ..Default::default() };
    let files = render(&mesh, &sets, &s);
    let near = (0..cov.len()).find(|i| dist[*i] == 2).unwrap();
    let far = (0..cov.len()).max_by_key(|i| dist[*i]).unwrap();
    let at = |name: &str, i: usize| files.iter().find(|f| f.0.contains(name)).unwrap().1.texel(i as u32 % RES, i as u32 / RES, false);
    let rough_default = ChannelKind::Roughness.default_value().0[0];
    assert_eq!(at("Rough", near)[0], 1.0, "dilated");
    assert!((at("Rough", far)[0] - rough_default).abs() < 1.0 / 255.0, "background = the channel default {rough_default}: {:?}", at("Rough", far));
    let n = at("Normal", far);
    assert!((n[0] - 0.5).abs() < 1.0 / 255.0 && (n[1] - 0.5).abs() < 1.0 / 255.0 && (n[2] - 1.0).abs() < 1.0 / 255.0, "flat normal: {n:?}");
    assert!((at("Gloss", far)[0] - (1.0 - rough_default)).abs() < 1.0 / 255.0, "the map's own conversion of the default (inverted)");
}

#[test]
fn diffusion_padding_blends_islands_into_the_gutter() {
    let (mesh, cov, dist) = island();
    // Left half of the island 0, right half 1.
    let mut rough = TiledImage::new(RES, RES, 1, [0.0; 4]);
    for (i, &c) in cov.iter().enumerate() {
        if c && i as u32 % RES >= RES / 2 {
            rough.set(i as u32 % RES, i as u32 / RES, [1.0; 4]);
        }
    }
    let mut ch = BTreeMap::new();
    ch.insert(ChannelKind::Roughness, rough);
    let sets = [ExportTextureSet { name: "C", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let map = |suffix: &str, padding| OutputMap { padding: Some(padding), ..OutputMap::new(suffix, FileFormat::Png16, vec![Source::ch(ChannelKind::Roughness, 0)], false) };
    let s = ExportSettings { template: user(vec![map("Diff", Padding::Diffusion(2)), map("Inf", Padding::Infinite)]), ..Default::default() };
    let files = render(&mesh, &sets, &s);
    let img = |name: &str| &files.iter().find(|f| f.0.contains(name)).unwrap().1;
    let (diff, inf) = (img("Diff"), img("Inf"));
    let v = |b: &subcraft_image::Bitmap, x: u32, y: u32| b.texel(x, y, false)[0];
    // Along the top gutter row, infinite dilation jumps from 0 to 1; diffusion ramps.
    let row: Vec<f32> = (0..RES).map(|x| v(diff, x, 2)).collect();
    assert!(row.windows(2).all(|w| w[1] >= w[0] - 1e-3), "monotonic left to right: {row:?}");
    let mid_values = row.iter().filter(|r| **r > 0.05 && **r < 0.95).count();
    let inf_mid = (0..RES).filter(|x| (0.05..0.95).contains(&v(inf, *x, 2))).count();
    assert!(mid_values > 8 && inf_mid == 0, "a ramp, not a step: {mid_values} vs {inf_mid}");
    // Texels within the dilation distance are a straight copy, like the other dilation modes.
    for i in (0..cov.len()).filter(|i| dist[*i] == 1) {
        let (x, y) = (i as u32 % RES, i as u32 / RES);
        assert!((v(diff, x, y) - v(inf, x, y)).abs() < 1e-3 || x.abs_diff(RES / 2) < 3, "dilated ring at ({x}, {y})");
    }
}

#[test]
fn window_size_applies_unless_the_preset_or_map_sets_one() {
    let ch = distinct_channels();
    let sets = [ExportTextureSet { name: "C", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let mesh = primitives::plane(2.0, 1);
    let src = vec![Source::ch(ChannelKind::Roughness, 0)];
    let own = OutputMap { size: Some(32), ..OutputMap::new("Own", FileFormat::Png8, src.clone(), false) };
    let inherit = OutputMap::new("Inherit", FileFormat::Png8, src, false);
    let mut s = ExportSettings { template: user(vec![own, inherit]), size: Some(64), ..Default::default() };
    let w = |s: &ExportSettings| render(&mesh, &sets, s).into_iter().map(|f| f.1.width).collect::<Vec<_>>();
    assert_eq!(w(&s), [32, 64], "map size first, then the window's");
    s.template.size = Some(256);
    assert_eq!(w(&s), [32, 256], "a preset size wins over the window's");
    s.template.size = None;
    s.size = None;
    assert_eq!(w(&s), [32, RES], "texture set size");
    assert_eq!(EXPORT_SIZES, [128, 256, 512, 1024, 2048, 4096, 8192]);
}

#[test]
fn dithering_follows_the_window_then_the_map() {
    let mut ch = BTreeMap::new();
    ch.insert(ChannelKind::Roughness, TiledImage::new(RES, RES, 1, [100.3 / 255.0; 4]));
    let sets = [ExportTextureSet { name: "C", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let mesh = primitives::plane(2.0, 1);
    let src = vec![Source::ch(ChannelKind::Roughness, 0)];
    let plain = OutputMap::new("Plain", FileFormat::Png8, src.clone(), false);
    let dithered = OutputMap { dither: Some(true), ..OutputMap::new("Dith", FileFormat::Png8, src.clone(), false) };
    let deep = OutputMap { dither: Some(true), ..OutputMap::new("Deep", FileFormat::Png16, src, false) };
    let levels = |b: &subcraft_image::Bitmap| {
        let BitmapData::U8(px) = &b.data else { return 0 };
        px.iter().collect::<std::collections::BTreeSet<_>>().len()
    };
    let mut s = ExportSettings { template: user(vec![plain, dithered, deep]), padding: Padding::None, ..Default::default() };
    let files = render(&mesh, &sets, &s);
    assert_eq!(levels(&files[0].1), 1, "rounded: one flat level");
    assert!(levels(&files[1].1) > 1, "the map asks for dithering");
    let BitmapData::U8(px) = &files[1].1.data else { panic!() };
    let mean = px.iter().map(|&v| v as f32).sum::<f32>() / px.len() as f32;
    assert!((mean - 100.3).abs() < 0.1, "dithering keeps the level: {mean}");
    assert!(matches!(files[2].1.data, BitmapData::U16(_)), "16-bit files are not dithered");
    s.dither = Some(false);
    assert_eq!(levels(&render(&mesh, &sets, &s)[1].1), 1, "the window's choice wins");
    s.dither = Some(true);
    assert!(levels(&render(&mesh, &sets, &s)[0].1) > 1);
}

/// Painter's Netpbm file types: PBM, PGM, PPM and PFM files are written and read back, and a
/// four-component map keeps its own file type when the window picks one without alpha.
#[test]
fn netpbm_file_types_export_and_read_back() {
    let ch = distinct_channels();
    let sets = [ExportTextureSet { name: "C", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let mesh = primitives::plane(2.0, 1);
    let rgb = || (0..3).map(|c| Source::ch(ChannelKind::BaseColor, c)).collect::<Vec<_>>();
    let maps = vec![
        OutputMap::new("Ppm", FileFormat::Ppm8, rgb(), true),
        OutputMap::new("Pgm", FileFormat::Pgm16, vec![Source::ch(ChannelKind::Roughness, 0)], false),
        OutputMap::new("Pbm", FileFormat::Pbm1, vec![Source::ch(ChannelKind::Metallic, 0)], false),
        OutputMap::new("Pfm", FileFormat::Pfm32, rgb(), true),
    ];
    let s = ExportSettings { template: user(maps), ..Default::default() };
    let mut files: Vec<(String, subcraft_image::netpbm::Decoded)> = Vec::new();
    render_maps(&mesh, "M", &sets, &s, |n, b| {
        files.push((n.to_string(), subcraft_image::netpbm::decode(b).expect("a Netpbm file")));
        Ok(())
    })
    .unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names, ["M_C_Ppm.ppm", "M_C_Pgm.pgm", "M_C_Pbm.pbm", "M_C_Pfm.pfm"]);
    let at = |f: &subcraft_image::netpbm::Decoded, c: usize| f.data[(9 * f.width as usize + 9) * f.comps + c];
    let base = ch[&ChannelKind::BaseColor].get(9, 9);
    for c in 0..3 {
        assert!((at(&files[0].1, c) - linear_to_srgb(base[c])).abs() < 1.0 / 255.0, "8-bit PPM is sRGB");
        assert!((at(&files[3].1, c) - base[c]).abs() < 1e-6, "PFM stays linear");
    }
    assert!((at(&files[1].1, 0) - 0.3).abs() < 1.0 / 65535.0, "16-bit PGM roughness");
    assert_eq!(at(&files[2].1, 0), 1.0, "metallic 0.8 is white in PBM");
    assert_eq!((files[1].1.width, files[1].1.comps), (RES, 1));
    // The window's PPM override leaves a four-component map in its own (alpha) file type.
    let settings = ExportSettings { template: ExportTemplate::unity_hdrp(), format_override: Some(FileFormat::Ppm8), ..Default::default() };
    for map in &settings.template.maps {
        assert_eq!(output_format(map, &settings) == FileFormat::Ppm8, map.components.len() != 4, "{}", map.suffix);
    }
}

#[test]
fn extra_file_types_export_and_read_back() {
    let ch = distinct_channels();
    let sets = [ExportTextureSet { name: "C", material_slot: 0, udim: None, channels: &ch, mesh_maps: None, mixing: subcraft_doc::MixingSettings::LEGACY }];
    let mesh = primitives::plane(2.0, 1);
    let rgb = || (0..3).map(|c| Source::ch(ChannelKind::BaseColor, c)).collect::<Vec<_>>();
    let maps = vec![
        OutputMap::new("Bmp", FileFormat::Bmp8, rgb(), true),
        OutputMap::new("Webp", FileFormat::Webp8, rgb(), true),
        OutputMap::new("Half", FileFormat::Exr16, rgb(), true),
        OutputMap::new("Hdr", FileFormat::Hdr32, rgb(), true),
    ];
    let s = ExportSettings { template: user(maps), ..Default::default() };
    let files = render(&mesh, &sets, &s);
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names, ["M_C_Bmp.bmp", "M_C_Webp.webp", "M_C_Half.exr", "M_C_Hdr.hdr"]);
    let base = ch[&ChannelKind::BaseColor].get(9, 9);
    let (bmp, webp, half, hdr) = (files[0].1.texel(9, 9, false), files[1].1.texel(9, 9, false), files[2].1.texel(9, 9, false), files[3].1.texel(9, 9, false));
    for c in 0..3 {
        let srgb = linear_to_srgb(base[c]);
        assert!((bmp[c] - srgb).abs() < 1.0 / 255.0 && (webp[c] - srgb).abs() < 1.0 / 255.0, "8-bit files are sRGB");
        assert!((half[c] - base[c]).abs() < 1e-3, "float files stay linear: {half:?} vs {base:?}");
        assert!((hdr[c] - base[c]).abs() < 0.02 * base[c].max(0.05), "RGBE: {hdr:?} vs {base:?}");
    }
}
