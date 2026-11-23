//! Coat, sheen, anisotropy, subsurface scattering, translucency and transmission in the viewport
//! shader (the extended channels packed by `extended.rs`). Skips (passes with a message) when no
//! adapter is available.
//!
//! Set `SUBCRAFT_RENDER_OUT=<dir>` to also write every render as a PNG.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::wgpu_api::{Device, Queue};
use subcraft_render::{EXTENDED_CHANNELS, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 128;
const TEX: u32 = 128;

fn gpu_test() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn solid(kind: ChannelKind, v: [f32; 4]) -> TiledImage {
    TiledImage::new(TEX, TEX, if kind.components() == 3 { 4 } else { 1 }, v)
}

fn scalar(kind: ChannelKind, v: f32) -> (ChannelKind, TiledImage) {
    (kind, solid(kind, [v; 4]))
}

fn color(kind: ChannelKind, c: [f32; 3]) -> (ChannelKind, TiledImage) {
    (kind, solid(kind, [c[0], c[1], c[2], 1.0]))
}

/// A sphere seen from the front under the default key light and studio environment.
fn renderer(device: &Device, queue: &Queue) -> (ViewportRenderer, ViewParams) {
    let mut r = ViewportRenderer::new(device, queue);
    r.set_mesh(device, &primitives::uv_sphere(1.0, 64, 32));
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.4), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 20.0));
    p.antialias = false;
    (r, p)
}

/// Render the sphere with a base material and `extra` channels on top; returns RGBA8 pixels.
fn shot(device: &Device, queue: &Queue, base: &[(ChannelKind, TiledImage)], extra: &[(ChannelKind, TiledImage)], tweak: impl Fn(&mut ViewParams), name: &str) -> Vec<u8> {
    let (mut r, mut p) = renderer(device, queue);
    tweak(&mut p);
    for (k, img) in base.iter().chain(extra) {
        r.upload_channel(device, queue, 0, *k, img, None);
    }
    let px = r.render(device, queue, [SIZE, SIZE], &p).read_rgba8(device, queue);
    save(&px, SIZE, name);
    px
}

fn save(px: &[u8], size: u32, name: &str) {
    let Ok(dir) = std::env::var("SUBCRAFT_RENDER_OUT") else { return };
    let data: Vec<f32> = px.iter().map(|&b| b as f32 / 255.0).collect();
    let path = std::path::Path::new(&dir).join(format!("{name}.png"));
    subcraft_image::save_image(&path, size, size, 4, &data, subcraft_image::FileFormat::Png8).expect("write png");
}

fn base_material(c: [f32; 3], rough: f32, metal: f32) -> Vec<(ChannelKind, TiledImage)> {
    vec![color(ChannelKind::BaseColor, c), scalar(ChannelKind::Roughness, rough), scalar(ChannelKind::Metallic, metal)]
}

fn px(img: &[u8], x: u32, y: u32) -> [f32; 3] {
    let i = ((y * SIZE + x) * 4) as usize;
    [img[i] as f32, img[i + 1] as f32, img[i + 2] as f32]
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Pixels whose RGB differs by more than `tol` in any component.
fn changed(a: &[u8], b: &[u8], tol: u8) -> usize {
    a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(x, y)| x[..3].iter().zip(&y[..3]).any(|(p, q)| p.abs_diff(*q) > tol)).count()
}

fn max_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter().zip(b).map(|(p, q)| p.abs_diff(*q)).max().unwrap_or(0)
}

/// Sum of a function over the pixels covered by the sphere (anything not the background).
fn over_sphere(img: &[u8], background: &[u8], f: impl Fn(u32, u32, [f32; 3]) -> f32) -> f32 {
    let mut s = 0.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            if px(img, x, y) != px(background, x, y) || px(background, x, y) != px(background, 0, 0) {
                s += f(x, y, px(img, x, y));
            }
        }
    }
    s
}

#[test]
fn extended_channels_at_their_defaults_change_nothing() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let base = base_material([0.6, 0.3, 0.2], 0.35, 0.0);
    let plain = shot(&device, &queue, &base, &[], |_| {}, "defaults_plain");
    let defaults: Vec<_> = EXTENDED_CHANNELS.iter().map(|&k| {
        let d = k.default_value().0;
        (k, solid(k, [d[0], d[1], d[2], 1.0]))
    }).collect();
    let all = shot(&device, &queue, &base, &defaults, |_| {}, "defaults_all");
    // Every extended effect is at zero strength by default (coat / sheen opacity 0, anisotropy 0,
    // no scattering, translucency or transmission): the image matches up to rounding.
    assert!(max_diff(&plain, &all) <= 2, "max difference {}", max_diff(&plain, &all));
}

#[test]
fn clear_coat_adds_a_sharp_highlight_over_a_rough_base() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let base = base_material([0.5, 0.05, 0.05], 0.8, 0.0);
    let plain = shot(&device, &queue, &base, &[], |_| {}, "coat_plain");
    let coated = shot(&device, &queue, &base, &[scalar(ChannelKind::CoatOpacity, 1.0), scalar(ChannelKind::CoatRoughness, 0.05)], |_| {}, "coat_on");
    let peak = |img: &[u8]| (0..SIZE * SIZE).map(|i| luma(px(img, i % SIZE, i / SIZE))).fold(0.0, f32::max);
    assert!(peak(&coated) > peak(&plain) + 40.0, "coat highlight {} vs rough base {}", peak(&coated), peak(&plain));
    assert!(changed(&plain, &coated, 6) > 1500, "the coat reflects the studio over the whole sphere");
    // A coat with opacity 0 is no coat.
    let off = shot(&device, &queue, &base, &[scalar(ChannelKind::CoatOpacity, 0.0), scalar(ChannelKind::CoatRoughness, 0.05)], |_| {}, "coat_off");
    assert!(max_diff(&plain, &off) <= 2);
    // A coloured coat tints the base under it: a blue coat over the red base drops its red.
    let tinted = shot(&device, &queue, &base, &[scalar(ChannelKind::CoatOpacity, 1.0), color(ChannelKind::CoatColor, [0.2, 0.4, 1.0])], |_| {}, "coat_tinted");
    let red = |img: &[u8]| over_sphere(img, &plain, |_, _, c| c[0]);
    assert!(red(&tinted) < red(&coated) * 0.8, "tinted coat red {} vs clear coat {}", red(&tinted), red(&coated));
}

#[test]
fn sheen_brightens_grazing_angles_most() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let base = base_material([0.05, 0.05, 0.15], 0.9, 0.0);
    let plain = shot(&device, &queue, &base, &[], |_| {}, "sheen_plain");
    let velvet = shot(&device, &queue, &base, &[scalar(ChannelKind::SheenOpacity, 1.0), color(ChannelKind::SheenColor, [1.0, 1.0, 1.0]), scalar(ChannelKind::SheenRoughness, 0.4)], |_| {}, "sheen_on");
    // Gain at the rim (outer ring of the disc) vs at the centre.
    let c = SIZE as f32 / 2.0;
    let radius = 0.335 * SIZE as f32; // projected radius of the unit sphere at this camera
    let gain = |lo: f32, hi: f32| {
        let mut sum = 0.0;
        let mut n: f32 = 0.0;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt() / radius;
                if d >= lo && d < hi {
                    sum += luma(px(&velvet, x, y)) - luma(px(&plain, x, y));
                    n += 1.0;
                }
            }
        }
        sum / n.max(1.0)
    };
    let (centre, rim) = (gain(0.0, 0.4), gain(0.8, 0.95));
    assert!(rim > 8.0 && rim > centre * 1.5, "sheen gain at the rim {rim} vs centre {centre}");
}

#[test]
fn anisotropy_reshapes_the_highlight_and_its_angle_rotates_it() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let base = base_material([0.9, 0.9, 0.9], 0.3, 1.0);
    let plain = shot(&device, &queue, &base, &[], |_| {}, "aniso_plain");
    let a0 = shot(&device, &queue, &base, &[scalar(ChannelKind::AnisotropyLevel, 0.9), scalar(ChannelKind::AnisotropyAngle, 0.0)], |_| {}, "aniso_0");
    let a90 = shot(&device, &queue, &base, &[scalar(ChannelKind::AnisotropyLevel, 0.9), scalar(ChannelKind::AnisotropyAngle, 0.25)], |_| {}, "aniso_90");
    assert!(changed(&plain, &a0, 8) > 800, "anisotropy changes the metal: {}", changed(&plain, &a0, 8));
    assert!(changed(&a0, &a90, 8) > 800, "a quarter turn of the angle changes the streaks: {}", changed(&a0, &a90, 8));
    // The angle has a period of half a turn: 0.5 looks like 0.
    let a180 = shot(&device, &queue, &base, &[scalar(ChannelKind::AnisotropyLevel, 0.9), scalar(ChannelKind::AnisotropyAngle, 0.5)], |_| {}, "aniso_180");
    assert!(max_diff(&a0, &a180) <= 3, "angle 0 vs half a turn: {}", max_diff(&a0, &a180));
}

#[test]
fn subsurface_scattering_carries_its_colour_past_the_terminator() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let base = base_material([0.8, 0.7, 0.6], 0.5, 0.0);
    // Key light from the side, little environment: the terminator crosses the sphere.
    let side = |p: &mut ViewParams| {
        p.light_dir = Vec3::new(1.0, 0.1, 0.2).normalize();
        p.environment_intensity = 0.15;
    };
    let plain = shot(&device, &queue, &base, &[], side, "sss_plain");
    let skin = shot(&device, &queue, &base, &[scalar(ChannelKind::Scattering, 1.0), color(ChannelKind::ScatteringColor, [1.0, 0.25, 0.1])], side, "sss_on");
    // Around the terminator (x near the centre) red rises more than blue.
    let band = |img: &[u8]| over_sphere(img, &plain, |x, _, c| if (x as i32 - SIZE as i32 / 2).abs() < 14 { c[0] - c[2] } else { 0.0 });
    assert!(band(&skin) > band(&plain) * 1.15 + 50.0, "red bleed at the terminator {} vs {}", band(&skin), band(&plain));
}

#[test]
fn translucency_lets_light_from_behind_through() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let base = base_material([0.3, 0.8, 0.2], 0.5, 0.0);
    let back = |p: &mut ViewParams| {
        p.light_dir = Vec3::new(0.0, 0.2, -1.0).normalize();
        p.light_intensity = 3.0;
        p.environment_intensity = 0.1;
    };
    let plain = shot(&device, &queue, &base, &[], back, "transl_plain");
    let leaf = shot(&device, &queue, &base, &[scalar(ChannelKind::Translucency, 1.0)], back, "transl_on");
    let sum = |img: &[u8]| over_sphere(img, &plain, |_, _, c| luma(c));
    assert!(sum(&leaf) > sum(&plain) * 1.3, "back-lit sphere {} vs opaque {}", sum(&leaf), sum(&plain));
}

#[test]
fn transmission_shows_the_refracted_environment() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let base = base_material([1.0, 1.0, 1.0], 0.02, 0.0);
    let plain = shot(&device, &queue, &base, &[], |_| {}, "trans_plain");
    let glass = shot(&device, &queue, &base, &[color(ChannelKind::Transmissive, [1.0, 1.0, 1.0]), scalar(ChannelKind::Ior, 0.5)], |_| {}, "trans_glass");
    assert!(changed(&plain, &glass, 10) > 2000, "glass differs from white plastic: {}", changed(&plain, &glass, 10));
    // The IOR bends what is seen through it.
    let water = shot(&device, &queue, &base, &[color(ChannelKind::Transmissive, [1.0, 1.0, 1.0]), scalar(ChannelKind::Ior, 0.0)], |_| {}, "trans_ior1");
    assert!(changed(&glass, &water, 10) > 500, "IOR 1.5 vs 1.0: {}", changed(&glass, &water, 10));
}

#[test]
fn incremental_coat_updates_match_a_full_upload() {
    let _gpu = gpu_test();
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let base = base_material([0.5, 0.05, 0.05], 0.8, 0.0);
    // Coat opacity 1 on the left half of the texture.
    let mut coat = TiledImage::new(TEX, TEX, 1, [0.0; 4]);
    for y in 0..TEX {
        for x in 0..TEX / 2 {
            coat.set(x, y, [1.0, 0.0, 0.0, 0.0]);
        }
    }
    let (mut r, p) = renderer(&device, &queue);
    for (k, img) in &base {
        r.upload_channel(&device, &queue, 0, *k, img, None);
    }
    r.upload_channel(&device, &queue, 0, ChannelKind::CoatOpacity, &coat, None);
    let _ = r.render(&device, &queue, [SIZE, SIZE], &p);
    // Then one tile in the right half gets coat too, uploaded alone.
    let tile = coat.tile_index((TEX - 1) / subcraft_image::TILE_SIZE, (TEX - 1) / subcraft_image::TILE_SIZE);
    let rect = coat.tile_rect(tile);
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            coat.set(x as u32, y as u32, [1.0, 0.0, 0.0, 0.0]);
        }
    }
    r.upload_channel(&device, &queue, 0, ChannelKind::CoatOpacity, &coat, Some(&[tile]));
    let incremental = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let full = shot(&device, &queue, &base, &[(ChannelKind::CoatOpacity, coat)], |_| {}, "coat_incremental");
    assert_eq!(max_diff(&incremental, &full), 0, "a one-tile repack equals packing everything");
}
