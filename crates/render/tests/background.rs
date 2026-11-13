//! The 3D view's background: flat by default, a vertical gradient with two colors.
//! Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_render::{ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 32;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn row(img: &[u8], y: u32) -> [u8; 3] {
    let i = ((y * SIZE + SIZE / 2) * 4) as usize;
    [img[i], img[i + 1], img[i + 2]]
}

fn srgb8(linear: f32) -> f32 {
    subcraft_core::color::linear_to_srgb(linear) * 255.0
}

#[test]
fn the_background_is_flat_or_a_vertical_gradient() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(0.8, 1.0, 0.1, 50.0));
    p.environment.background_opacity = 0.0;
    p.background = [0.05, 0.1, 0.2];
    // Flat: every pixel is the background color.
    let flat = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let first = [flat[0], flat[1], flat[2]];
    assert!(flat.chunks_exact(4).all(|c| [c[0], c[1], c[2]] == first));
    for (k, v) in p.background.iter().enumerate() {
        assert!((first[k] as f32 - srgb8(*v)).abs() <= 1.5, "flat {k}: {} vs {}", first[k], srgb8(*v));
    }
    // Gradient: the bottom color at the bottom, the top color at the top, in between in the middle.
    let top = [0.6, 0.3, 0.02];
    p.background_top = Some(top);
    let grad = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let (up, mid, down) = (row(&grad, 0), row(&grad, SIZE / 2), row(&grad, SIZE - 1));
    for k in 0..3 {
        assert!((up[k] as f32 - srgb8(top[k])).abs() <= 4.0, "top {k}: {} vs {}", up[k], srgb8(top[k]));
        assert!((down[k] as f32 - srgb8(p.background[k])).abs() <= 4.0, "bottom {k}: {} vs {}", down[k], srgb8(p.background[k]));
        let (lo, hi) = (up[k].min(down[k]), up[k].max(down[k]));
        assert!(mid[k] > lo && mid[k] < hi, "middle {k} between: {lo} < {} < {hi}", mid[k]);
    }
    // Every column changes steadily from bottom to top (red rises, blue falls).
    let reds: Vec<u8> = (0..SIZE).map(|y| row(&grad, y)[0]).collect();
    assert!(reds.windows(2).all(|w| w[0] >= w[1]), "red rises towards the top: {reds:?}");
    // Back to flat: exactly the flat image again.
    p.background_top = None;
    assert_eq!(r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue), flat);
}
