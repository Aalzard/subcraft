//! Image-based lighting of the viewport shader: energy of rough reflections (multiple-scattering
//! compensation), specular occlusion from AO and the roughness-to-blur mapping of the HDRI.
//! Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::wgpu_api::{Device, Queue};
use subcraft_render::{HdriUpload, PostParams, ToneMap, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 64;
const ENV: f32 = 0.25;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn solid(comps: usize, v: f32) -> TiledImage {
    TiledImage::new(32, 32, comps, [v, v, v, 1.0])
}

/// A sphere filling the view, lit only by an HDRI: `env(u, v)` at every level (1024 .. 16 wide,
/// the shape the prefilter produces), no key light, linear tone mapping.
fn scene(
    device: &Device,
    queue: &Queue,
    env: impl Fn(f32, f32, usize) -> f32,
) -> (ViewportRenderer, ViewParams) {
    let mut r = ViewportRenderer::new(device, queue);
    r.set_mesh(device, &primitives::uv_sphere(1.0, 48, 24));
    let data: Vec<(u32, Vec<f32>)> = (0..7)
        .map(|k| {
            let w = 1024u32 >> k;
            let h = w / 2;
            let px = (0..w * h).flat_map(|i| {
                let v = env(
                    ((i % w) as f32 + 0.5) / w as f32,
                    ((i / w) as f32 + 0.5) / h as f32,
                    k,
                );
                [v, v, v, 1.0]
            });
            (w, px.collect())
        })
        .collect();
    let levels: Vec<(u32, u32, &[f32])> = data
        .iter()
        .map(|(w, d)| (*w, *w / 2, d.as_slice()))
        .collect();
    // Constant irradiance ENV (SH band 0 only; the shader evaluates sum(sh * basis)).
    let mut sh = [[0.0f32; 3]; 9];
    sh[0] = [ENV / 0.282_095; 3];
    r.set_hdri(
        device,
        queue,
        Some(HdriUpload {
            levels: &levels,
            sh,
        }),
    );
    let mut p = ViewParams::new(
        Mat4::look_at_rh(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y),
        Mat4::orthographic_rh(-1.1, 1.1, -1.1, 1.1, 0.1, 20.0),
    );
    p.antialias = false;
    p.light_intensity = 0.0;
    p.environment.use_hdri = true;
    p.environment.background_opacity = 0.0;
    p.post = PostParams {
        tone_map: ToneMap::Linear,
        emission_glow: 0.0,
        ..PostParams::default()
    };
    (r, p)
}

fn material(
    r: &mut ViewportRenderer,
    device: &Device,
    queue: &Queue,
    metal: f32,
    rough: f32,
    ao: f32,
) {
    r.upload_channel(
        device,
        queue,
        0,
        ChannelKind::BaseColor,
        &solid(4, 1.0),
        None,
    );
    r.upload_channel(
        device,
        queue,
        0,
        ChannelKind::Metallic,
        &solid(1, metal),
        None,
    );
    r.upload_channel(
        device,
        queue,
        0,
        ChannelKind::Roughness,
        &solid(1, rough),
        None,
    );
    r.upload_channel(
        device,
        queue,
        0,
        ChannelKind::AmbientOcclusion,
        &solid(1, ao),
        None,
    );
}

fn linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Mean linear red of the pixels within `radius` (fraction of the sphere's radius) of the centre.
fn mean(img: &[u8], radius: f32) -> f32 {
    let (mut sum, mut n) = (0.0, 0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (
                (x as f32 + 0.5) / SIZE as f32 * 2.2 - 1.1,
                (y as f32 + 0.5) / SIZE as f32 * 2.2 - 1.1,
            );
            if dx * dx + dy * dy < radius * radius {
                sum += linear(img[((y * SIZE + x) * 4) as usize]);
                n += 1;
            }
        }
    }
    sum / n as f32
}

#[test]
fn rough_white_metal_keeps_its_energy_in_a_uniform_environment() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no adapter: skipping");
        return;
    };
    let (mut r, p) = scene(&device, &queue, |_, _, _| ENV);
    let shot = |r: &mut ViewportRenderer, rough: f32| {
        material(r, &device, &queue, 1.0, rough, 1.0);
        mean(
            &r.render(&device, &queue, [SIZE, SIZE], &p)
                .read_rgba8(&device, &queue),
            0.6,
        )
    };
    // White furnace: a white metal reflects all the light around it whatever its roughness.
    // Single-scatter GGX alone loses about half of it at roughness 1.
    for rough in [0.05, 0.5, 1.0] {
        let m = shot(&mut r, rough);
        assert!(
            (m / ENV - 1.0).abs() < 0.12,
            "roughness {rough}: {m} reflected of {ENV}"
        );
    }
}

#[test]
fn ambient_occlusion_hides_reflections_less_than_diffuse_light_at_normal_incidence() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no adapter: skipping");
        return;
    };
    let (mut r, p) = scene(&device, &queue, |_, _, _| ENV);
    let shot = |r: &mut ViewportRenderer, metal: f32, ao: f32| {
        // Glossy metal: specular occlusion matters most for sharp reflections.
        material(
            r,
            &device,
            &queue,
            metal,
            if metal > 0.5 { 0.1 } else { 0.4 },
            ao,
        );
        mean(
            &r.render(&device, &queue, [SIZE, SIZE], &p)
                .read_rgba8(&device, &queue),
            0.5,
        )
    };
    let diffuse = shot(&mut r, 0.0, 0.5) / shot(&mut r, 0.0, 1.0);
    let metal = shot(&mut r, 1.0, 0.5) / shot(&mut r, 1.0, 1.0);
    // Diffuse light scales by AO; reflections facing the camera keep more (specular occlusion).
    // (A dielectric also reflects ~4%, which AO hides less: a little above 0.5.)
    assert!(diffuse < 0.66, "diffuse x{diffuse}");
    eprintln!("AO 0.5: diffuse x{diffuse}, reflections x{metal}");
    assert!(
        metal > diffuse + 0.05 && metal < 1.0,
        "reflections x{metal} vs diffuse x{diffuse}"
    );
}

#[test]
fn roughness_picks_the_blur_level_of_a_ggx_lobe() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no adapter: skipping");
        return;
    };
    // Each level holds its own index: the reflection tells which level the shader sampled.
    let (mut r, p) = scene(&device, &queue, |_, _, k| k as f32 * 0.1);
    let mut level = |rough: f32| {
        material(&mut r, &device, &queue, 1.0, rough, 1.0);
        // Centre pixel only: normal incidence, where the energy terms are smallest.
        let img = r
            .render(&device, &queue, [SIZE, SIZE], &p)
            .read_rgba8(&device, &queue);
        mean(&img, 0.1)
    };
    let smooth = level(0.0);
    let half = level(0.5);
    let full = level(1.0);
    // Roughness 0 reads the sharp level, 1 the last level (6), 0.5 two levels below the last
    // (lobe width alpha = 0.25 is a quarter of alpha = 1).
    assert!(smooth < 0.01, "roughness 0 samples level 0: {smooth}");
    assert!(full > 0.5, "roughness 1 samples the last level: {full}");
    let ratio = half / full;
    assert!(
        ratio > 0.55 && ratio < 0.8,
        "roughness 0.5 samples about level 4 of 6: {half} vs {full}"
    );
}
