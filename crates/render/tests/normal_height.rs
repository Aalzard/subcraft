//! The "Normal + Height" display mode: the tangent-space normal the shading uses, the Normal
//! channel combined with the Height bump. Skips (passes with a message) when no adapter is
//! available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
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

#[test]
fn normal_plus_height_tilts_the_normal_by_the_height_slope() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2.0, 1));
    // A flat normal map and a height rising along u (left to right in texture space).
    r.upload_channel(&device, &queue, 0, ChannelKind::Normal, &TiledImage::new(64, 64, 4, [0.5, 0.5, 1.0, 1.0]), None);
    let mut ramp = TiledImage::new(64, 64, 1, [0.0; 4]);
    for y in 0..64 {
        for x in 0..64 {
            ramp.set(x, y, [x as f32 / 63.0, 0.0, 0.0, 0.0]);
        }
    }
    r.upload_channel(&device, &queue, 0, ChannelKind::Height, &ramp, None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z), Mat4::orthographic_rh(-1.0, 1.0, -1.0, 1.0, 0.1, 10.0));
    p.antialias = false;
    p.channel_display.unlit = true;
    p.height_scale = 0.5;
    let mut shot = |mode: ViewMode, height_scale: f32| {
        p.view_mode = mode;
        p.height_scale = height_scale;
        centre(&r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue))
    };
    let flat = shot(ViewMode::Normal, 0.5);
    assert!((flat[0] - 128).abs() <= 2 && (flat[1] - 128).abs() <= 2 && flat[2] >= 250, "the Normal channel is flat: {flat:?}");
    // The height slope tilts the combined normal against the rise (-x in tangent space).
    let combined = shot(ViewMode::NormalHeight, 0.5);
    assert!(combined[0] < flat[0] - 10, "tilted towards -u: {combined:?} vs flat {flat:?}");
    assert!((combined[1] - 128).abs() <= 3, "no slope along v: {combined:?}");
    assert!(combined[2] < flat[2], "less of it points straight out: {combined:?}");
    // The CPU reference: slope 1 per texture width times the height scale.
    let slope = 0.5f32 * (64.0 / 63.0);
    let expected = (-slope / (slope * slope + 1.0).sqrt() * 0.5 + 0.5) * 255.0;
    assert!((combined[0] as f32 - expected).abs() <= 4.0, "red {} vs expected {expected}", combined[0]);
    // Without height the view is the Normal channel itself.
    let no_height = shot(ViewMode::NormalHeight, 0.0);
    for k in 0..3 {
        assert!((no_height[k] - flat[k]).abs() <= 1, "height 0 equals the Normal view: {no_height:?} vs {flat:?}");
    }
}
