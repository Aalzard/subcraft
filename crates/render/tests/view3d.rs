//! 3D view extras: polygon wireframe overlay and the baked-normal view modes.
//! Skips (passes with a message) when no adapter exists.

use glam::{Mat4, Vec3};
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::{ViewMode, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 192;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn solid(comps: usize, v: [f32; 4]) -> TiledImage {
    TiledImage::new(64, 64, comps, v)
}

fn px(img: &[u8], x: u32, y: u32) -> [u8; 4] {
    let o = ((y * SIZE + x) * 4) as usize;
    [img[o], img[o + 1], img[o + 2], img[o + 3]]
}

fn diff(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 6)).count()
}

/// Straight-down orthographic view of a plane: the surface covers the whole image.
fn top_down() -> ViewParams {
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0., 3., 0.), Vec3::ZERO, Vec3::NEG_Z), Mat4::orthographic_rh(-1., 1., -1., 1., 0.1, 10.));
    p.antialias = false;
    p
}

#[test]
fn wireframe_draws_in_its_color_and_opacity_and_off_restores_the_image() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2., 1));
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.2, 0.2, 0.2, 1.]), None);
    let mut p = top_down();
    let render = |r: &mut ViewportRenderer, p: &ViewParams| r.render(&device, &queue, [SIZE, SIZE], p).read_rgba8(&device, &queue);
    let baseline = render(&mut r, &p);
    // One line across the middle of the plane (world X axis at z = 0, 1 pixel high at least).
    let line = [[Vec3::new(-0.9, 0.0, 0.0), Vec3::new(0.9, 0.0, 0.0)]];
    r.set_wireframe(&device, &line, [0.0, 1.0, 0.0], 1.0);
    assert_eq!(render(&mut r, &p), baseline, "uploaded but not enabled draws nothing");
    p.wireframe = true;
    let opaque = render(&mut r, &p);
    let green = opaque.chunks_exact(4).filter(|c| c[1] > 200 && c[0] < 40 && c[2] < 40).count();
    assert!(green >= 100, "an opaque green line spans most of the image: {green}");
    // Half opacity lands between the surface and the line color.
    r.set_wireframe(&device, &line, [0.0, 1.0, 0.0], 0.5);
    let half = render(&mut r, &p);
    let y = (0..SIZE).find(|&y| px(&opaque, SIZE / 2, y)[1] > 200).expect("line row");
    let (a, b, c) = (px(&baseline, SIZE / 2, y)[1], px(&half, SIZE / 2, y)[1], px(&opaque, SIZE / 2, y)[1]);
    assert!(a < b && b < c, "half opacity is in between: {a} < {b} < {c}");
    // Color change.
    r.set_wireframe(&device, &line, [1.0, 0.0, 0.0], 1.0);
    let red = render(&mut r, &p);
    assert!(px(&red, SIZE / 2, y)[0] > 200 && px(&red, SIZE / 2, y)[1] < 60);
    // Off and removed.
    p.wireframe = false;
    assert_eq!(render(&mut r, &p), baseline);
    p.wireframe = true;
    r.set_wireframe(&device, &[], [1.0; 3], 1.0);
    assert_eq!(render(&mut r, &p), baseline, "an empty line list removes the overlay");
}

fn tilted_normal() -> TiledImage {
    // Tangent-space normal leaning towards +U: (0.6, 0, 0.8) encoded.
    solid(4, [0.8, 0.5, 0.9, 1.0])
}

#[test]
fn baked_normal_shades_with_the_baked_map_and_ignores_the_painted_normal() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2., 1));
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.5, 0.5, 0.5, 1.]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [0.4; 4]), None);
    let mut p = top_down();
    let render = |r: &mut ViewportRenderer, p: &ViewParams| r.render(&device, &queue, [SIZE, SIZE], p).read_rgba8(&device, &queue);

    p.view_mode = ViewMode::Material;
    let flat = render(&mut r, &p);
    p.view_mode = ViewMode::BakedNormal;
    assert_eq!(render(&mut r, &p), flat, "no bake yet: flat normal, identical to the unpainted material");

    // A painted normal changes the material view but not the baked-normal view.
    r.upload_channel(&device, &queue, 0, ChannelKind::Normal, &tilted_normal(), None);
    p.view_mode = ViewMode::Material;
    let painted = render(&mut r, &p);
    assert!(diff(&flat, &painted) > 1000, "the painted normal is visible in the material view");
    p.view_mode = ViewMode::BakedNormal;
    assert_eq!(render(&mut r, &p), flat, "the painted normal is not used in the baked-normal view");

    // A baked normal map changes it, and the material view still shows only the painted one.
    r.upload_mesh_map(&device, &queue, 0, MeshMapKind::Normal, &solid(4, [0.2, 0.5, 0.9, 1.0]));
    let baked = render(&mut r, &p);
    assert!(diff(&flat, &baked) > 1000, "baked normal is visible in shading");
    assert!(diff(&painted, &baked) > 1000, "tilted the other way than the painted one");
    p.view_mode = ViewMode::Material;
    assert_eq!(render(&mut r, &p), painted);
    // Re-bake: the shaded view follows the new map.
    r.upload_mesh_map(&device, &queue, 0, MeshMapKind::Normal, &solid(4, [0.8, 0.5, 0.9, 1.0]));
    p.view_mode = ViewMode::BakedNormal;
    let rebaked = render(&mut r, &p);
    assert!(diff(&baked, &rebaked) > 1000);
    assert_eq!(rebaked, {
        // Same normal as the painted channel, so the same picture as the material view.
        p.view_mode = ViewMode::Material;
        render(&mut r, &p)
    });
}

#[test]
fn baked_normal_map_view_shows_the_map_unlit() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2., 1));
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.5, 0.5, 0.5, 1.]), None);
    let mut p = top_down();
    p.view_mode = ViewMode::BakedNormalMap;
    let neutral = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let c = px(&neutral, SIZE / 2, SIZE / 2);
    assert!(c[0].abs_diff(128) <= 1 && c[1].abs_diff(128) <= 1 && c[2] == 255, "no bake: the flat normal color {c:?}");
    r.upload_mesh_map(&device, &queue, 0, MeshMapKind::Normal, &solid(4, [0.5, 0.25, 1.0, 1.0]));
    let img = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let c = px(&img, SIZE / 2, SIZE / 2);
    assert!(c[0].abs_diff(128) <= 1 && c[1].abs_diff(64) <= 1 && c[2] == 255, "baked map color {c:?}");
}
