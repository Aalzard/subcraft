//! Texture Set mixing settings in the viewport shader: Normal mixing (Combine / Replace), Height
//! to normal method (Sharp / Smooth) and AO mixing (Multiply / Replace), as the export applies
//! them. Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_doc::{AoMixing, HeightToNormal, MixingSettings, NormalMixing, ShaderParams};
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::{ViewMode, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 32;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn centre(img: &[u8]) -> [i32; 3] {
    let i = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    [img[i] as i32, img[i + 1] as i32, img[i + 2] as i32]
}

fn mean(img: &[u8]) -> f32 {
    let px = img.chunks_exact(4);
    let n = px.len() as f32;
    px.map(|c| (c[0] as f32 + c[1] as f32 + c[2] as f32) / 3.0).sum::<f32>() / n
}

fn mixing(f: impl FnOnce(&mut MixingSettings)) -> MixingSettings {
    let mut m = MixingSettings::LEGACY;
    f(&mut m);
    m
}

#[test]
fn normal_mixing_and_height_to_normal_method_change_the_shaded_normal() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2.0, 1));
    r.upload_channel(&device, &queue, 0, ChannelKind::Normal, &TiledImage::new(64, 64, 4, [0.5, 0.5, 1.0, 1.0]), None);
    // A 3 x 3 height map rising along u on the middle row only: the central difference reads
    // the middle row (slope 1.5 per UV unit); Sobel weighs it 2 of 4 with the flat rows above
    // and below, half the slope.
    let mut h = TiledImage::new(3, 3, 1, [0.0; 4]);
    for x in 0..3 {
        h.set(x, 1, [x as f32 * 0.5, 0.0, 0.0, 0.0]);
    }
    r.upload_channel(&device, &queue, 0, ChannelKind::Height, &h, None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z), Mat4::orthographic_rh(-1.0, 1.0, -1.0, 1.0, 0.1, 10.0));
    p.antialias = false;
    p.channel_display.unlit = true;
    p.height_scale = 0.2;
    p.view_mode = ViewMode::NormalHeight;
    let mut shot = |m: MixingSettings| {
        r.set_mixing(0, m, false);
        centre(&r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue))
    };
    let sharp = shot(MixingSettings::LEGACY);
    let smooth = shot(mixing(|m| m.height_to_normal = HeightToNormal::Smooth));
    let replace = shot(mixing(|m| m.normal = NormalMixing::Replace));
    eprintln!("sharp {sharp:?} smooth {smooth:?} replace {replace:?}");
    // The CPU reference: tilt x = -slope * height scale, normalized.
    let red = |slope: f32| { let x = -slope * 0.2; (x / (x * x + 1.0).sqrt() * 0.5 + 0.5) * 255.0 };
    assert!((sharp[0] as f32 - red(1.5)).abs() <= 5.0, "Sharp red {} vs {}", sharp[0], red(1.5));
    assert!((smooth[0] as f32 - red(0.75)).abs() <= 5.0, "Smooth red {} vs {}", smooth[0], red(0.75));
    assert!((sharp[1] - 128).abs() <= 3 && (smooth[1] - 128).abs() <= 3, "no slope along v");
    // Replace: the Normal channel alone, flat whatever the height.
    assert!((replace[0] - 128).abs() <= 2 && (replace[1] - 128).abs() <= 2 && replace[2] >= 250, "Replace ignores the height: {replace:?}");
}

#[test]
fn ao_mixing_multiply_darkens_the_painted_ao_by_the_baked_map() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2.0, 1));
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z), Mat4::orthographic_rh(-0.8, 0.8, -0.8, 0.8, 0.1, 10.0));
    p.antialias = false;
    p.view_mode = ViewMode::Material;
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &TiledImage::new(64, 64, 4, [0.5, 0.5, 0.5, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &TiledImage::new(64, 64, 1, [1.0; 4]), None);
    // A painted AO channel with no occlusion over a fully occluding baked map.
    r.upload_channel(&device, &queue, 0, ChannelKind::AmbientOcclusion, &TiledImage::new(64, 64, 1, [1.0; 4]), None);
    r.upload_mesh_map(&device, &queue, 0, MeshMapKind::AmbientOcclusion, &TiledImage::new(64, 64, 1, [0.0; 4]));
    r.set_shader(0, ShaderParams { ao_intensity: 1.0, ..Default::default() });
    let mut shot = |m: MixingSettings, has_channel: bool| {
        r.set_mixing(0, m, has_channel);
        mean(&r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue))
    };
    let replace = shot(MixingSettings::LEGACY, true);
    let multiply = shot(mixing(|m| m.ao = AoMixing::Multiply), true);
    // Without an AO channel of its own the baked map already stands in: no second multiply.
    let no_channel = shot(mixing(|m| m.ao = AoMixing::Multiply), false);
    assert!(multiply < replace - 5.0, "Multiply applies the baked AO: {multiply} vs Replace {replace}");
    assert_eq!(no_channel, replace, "no AO channel: nothing to multiply");
}
