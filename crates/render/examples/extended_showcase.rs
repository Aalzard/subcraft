//! Renders one sphere per extended viewport feature (coat, sheen, anisotropy, subsurface,
//! translucency, transmission, displacement) next to the plain metal/roughness material, into a
//! contact sheet PNG. Needs a GPU adapter (lavapipe works).
//!
//! `cargo run --release -p subcraft-render --example extended_showcase -- out.png`

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::{ViewParams, ViewportRenderer, headless};

const CELL: u32 = 360;
const TEX: u32 = 256;

fn solid(kind: ChannelKind, v: [f32; 4]) -> TiledImage {
    TiledImage::new(TEX, TEX, if kind.components() == 3 { 4 } else { 1 }, v)
}
fn s(kind: ChannelKind, v: f32) -> (ChannelKind, TiledImage) {
    (kind, solid(kind, [v; 4]))
}
fn c(kind: ChannelKind, rgb: [f32; 3]) -> (ChannelKind, TiledImage) {
    (kind, solid(kind, [rgb[0], rgb[1], rgb[2], 1.0]))
}
fn pattern(kind: ChannelKind, f: impl Fn(f32, f32) -> f32) -> (ChannelKind, TiledImage) {
    let mut img = TiledImage::new(TEX, TEX, 1, [0.0; 4]);
    for y in 0..TEX {
        for x in 0..TEX {
            img.set(x, y, [f((x as f32 + 0.5) / TEX as f32, (y as f32 + 0.5) / TEX as f32), 0.0, 0.0, 0.0]);
        }
    }
    (kind, img)
}

struct Look {
    channels: Vec<(ChannelKind, TiledImage)>,
    light: Vec3,
    env: f32,
    displacement: Option<u32>,
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "extended_showcase.png".into());
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no GPU adapter");
        std::process::exit(1);
    };
    use ChannelKind as K;
    let key = Vec3::new(0.65, 0.75, 0.35).normalize();
    let looks = vec![
        ("plain", Look { channels: vec![c(K::BaseColor, [0.55, 0.05, 0.04]), s(K::Roughness, 0.55), s(K::Metallic, 0.0)], light: key, env: 1.0, displacement: None }),
        ("clear coat", Look { channels: vec![c(K::BaseColor, [0.55, 0.05, 0.04]), s(K::Roughness, 0.55), s(K::Metallic, 0.0), s(K::CoatOpacity, 1.0), s(K::CoatRoughness, 0.03)], light: key, env: 1.0, displacement: None }),
        ("sheen", Look { channels: vec![c(K::BaseColor, [0.06, 0.02, 0.12]), s(K::Roughness, 0.9), c(K::SheenColor, [0.85, 0.7, 1.0]), s(K::SheenOpacity, 1.0), s(K::SheenRoughness, 0.35)], light: key, env: 1.0, displacement: None }),
        ("anisotropy", Look { channels: vec![c(K::BaseColor, [0.62, 0.6, 0.58]), s(K::Roughness, 0.35), s(K::Metallic, 1.0), s(K::AnisotropyLevel, 0.8), s(K::AnisotropyAngle, 0.0)], light: key, env: 1.0, displacement: None }),
        ("subsurface", Look { channels: vec![c(K::BaseColor, [0.85, 0.62, 0.5]), s(K::Roughness, 0.5), s(K::Scattering, 1.0), c(K::ScatteringColor, [1.0, 0.3, 0.15])], light: Vec3::new(1.0, 0.3, 0.1).normalize(), env: 0.35, displacement: None }),
        ("translucency", Look { channels: vec![c(K::BaseColor, [0.25, 0.65, 0.12]), s(K::Roughness, 0.5), s(K::Translucency, 0.8)], light: Vec3::new(-0.3, 0.4, -1.0).normalize(), env: 0.35, displacement: None }),
        ("transmission", Look { channels: vec![c(K::BaseColor, [0.92, 0.97, 1.0]), s(K::Roughness, 0.03), c(K::Transmissive, [1.0, 1.0, 1.0]), s(K::Ior, 0.5)], light: key, env: 1.0, displacement: None }),
        ("displacement", Look {
            channels: vec![
                c(K::BaseColor, [0.7, 0.66, 0.6]),
                s(K::Roughness, 0.45),
                pattern(K::Height, |u, v| ((u * 12.0 * std::f32::consts::TAU).sin() * (v * 6.0 * std::f32::consts::TAU).sin()).max(0.0)),
            ],
            light: key,
            env: 1.0,
            displacement: Some(4),
        }),
    ];
    let cols = 4;
    let rows = looks.len().div_ceil(cols) as u32;
    let mut sheet = vec![0.0f32; (CELL * cols as u32 * CELL * rows * 4) as usize];
    for (i, (name, look)) in looks.iter().enumerate() {
        let mut r = ViewportRenderer::new(&device, &queue);
        r.set_mesh(&device, &primitives::uv_sphere(1.0, 96, 48));
        for (k, img) in &look.channels {
            r.upload_channel(&device, &queue, 0, *k, img, None);
        }
        let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 0.15, 3.6), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(38f32.to_radians(), 1.0, 0.1, 20.0));
        p.light_dir = look.light;
        p.environment_intensity = look.env;
        p.environment.background_opacity = 1.0;
        p.environment.background_blur = 0.6;
        p.height_scale = 0.06;
        p.displacement = look.displacement;
        let t0 = std::time::Instant::now();
        let px = r.render(&device, &queue, [CELL, CELL], &p).read_rgba8(&device, &queue);
        eprintln!("{name}: {:.0} ms", t0.elapsed().as_secs_f32() * 1000.0);
        let (cx, cy) = ((i % cols) as u32 * CELL, (i / cols) as u32 * CELL);
        for y in 0..CELL {
            for x in 0..CELL {
                let src = ((y * CELL + x) * 4) as usize;
                let dst = (((cy + y) * CELL * cols as u32 + cx + x) * 4) as usize;
                for k in 0..4 {
                    sheet[dst + k] = px[src + k] as f32 / 255.0;
                }
            }
        }
    }
    subcraft_image::save_image(std::path::Path::new(&out), CELL * cols as u32, CELL * rows, 4, &sheet, subcraft_image::FileFormat::Png8).expect("write png");
    eprintln!("wrote {out}");
}
