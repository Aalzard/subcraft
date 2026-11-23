//! Displacement preview: the subdivided mesh moved by Height + Displacement in the 3D view.
//! Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::wgpu_api::{Device, Queue};
use subcraft_render::{ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 96;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A unit-radius sphere seen from the front against a plain background (environment hidden).
fn setup(device: &Device, queue: &Queue) -> (ViewportRenderer, ViewParams) {
    let mut r = ViewportRenderer::new(device, queue);
    r.set_mesh(device, &primitives::uv_sphere(1.0, 24, 12));
    r.upload_channel(device, queue, 0, ChannelKind::BaseColor, &TiledImage::new(64, 64, 4, [0.8, 0.8, 0.8, 1.0]), None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y), Mat4::orthographic_rh(-2.0, 2.0, -2.0, 2.0, 0.1, 20.0));
    p.antialias = false;
    p.environment.background_opacity = 0.0;
    (r, p)
}

/// Pixels not equal to the plain background colour (the corner pixel).
fn covered(img: &[u8]) -> usize {
    let bg = &img[..4];
    img.chunks_exact(4).filter(|c| c[..3] != bg[..3]).count()
}

fn shot(r: &mut ViewportRenderer, device: &Device, queue: &Queue, p: &ViewParams) -> Vec<u8> {
    r.render(device, queue, [SIZE, SIZE], p).read_rgba8(device, queue)
}

#[test]
fn height_grows_the_silhouette_only_with_the_preview_on() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let (mut r, mut p) = setup(&device, &queue);
    p.height_scale = 0.25;
    let flat = covered(&shot(&mut r, &device, &queue, &p));
    r.upload_channel(&device, &queue, 0, ChannelKind::Height, &TiledImage::new(64, 64, 1, [1.0; 4]), None);
    // Off: height only bumps the shading, the outline stays.
    let bumped = covered(&shot(&mut r, &device, &queue, &p));
    assert_eq!(bumped, flat);
    // On: every vertex moves out by 0.25, a sphere of radius 1.25: area x 1.5625.
    p.displacement = Some(2);
    let grown = covered(&shot(&mut r, &device, &queue, &p));
    let ratio = grown as f32 / flat as f32;
    assert!((ratio - 1.5625).abs() < 0.08, "covered {flat} -> {grown} (x{ratio})");
    // Negative height shrinks it.
    r.upload_channel(&device, &queue, 0, ChannelKind::Height, &TiledImage::new(64, 64, 1, [-1.0; 4]), None);
    let shrunk = covered(&shot(&mut r, &device, &queue, &p)) as f32 / flat as f32;
    assert!((shrunk - 0.5625).abs() < 0.06, "x{shrunk}");
    // The Displacement channel adds to Height: -1 + 1 = flat again.
    r.upload_channel(&device, &queue, 0, ChannelKind::Displacement, &TiledImage::new(64, 64, 1, [1.0; 4]), None);
    let back = covered(&shot(&mut r, &device, &queue, &p)) as f32 / flat as f32;
    assert!((back - 1.0).abs() < 0.03, "x{back}");
    // Turning the preview off restores the modelled outline exactly.
    p.displacement = None;
    assert_eq!(covered(&shot(&mut r, &device, &queue, &p)), flat);
}

#[test]
fn painted_height_changes_the_outline_locally() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let (mut r, mut p) = setup(&device, &queue);
    p.height_scale = 0.3;
    p.displacement = Some(3);
    // Height 1 on one half of the UV space only (v < 0.5): one hemisphere bulges.
    let mut h = TiledImage::new(128, 128, 1, [0.0; 4]);
    for y in 0..64 {
        for x in 0..128 {
            h.set(x, y, [1.0, 0.0, 0.0, 0.0]);
        }
    }
    let flat = shot(&mut r, &device, &queue, &p);
    r.upload_channel(&device, &queue, 0, ChannelKind::Height, &h, None);
    let bulge = shot(&mut r, &device, &queue, &p);
    assert!(covered(&bulge) > covered(&flat) + 300, "{} vs {}", covered(&bulge), covered(&flat));
    // The column through the middle: the covered span grows at one pole only.
    let row = |img: &[u8]| {
        let x = SIZE / 2;
        let bg = img[..3].to_vec();
        let ys: Vec<u32> = (0..SIZE).filter(|&y| img[((y * SIZE + x) * 4) as usize..][..3] != bg[..]).collect();
        (*ys.first().unwrap(), *ys.last().unwrap())
    };
    let (f0, f1) = row(&flat);
    let (b0, b1) = row(&bulge);
    let (left, right) = (f0 as i32 - b0 as i32, b1 as i32 - f1 as i32);
    assert!((left >= 4 && right <= 1) || (right >= 4 && left <= 1), "one pole grows: top {left} px, bottom {right} px");
}
