//! Path-traced contact sheet of the extended materials: one sphere per feature (plain, clear coat,
//! sheen, anisotropy, subsurface, translucency, glass, cut-out opacity) on the ground under a
//! bundled HDRI, with the raster viewport's render of the same scene below each for comparison.
//! Needs a GPU adapter (lavapipe works, slowly).
//!
//! `cargo run --release -p subcraft-render --example pathtrace_showcase -- out.png [samples] ["Library Hall"]`

use glam::{Mat4, Vec3};
use subcraft_app::assets::Library;
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Bvh, primitives};
use subcraft_render::{HdriUpload, PathTraceScene, PathTraceSettings, ViewParams, ViewportRenderer, headless};

const CELL: u32 = 240;
const TEX: u32 = 128;

fn solid(kind: ChannelKind, v: [f32; 4]) -> TiledImage {
    TiledImage::new(TEX, TEX, if kind.components() == 3 { 4 } else { 1 }, v)
}
fn s(kind: ChannelKind, v: f32) -> (ChannelKind, TiledImage) {
    (kind, solid(kind, [v; 4]))
}
fn c(kind: ChannelKind, rgb: [f32; 3]) -> (ChannelKind, TiledImage) {
    (kind, solid(kind, [rgb[0], rgb[1], rgb[2], 1.0]))
}
/// Holes in a grid (a cut-out opacity pattern).
fn holes() -> (ChannelKind, TiledImage) {
    let mut img = TiledImage::new(TEX, TEX, 1, [1.0; 4]);
    for y in 0..TEX {
        for x in 0..TEX {
            let (u, v) = ((x % 16) as f32 - 7.5, (y % 16) as f32 - 7.5);
            if u * u + v * v < 25.0 {
                img.set(x, y, [0.0; 4]);
            }
        }
    }
    (ChannelKind::Opacity, img)
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "pathtrace_showcase.png".into());
    let samples: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(256);
    let env_name = std::env::args().nth(3).unwrap_or_else(|| "Library Hall".into());
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no GPU adapter");
        std::process::exit(1);
    };
    let lib = Library::builtin();
    let env = lib.environments.iter().find(|e| e.name == env_name).expect("bundled environment");
    let hdri = env.source.hdri().expect("an HDRI environment");
    let levels: Vec<(u32, u32, &[f32])> = hdri.levels.iter().map(|l| (l.width, l.height, l.rgba.as_slice())).collect();
    use ChannelKind as K;
    let looks: Vec<(&str, Vec<(ChannelKind, TiledImage)>)> = vec![
        ("plain", vec![c(K::BaseColor, [0.55, 0.05, 0.04]), s(K::Roughness, 0.5)]),
        ("clear coat", vec![c(K::BaseColor, [0.55, 0.05, 0.04]), s(K::Roughness, 0.5), s(K::CoatOpacity, 1.0), s(K::CoatRoughness, 0.03)]),
        ("sheen", vec![c(K::BaseColor, [0.06, 0.02, 0.12]), s(K::Roughness, 0.9), c(K::SheenColor, [0.85, 0.7, 1.0]), s(K::SheenOpacity, 1.0), s(K::SheenRoughness, 0.35)]),
        ("anisotropy", vec![c(K::BaseColor, [0.9, 0.88, 0.85]), s(K::Roughness, 0.35), s(K::Metallic, 1.0), s(K::AnisotropyLevel, 0.85)]),
        ("subsurface", vec![c(K::BaseColor, [0.85, 0.6, 0.48]), s(K::Roughness, 0.45), s(K::Scattering, 1.0), c(K::ScatteringColor, [1.0, 0.35, 0.2])]),
        ("translucency", vec![c(K::BaseColor, [0.3, 0.7, 0.15]), s(K::Roughness, 0.55), s(K::Translucency, 0.9)]),
        ("glass", vec![c(K::BaseColor, [1.0, 1.0, 1.0]), s(K::Roughness, 0.02), c(K::Transmissive, [0.85, 0.95, 1.0]), s(K::Ior, 0.5)]),
        ("cut-out", vec![c(K::BaseColor, [0.8, 0.7, 0.3]), s(K::Roughness, 0.3), s(K::Metallic, 1.0), holes()]),
    ];
    let cols = looks.len() as u32;
    let (sw, sh) = (CELL * cols, CELL * 2);
    let mut sheet = vec![0.0f32; (sw * sh * 4) as usize];
    let mesh = primitives::uv_sphere(1.0, 96, 48);
    let bvh = Bvh::build(&mesh);
    for (i, (name, channels)) in looks.iter().enumerate() {
        let mut r = ViewportRenderer::new(&device, &queue);
        r.set_hdri(&device, &queue, Some(HdriUpload { levels: &levels, sh: hdri.sh }));
        r.set_mesh(&device, &mesh);
        for (k, img) in channels {
            r.upload_channel(&device, &queue, 0, *k, img, None);
        }
        let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 0.6, 3.6), Vec3::new(0.0, -0.1, 0.0), Vec3::Y), Mat4::perspective_rh(38f32.to_radians(), 1.0, 0.1, 30.0));
        p.environment.use_hdri = true;
        p.environment.background_opacity = 1.0;
        p.environment.background_blur = 0.5;
        p.light_dir = Vec3::new(0.6, 0.8, 0.5).normalize();
        p.light_intensity = 1.5;
        let settings = PathTraceSettings { ground: true, denoise: true, max_samples: samples, max_bounces: 8, frame_budget_ms: 4000.0, ..Default::default() };
        let scene = PathTraceScene { mesh: &mesh, bvh: &bvh, key: 1 };
        let t0 = std::time::Instant::now();
        let mut reached = 0;
        while reached < samples {
            let (_, status) = r.render_path_traced(&device, &queue, [CELL, CELL], &p, &scene, &settings).expect("path tracing");
            if status.samples == reached && status.done {
                break;
            }
            reached = status.samples;
        }
        let traced = r.path_target().expect("traced").read_rgba8(&device, &queue);
        let raster = r.render(&device, &queue, [CELL, CELL], &p).read_rgba8(&device, &queue);
        eprintln!("{name}: {reached} samples in {:.1} s", t0.elapsed().as_secs_f32());
        for (row, px) in [(0u32, &traced), (1, &raster)] {
            for y in 0..CELL {
                for x in 0..CELL {
                    let src = ((y * CELL + x) * 4) as usize;
                    let dst = (((row * CELL + y) * sw + i as u32 * CELL + x) * 4) as usize;
                    for k in 0..4 {
                        sheet[dst + k] = px[src + k] as f32 / 255.0;
                    }
                }
            }
        }
    }
    subcraft_image::save_image(std::path::Path::new(&out), sw, sh, 4, &sheet, subcraft_image::FileFormat::Png8).expect("write png");
    eprintln!("wrote {out}");
}
