use glam::Vec3;
use subcraft_core::ChannelKind;
use subcraft_doc::{LayerContent, TextureSet};
use subcraft_image::Bitmap;

use super::*;

#[test]
fn library_contents_are_well_formed() {
    let lib = Library::builtin();
    assert!(lib.alphas.len() >= 6 && lib.materials.len() >= 12 && lib.smart_materials.len() >= 4 && lib.smart_masks.len() >= 4);
    assert!(lib.environments.len() >= 4);
    let mut ids: Vec<_> = lib.materials.iter().map(|m| m.id).chain(lib.alphas.iter().map(|a| a.id)).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "asset ids are unique");
    assert!(lib.alphas[0].bitmap.is_none(), "first tip is the procedural round brush");
    for a in lib.alphas.iter().skip(1) {
        let b = a.bitmap.as_ref().unwrap();
        let mut lit = 0;
        for y in 0..b.height {
            for x in 0..b.width {
                let t = b.texel(x, y, false);
                assert!((0.0..=1.0).contains(&t[0]));
                if t[0] > 0.5 {
                    lit += 1;
                }
            }
        }
        assert!(lit > 100, "{} paints something ({lit})", a.name);
    }
}

#[test]
fn presets_build_for_a_texture_set() {
    let lib = Library::builtin();
    let ts = TextureSet::new("T", 0, 256);
    for m in &lib.materials {
        let (ids, _) = presets::decode_images(&m.images);
        let layer = m.build_with(&ts, &ids);
        let LayerContent::Fill(f) = &layer.content else { panic!("{} is a fill", m.name) };
        let enabled: Vec<_> = f.channels.iter().filter(|(_, c)| c.enabled).map(|(k, _)| *k).collect();
        assert!(!enabled.is_empty(), "{}", m.name);
        let in_preset = |k: &ChannelKind| m.channels.iter().any(|(pk, _)| pk == k) || m.images.iter().any(|i| i.channel == *k);
        assert!(enabled.iter().all(in_preset), "{}: only preset channels enabled", m.name);
    }
    for s in &lib.smart_materials {
        let layer = s.build(&ts);
        assert!(layer.is_folder(), "{}", s.name);
        assert!(layer.children().len() >= 2, "{}", s.name);
        assert!(layer.children().iter().any(|c| c.mask.as_ref().is_some_and(|m| !m.effects.is_empty())), "{} has masked layers", s.name);
    }
    assert!(lib.smart_masks.iter().all(|m| !m.effects.is_empty()));
}

#[test]
fn constant_environment_prefilters_to_constant_irradiance() {
    let bmp = Bitmap::new_f32(64, 32, 3, vec![0.5; 64 * 32 * 3]);
    let p = environment::prefilter(&bmp);
    assert_eq!(p.levels.len(), 6);
    assert_eq!((p.levels[0].width, p.levels[0].height), (512, 256));
    for l in &p.levels {
        assert!(l.rgba.chunks_exact(4).all(|px| (px[0] - 0.5).abs() < 1e-3), "levels stay constant");
    }
    for n in [Vec3::Y, -Vec3::Y, Vec3::X, Vec3::new(0.3, -0.5, 0.8).normalize()] {
        let e = environment::irradiance(&p, n);
        assert!((e[0] - 0.5).abs() < 0.02, "irradiance of a uniform sky is the sky value: {e:?} at {n}");
    }
}

#[test]
fn bright_sky_lights_upward_normals_and_finds_the_sun() {
    // Upper hemisphere 1, lower 0.05, plus a bright spot (sun) at u=0.25, v=0.2.
    let (w, h) = (128u32, 64u32);
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let mut c = if v < 0.5 { 1.0 } else { 0.05 };
            if (u - 0.25).abs() < 0.03 && (v - 0.2).abs() < 0.03 {
                c = 200.0;
            }
            data.extend_from_slice(&[c, c * 0.9, c * 0.7]);
        }
    }
    let p = environment::prefilter(&Bitmap::new_f32(w, h, 3, data));
    let up = environment::irradiance(&p, Vec3::Y);
    let down = environment::irradiance(&p, -Vec3::Y);
    assert!(up[0] > 3.0 * down[0], "up {up:?} vs down {down:?}");
    let sun = Vec3::from_array(p.sun_direction);
    let expect = environment::dir_from_uv(0.25, 0.2);
    assert!(sun.dot(expect) > 0.97, "sun {sun} vs {expect}");
    assert!(p.sun_color[2] < p.sun_color[0], "warm sun color {:?}", p.sun_color);
    let (u, v) = environment::uv_from_dir(expect);
    assert!((u - 0.25).abs() < 1e-4 && (v - 0.2).abs() < 1e-4);
}

#[test]
fn import_alpha_from_png() {
    let dir = std::env::temp_dir().join(format!("subcraft-assets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tip.png");
    let data: Vec<f32> = (0..32 * 32).map(|i| if (i % 32) < 16 { 1.0 } else { 0.0 }).collect();
    subcraft_image::save_image(&path, 32, 32, 1, &data, subcraft_image::FileFormat::Png8).unwrap();
    let a = alphas::import(&path).unwrap();
    assert_eq!(a.name, "tip");
    let b = a.bitmap.unwrap();
    assert_eq!((b.width, b.height, b.channels), (32, 32, 2));
    assert!(b.texel(2, 2, false)[0] > 0.99 && b.texel(30, 2, false)[0] < 0.01);
    std::fs::remove_dir_all(&dir).unwrap();
}
