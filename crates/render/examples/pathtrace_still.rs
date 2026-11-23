//! A portfolio-style path-traced still: five spheres of different materials receding from the
//! camera under a bundled HDRI drawn behind them, the camera focused on the middle one (thin-lens
//! depth of field), and the HDRI importance sampled. Needs a GPU adapter (lavapipe works, slowly).
//!
//! `cargo run -p subcraft-render --example pathtrace_still -- out.png [samples] [width] [height] [aperture] ["Library Hall"]`

use glam::{Mat4, Vec3};
use subcraft_app::assets::Library;
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, Mesh, primitives};
use subcraft_render::{HdriUpload, PathTraceScene, PathTraceSettings, ViewParams, ViewportRenderer, headless};

fn solid(kind: ChannelKind, v: [f32; 4]) -> TiledImage {
    TiledImage::new(64, 64, if kind.components() == 3 { 4 } else { 1 }, v)
}

fn main() {
    let arg = |i: usize| std::env::args().nth(i);
    let out = arg(1).unwrap_or_else(|| "pathtrace_still.png".into());
    let samples: u32 = arg(2).and_then(|s| s.parse().ok()).unwrap_or(256);
    let width: u32 = arg(3).and_then(|s| s.parse().ok()).unwrap_or(960);
    let height: u32 = arg(4).and_then(|s| s.parse().ok()).unwrap_or(540);
    let aperture: f32 = arg(5).and_then(|s| s.parse().ok()).unwrap_or(0.6);
    let env_name = arg(6).unwrap_or_else(|| "Library Hall".into());
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no GPU adapter");
        std::process::exit(1);
    };
    let lib = Library::builtin();
    let env = lib.environments.iter().find(|e| e.name == env_name).expect("bundled environment");
    let hdri = env.source.hdri().expect("an HDRI environment");
    let levels: Vec<(u32, u32, &[f32])> = hdri.levels.iter().map(|l| (l.width, l.height, l.rgba.as_slice())).collect();

    use ChannelKind as K;
    let c = |k: K, rgb: [f32; 3]| (k, [rgb[0], rgb[1], rgb[2], 1.0]);
    let s = |k: K, v: f32| (k, [v; 4]);
    let looks: Vec<Vec<(K, [f32; 4])>> = vec![
        vec![c(K::BaseColor, [0.55, 0.04, 0.03]), s(K::Roughness, 0.45), s(K::CoatOpacity, 1.0), s(K::CoatRoughness, 0.03)],
        vec![c(K::BaseColor, [1.0, 0.78, 0.34]), s(K::Roughness, 0.25), s(K::Metallic, 1.0)],
        vec![c(K::BaseColor, [0.85, 0.6, 0.48]), s(K::Roughness, 0.45), s(K::Scattering, 1.0), c(K::ScatteringColor, [1.0, 0.35, 0.2])],
        vec![c(K::BaseColor, [1.0, 1.0, 1.0]), s(K::Roughness, 0.02), c(K::Transmissive, [0.85, 0.95, 1.0]), s(K::Ior, 0.5)],
        vec![c(K::BaseColor, [0.05, 0.03, 0.12]), s(K::Roughness, 0.9), c(K::SheenColor, [0.8, 0.65, 1.0]), s(K::SheenOpacity, 1.0), s(K::SheenRoughness, 0.35)],
    ];
    // Spheres on a diagonal line receding from the camera; the middle one (index 2) is in focus.
    let sphere = primitives::uv_sphere(1.0, 96, 48);
    let parts: Vec<Mesh> = (0..looks.len()).map(|i| sphere.transformed(&Mat4::from_translation(Vec3::new(-3.2 + 1.6 * i as f32, 0.0, 2.4 - 2.4 * i as f32)))).collect();
    let mesh = Mesh::merged(&parts.iter().collect::<Vec<_>>());
    let bvh = Bvh::build(&mesh);

    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_hdri(&device, &queue, Some(HdriUpload { levels: &levels, sh: hdri.sh }));
    r.set_mesh(&device, &mesh);
    for (slot, channels) in looks.iter().enumerate() {
        for (k, v) in channels {
            r.upload_channel(&device, &queue, slot as u32, *k, &solid(*k, *v), None);
        }
    }
    let eye = Vec3::new(1.0, 1.3, 8.0);
    let target = Vec3::new(0.0, 0.0, -2.4);
    let mut p = ViewParams::new(Mat4::look_at_rh(eye, target, Vec3::Y), Mat4::perspective_rh(40f32.to_radians(), width as f32 / height as f32, 0.1, 100.0));
    p.environment.use_hdri = true;
    p.environment.background_opacity = 1.0;
    p.environment.background_blur = 0.15;
    p.light_intensity = 0.0;
    let settings = PathTraceSettings {
        ground: true,
        denoise: true,
        max_samples: samples,
        max_bounces: 8,
        frame_budget_ms: 4000.0,
        depth_of_field: aperture,
        focus: (eye - target).length(),
        ..Default::default()
    };
    let scene = PathTraceScene { mesh: &mesh, bvh: &bvh, key: 1 };
    let t0 = std::time::Instant::now();
    let mut reached = 0;
    while reached < samples {
        let (_, status) = r.render_path_traced(&device, &queue, [width, height], &p, &scene, &settings).expect("path tracing");
        if status.samples == reached && status.done {
            break;
        }
        reached = status.samples;
    }
    let px = r.path_target().expect("traced").read_rgba8(&device, &queue);
    eprintln!("{reached} samples at {width} x {height} in {:.1} s", t0.elapsed().as_secs_f32());
    let data: Vec<f32> = px.iter().map(|&b| b as f32 / 255.0).collect();
    subcraft_image::save_image(std::path::Path::new(&out), width, height, 4, &data, subcraft_image::FileFormat::Png8).expect("write png");
    eprintln!("wrote {out}");
}
