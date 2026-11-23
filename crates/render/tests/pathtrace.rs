//! Path-traced Rendering mode on whatever adapter exists. Skips (passes with a message) when no
//! adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::{Bvh, primitives};
use subcraft_render::wgpu_api::{Device, Queue};
use subcraft_render::{Denoiser, EnvironmentParams, GroundSettings, PathTraceScene, PathTraceSettings, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 64;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn solid(comps: usize, v: [f32; 4]) -> TiledImage {
    TiledImage::new(64, 64, comps, v)
}

fn params(eye: Vec3) -> ViewParams {
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let proj = Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0);
    ViewParams::new(view, proj)
}

/// Uniform white environment, no key light, black background.
fn furnace(mut p: ViewParams) -> ViewParams {
    p.environment = EnvironmentParams { ground: [1.0; 3], horizon: [1.0; 3], zenith: [1.0; 3], panels: 0.0, top: 0.0, key_softbox: 0.0, ..Default::default() };
    p.light_intensity = 0.0;
    p.background = [0.0; 3];
    p
}

fn settings() -> PathTraceSettings {
    PathTraceSettings { ground: false, denoise: false, max_samples: 0, frame_budget_ms: 2000.0, ..Default::default() }
}

/// Render until at least `samples` samples have accumulated; returns the count reached.
fn accumulate(r: &mut ViewportRenderer, device: &Device, queue: &Queue, p: &ViewParams, scene: &PathTraceScene, s: &PathTraceSettings, samples: u32) -> u32 {
    let mut reached = 0;
    for _ in 0..1000 {
        let (_, status) = r.render_path_traced(device, queue, [SIZE, SIZE], p, scene, s).expect("path tracing works");
        reached = status.samples;
        if reached >= samples {
            break;
        }
    }
    reached
}

fn region_mean(img: &[[f32; 4]], x0: u32, y0: u32, n: u32) -> [f32; 3] {
    let mut sum = [0.0f64; 3];
    for y in y0..y0 + n {
        for x in x0..x0 + n {
            let c = img[(y * SIZE + x) as usize];
            for k in 0..3 {
                sum[k] += c[k] as f64;
            }
        }
    }
    sum.map(|s| (s / (n * n) as f64) as f32)
}

fn save(name: &str, img: &[u8]) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let data: Vec<f32> = img.iter().map(|&b| b as f32 / 255.0).collect();
    save_image(&dir.join(name), SIZE, SIZE, 4, &data, FileFormat::Png8).unwrap();
}

fn material(r: &mut ViewportRenderer, device: &Device, queue: &Queue, base: [f32; 3], rough: f32, metal: f32) {
    r.upload_channel(device, queue, 0, ChannelKind::BaseColor, &solid(4, [base[0], base[1], base[2], 1.0]), None);
    r.upload_channel(device, queue, 0, ChannelKind::Roughness, &solid(1, [rough; 4]), None);
    r.upload_channel(device, queue, 0, ChannelKind::Metallic, &solid(1, [metal; 4]), None);
}

#[test]
fn white_diffuse_sphere_in_a_uniform_environment_converges_to_the_environment_radiance() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mesh = primitives::uv_sphere(1.0, 64, 32);
    let bvh = Bvh::build(&mesh);
    let scene = PathTraceScene { mesh: &mesh, bvh: &bvh, key: 1 };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &mesh);
    material(&mut r, &device, &queue, [1.0; 3], 1.0, 0.0);
    let p = furnace(params(Vec3::new(0.0, 0.0, 4.0)));
    let reached = accumulate(&mut r, &device, &queue, &p, &scene, &settings(), 256);
    assert!(reached >= 256, "samples {reached}");
    let img = r.path_traced_linear(&device, &queue).unwrap();
    // Furnace: Lambert with albedo 1 under radiance 1 returns 1 (minus the small single-scatter
    // GGX energy loss of the dielectric coat at roughness 1).
    let c = region_mean(&img, SIZE / 2 - 8, SIZE / 2 - 8, 16);
    eprintln!("furnace: {c:?} after {reached} samples");
    for k in 0..3 {
        assert!((0.93..=1.02).contains(&c[k]), "furnace radiance {c:?}");
    }
    // Background: the camera sees the background color, not the environment.
    assert!(region_mean(&img, 0, 0, 4).iter().all(|&v| v < 1e-4));
    // A gray sphere returns its albedo.
    material(&mut r, &device, &queue, [0.5; 3], 1.0, 0.0);
    accumulate(&mut r, &device, &queue, &p, &scene, &settings(), 256);
    let img = r.path_traced_linear(&device, &queue).unwrap();
    let c = region_mean(&img, SIZE / 2 - 8, SIZE / 2 - 8, 16);
    assert!((0.45..=0.52).contains(&c[1]), "gray furnace {c:?}");
}

#[test]
fn traced_silhouette_matches_the_raster_viewport_and_the_ground_shows_a_shadow() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mesh = primitives::cube(2.0);
    let bvh = Bvh::build(&mesh);
    let scene = PathTraceScene { mesh: &mesh, bvh: &bvh, key: 7 };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &mesh);
    material(&mut r, &device, &queue, [0.8, 0.1, 0.1], 0.5, 0.0);
    let mut p = params(Vec3::new(3.0, 2.5, 4.0));
    p.antialias = false;
    p.background = [0.0; 3];
    let raster = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    save("pt-raster.png", &raster);
    accumulate(&mut r, &device, &queue, &p, &scene, &settings(), 64);
    let traced = r.path_traced_linear(&device, &queue).unwrap();
    let (target, _) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &settings()).unwrap();
    save("pt-traced.png", &target.read_rgba8(&device, &queue));
    let mut disagree = 0;
    for i in 0..(SIZE * SIZE) as usize {
        let raster_hit = raster[i * 4..i * 4 + 3].iter().any(|&v| v > 2);
        let traced_hit = traced[i][..3].iter().any(|&v| v > 1e-3);
        disagree += (raster_hit != traced_hit) as u32;
    }
    // Edge pixels may differ (the trace is anti-aliased by jitter, the raster is not).
    assert!(disagree < SIZE * 3, "silhouette pixels disagreeing: {disagree}");
    // Red material stays red when traced.
    let c = region_mean(&traced, SIZE / 2 - 3, SIZE / 2 - 3, 6);
    assert!(c[0] > c[1] * 2.5 && c[0] > c[2] * 2.5, "red cube: {c:?}");

    // Ground disc with the key light overhead: the floor right beside the cube (in its shadow)
    // is darker than floor far from it.
    let mut p = params(Vec3::new(0.0, 6.0, 9.0));
    p.light_dir = Vec3::new(0.3, 1.0, 0.0);
    p.background = [0.0; 3];
    let ground = PathTraceSettings { ground: true, ..settings() };
    accumulate(&mut r, &device, &queue, &p, &scene, &ground, 64);
    let img = r.path_traced_linear(&device, &queue).unwrap();
    let (target, _) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &ground).unwrap();
    save("pt-ground.png", &target.read_rgba8(&device, &queue));
    let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let vp = p.proj * p.view;
    let pixel = |w: Vec3| {
        let c = vp.project_point3(w);
        (((c.x * 0.5 + 0.5) * SIZE as f32) as u32, ((0.5 - c.y * 0.5) * SIZE as f32) as u32)
    };
    // Light from +x: the shadow falls on the -x side of the cube.
    let (sx, sy) = pixel(Vec3::new(-1.2, -1.0, 0.0));
    let (fx, fy) = pixel(Vec3::new(1.6, -1.0, 0.0));
    let shadow = lum(region_mean(&img, sx - 1, sy - 1, 3));
    let lit = lum(region_mean(&img, fx - 1, fy - 1, 3));
    assert!(lit > 0.02 && shadow < lit * 0.8, "contact shadow {shadow} vs lit floor {lit}");
}

#[test]
fn accumulation_restarts_on_camera_and_texture_changes_and_pauses_and_stops() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mesh = primitives::cube(2.0);
    let bvh = Bvh::build(&mesh);
    let scene = PathTraceScene { mesh: &mesh, bvh: &bvh, key: 3 };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &mesh);
    material(&mut r, &device, &queue, [0.5; 3], 0.5, 0.0);
    let mut p = params(Vec3::new(3.0, 2.5, 4.0));
    let mut s = settings();
    s.max_samples = 24;
    let reached = accumulate(&mut r, &device, &queue, &p, &scene, &s, 24);
    assert_eq!(reached, 24, "stops at the limit");
    let (_, status) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &s).unwrap();
    assert_eq!((status.samples, status.passes), (24, 0), "no work once converged");
    assert!(status.converged());
    // Exposure is display-only: no restart.
    p.exposure = 2.0;
    let (_, status) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &s).unwrap();
    assert_eq!(status.samples, 24);
    // A texture upload (painting) restarts without a geometry change.
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.9, 0.2, 0.2, 1.0]), Some(&[0]));
    let (_, status) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &s).unwrap();
    assert!(status.samples <= status.passes.max(1), "restarted after paint: {status:?}");
    let img = r.path_traced_linear(&device, &queue).unwrap();
    let c = region_mean(&img, SIZE / 2 - 3, SIZE / 2 - 3, 6);
    assert!(c[0] > c[1] * 2.0, "the new paint is traced: {c:?}");
    // Camera move restarts.
    accumulate(&mut r, &device, &queue, &p, &scene, &s, 24);
    p = params(Vec3::new(-3.0, 2.5, 4.0));
    let (_, status) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &s).unwrap();
    assert_eq!(status.samples, status.passes, "restarted from zero after the camera moved");
    // Paused: the sample count holds.
    let paused = PathTraceSettings { paused: true, ..s };
    let before = status.samples;
    for _ in 0..3 {
        let (_, status) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &paused).unwrap();
        assert_eq!(status.samples, before);
    }
    // The raster viewport is untouched by the mode.
    p.antialias = false;
    let a = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    r.release_path_tracer();
    let b = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    assert_eq!(a, b);
}

#[test]
fn denoiser_keeps_a_flat_image_flat_reduces_noise_and_keeps_normal_edges() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let d = Denoiser::new(&device);
    let n = (SIZE * SIZE) as usize;
    let samples = 8u32;
    let v = 0.4f32;
    let s = samples as f32;
    let accum = vec![[v * s, v * s, v * s, v * v * s]; n];
    let albedo = vec![[1.0, 1.0, 1.0, 5.0]; n];
    let normal = vec![[0.0, 1.0, 0.0, 1.0]; n];
    let out = d.denoise_image(&device, &queue, [SIZE, SIZE], samples, &accum, &albedo, &normal);
    let worst = out.iter().flat_map(|c| c[..3].iter()).map(|&c| (c - v).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-4, "flat image changed by {worst}");

    // Noise on two faces split by a normal edge: the noise drops, the edge stays.
    let mut seed = 12345u32;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let mut accum = vec![[0.0; 4]; n];
    let mut normal = vec![[0.0; 4]; n];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = (y * SIZE + x) as usize;
            let left = x < SIZE / 2;
            let mean = if left { 0.2 } else { 0.8 };
            let c = mean * (0.5 + rand());
            accum[i] = [c, c, c, c * c];
            normal[i] = if left { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] };
        }
    }
    let out = d.denoise_image(&device, &queue, [SIZE, SIZE], 1, &accum, &albedo, &normal);
    let spread = |img: &[[f32; 4]], x0: u32, x1: u32| {
        let vals: Vec<f32> = (8..SIZE - 8).flat_map(|y| (x0..x1).map(move |x| (y, x))).map(|(y, x)| img[(y * SIZE + x) as usize][0]).collect();
        let m = vals.iter().sum::<f32>() / vals.len() as f32;
        (m, (vals.iter().map(|v| (v - m).powi(2)).sum::<f32>() / vals.len() as f32).sqrt())
    };
    let (m_in, s_in) = spread(&accum, SIZE / 2 + 4, SIZE - 4);
    let (m_out, s_out) = spread(&out, SIZE / 2 + 4, SIZE - 4);
    assert!(s_out < s_in * 0.5, "noise {s_in} -> {s_out}");
    assert!((m_out - m_in).abs() < 0.05, "mean kept: {m_in} -> {m_out}");
    let (left, _) = spread(&out, SIZE / 2 - 2, SIZE / 2);
    let (right, _) = spread(&out, SIZE / 2, SIZE / 2 + 2);
    assert!(left < 0.3 && right > 0.6, "edge kept: {left} | {right}");
}

/// Pixel of world point `w` in a `SIZE` x `SIZE` image of `p`.
fn pixel_of(p: &ViewParams, w: Vec3) -> (u32, u32) {
    let c = (p.proj * p.view).project_point3(w);
    (((c.x * 0.5 + 0.5) * SIZE as f32) as u32, ((0.5 - c.y * 0.5) * SIZE as f32) as u32)
}

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[test]
fn clear_color_time_limit_and_minimum_samples() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mesh = primitives::uv_sphere(1.0, 32, 16);
    let bvh = Bvh::build(&mesh);
    let scene = PathTraceScene { mesh: &mesh, bvh: &bvh, key: 21 };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &mesh);
    material(&mut r, &device, &queue, [0.5; 3], 1.0, 0.0);
    let p = furnace(params(Vec3::new(0.0, 0.0, 4.0)));
    // Camera rays that miss show the clear color exactly; the model is still lit by the environment.
    let clear = PathTraceSettings { clear_color: Some([0.2, 0.4, 0.6]), max_samples: 16, ..settings() };
    accumulate(&mut r, &device, &queue, &p, &scene, &clear, 16);
    let img = r.path_traced_linear(&device, &queue).unwrap();
    let corner = region_mean(&img, 0, 0, 4);
    for (k, want) in [0.2, 0.4, 0.6].into_iter().enumerate() {
        assert!((corner[k] - want).abs() < 1e-4, "clear color {corner:?}");
    }
    assert!(lum(region_mean(&img, SIZE / 2 - 4, SIZE / 2 - 4, 8)) > 0.3, "the sphere is still lit");
    // Changing a setting that changes the image restarts the accumulation.
    let other = PathTraceSettings { clear_color: Some([0.0, 1.0, 0.0]), ..clear };
    let (_, status) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &other).unwrap();
    assert_eq!(status.samples, status.passes, "restarted after the clear color changed: {status:?}");

    // A time limit already passed stops after the first frame...
    let timed = PathTraceSettings { max_samples: 0, max_time_s: 1e-6, min_samples: 0, ..settings() };
    let (_, status) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &timed).unwrap();
    let first = status.samples;
    assert!(status.done && status.converged() && status.elapsed_s > 0.0, "time is up: {status:?}");
    assert_eq!(status.size, [SIZE, SIZE], "the trace resolution is reported");
    let (_, status) = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &timed).unwrap();
    assert_eq!((status.samples, status.passes), (first, 0), "no work after the time limit");
    // ...unless the minimum sample count is not reached yet.
    let minimum = PathTraceSettings { min_samples: first + 6, ..timed };
    let mut status = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &minimum).unwrap().1;
    for _ in 0..100 {
        if status.done { break; }
        status = r.render_path_traced(&device, &queue, [SIZE, SIZE], &p, &scene, &minimum).unwrap().1;
    }
    assert!(status.done && status.samples >= first + 6, "the minimum samples are taken first: {status:?}");
}

#[test]
fn ground_reflectivity_offset_shadow_intensity_and_visibility_from_below() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mesh = primitives::cube(2.0);
    let bvh = Bvh::build(&mesh);
    let scene = PathTraceScene { mesh: &mesh, bvh: &bvh, key: 23 };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &mesh);
    material(&mut r, &device, &queue, [0.5; 3], 0.5, 0.0);
    let mut floor_at = |p: &ViewParams, s: &PathTraceSettings, w: Vec3| {
        accumulate(&mut r, &device, &queue, p, &scene, s, 64);
        let img = r.path_traced_linear(&device, &queue).unwrap();
        let (x, y) = pixel_of(p, w);
        lum(region_mean(&img, x - 1, y - 1, 3))
    };
    let above = furnace(params(Vec3::new(0.0, 6.0, 9.0)));
    let spot = Vec3::new(2.0, -1.0, 1.0);
    let plain = PathTraceSettings { ground: true, max_samples: 64, ..settings() };
    let diffuse = floor_at(&above, &plain, spot);
    // A mirror ground reflects the white environment; the plain floor is an 18% gray.
    let mirror = PathTraceSettings { ground_settings: GroundSettings { reflectivity: [1.0; 3], glossiness: 1.0, ..Default::default() }, ..plain };
    let reflected = floor_at(&above, &mirror, spot);
    assert!(diffuse > 0.05 && diffuse < 0.25, "plain floor {diffuse}");
    assert!(reflected > 0.7 && reflected > diffuse * 3.0, "mirror floor {reflected} vs plain {diffuse}");
    // Moving the ground far to the side leaves that spot showing the background.
    let moved = PathTraceSettings { ground_settings: GroundSettings { offset: [100.0, 0.0, 0.0], ..Default::default() }, ..plain };
    assert!(floor_at(&above, &moved, spot) < 1e-4, "the ground moved away");

    // From underneath: the floor shows only when it is visible from below.
    let below = furnace(params(Vec3::new(0.0, -5.0, 6.0)));
    let seen = floor_at(&below, &plain, spot);
    let hidden = PathTraceSettings { ground_settings: GroundSettings { visible_from_below: false, ..Default::default() }, ..plain };
    assert!(seen > 0.05, "floor seen from below {seen}");
    assert!(floor_at(&below, &hidden, spot) < 1e-4, "floor hidden from below");

    // Shadow intensity 0 lets the key light reach the shadowed floor.
    let mut lit = params(Vec3::new(0.0, 6.0, 9.0));
    lit.light_dir = Vec3::new(0.3, 1.0, 0.0);
    lit.background = [0.0; 3];
    let shade = Vec3::new(-1.2, -1.0, 0.0);
    let full = floor_at(&lit, &plain, shade);
    let none = PathTraceSettings { ground_settings: GroundSettings { shadow_intensity: 0.0, ..Default::default() }, ..plain };
    let lifted = floor_at(&lit, &none, shade);
    assert!(lifted > full * 1.15, "shadow intensity 0: {lifted} vs full shadow {full}");
}

/// Speed at 1920x1080 (run: `cargo test --release -p subcraft-render --test pathtrace -- --ignored --nocapture`).
#[test]
#[ignore]
fn path_trace_speed_at_1080p() {
    let _gpu = gpu_test();
    let Some((adapter, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    for (name, mesh) in [("sample crate", primitives::sample_crate()), ("sphere 262k tris", primitives::uv_sphere(1.0, 512, 256))] {
        let bvh = Bvh::build(&mesh);
        let scene = PathTraceScene { mesh: &mesh, bvh: &bvh, key: 11 };
        let mut r = ViewportRenderer::new(&device, &queue);
        r.set_mesh(&device, &mesh);
        let b = mesh.bounds;
        let center = (b.min + b.max) * 0.5;
        let eye = center + Vec3::new(1.0, 0.7, 1.4) * (b.max - b.min).length();
        let mut p = ViewParams::new(Mat4::look_at_rh(eye, center, Vec3::Y), Mat4::perspective_rh(40f32.to_radians(), 16.0 / 9.0, 0.01, 100.0));
        p.eye = eye;
        for bounces in [1, 4] {
            let s = PathTraceSettings { max_bounces: bounces, max_samples: 0, ..Default::default() };
            r.render_path_traced(&device, &queue, [1920, 1080], &p, &scene, &s).unwrap();
            let started = std::time::Instant::now();
            let (mut frames, mut samples) = (0, 0);
            while started.elapsed().as_secs_f32() < 3.0 {
                let (_, status) = r.render_path_traced(&device, &queue, [1920, 1080], &p, &scene, &s).unwrap();
                frames += 1;
                samples += status.passes;
            }
            let secs = started.elapsed().as_secs_f32();
            eprintln!("{} | {name}, {bounces} bounce(s), denoise on: {:.1} samples/s at 1920x1080, {:.1} ms per frame", adapter.get_info().name, samples as f32 / secs, secs * 1000.0 / frames as f32);
        }
    }
}
