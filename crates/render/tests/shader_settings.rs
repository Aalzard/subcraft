//! Shader Settings in the viewport shader: AO intensity, emissive intensity and horizon fading,
//! per texture set. Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_doc::ShaderParams;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::wgpu_api::{Device, Queue};
use subcraft_render::{ViewMode, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 64;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn solid(comps: usize, v: [f32; 4]) -> TiledImage {
    TiledImage::new(64, 64, comps, v)
}

/// A plane seen straight from above, filling the view.
fn scene(device: &Device, queue: &Queue) -> (ViewportRenderer, ViewParams) {
    let mut r = ViewportRenderer::new(device, queue);
    r.set_mesh(device, &primitives::plane(2.0, 1));
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z), Mat4::orthographic_rh(-0.8, 0.8, -0.8, 0.8, 0.1, 10.0));
    p.antialias = false;
    p.view_mode = ViewMode::Material;
    (r, p)
}

/// Mean of the red, green and blue bytes over the whole image (the plane fills it).
fn mean(img: &[u8]) -> f32 {
    let px = img.chunks_exact(4);
    let n = px.len() as f32;
    px.map(|c| (c[0] as f32 + c[1] as f32 + c[2] as f32) / 3.0).sum::<f32>() / n
}

#[test]
fn ao_and_emissive_intensity_scale_their_channels() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let (mut r, p) = scene(&device, &queue);
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.5, 0.5, 0.5, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [1.0; 4]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::AmbientOcclusion, &solid(1, [0.0; 4]), None);
    let mut shot = |shader: ShaderParams| {
        r.set_shader(0, shader);
        mean(&r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue))
    };
    // A fully occluding AO map: intensity 0 ignores it, 1 applies all of it; Painter's 0.75 is in between.
    let ao = |v: f32| ShaderParams { ao_intensity: v, ..Default::default() };
    let (none, half, painter, full) = (shot(ao(0.0)), shot(ao(0.5)), shot(ShaderParams::default()), shot(ao(1.0)));
    assert!(none > half && half > painter && painter > full, "AO intensity 0 {none} > 0.5 {half} > 0.75 {painter} > 1 {full}");

    // Emission scales with the intensity; 0 removes it.
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.0, 0.0, 0.0, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Emissive, &solid(4, [0.15, 0.15, 0.15, 1.0]), None);
    let mut shot = |shader: ShaderParams| {
        r.set_shader(0, shader);
        mean(&r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue))
    };
    let emit = |v: f32| ShaderParams { emissive_intensity: v, ..Default::default() };
    let (off, one, three) = (shot(emit(0.0)), shot(emit(1.0)), shot(emit(3.0)));
    assert!(one > off + 10.0 && three > one + 10.0, "emissive intensity 0 {off} < 1 {one} < 3 {three}");
    // The renderer clamps what it is given: 1e9 draws like 100.
    let (huge, hundred) = (shot(emit(1e9)), shot(emit(100.0)));
    assert_eq!(huge, hundred);
}

#[test]
fn horizon_fading_darkens_reflections_tilted_below_the_surface() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let (mut r, mut p) = scene(&device, &queue);
    p.light_intensity = 0.0;
    // A mirror-like metal whose normal map tilts the normal 70 degrees: seen from above, the
    // reflection points 50 degrees below the plane.
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [1.0, 1.0, 1.0, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Metallic, &solid(1, [1.0; 4]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [0.1; 4]), None);
    let tilt = 70f32.to_radians();
    let n = [tilt.sin(), 0.0, tilt.cos()];
    r.upload_channel(&device, &queue, 0, ChannelKind::Normal, &solid(4, [n[0] * 0.5 + 0.5, n[1] * 0.5 + 0.5, n[2] * 0.5 + 0.5, 1.0]), None);
    let shot = |r: &mut ViewportRenderer, fading: f32| {
        r.set_shader(0, ShaderParams { horizon_fading: fading, ..Default::default() });
        mean(&r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue))
    };
    let (off, painter, strong) = (shot(&mut r, 0.0), shot(&mut r, 1.3), shot(&mut r, 2.0));
    assert!(off > painter + 5.0, "fading 0 keeps the reflection ({off}), Painter's 1.3 fades it ({painter})");
    assert!(painter >= strong, "more fading is never brighter: {painter} vs {strong}");

    // An untilted normal reflects above the surface: fading changes nothing.
    r.upload_channel(&device, &queue, 0, ChannelKind::Normal, &solid(4, [0.5, 0.5, 1.0, 1.0]), None);
    let (flat_off, flat_strong) = (shot(&mut r, 0.0), shot(&mut r, 2.0));
    assert_eq!(flat_off, flat_strong, "no fading without a tilted normal");
}
