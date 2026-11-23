//! The path tracer as a still renderer: the HDRI drawn behind the model, HDRI importance sampling
//! (a small bright sun converges much faster with it, to the same answer) and thin-lens depth of
//! field. Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_render::wgpu_api::{Device, Queue};
use subcraft_render::{EnvironmentParams, HdriUpload, PathTraceScene, PathTraceSettings, ViewParams, ViewportRenderer, environment_cdf, headless};

const SIZE: u32 = 64;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An equirect map of a dim sky with a small, very bright sun in front of the camera (+Z).
fn sun_hdri() -> (u32, u32, Vec<f32>) {
    let (w, h) = (64u32, 32u32);
    let mut px = vec![0.0f32; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let v = if (47..49).contains(&x) && (8..10).contains(&y) { 400.0 } else { 0.05 };
            px[i..i + 4].copy_from_slice(&[v, v, v, 1.0]);
        }
    }
    (w, h, px)
}

fn uniform_hdri(c: [f32; 3]) -> (u32, u32, Vec<f32>) {
    let (w, h) = (16u32, 8u32);
    (w, h, (0..w * h).flat_map(|_| [c[0], c[1], c[2], 1.0]).collect())
}

struct Scene {
    r: ViewportRenderer,
    mesh: Mesh,
    bvh: Bvh,
}

impl Scene {
    fn sphere(device: &Device, queue: &Queue) -> Self {
        let mesh = primitives::uv_sphere(1.0, 48, 24);
        let mut r = ViewportRenderer::new(device, queue);
        r.set_mesh(device, &mesh);
        for (k, v) in [(ChannelKind::BaseColor, [1.0, 1.0, 1.0, 1.0]), (ChannelKind::Roughness, [1.0; 4])] {
            let comps = if k.components() == 3 { 4 } else { 1 };
            r.upload_channel(device, queue, 0, k, &TiledImage::new(32, 32, comps, v), None);
        }
        let bvh = Bvh::build(&mesh);
        Self { r, mesh, bvh }
    }

    fn hdri(&mut self, device: &Device, queue: &Queue, map: &(u32, u32, Vec<f32>)) {
        // A box-filtered mip chain (the renderer treats a single level as "no HDRI").
        let mut chain = vec![(map.0, map.1, map.2.clone())];
        while chain.last().unwrap().1 > 1 {
            let (w, h, px) = chain.last().unwrap();
            let (nw, nh) = (w / 2, h / 2);
            let mut next = vec![0.0f32; (nw * nh * 4) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    for k in 0..4 {
                        let at = |xx: u32, yy: u32| px[((yy * w + xx) * 4 + k) as usize];
                        next[((y * nw + x) * 4 + k) as usize] = 0.25 * (at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1));
                    }
                }
            }
            chain.push((nw, nh, next));
        }
        let levels: Vec<(u32, u32, &[f32])> = chain.iter().map(|(w, h, px)| (*w, *h, px.as_slice())).collect();
        self.r.set_hdri(device, queue, Some(HdriUpload { levels: &levels, sh: [[0.0; 3]; 9] }));
    }

    /// The linear image after `samples` samples.
    fn trace(&mut self, device: &Device, queue: &Queue, p: &ViewParams, settings: &PathTraceSettings, samples: u32) -> Vec<[f32; 4]> {
        let scene = PathTraceScene { mesh: &self.mesh, bvh: &self.bvh, key: 1 };
        for _ in 0..4000 {
            let (_, status) = self.r.render_path_traced(device, queue, [SIZE, SIZE], p, &scene, settings).expect("path tracing works");
            if status.samples >= samples {
                break;
            }
        }
        self.r.path_traced_linear(device, queue).unwrap()
    }
}

fn view() -> ViewParams {
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    p.environment = EnvironmentParams { use_hdri: true, hdri_intensity: 1.0, rotation: 0.0, background_opacity: 0.0, ..Default::default() };
    p.light_intensity = 0.0;
    p.background = [0.0; 3];
    p
}

fn settings() -> PathTraceSettings {
    PathTraceSettings { ground: false, denoise: false, max_samples: 0, max_bounces: 4, frame_budget_ms: 2000.0, ..Default::default() }
}

fn mean(img: &[[f32; 4]], x0: u32, y0: u32, n: u32) -> f32 {
    let mut s = 0.0f64;
    for y in y0..y0 + n {
        for x in x0..x0 + n {
            s += img[(y * SIZE + x) as usize][1] as f64;
        }
    }
    (s / (n * n) as f64) as f32
}

/// Mean absolute difference between horizontal neighbours in a block: the noise of a smooth image.
fn roughness(img: &[[f32; 4]], x0: u32, y0: u32, n: u32) -> f32 {
    let mut s = 0.0f64;
    for y in y0..y0 + n {
        for x in x0..x0 + n - 1 {
            s += (img[(y * SIZE + x + 1) as usize][1] - img[(y * SIZE + x) as usize][1]).abs() as f64;
        }
    }
    (s / (n * (n - 1)) as f64) as f32
}

#[test]
fn environment_table_puts_the_samples_where_the_light_is() {
    let (w, h, px) = sun_hdri();
    let t = environment_cdf(w, h, &px);
    let (w, h) = (w as usize, h as usize);
    assert_eq!(t.len(), 4 + h + 1 + h * (w + 1) + w * h);
    assert_eq!((t[0], t[1]), (w as f32, h as f32));
    let marginal = &t[4..4 + h + 1];
    assert!(marginal.windows(2).all(|p| p[1] >= p[0]) && (marginal[h] - 1.0).abs() < 1e-5);
    // The sun (dilated by a texel for the bilinear lookup) holds almost all the light: rows 7 to 10
    // take > 95 % of the marginal, columns 46 to 49 of their rows, and the sun itself most of it.
    assert!(marginal[11] - marginal[7] > 0.95, "{:?}", &marginal[6..12]);
    let row = &t[4 + h + 1 + 8 * (w + 1)..4 + h + 1 + 9 * (w + 1)];
    assert!(row[50] - row[46] > 0.95 && (row[w] - 1.0).abs() < 1e-5);
    assert!(marginal[10] - marginal[8] > 0.5 && row[49] - row[47] > 0.5, "{:?}", &row[45..51]);
    // A black map still samples (uniformly by solid angle) instead of dividing by zero.
    let black = environment_cdf(4, 2, &[0.0; 32]);
    assert!(black.iter().all(|v| v.is_finite()) && black[2] > 0.0);
}

#[test]
fn hdri_importance_sampling_lights_a_small_sun_with_far_less_noise_and_the_same_brightness() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut s = Scene::sphere(&device, &queue);
    s.hdri(&device, &queue, &sun_hdri());
    let p = view();
    let on = settings();
    let off = PathTraceSettings { environment_sampling: false, ..settings() };
    // Low sample counts: noise.
    let quick_on = s.trace(&device, &queue, &p, &on, 8);
    let quick_off = s.trace(&device, &queue, &p, &off, 8);
    let (n_on, n_off) = (roughness(&quick_on, 20, 20, 24), roughness(&quick_off, 20, 20, 24));
    // Many samples: the converged brightness of the lit front.
    let full_on = s.trace(&device, &queue, &p, &on, 256);
    let full_off = s.trace(&device, &queue, &p, &off, 1024);
    let (m_on, m_off) = (mean(&full_on, 12, 12, 40), mean(&full_off, 12, 12, 40));
    eprintln!("sun: noise at 8 spp {n_on:.4} (sampled) vs {n_off:.4} (BSDF only); mean {m_on:.4} vs {m_off:.4}");
    assert!(m_on > 0.3, "the sun lights the front: {m_on}");
    assert!((m_on - m_off).abs() < 0.1 * m_off, "same answer with and without sampling: {m_on} vs {m_off}");
    assert!(n_on * 6.0 < n_off, "sampling the sun removes most of the noise: {n_on} vs {n_off}");
}

#[test]
fn the_hdri_is_drawn_behind_the_model_by_the_background_opacity() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut s = Scene::sphere(&device, &queue);
    let sky = [0.2, 0.4, 0.8];
    s.hdri(&device, &queue, &uniform_hdri(sky));
    let mut p = view();
    p.background = [0.1, 0.1, 0.1];
    let corner = |img: &[[f32; 4]]| img[(2 * SIZE + 2) as usize];
    let plain = s.trace(&device, &queue, &p, &settings(), 4);
    p.environment.background_opacity = 1.0;
    let full = s.trace(&device, &queue, &p, &settings(), 4);
    p.environment.background_opacity = 0.5;
    let half = s.trace(&device, &queue, &p, &settings(), 4);
    eprintln!("backdrop: plain {:?}, full {:?}, half {:?}", corner(&plain), corner(&full), corner(&half));
    for k in 0..3 {
        assert!((corner(&plain)[k] - 0.1).abs() < 0.01, "opacity 0 keeps the background");
        assert!((corner(&full)[k] - sky[k]).abs() < 0.02, "opacity 1 shows the HDRI");
        assert!((corner(&half)[k] - (0.1 + sky[k]) * 0.5).abs() < 0.02, "opacity 0.5 mixes them");
    }
}

#[test]
fn depth_of_field_blurs_what_is_off_the_focus_distance_and_keeps_what_is_on_it() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut s = Scene::sphere(&device, &queue);
    s.hdri(&device, &queue, &uniform_hdri([1.0; 3]));
    let p = view();
    // The steepest step across the sphere's silhouette along the middle row (bright sphere, black backdrop).
    let edge = |img: &[[f32; 4]]| {
        let y = SIZE / 2;
        (0..SIZE - 1).map(|x| (img[(y * SIZE + x + 1) as usize][1] - img[(y * SIZE + x) as usize][1]).abs()).fold(0.0f32, f32::max)
    };
    let pinhole = s.trace(&device, &queue, &p, &settings(), 128);
    let focused = s.trace(&device, &queue, &p, &PathTraceSettings { depth_of_field: 1.0, focus: 3.87, ..settings() }, 128);
    let blurred = s.trace(&device, &queue, &p, &PathTraceSettings { depth_of_field: 1.0, focus: 12.0, ..settings() }, 128);
    let (e0, e1, e2) = (edge(&pinhole), edge(&focused), edge(&blurred));
    eprintln!("silhouette step: pinhole {e0:.3}, in focus {e1:.3}, out of focus {e2:.3}");
    assert!(e1 > 0.75 * e0, "in focus stays sharp: {e0} vs {e1}");
    assert!(e2 < 0.5 * e0, "out of focus is blurred: {e0} vs {e2}");
    // The blur moves light around but keeps it: the whole frame is about as bright.
    let total = |img: &[[f32; 4]]| img.iter().map(|c| c[1] as f64).sum::<f64>();
    assert!((total(&blurred) / total(&pinhole) - 1.0).abs() < 0.05);
}
