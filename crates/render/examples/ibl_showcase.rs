//! Image-based lighting contact sheet: a white-ish metal row and a red plastic row from roughness
//! 0 to 1 under a bundled HDRI, then the same spheres with a cavity AO pattern, then a sphere on a
//! floor with the key light's soft shadow. Needs a GPU adapter (lavapipe works).
//!
//! `cargo run --release -p subcraft-render --example ibl_showcase -- out.png ["Paint Booth"]`

use glam::{Mat4, Vec3};
use subcraft_app::assets::Library;
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Mesh, primitives};
use subcraft_render::{
    HdriUpload, ShadowParams, ShadowQuality, ViewParams, ViewportRenderer, headless,
};

const CELL: u32 = 220;
const COLS: u32 = 5;
const TEX: u32 = 128;

fn solid(comps: usize, v: [f32; 4]) -> TiledImage {
    TiledImage::new(TEX, TEX, comps, v)
}

/// Bands of AO 0.25 across the sphere (a crude cavity map).
fn ao_bands() -> TiledImage {
    let mut img = TiledImage::new(TEX, TEX, 1, [1.0; 4]);
    for y in 0..TEX {
        for x in 0..TEX {
            if (y / 8) % 3 == 0 {
                img.set(x, y, [0.25, 0.0, 0.0, 0.0]);
            }
        }
    }
    img
}

fn sphere_on_floor() -> Mesh {
    let floor =
        primitives::plane(6.0, 1).transformed(&Mat4::from_translation(Vec3::new(0.0, -0.8, 0.0)));
    Mesh::merged(&[&primitives::uv_sphere(0.8, 64, 32), &floor])
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ibl_showcase.png".into());
    let env_name = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "Library Hall".into());
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no GPU adapter");
        std::process::exit(1);
    };
    let lib = Library::builtin();
    let env = lib
        .environments
        .iter()
        .find(|e| e.name == env_name)
        .expect("bundled environment");
    let t0 = std::time::Instant::now();
    let hdri = env.source.hdri().expect("an HDRI environment");
    eprintln!(
        "prefilter {}: {} levels from {} px, {:.0} ms",
        env.name,
        hdri.levels.len(),
        hdri.levels[0].width,
        t0.elapsed().as_secs_f32() * 1000.0
    );
    let levels: Vec<(u32, u32, &[f32])> = hdri
        .levels
        .iter()
        .map(|l| (l.width, l.height, l.rgba.as_slice()))
        .collect();

    let rows = 3u32;
    let (sw, sh) = (CELL * COLS, CELL * rows + CELL * 2);
    let mut sheet = vec![0.0f32; (sw * sh * 4) as usize];
    let mut blit = |px: &[u8], w: u32, h: u32, ox: u32, oy: u32| {
        for y in 0..h {
            for x in 0..w {
                let src = ((y * w + x) * 4) as usize;
                let dst = (((oy + y) * sw + ox + x) * 4) as usize;
                for k in 0..4 {
                    sheet[dst + k] = px[src + k] as f32 / 255.0;
                }
            }
        }
    };
    let view = |eye: Vec3, at: Vec3, fov: f32, aspect: f32| {
        let mut p = ViewParams::new(
            Mat4::look_at_rh(eye, at, Vec3::Y),
            Mat4::perspective_rh(fov.to_radians(), aspect, 0.1, 30.0),
        );
        p.environment.use_hdri = true;
        p.environment.background_opacity = 1.0;
        p.environment.background_blur = 0.5;
        p.light_intensity = 0.0;
        p
    };
    for row in 0..rows {
        for col in 0..COLS {
            let rough = col as f32 / (COLS - 1) as f32;
            let mut r = ViewportRenderer::new(&device, &queue);
            r.set_hdri(
                &device,
                &queue,
                Some(HdriUpload {
                    levels: &levels,
                    sh: hdri.sh,
                }),
            );
            r.set_mesh(&device, &primitives::uv_sphere(1.0, 96, 48));
            let (base, metal) = if row == 1 {
                ([0.6, 0.06, 0.04, 1.0], 0.0)
            } else {
                ([0.95, 0.93, 0.88, 1.0], 1.0)
            };
            r.upload_channel(
                &device,
                &queue,
                0,
                ChannelKind::BaseColor,
                &solid(4, base),
                None,
            );
            r.upload_channel(
                &device,
                &queue,
                0,
                ChannelKind::Metallic,
                &solid(1, [metal; 4]),
                None,
            );
            r.upload_channel(
                &device,
                &queue,
                0,
                ChannelKind::Roughness,
                &solid(1, [rough; 4]),
                None,
            );
            if row == 2 {
                r.upload_channel(
                    &device,
                    &queue,
                    0,
                    ChannelKind::AmbientOcclusion,
                    &ao_bands(),
                    None,
                );
            }
            let p = view(Vec3::new(0.0, 0.2, 3.4), Vec3::ZERO, 40.0, 1.0);
            let px = r
                .render(&device, &queue, [CELL, CELL], &p)
                .read_rgba8(&device, &queue);
            blit(&px, CELL, CELL, col * CELL, row * CELL);
        }
    }
    // Soft shadow of the key light on a floor, close enough to see the penumbra.
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_hdri(
        &device,
        &queue,
        Some(HdriUpload {
            levels: &levels,
            sh: hdri.sh,
        }),
    );
    r.set_mesh(&device, &sphere_on_floor());
    for slot in 0..2 {
        r.upload_channel(
            &device,
            &queue,
            slot,
            ChannelKind::BaseColor,
            &solid(4, [0.7, 0.7, 0.7, 1.0]),
            None,
        );
        r.upload_channel(
            &device,
            &queue,
            slot,
            ChannelKind::Roughness,
            &solid(1, [0.6; 4]),
            None,
        );
    }
    let (w, h) = (CELL * COLS, CELL * 2);
    let mut p = view(
        Vec3::new(0.0, 1.6, 3.6),
        Vec3::new(0.0, -0.4, 0.0),
        35.0,
        w as f32 / h as f32,
    );
    p.light_dir = Vec3::new(0.6, 0.8, 0.3).normalize();
    p.light_intensity = 3.0;
    p.environment_intensity = 0.35;
    p.shadows = ShadowParams {
        enabled: true,
        opacity: 1.0,
        quality: ShadowQuality::Average,
    };
    let px = r
        .render(&device, &queue, [w, h], &p)
        .read_rgba8(&device, &queue);
    blit(&px, w, h, 0, rows * CELL);
    subcraft_image::save_image(
        std::path::Path::new(&out),
        sw,
        sh,
        4,
        &sheet,
        subcraft_image::FileFormat::Png8,
    )
    .expect("write png");
    eprintln!("wrote {out}");
}
