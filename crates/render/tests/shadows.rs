//! Raster shadows of the key light and the Specular level channel in the viewport shader.
//! Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec2, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Corner, Mesh, MeshBuilder, primitives};
use subcraft_render::{ShadowParams, ShadowQuality, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 96;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn solid(comps: usize, v: [f32; 4]) -> TiledImage {
    TiledImage::new(64, 64, comps, v)
}

/// A horizontal square at height `y`, `half` wide each way, facing up.
fn square(b: &mut MeshBuilder, m: u32, y: f32, half: f32) {
    let c = |x: f32, z: f32| Corner { position: Vec3::new(x, y, z), normal: Some(Vec3::Y), uv: Some(Vec2::new((x / half + 1.0) * 0.5, (z / half + 1.0) * 0.5)) };
    b.push_triangle(m, [c(-half, -half), c(-half, half), c(half, half)]);
    b.push_triangle(m, [c(-half, -half), c(half, half), c(half, -half)]);
}

/// A 6 x 6 floor with a 1 x 1 square floating 1.5 above its centre.
fn floor_and_occluder() -> Mesh {
    let mut b = MeshBuilder::new("Shadow test");
    let m = b.material("Gray");
    square(&mut b, m, 0.0, 3.0);
    square(&mut b, m, 1.5, 0.5);
    b.build().unwrap()
}

fn lum(img: &[u8], x: u32, y: u32) -> f32 {
    // 3 x 3 average around the pixel.
    let mut sum = 0.0;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            let i = (((y as i32 + dy) as u32 * SIZE + (x as i32 + dx) as u32) * 4) as usize;
            sum += 0.2126 * img[i] as f32 + 0.7152 * img[i + 1] as f32 + 0.0722 * img[i + 2] as f32;
        }
    }
    sum / 9.0
}

fn pixel(p: &ViewParams, w: Vec3) -> (u32, u32) {
    let c = (p.proj * p.view).project_point3(w);
    (((c.x * 0.5 + 0.5) * SIZE as f32) as u32, ((0.5 - c.y * 0.5) * SIZE as f32) as u32)
}

#[test]
fn the_key_light_casts_a_shadow_only_when_asked_and_opacity_sets_its_depth() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &floor_and_occluder());
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.18, 0.18, 0.18, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [1.0; 4]), None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 4.0, 6.0), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0));
    p.antialias = false;
    p.light_dir = Vec3::Y;
    p.light_intensity = 3.0;
    // Pixels under the square and on open floor (shadows do not move the camera).
    let (sx, sy) = pixel(&p, Vec3::new(0.0, 0.0, 0.0));
    let (lx, ly) = pixel(&p, Vec3::new(2.0, 0.0, 0.0));
    let mut shot = |shadows: ShadowParams| {
        p.shadows = shadows;
        r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue)
    };
    let off = shot(ShadowParams::default());
    let none = shot(ShadowParams { enabled: true, opacity: 0.0, quality: ShadowQuality::Average });
    assert_eq!(off, none, "opacity 0 draws exactly what shadows off draw");
    let full = shot(ShadowParams { enabled: true, opacity: 1.0, quality: ShadowQuality::Average });
    let half = shot(ShadowParams { enabled: true, opacity: 0.5, quality: ShadowQuality::Average });
    // Under the square the floor loses the key light; away from it nothing changes.
    let (lit_off, lit_on) = (lum(&off, lx, ly), lum(&full, lx, ly));
    assert!((lit_off - lit_on).abs() <= 1.0, "open floor is unchanged: {lit_off} vs {lit_on}");
    let (under_off, under_half, under_full) = (lum(&off, sx, sy), lum(&half, sx, sy), lum(&full, sx, sy));
    assert!(under_full < under_off - 10.0, "the square shadows the floor: {under_full} vs {under_off} without shadows");
    assert!(under_half < under_off && under_half > under_full, "half opacity is in between: {under_off} > {under_half} > {under_full}");
    // The environment light still reaches the shadow: it never turns black.
    assert!(under_full > 1.0, "ambient light remains in the shadow: {under_full}");
    // Every computation mode shadows the same spot.
    for quality in [ShadowQuality::Lightweight, ShadowQuality::Intensive] {
        let img = shot(ShadowParams { enabled: true, opacity: 1.0, quality });
        assert!(lum(&img, sx, sy) < under_off - 10.0, "{quality:?} shadows the floor");
    }
}

#[test]
fn specular_level_sets_the_reflectance_of_non_metals() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2.0, 1));
    // A black, glossy dielectric: everything it shows is reflection.
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.0, 0.0, 0.0, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [0.3; 4]), None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 3.0, 2.0), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    p.antialias = false;
    let mean = |img: &[u8]| img.chunks_exact(4).map(|c| c[0] as f32 + c[1] as f32 + c[2] as f32).sum::<f32>() / img.len() as f32;
    let default = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let mut shot = |level: f32| {
        r.upload_channel(&device, &queue, 0, ChannelKind::Specular, &solid(1, [level; 4]), None);
        r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue)
    };
    let (zero, half, one) = (shot(0.0), shot(0.5), shot(1.0));
    assert_eq!(half, default, "0.5 is the 4% reflectance the shader always used");
    assert!(mean(&one) > mean(&half) + 1.0 && mean(&half) > mean(&zero) + 1.0, "specular 0 {} < 0.5 {} < 1 {}", mean(&zero), mean(&half), mean(&one));
}
