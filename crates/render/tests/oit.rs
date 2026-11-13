//! Order-independent transparency: overlapping alpha-blended surfaces look the same whatever
//! order the mesh lists them in. Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec2, Vec3};
use subcraft_core::ChannelKind;
use subcraft_doc::{AlphaMode, AlphaSettings};
use subcraft_image::TiledImage;
use subcraft_mesh::{Corner, Mesh, MeshBuilder};
use subcraft_render::{PostParams, ViewParams, ViewportRenderer, headless};

const N: u32 = 64;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A 2 x 2 square facing +Z at depth `z`, its UVs inside `u0..u1` (one colour of the texture).
fn square(b: &mut MeshBuilder, m: u32, z: f32, u0: f32, u1: f32) {
    let c = |x: f32, y: f32| Corner {
        position: Vec3::new(x, y, z),
        normal: Some(Vec3::Z),
        uv: Some(Vec2::new(u0 + (x + 1.0) * 0.5 * (u1 - u0), 0.1 + (y + 1.0) * 0.4)),
    };
    b.push_triangle(m, [c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0)]);
    b.push_triangle(m, [c(-1.0, -1.0), c(1.0, 1.0), c(-1.0, 1.0)]);
}

/// A red square in front of a blue one, listed near-first or far-first.
fn two_squares(near_first: bool) -> Mesh {
    let mut b = MeshBuilder::new("OIT test");
    let m = b.material("Glass");
    let near = |b: &mut MeshBuilder| square(b, m, 0.5, 0.05, 0.45);
    let far = |b: &mut MeshBuilder| square(b, m, -0.5, 0.55, 0.95);
    if near_first { near(&mut b); far(&mut b); } else { far(&mut b); near(&mut b); }
    b.build().unwrap()
}

fn centre(img: &[u8]) -> [i32; 3] {
    let i = ((N / 2 * N + N / 2) * 4) as usize;
    [img[i] as i32, img[i + 1] as i32, img[i + 2] as i32]
}

#[test]
fn transparent_layers_do_not_depend_on_their_order_with_oit() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    // Left half of the texture red, right half blue; half transparent everywhere.
    let mut base = TiledImage::new(64, 64, 4, [0.0, 0.0, 0.9, 1.0]);
    for y in 0..64 {
        for x in 0..32 {
            base.set(x, y, [0.9, 0.0, 0.0, 1.0]);
        }
    }
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::Y);
    let mut params = ViewParams::new(view, Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    params.antialias = false;
    params.background = [0.02, 0.02, 0.02];
    params.environment.background_opacity = 0.0;
    params.post = PostParams { emission_glow: 0.0, ..PostParams::default() };
    let shot = |near_first: bool, oit: bool| {
        let mut r = ViewportRenderer::new(&device, &queue);
        r.set_mesh(&device, &two_squares(near_first));
        r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
        r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &TiledImage::new(64, 64, 1, [1.0; 4]), None);
        r.upload_channel(&device, &queue, 0, ChannelKind::Opacity, &TiledImage::new(64, 64, 1, [0.5; 4]), None);
        r.set_alpha(0, AlphaSettings { mode: AlphaMode::Blend, threshold: 0.33, dithering: false });
        r.render(&device, &queue, [N, N], &ViewParams { oit, ..params }).read_rgba8(&device, &queue)
    };
    // Plain blending follows the mesh order: the last square drawn dominates.
    let (plain_a, plain_b) = (centre(&shot(true, false)), centre(&shot(false, false)));
    assert!(plain_a[2] > plain_a[0], "near first: the far blue is blended over the red: {plain_a:?}");
    assert!(plain_b[0] > plain_b[2], "far first: the near red is over the blue: {plain_b:?}");
    // OIT: the same picture either way, with both colours present (their weights depend on the
    // depth range; here they are close, so the average is near even).
    let (oit_a, oit_b) = (centre(&shot(true, true)), centre(&shot(false, true)));
    for k in 0..3 {
        assert!((oit_a[k] - oit_b[k]).abs() <= 1, "order-independent: {oit_a:?} vs {oit_b:?}");
    }
    assert!(oit_a[0] > 10 && oit_a[2] > 10, "both layers show: {oit_a:?}");
    // Opaque sets are not affected by the switch.
    let opaque = |oit: bool| {
        let mut r = ViewportRenderer::new(&device, &queue);
        r.set_mesh(&device, &two_squares(true));
        r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
        r.render(&device, &queue, [N, N], &ViewParams { oit, ..params }).read_rgba8(&device, &queue)
    };
    assert_eq!(opaque(true), opaque(false));
    // QA 2026-11: OIT is for the material view only; a channel view is unchanged by the switch.
    let channel = |oit: bool| {
        let mut r = ViewportRenderer::new(&device, &queue);
        r.set_mesh(&device, &two_squares(true));
        r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
        r.upload_channel(&device, &queue, 0, ChannelKind::Opacity, &TiledImage::new(64, 64, 1, [0.5; 4]), None);
        r.set_alpha(0, AlphaSettings { mode: AlphaMode::Blend, threshold: 0.33, dithering: false });
        let p = ViewParams { oit, view_mode: subcraft_render::ViewMode::BaseColor, ..params };
        r.render(&device, &queue, [N, N], &p).read_rgba8(&device, &queue)
    };
    assert_eq!(channel(true), channel(false));
}
