//! Colour profile: the material view shown raw (not sRGB-encoded) or through a 3D LUT in
//! OpenColorIO's `ociolutimage` layout. Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::color_profile::identity_lut;
use subcraft_render::{ViewMode, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 32;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn centre(img: &[u8]) -> [f32; 3] {
    let i = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    [img[i] as f32, img[i + 1] as f32, img[i + 2] as f32]
}

fn srgb_to_linear(v: f32) -> f32 {
    subcraft_core::color::srgb_to_linear(v)
}

#[test]
fn the_profile_shows_the_material_raw_or_through_the_lut() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2.0, 1));
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &TiledImage::new(64, 64, 4, [0.6, 0.35, 0.15, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &TiledImage::new(64, 64, 1, [1.0; 4]), None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z), Mat4::orthographic_rh(-1.0, 1.0, -1.0, 1.0, 0.1, 10.0));
    p.antialias = false;
    let mut shot = |r: &mut ViewportRenderer, on: bool, mode: ViewMode| {
        p.color_profile = on;
        p.view_mode = mode;
        r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue)
    };
    let off = shot(&mut r, false, ViewMode::Material);
    // On, without a LUT: the same linear values, stored raw instead of sRGB-encoded.
    let raw = shot(&mut r, true, ViewMode::Material);
    let (o, w) = (centre(&off), centre(&raw));
    for k in 0..3 {
        let expected = srgb_to_linear(o[k] / 255.0) * 255.0;
        assert!((w[k] - expected).abs() <= 2.5, "channel {k}: raw {} vs expected {expected} (sRGB {})", w[k], o[k]);
    }
    assert!(w[0] < o[0], "raw values are darker than their sRGB encoding");
    // An identity LUT changes nothing.
    let n = 16;
    let id = identity_lut(n);
    r.set_color_lut(&device, &queue, Some((&id, n * n, n))).unwrap();
    assert_eq!(r.color_lut_size(), Some(n));
    let through_id = shot(&mut r, true, ViewMode::Material);
    let t = centre(&through_id);
    for k in 0..3 {
        assert!((t[k] - w[k]).abs() <= 1.0, "identity LUT {k}: {} vs {}", t[k], w[k]);
    }
    // An inverting LUT inverts.
    let inv: Vec<[f32; 4]> = id.iter().map(|c| [1.0 - c[0], 1.0 - c[1], 1.0 - c[2], 1.0]).collect();
    r.set_color_lut(&device, &queue, Some((&inv, n * n, n))).unwrap();
    let i = centre(&shot(&mut r, true, ViewMode::Material));
    for k in 0..3 {
        assert!((i[k] - (255.0 - w[k])).abs() <= 2.0, "inverted {k}: {} vs {}", i[k], 255.0 - w[k]);
    }
    // A channel view is not touched by the profile.
    let channel_off = shot(&mut r, false, ViewMode::BaseColor);
    assert_eq!(shot(&mut r, true, ViewMode::BaseColor), channel_off);
    // Not a cube: refused, and the previous LUT stays.
    assert!(r.set_color_lut(&device, &queue, Some((&inv[..100], 10, 10))).is_err());
    assert_eq!(r.color_lut_size(), Some(n));
    // Cleared: raw again.
    r.set_color_lut(&device, &queue, None).unwrap();
    assert_eq!(r.color_lut_size(), None);
    assert_eq!(shot(&mut r, true, ViewMode::Material), raw);
    // Off: exactly the image from before.
    assert_eq!(shot(&mut r, false, ViewMode::Material), off);
}
