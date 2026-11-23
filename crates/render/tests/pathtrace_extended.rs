//! The path tracer's extended materials: subsurface scattering, transmission, coat, sheen,
//! translucency, anisotropy and cut-out opacity. Skips (passes with a message) when no adapter
//! is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_render::wgpu_api::{Device, Queue};
use subcraft_render::{EnvironmentParams, PathTraceScene, PathTraceSettings, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 64;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn solid(comps: usize, v: [f32; 4]) -> TiledImage {
    TiledImage::new(32, 32, comps, v)
}

/// Uniform white environment (radiance 1), no key light, black background.
fn furnace() -> ViewParams {
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    p.environment = EnvironmentParams { ground: [1.0; 3], horizon: [1.0; 3], zenith: [1.0; 3], panels: 0.0, top: 0.0, key_softbox: 0.0, ..Default::default() };
    p.light_intensity = 0.0;
    p.background = [0.0; 3];
    p
}

fn settings() -> PathTraceSettings {
    PathTraceSettings { ground: false, denoise: false, max_samples: 0, max_bounces: 12, frame_budget_ms: 2000.0, ..Default::default() }
}

struct Scene {
    r: ViewportRenderer,
    mesh: Mesh,
    bvh: Bvh,
    key: u64,
}

impl Scene {
    fn new(device: &Device, queue: &Queue, mesh: Mesh) -> Self {
        let mut r = ViewportRenderer::new(device, queue);
        r.set_mesh(device, &mesh);
        let bvh = Bvh::build(&mesh);
        Self { r, mesh, bvh, key: 1 }
    }

    fn set(&mut self, device: &Device, queue: &Queue, channels: &[(ChannelKind, [f32; 4])]) {
        for (k, v) in channels {
            let comps = if k.components() == 3 { 4 } else { 1 };
            self.r.upload_channel(device, queue, 0, *k, &solid(comps, *v), None);
        }
    }

    /// Linear mean of the centre `n` x `n` pixels after `samples` samples.
    fn trace(&mut self, device: &Device, queue: &Queue, p: &ViewParams, samples: u32, n: u32) -> [f32; 3] {
        let scene = PathTraceScene { mesh: &self.mesh, bvh: &self.bvh, key: self.key };
        for _ in 0..2000 {
            let (_, status) = self.r.render_path_traced(device, queue, [SIZE, SIZE], p, &scene, &settings()).expect("path tracing works");
            if status.samples >= samples {
                break;
            }
        }
        let img = self.r.path_traced_linear(device, queue).unwrap();
        let mut sum = [0.0f64; 3];
        let o = (SIZE - n) / 2;
        for y in o..o + n {
            for x in o..o + n {
                let c = img[(y * SIZE + x) as usize];
                for k in 0..3 {
                    sum[k] += c[k] as f64;
                }
            }
        }
        sum.map(|s| (s / (n * n) as f64) as f32)
    }
}

fn rgb(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

#[test]
fn subsurface_scattering_keeps_the_energy_of_a_white_material_and_the_colour_of_a_tinted_one() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut s = Scene::new(&device, &queue, primitives::uv_sphere(1.0, 48, 24));
    let p = furnace();
    s.set(&device, &queue, &[(ChannelKind::BaseColor, rgb([1.0; 3])), (ChannelKind::Roughness, [1.0; 4]), (ChannelKind::Scattering, [1.0; 4]), (ChannelKind::ScatteringColor, rgb([1.0; 3]))]);
    let white = s.trace(&device, &queue, &p, 384, 16);
    eprintln!("white subsurface furnace: {white:?}");
    for k in 0..3 {
        assert!((0.85..=1.05).contains(&white[k]), "white scattering material returns the light: {white:?}");
    }
    // Skin-like: the albedo inversion keeps the base colour's balance.
    s.set(&device, &queue, &[(ChannelKind::BaseColor, rgb([0.8, 0.45, 0.35]))]);
    let skin = s.trace(&device, &queue, &p, 384, 16);
    eprintln!("tinted subsurface furnace: {skin:?}");
    assert!(skin[0] > skin[1] && skin[1] > skin[2], "{skin:?}");
    assert!((skin[0] - 0.8).abs() < 0.15 && (skin[2] - 0.35).abs() < 0.12, "close to the base colour: {skin:?}");
}

#[test]
fn clear_glass_neither_gains_nor_loses_light_and_cutout_opacity_disappears() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut s = Scene::new(&device, &queue, primitives::uv_sphere(1.0, 48, 24));
    let p = furnace();
    // Glass: everything reflected or refracted, so a uniform environment looks uniform through it.
    s.set(&device, &queue, &[(ChannelKind::BaseColor, rgb([1.0; 3])), (ChannelKind::Roughness, [0.05; 4]), (ChannelKind::Transmissive, rgb([1.0; 3])), (ChannelKind::Ior, [0.5; 4])]);
    let glass = s.trace(&device, &queue, &p, 384, 16);
    eprintln!("glass furnace: {glass:?}");
    for k in 0..3 {
        assert!((0.88..=1.04).contains(&glass[k]), "clear glass in a furnace: {glass:?}");
    }
    // Tinted glass passes its colour.
    s.set(&device, &queue, &[(ChannelKind::Transmissive, rgb([0.2, 0.9, 0.3]))]);
    let green = s.trace(&device, &queue, &p, 256, 16);
    assert!(green[1] > green[0] + 0.2 && green[1] > green[2] + 0.2, "green glass {green:?}");

    // Opacity 0: the camera sees the black background through the cut-out.
    s.set(&device, &queue, &[(ChannelKind::Transmissive, rgb([0.0; 3])), (ChannelKind::Roughness, [1.0; 4]), (ChannelKind::Opacity, [0.0; 4])]);
    let gone = s.trace(&device, &queue, &p, 64, 16);
    assert!(gone.iter().all(|&v| v < 0.01), "fully cut out {gone:?}");
    s.set(&device, &queue, &[(ChannelKind::Opacity, [0.5; 4])]);
    let half = s.trace(&device, &queue, &p, 256, 16);
    // Half the rays shade the front, a quarter the inside of the back (lit through the holes),
    // a quarter see the background: 0.5 + 0.25 x (inside < 1).
    assert!(half[1] > 0.55 && half[1] < 0.78, "half cut-out of a white diffuse sphere: {half:?}");
}

#[test]
fn coat_and_sheen_add_reflection_on_top_of_a_dark_base() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut s = Scene::new(&device, &queue, primitives::uv_sphere(1.0, 48, 24));
    let p = furnace();
    s.set(&device, &queue, &[(ChannelKind::BaseColor, rgb([0.0; 3])), (ChannelKind::Roughness, [0.6; 4])]);
    let bare = s.trace(&device, &queue, &p, 256, 40);
    s.set(&device, &queue, &[(ChannelKind::CoatOpacity, [1.0; 4]), (ChannelKind::CoatRoughness, [0.05; 4])]);
    let coated = s.trace(&device, &queue, &p, 256, 40);
    s.set(&device, &queue, &[(ChannelKind::CoatOpacity, [0.0; 4]), (ChannelKind::SheenOpacity, [1.0; 4]), (ChannelKind::SheenColor, rgb([1.0; 3]))]);
    let sheen = s.trace(&device, &queue, &p, 256, 40);
    eprintln!("black base: bare {bare:?}, coated {coated:?}, sheen {sheen:?}");
    assert!(coated[1] > bare[1] + 0.02 && coated[1] < 0.3, "the coat reflects a few percent more: {bare:?} -> {coated:?}");
    assert!(sheen[1] > bare[1] + 0.03 && sheen[1] < 0.6, "sheen adds a soft rim: {bare:?} -> {sheen:?}");
}

#[test]
fn translucency_lets_a_back_light_through_a_thin_sheet_and_anisotropy_changes_the_highlight() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    // A sheet facing the camera, the key light behind it, dark environment.
    let sheet = primitives::plane(3.0, 1).transformed(&Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2));
    let mut s = Scene::new(&device, &queue, sheet);
    let mut p = furnace();
    p.environment = EnvironmentParams { ground: [0.0; 3], horizon: [0.0; 3], zenith: [0.0; 3], panels: 0.0, top: 0.0, key_softbox: 0.0, ..Default::default() };
    p.light_intensity = 3.0;
    p.light_dir = Vec3::new(0.0, 0.3, -1.0).normalize();
    s.set(&device, &queue, &[(ChannelKind::BaseColor, rgb([0.3, 0.8, 0.2])), (ChannelKind::Roughness, [0.6; 4])]);
    let opaque = s.trace(&device, &queue, &p, 64, 16);
    s.set(&device, &queue, &[(ChannelKind::Translucency, [1.0; 4])]);
    let lit = s.trace(&device, &queue, &p, 64, 16);
    eprintln!("back-lit sheet: opaque {opaque:?}, translucent {lit:?}");
    assert!(opaque[1] < 0.01, "the light is behind: {opaque:?}");
    assert!(lit[1] > 0.05 && lit[1] > lit[0] && lit[1] > lit[2], "light through the leaf, tinted by it: {lit:?}");

    // Anisotropy: a brushed metal sphere under the key light differs from the isotropic one.
    let mut s = Scene::new(&device, &queue, primitives::uv_sphere(1.0, 48, 24));
    let mut p = furnace();
    p.environment = EnvironmentParams { ground: [0.02; 3], horizon: [0.02; 3], zenith: [0.02; 3], panels: 0.0, top: 0.0, key_softbox: 0.0, ..Default::default() };
    p.light_intensity = 3.0;
    p.light_dir = Vec3::new(0.3, 0.4, 1.0).normalize();
    s.set(&device, &queue, &[(ChannelKind::BaseColor, rgb([0.9; 3])), (ChannelKind::Metallic, [1.0; 4]), (ChannelKind::Roughness, [0.35; 4])]);
    let iso = s.trace(&device, &queue, &p, 64, 48);
    s.set(&device, &queue, &[(ChannelKind::AnisotropyLevel, [0.9; 4])]);
    let aniso = s.trace(&device, &queue, &p, 64, 48);
    eprintln!("highlight: isotropic {iso:?}, anisotropic {aniso:?}");
    assert!((aniso[1] - iso[1]).abs() > 0.01 * iso[1].max(0.01), "anisotropy changes the highlight: {iso:?} vs {aniso:?}");
}
