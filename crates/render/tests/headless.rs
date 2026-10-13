//! Offscreen rendering on whatever adapter exists (lavapipe in CI/cloud, the real GPU locally).
//! Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::primitives;
use subcraft_render::{ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 192;

fn params() -> ViewParams {
    let view = Mat4::look_at_rh(Vec3::new(3.0, 2.5, 4.0), Vec3::ZERO, Vec3::Y);
    let proj = Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0);
    ViewParams::new(view, proj)
}

fn solid(comps: usize, v: [f32; 4]) -> TiledImage {
    TiledImage::new(256, 256, comps, v)
}

fn px(img: &[u8], x: u32, y: u32) -> [u8; 4] {
    let o = ((y * SIZE + x) * 4) as usize;
    [img[o], img[o + 1], img[o + 2], img[o + 3]]
}

fn save(name: &str, img: &[u8]) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let data: Vec<f32> = img.iter().map(|&b| b as f32 / 255.0).collect();
    save_image(&dir.join(name), SIZE, SIZE, 4, &data, FileFormat::Png8).unwrap();
}

#[test]
fn renders_textured_cube_and_partial_uploads_match_full() {
    let Some((adapter, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    eprintln!("adapter: {:?}", adapter.get_info().name);
    let mesh = primitives::cube(2.0);

    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &mesh);
    let mut base = solid(4, [0.8, 0.05, 0.05, 1.0]);
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [0.5; 4]), None);
    let img = r.render(&device, &queue, [SIZE, SIZE], &params()).read_rgba8(&device, &queue);
    save("cube_red.png", &img);

    let c = px(&img, SIZE / 2, SIZE / 2);
    assert!(c[0] > 80 && c[0] > 2 * c[1] && c[0] > 2 * c[2], "center should be lit red: {c:?}");
    let corner = px(&img, 2, 2);
    let bg = (subcraft_core::color::linear_to_srgb(params().background[0]) * 255.0).round() as i32;
    assert!((corner[0] as i32 - bg).abs() <= 1, "corner is background ({bg}): {corner:?}");
    // Three visible faces are shaded differently.
    let top = px(&img, SIZE / 2, SIZE / 4);
    assert_ne!(top, c, "faces receive different lighting");

    // Paint two tiles green; upload only those; compare with a fresh full upload.
    for y in 0..64 {
        for x in 0..128 {
            base.set(x, y, [0.05, 0.7, 0.1, 1.0]);
        }
    }
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, Some(&[0, 1]));
    let partial = r.render(&device, &queue, [SIZE, SIZE], &params()).read_rgba8(&device, &queue);
    let mut fresh = ViewportRenderer::new(&device, &queue);
    fresh.set_mesh(&device, &mesh);
    fresh.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
    fresh.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [0.5; 4]), None);
    let full = fresh.render(&device, &queue, [SIZE, SIZE], &params()).read_rgba8(&device, &queue);
    save("cube_partial.png", &partial);
    assert_eq!(partial, full, "tile uploads must equal a full upload");
    assert_ne!(partial, img, "the green tiles are visible");
}

#[test]
fn empty_scene_is_background_and_resizes() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    let mut r = ViewportRenderer::new(&device, &queue);
    let a = r.render(&device, &queue, [64, 32], &params()).read_rgba8(&device, &queue);
    assert_eq!(a.len(), 64 * 32 * 4);
    assert!(a.chunks_exact(4).all(|p| p == &a[..4]), "uniform background");
    let b = r.render(&device, &queue, [100, 70], &params()).read_rgba8(&device, &queue);
    assert_eq!(b.len(), 100 * 70 * 4);
}

fn render_with(device: &subcraft_render::wgpu_api::Device, queue: &subcraft_render::wgpu_api::Queue, channels: &[(ChannelKind, TiledImage)], p: &ViewParams) -> Vec<u8> {
    let mut r = ViewportRenderer::new(device, queue);
    r.set_mesh(device, &primitives::cube(2.0));
    r.upload_channel(device, queue, 0, ChannelKind::BaseColor, &solid(4, [0.6, 0.6, 0.6, 1.0]), None);
    for (k, img) in channels {
        r.upload_channel(device, queue, 0, *k, img, None);
    }
    r.render(device, queue, [SIZE, SIZE], p).read_rgba8(device, queue)
}

fn diff(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 6)).count()
}

#[test]
fn height_bumps_and_metallic_change_shading() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    let flat = render_with(&device, &queue, &[], &params());
    // A raised disc in the middle of every cube face cell.
    let mut height = solid(1, [0.0; 4]);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let (cx, cy) = ((x % 85) as f32 - 42.0, (y % 128) as f32 - 64.0);
            if (cx * cx + cy * cy).sqrt() < 25.0 {
                height.set(x, y, [0.5; 4]);
            }
        }
    }
    let bumped = render_with(&device, &queue, &[(ChannelKind::Height, height.clone())], &params());
    save("cube_height.png", &bumped);
    let changed = diff(&flat, &bumped);
    assert!(changed > 200, "height bumps should change shading ({changed} px)");
    let off = render_with(&device, &queue, &[(ChannelKind::Height, height)], &ViewParams { height_scale: 0.0, ..params() });
    assert_eq!(diff(&flat, &off), 0, "height scale 0 disables bumps");

    let metal = render_with(
        &device,
        &queue,
        &[(ChannelKind::Metallic, solid(1, [1.0; 4])), (ChannelKind::Roughness, solid(1, [0.15; 4]))],
        &params(),
    );
    save("cube_metal.png", &metal);
    let changed = diff(&flat, &metal);
    assert!(changed > 5000, "metallic + low roughness should look clearly different ({changed} px)");
}
