//! Temporal anti-aliasing: jittered frames averaged into smooth edges, restarted by frame 1.
//! Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::{ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 64;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn renderer(device: &subcraft_render::wgpu_api::Device, queue: &subcraft_render::wgpu_api::Queue) -> ViewportRenderer {
    let mut r = ViewportRenderer::new(device, queue);
    r.set_mesh(device, &primitives::plane(2.0, 1));
    r.upload_channel(device, queue, 0, ChannelKind::BaseColor, &TiledImage::new(64, 64, 4, [0.8, 0.8, 0.8, 1.0]), None);
    r.upload_channel(device, queue, 0, ChannelKind::Roughness, &TiledImage::new(64, 64, 1, [1.0; 4]), None);
    r
}

fn params() -> ViewParams {
    // The plane turned so its edges cross the pixels diagonally.
    let view = Mat4::look_at_rh(Vec3::new(1.3, 2.6, 1.9), Vec3::ZERO, Vec3::new(0.3, 1.0, 0.0).normalize());
    let mut p = ViewParams::new(view, Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0));
    p.antialias = false;
    p.background = [0.0; 3];
    p.environment.background_opacity = 0.0;
    p
}

/// Pixels whose brightness is clearly between the background and the plane: edge coverage.
fn partial_pixels(img: &[u8]) -> usize {
    let lum: Vec<u8> = img.chunks_exact(4).map(|c| c[1]).collect();
    let hi = *lum.iter().max().unwrap();
    lum.iter().filter(|&&l| l > 12 && (l as u32) < hi as u32 * 85 / 100).count()
}

#[test]
fn accumulated_frames_smooth_the_edges_and_frame_one_restarts() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = renderer(&device, &queue);
    let mut p = params();
    let plain = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let mut last = Vec::new();
    for frame in 1..=16 {
        p.taa_frame = frame;
        last = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    }
    let (hard, smooth) = (partial_pixels(&plain), partial_pixels(&last));
    assert!(smooth > hard + 10, "averaged edges have partial coverage: {smooth} vs {hard} without TAA");
    // Away from the edges nothing moves: the centre of the plane keeps its value.
    let c = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    for k in 0..3 {
        assert!((plain[c + k] as i32 - last[c + k] as i32).abs() <= 2, "centre {k}: {} vs {}", plain[c + k], last[c + k]);
    }
    // Frame 1 forgets the average: the same image as frame 1 on a fresh renderer.
    p.taa_frame = 1;
    let restarted = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let mut fresh = renderer(&device, &queue);
    let first = fresh.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    assert_eq!(restarted, first);
    // Off again: exactly the plain image.
    p.taa_frame = 0;
    assert_eq!(r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue), plain);
}

/// QA 2026-11: a new render size part-way through an average (resolution scale, a window resize
/// keeping the aspect) folded the new, black history in: the image stayed darker.
#[test]
fn a_new_size_while_accumulating_does_not_darken_the_image() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = renderer(&device, &queue);
    let mut p = params();
    for frame in 1..=4 {
        p.taa_frame = frame;
        let _ = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    }
    let big = SIZE * 2;
    p.taa_frame = 5;
    let after = r.render(&device, &queue, [big, big], &p).read_rgba8(&device, &queue);
    p.taa_frame = 1;
    let fresh = renderer(&device, &queue).render(&device, &queue, [big, big], &p).read_rgba8(&device, &queue);
    let c = ((big / 2 * big + big / 2) * 4) as usize;
    for k in 0..3 {
        assert!((after[c + k] as i32 - fresh[c + k] as i32).abs() <= 2, "centre {k}: {} after the resize vs {} fresh", after[c + k], fresh[c + k]);
    }
}
