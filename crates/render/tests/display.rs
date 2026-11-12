//! Numeric GPU checks for the Display Settings: the environment drawn behind the model, the tone
//! functions and the lens and color stage (checked against the CPU reference in `grade`).
use glam::{Mat4, Vec3};
use subcraft_render::grade::{self, Grade};
use subcraft_render::{HdriUpload, PostParams, ToneMap, ViewParams, ViewportRenderer, headless};

const N: u32 = 64;

fn srgb(linear: f32) -> f32 {
    let v = linear.clamp(0.0, 1.0);
    255.0 * if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn pixel(image: &[u8], x: u32, y: u32) -> [f32; 3] {
    let i = ((y * N + x) * 4) as usize;
    [image[i] as f32, image[i + 1] as f32, image[i + 2] as f32]
}

fn assert_color(got: [f32; 3], linear: [f32; 3], what: &str) {
    let want = linear.map(srgb);
    for c in 0..3 {
        assert!((got[c] - want[c]).abs() <= 2.0, "{what}: got {got:?}, expected {want:?}");
    }
}

const LEFT: [f32; 3] = [0.60, 0.10, 0.05];
const RIGHT: [f32; 3] = [0.05, 0.20, 0.70];
const BLURRED: [f32; 3] = [0.30, 0.30, 0.30];
const BACKGROUND: [f32; 3] = [0.10, 0.02, 0.40];

type Scene = (subcraft_render::wgpu_api::Device, subcraft_render::wgpu_api::Queue, ViewportRenderer, ViewParams);

fn scene() -> Option<Scene> {
    scene_with(LEFT, RIGHT)
}

/// An environment whose two halves have different colors (the seam faces +X) and whose blurred
/// level is one flat gray, with a camera at the origin looking along +X (screen right is +Z).
fn scene_with(left: [f32; 3], right: [f32; 3]) -> Option<Scene> {
    let (_, device, queue) = headless::device()?;
    let mut renderer = ViewportRenderer::new(&device, &queue);
    let (w, h) = (64u32, 32u32);
    let mut sharp = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..h {
        for x in 0..w {
            let c = if x < w / 2 { left } else { right };
            sharp.extend([c[0], c[1], c[2], 1.0]);
        }
    }
    let soft: Vec<f32> = (0..(w / 2) * (h / 2)).flat_map(|_| [BLURRED[0], BLURRED[1], BLURRED[2], 1.0]).collect();
    let levels = [(w, h, sharp.as_slice()), (w / 2, h / 2, soft.as_slice())];
    renderer.set_hdri(&device, &queue, Some(HdriUpload { levels: &levels, sh: [[0.0; 3]; 9] }));
    let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Y);
    let mut params = ViewParams::new(view, Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    params.antialias = false;
    params.background = BACKGROUND;
    params.environment.use_hdri = true;
    // Linear tone mapping: the expected pixel is the environment value itself.
    params.post = PostParams { tone_map: ToneMap::Linear, emission_glow: 0.0, ..PostParams::default() };
    Some((device, queue, renderer, params))
}

#[test]
fn the_environment_shows_behind_the_model_by_its_opacity() {
    let Some((device, queue, mut renderer, mut params)) = scene() else { return };
    let mut frame = |p: &ViewParams| renderer.render(&device, &queue, [N, N], p).read_rgba8(&device, &queue);
    // Opacity 0: the plain background color, exactly as before the setting existed.
    let plain = frame(&params);
    assert_color(pixel(&plain, 8, 32), BACKGROUND, "no environment at opacity 0");
    assert_color(pixel(&plain, 56, 32), BACKGROUND, "no environment at opacity 0");
    // Opacity 1: each half of the view shows its half of the map (u < 0.5 is -Z, screen left).
    params.environment.background_opacity = 1.0;
    let full = frame(&params);
    assert_color(pixel(&full, 8, 32), LEFT, "left of the seam");
    assert_color(pixel(&full, 56, 32), RIGHT, "right of the seam");
    // Half opacity mixes the map over the background color in linear light.
    params.environment.background_opacity = 0.5;
    let half = frame(&params);
    let mix = |a: [f32; 3], b: [f32; 3]| [0, 1, 2].map(|i| 0.5 * (a[i] + b[i]));
    assert_color(pixel(&half, 8, 32), mix(BACKGROUND, LEFT), "half opacity, left");
    assert_color(pixel(&half, 56, 32), mix(BACKGROUND, RIGHT), "half opacity, right");
}

#[test]
fn exposure_rotation_and_blur_change_the_backdrop_as_documented() {
    let Some((device, queue, mut renderer, mut params)) = scene() else { return };
    params.environment.background_opacity = 1.0;
    let mut frame = |p: &ViewParams| renderer.render(&device, &queue, [N, N], p).read_rgba8(&device, &queue);
    // +1 EV is a multiplier of 2 on the environment (0.6 * 2 clips at 1 under the linear curve).
    params.environment.hdri_intensity = 2.0;
    let bright = frame(&params);
    assert_color(pixel(&bright, 8, 32), LEFT.map(|c| c * 2.0), "+1 EV doubles the light");
    assert_color(pixel(&bright, 56, 32), RIGHT.map(|c| c * 2.0), "+1 EV doubles the light");
    params.environment.hdri_intensity = 1.0;
    // Half a turn swaps the two halves.
    params.environment.rotation = std::f32::consts::PI;
    let turned = frame(&params);
    assert_color(pixel(&turned, 8, 32), RIGHT, "rotated half a turn, left");
    assert_color(pixel(&turned, 56, 32), LEFT, "rotated half a turn, right");
    params.environment.rotation = 0.0;
    // Full blur reads the most diffused level of the map; half blur is halfway between the levels.
    params.environment.background_blur = 1.0;
    let soft = frame(&params);
    assert_color(pixel(&soft, 8, 32), BLURRED, "fully blurred");
    assert_color(pixel(&soft, 56, 32), BLURRED, "fully blurred");
    params.environment.background_blur = 0.5;
    let between = frame(&params);
    assert_color(pixel(&between, 8, 32), [0, 1, 2].map(|i| 0.5 * (LEFT[i] + BLURRED[i])), "half blurred");
    // The view-wide exposure applies to the backdrop like it does to the model.
    params.environment.background_blur = 0.0;
    params.exposure = 0.5;
    let dim = frame(&params);
    assert_color(pixel(&dim, 56, 32), RIGHT.map(|c| c * 0.5), "view exposure 0.5");
}

#[test]
fn every_tone_function_matches_the_reference() {
    let Some((device, queue, mut renderer, mut params)) = scene() else { return };
    params.environment.background_opacity = 1.0;
    // Three times the map: values from 0.15 to 2.1, well into the range the curves compress.
    params.environment.hdri_intensity = 3.0;
    for function in ToneMap::ALL {
        for factor in [1.0, 2.5] {
            params.post = PostParams { tone_map: function, mapping_factor: factor, emission_glow: 0.0, ..PostParams::default() };
            let frame = renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue);
            for (x, value) in [(8, LEFT), (56, RIGHT)] {
                let want = grade::tone_map(function, value.map(|c| c * 3.0), factor);
                assert_color(pixel(&frame, x, 32), want, &format!("{function:?} with factor {factor}"));
            }
        }
    }
}

/// Largest difference between two images, in 8-bit levels, and where.
fn worst(a: &[u8], b: &[u8]) -> (i32, usize) {
    a.iter().zip(b).enumerate().map(|(i, (x, y))| ((*x as i32 - *y as i32).abs(), i)).max().unwrap()
}

#[test]
fn the_lens_and_color_stage_matches_the_cpu_reference() {
    let Some((device, queue, mut renderer, mut params)) = scene() else { return };
    // A backdrop with an edge and a gradient (half blurred), so every term has something to do.
    params.environment.background_opacity = 0.85;
    params.environment.background_blur = 0.35;
    let mut frame = |grade: Grade| {
        params.post = PostParams { tone_map: ToneMap::Linear, emission_glow: 0.0, grade, ..PostParams::default() };
        renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue)
    };
    let plain = frame(Grade::default());
    assert_ne!(pixel(&plain, 8, 32), pixel(&plain, 56, 32), "the test picture has two sides");
    let color = Grade { color_correction: true, saturation: 0.4, contrast: 1.3, brightness: 0.9, bias: 0.05, sepia: 0.3, temperature: 4500.0, ..Grade::default() };
    let fringe = Grade { aberration: 2.0, aberration_radius: 0.1, aberration_softness: 0.05, aberration_center: [0.2, 0.5], ..Grade::default() };
    let cases = [
        ("vignette", Grade { vignette: 0.7, ..Grade::default() }),
        ("color correction", color),
        ("saturation 0", Grade { color_correction: true, saturation: 0.0, ..Grade::default() }),
        ("cool white balance", Grade { color_correction: true, temperature: 3200.0, ..Grade::default() }),
        ("warm white balance", Grade { color_correction: true, temperature: 11000.0, ..Grade::default() }),
        ("gamma", Grade { gamma: 2.2, ..Grade::default() }),
        ("lateral aberration", fringe),
        ("oval aberration", Grade { aberration_ratio: 2.5, ..fringe }),
        ("achromatic aberration", Grade { aberration_achromatic: true, ..fringe }),
        ("sharpen", Grade { sharpen: 1.5, sharpen_diameter: 3.0, ..Grade::default() }),
        ("everything", Grade { vignette: 0.5, sharpen: 0.8, sharpen_diameter: 2.0, gamma: 1.4, ..Grade { aberration: 1.5, ..color } }),
    ];
    for (name, grade) in cases {
        let gpu = frame(grade);
        let reference = grade.apply(&plain, [N, N]);
        assert_ne!(gpu, plain, "{name} changes the picture");
        let (diff, at) = worst(&gpu, &reference);
        let (x, y) = ((at / 4) as u32 % N, (at / 4) as u32 / N);
        assert!(diff <= 3, "{name}: GPU and reference differ by {diff} levels at ({x}, {y}): {:?} vs {:?}", pixel(&gpu, x, y), pixel(&reference, x, y));
    }
    // The split is visible where it should be: right of the seam red comes from further out
    // (the right side) and blue from nearer the centre (the left side).
    let split = frame(fringe);
    let (before, after) = (pixel(&plain, 32, 32), pixel(&split, 32, 32));
    assert!((after[1] - before[1]).abs() <= 1.0 && (after[0] - before[0]).abs() + (after[2] - before[2]).abs() > 6.0, "green stays, red and blue move: {before:?} -> {after:?}");
}

#[test]
fn film_grain_is_noise_of_the_asked_strength_size_and_kind() {
    let Some((device, queue, mut renderer, mut params)) = scene() else { return };
    params.background = [0.2; 3];
    let mut frame = |grade: Grade| {
        params.post = PostParams { tone_map: ToneMap::Linear, emission_glow: 0.0, grade, ..PostParams::default() };
        renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue)
    };
    let linear = |image: &[u8], c: usize| -> Vec<f32> { image.chunks_exact(4).map(|p| grade::srgb_to_linear(p[c])).collect() };
    let stats = |v: &[f32]| {
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        (mean, (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt())
    };
    let mono = frame(Grade { grain: 0.2, ..Grade::default() });
    let (mean, deviation) = stats(&linear(&mono, 0));
    // Uniform noise of width 0.2 around the flat 0.2: standard deviation 0.2 / sqrt(12) = 0.058.
    assert!((mean - 0.2).abs() < 0.01 && (deviation - 0.0577).abs() < 0.012, "mean {mean}, deviation {deviation}");
    assert!(mono.chunks_exact(4).all(|p| p[0] == p[1] && p[1] == p[2]), "monochromatic grain is the same on every channel");
    let colored = frame(Grade { grain: 0.2, grain_colored: true, ..Grade::default() });
    let differing = colored.chunks_exact(4).filter(|p| p[0] != p[1] || p[1] != p[2]).count();
    assert!(differing > (N * N) as usize * 9 / 10, "colored grain differs per channel on almost every pixel: {differing}");
    // Twice the strength, twice the deviation; a radius of 4 px makes 4 x 4 blocks.
    let strong = stats(&linear(&frame(Grade { grain: 0.1, ..Grade::default() }), 0)).1;
    assert!((deviation / strong - 2.0).abs() < 0.25, "{deviation} vs {strong}");
    let coarse = frame(Grade { grain: 0.2, grain_radius: 4.0, ..Grade::default() });
    for (bx, by) in [(0, 0), (5, 9), (15, 3)] {
        let first = pixel(&coarse, bx * 4, by * 4);
        for (dx, dy) in [(1, 0), (3, 3), (0, 2)] {
            assert_eq!(pixel(&coarse, bx * 4 + dx, by * 4 + dy), first, "one grain covers its 4 x 4 block");
        }
    }
    let blocks: std::collections::HashSet<u8> = (0..N / 4).map(|b| coarse[(b * 4 * 4) as usize]).collect();
    assert!(blocks.len() > 8, "neighbouring grains differ");
}

fn luma_at(image: &[u8], x: u32, y: u32) -> f32 {
    let p = pixel(image, x, y);
    (p[0] + p[1] + p[2]) / 3.0
}

#[test]
fn bloom_falloff_widens_the_glow_and_lens_flare_mirrors_bright_areas() {
    // Dark on the left, white on the right.
    let Some((device, queue, mut renderer, mut params)) = scene_with([0.01; 3], [1.0; 3]) else { return };
    params.environment.background_opacity = 1.0;
    let mut frame = |post: PostParams| {
        params.post = PostParams { tone_map: ToneMap::Linear, emission_glow: 0.0, ..post };
        renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue)
    };
    let plain = frame(PostParams::default());
    let tight = frame(PostParams { bloom: 1.0, bloom_threshold: 0.5, bloom_falloff: 0.0, ..PostParams::default() });
    let wide = frame(PostParams { bloom: 1.0, bloom_threshold: 0.5, bloom_falloff: 1.0, ..PostParams::default() });
    let original = frame(PostParams { bloom: 1.0, bloom_threshold: 0.5, ..PostParams::default() });
    // Next to the edge both glow; 14 px into the dark only the wide falloff still reaches.
    assert!(luma_at(&tight, 30, 32) > luma_at(&plain, 30, 32) + 3.0 && luma_at(&wide, 30, 32) > luma_at(&plain, 30, 32) + 3.0);
    let far = |image: &[u8]| luma_at(image, 18, 32) - luma_at(&plain, 18, 32);
    assert!(far(&wide) > far(&tight) + 2.0, "falloff 1 reaches further than falloff 0: {} vs {}", far(&wide), far(&tight));
    assert!(far(&original) >= far(&tight) && far(&original) <= far(&wide), "the default falloff sits between");
    // Lens flare with two ghosts: the white half shows up mirrored through the centre on the dark half.
    let flare = frame(PostParams { flare: 1.0, flare_threshold: 0.5, flare_ghosts: 2, flare_scale: 1.0, ..PostParams::default() });
    let gain = |x: u32| luma_at(&flare, x, 32) - luma_at(&plain, x, 32);
    assert!(gain(19) > 8.0, "a ghost of the bright side on the dark side: {}", gain(19));
    assert!(gain(45) < 1.5, "nothing dark is mirrored onto the bright side: {}", gain(45));
    let off = frame(PostParams { flare: 0.0, flare_ghosts: 2, ..PostParams::default() });
    assert_eq!(off, plain, "amount 0 is off");
    // A telephoto-like spacing keeps the ghosts near the centre: the far left stays dark.
    let near = frame(PostParams { flare: 1.0, flare_threshold: 0.5, flare_ghosts: 2, flare_scale: 0.25, ..PostParams::default() });
    assert!(luma_at(&near, 6, 32) - luma_at(&plain, 6, 32) < gain(6).max(1.0), "smaller spacing, less reach");
}

// ----- viewport visualisation: single-channel display, texture filtering, the grid ---------------

use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_render::{ChannelComponent, ChannelDisplay, GridParams};

/// A cube seen from the front with flat channel values, lit only enough to tell lit from unlit.
fn cube_scene() -> Option<Scene> {
    let (_, device, queue) = headless::device()?;
    let mut renderer = ViewportRenderer::new(&device, &queue);
    let mesh = subcraft_mesh::primitives::cube(2.0);
    renderer.set_mesh(&device, &mesh);
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::Y);
    let mut params = ViewParams::new(view, Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    params.antialias = false;
    params.post = PostParams { emission_glow: 0.0, ..PostParams::default() };
    Some((device, queue, renderer, params))
}

fn stored(value: f32) -> f32 {
    (value.clamp(0.0, 1.0) * 255.0).round()
}

fn assert_stored(got: [f32; 3], want: [f32; 3], what: &str) {
    for c in 0..3 {
        assert!((got[c] - stored(want[c])).abs() <= 1.5, "{what}: got {got:?}, expected {:?}", want.map(stored));
    }
}

#[test]
fn any_channel_can_be_shown_alone_as_stored() {
    let Some((device, queue, mut renderer, mut params)) = cube_scene() else { return };
    for slot in 0..1 {
        renderer.upload_channel(&device, &queue, slot, ChannelKind::Opacity, &TiledImage::new(64, 64, 1, [0.25, 0.0, 0.0, 0.0]), None);
        renderer.upload_channel(&device, &queue, slot, ChannelKind::Height, &TiledImage::new(64, 64, 1, [-0.5, 0.0, 0.0, 0.0]), None);
        renderer.upload_channel(&device, &queue, slot, ChannelKind::Normal, &TiledImage::new(64, 64, 4, [0.2, 0.6, 0.9, 0.5]), None);
        renderer.upload_channel(&device, &queue, slot, ChannelKind::Emissive, &TiledImage::new(64, 64, 4, [0.1, 0.5, 0.2, 1.0]), None);
    }
    let mut frame = |p: &ViewParams| renderer.render(&device, &queue, [N, N], p).read_rgba8(&device, &queue);
    let centre = |image: &[u8]| pixel(image, 32, 32);
    // Opacity is not a channel the shading reads: it still shows, as the stored gray.
    params.solo_channel = Some(ChannelKind::Opacity);
    assert_stored(centre(&frame(&params)), [0.25; 3], "Opacity alone");
    // A channel with no texture uploaded shows its default value (Specular: see ChannelKind).
    params.solo_channel = Some(ChannelKind::Specular);
    let default = ChannelKind::Specular.default_value().0[0];
    assert_stored(centre(&frame(&params)), [default; 3], "Specular without a texture");
    // Emissive is a color channel: shown as a color (linear light), like Base Color.
    params.solo_channel = Some(ChannelKind::Emissive);
    assert_color(centre(&frame(&params)), [0.1, 0.5, 0.2], "Emissive alone");
    // The three stored components of the normal map, and each one alone as gray.
    params.solo_channel = Some(ChannelKind::Normal);
    assert_stored(centre(&frame(&params)), [0.2, 0.6, 0.9], "Normal alone");
    for (component, value) in [(ChannelComponent::R, 0.2), (ChannelComponent::G, 0.6), (ChannelComponent::B, 0.9), (ChannelComponent::Alpha, 0.5)] {
        params.channel_display = ChannelDisplay { component, ..ChannelDisplay::default() };
        assert_stored(centre(&frame(&params)), [value; 3], &format!("Normal {component:?}"));
    }
    params.channel_display = ChannelDisplay::default();
    // Outside the model nothing changes: the corner is the background.
    assert_eq!(pixel(&frame(&params), 1, 1), pixel(&{ params.solo_channel = None; frame(&params) }, 1, 1));
}

#[test]
fn hdr_scale_signed_colors_and_lighting_apply_to_the_solo_view() {
    let Some((device, queue, mut renderer, mut params)) = cube_scene() else { return };
    renderer.upload_channel(&device, &queue, 0, ChannelKind::Height, &TiledImage::new(64, 64, 1, [-0.5, 0.0, 0.0, 0.0]), None);
    renderer.upload_channel(&device, &queue, 0, ChannelKind::Opacity, &TiledImage::new(64, 64, 1, [3.0, 0.0, 0.0, 0.0]), None);
    let mut frame = |p: &ViewParams| renderer.render(&device, &queue, [N, N], p).read_rgba8(&device, &queue);
    let centre = |image: &[u8]| pixel(image, 32, 32);
    let (positive, negative) = ([0.2, 0.8, 0.4], [0.9, 0.1, 0.3]);
    // A value of 3 clips to white; divided by 4 it is 0.75.
    params.solo_channel = Some(ChannelKind::Opacity);
    assert_stored(centre(&frame(&params)), [1.0; 3], "HDR value unscaled");
    params.channel_display = ChannelDisplay { hdr_scale: 4.0, ..ChannelDisplay::default() };
    assert_stored(centre(&frame(&params)), [0.75; 3], "HDR value divided by 4");
    // Two colors for the sign: +0.75 in the positive color, -0.5 in the negative one.
    params.channel_display = ChannelDisplay { hdr_scale: 4.0, signed_colors: true, positive, negative, ..ChannelDisplay::default() };
    assert_stored(centre(&frame(&params)), positive.map(|c| c * 0.75), "positive value");
    params.solo_channel = Some(ChannelKind::Height);
    params.channel_display = ChannelDisplay { signed_colors: true, positive, negative, ..ChannelDisplay::default() };
    assert_stored(centre(&frame(&params)), negative.map(|c| c * 0.5), "negative value");
    params.channel_display = ChannelDisplay::default();
    assert_stored(centre(&frame(&params)), [0.0; 3], "a negative height is black without the sign colors");
    // The fixed Height view mode goes through the same display settings.
    params.solo_channel = None;
    params.view_mode = subcraft_render::ViewMode::Height;
    params.channel_display = ChannelDisplay { signed_colors: true, positive, negative, ..ChannelDisplay::default() };
    assert_stored(centre(&frame(&params)), negative.map(|c| c * 0.5), "Height view mode");
    // Lit: the front face (normal +Z) under a light from the upper right is 0.35 + 0.65 * n.l.
    params.solo_channel = Some(ChannelKind::Opacity);
    params.view_mode = subcraft_render::ViewMode::Material;
    params.light_dir = Vec3::new(0.0, 0.6, 0.8);
    params.channel_display = ChannelDisplay { hdr_scale: 4.0, unlit: false, ..ChannelDisplay::default() };
    assert_stored(centre(&frame(&params)), [0.75 * (0.35 + 0.65 * 0.8); 3], "lit solo view");
}

/// A large checkered floor seen at a grazing angle: the far part is where filtering shows.
fn floor_scene() -> Option<Scene> {
    let (_, device, queue) = headless::device()?;
    let mut renderer = ViewportRenderer::new(&device, &queue);
    let mesh = subcraft_mesh::primitives::plane(40.0, 2);
    renderer.set_mesh(&device, &mesh);
    let mut tex = TiledImage::new(256, 256, 4, [0.9, 0.9, 0.9, 1.0]);
    for y in 0..256 {
        for x in 0..256 {
            if (x / 2 + y / 2) % 2 == 0 {
                tex.set(x, y, [0.02, 0.02, 0.02, 1.0]);
            }
        }
    }
    renderer.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &tex, None);
    let view = Mat4::look_at_rh(Vec3::new(0.0, 1.2, 12.0), Vec3::new(0.0, 0.0, 0.0), Vec3::Y);
    let mut params = ViewParams::new(view, Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 200.0));
    params.antialias = false;
    params.view_mode = subcraft_render::ViewMode::BaseColor;
    params.post = PostParams { emission_glow: 0.0, ..PostParams::default() };
    Some((device, queue, renderer, params))
}

/// Contrast of the floor's pattern in a band of rows: mean absolute difference between
/// horizontal neighbours (0 = the checker has blurred to flat gray).
fn detail(image: &[u8], rows: std::ops::Range<u32>) -> f32 {
    let mut sum = 0.0;
    let mut count = 0.0;
    for y in rows {
        for x in 0..N - 1 {
            sum += (luma_at(image, x, y) - luma_at(image, x + 1, y)).abs();
            count += 1.0;
        }
    }
    sum / count
}

#[test]
fn anisotropic_filtering_and_a_sharper_mip_bias_keep_distant_texture_detail() {
    let Some((device, queue, mut renderer, mut params)) = floor_scene() else { return };
    let plain = renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue);
    // Find the floor's far band: the rows just below the horizon.
    let band = 34..40u32;
    assert_eq!(renderer.texture_filtering(), 1, "off until asked for");
    renderer.set_texture_filtering(&device, 16);
    assert_eq!(renderer.texture_filtering(), 16);
    let aniso = renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue);
    assert!(detail(&aniso, band.clone()) > detail(&plain, band.clone()) * 1.3 + 0.5, "16 samples keep the checker visible at a grazing angle: {} vs {}", detail(&aniso, band.clone()), detail(&plain, band.clone()));
    renderer.set_texture_filtering(&device, 1);
    let back = renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue);
    assert_eq!(back, plain, "switching it off again gives the first picture");
    // A bias of -3 reads three levels sharper. The single-channel views are not biased, so
    // compare in the full material view.
    params.view_mode = subcraft_render::ViewMode::Material;
    let soft = renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue);
    params.mip_bias = -3.0;
    let sharp = renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue);
    assert!(detail(&sharp, band.clone()) > detail(&soft, band.clone()) * 1.3 + 0.5, "a sharper bias keeps more detail: {} vs {}", detail(&sharp, band.clone()), detail(&soft, band));
    params.mip_bias = 0.0;
    assert_eq!(renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue), soft, "bias 0 is the plain sampling");
}

#[test]
fn the_grid_draws_in_the_chosen_plane_with_its_color_and_opacity() {
    let Some((device, queue, mut renderer, mut params)) = cube_scene() else { return };
    // Look down from above and to the side: the floor (Y) grid covers the lower view.
    params.view = Mat4::look_at_rh(Vec3::new(3.0, 5.0, 6.0), Vec3::ZERO, Vec3::Y);
    params.eye = Vec3::new(3.0, 5.0, 6.0);
    let bounds = subcraft_mesh::primitives::cube(2.0).bounds;
    let mut frame = |p: &ViewParams| renderer.render(&device, &queue, [N, N], p).read_rgba8(&device, &queue);
    let none = frame(&params);
    let changed = |image: &[u8]| image.chunks_exact(4).zip(none.chunks_exact(4)).filter(|(a, b)| a != b).count();
    let red = |image: &[u8]| image.chunks_exact(4).zip(none.chunks_exact(4)).filter(|(a, b)| a[0] as i32 - b[0] as i32 > 12 && (a[2] as i32 - b[2] as i32).abs() < 6).count();
    let loud = GridParams { color: [1.0, 0.0, 0.0], opacity: 8.0, axes: [false; 3], ..GridParams::new(bounds) };
    params.grid = Some(loud);
    let floor = frame(&params);
    assert!(red(&floor) > 40, "a red grid at full strength: {} red pixels", red(&floor));
    // Opacity 0 draws nothing.
    params.grid = Some(GridParams { opacity: 0.0, ..loud });
    assert_eq!(changed(&frame(&params)), 0, "an invisible grid changes no pixel");
    // The default gray at default strength is there but quiet: no red.
    params.grid = Some(GridParams { axes: [false; 3], ..GridParams::new(bounds) });
    let quiet = frame(&params);
    assert!(changed(&quiet) > 0 && red(&quiet) == 0);
    // Another plane: a different picture, still red lines.
    for plane in [0, 2] {
        params.grid = Some(GridParams { plane, ..loud });
        let other = frame(&params);
        assert!(red(&other) > 10 && other != floor, "plane {plane}: {} red pixels", red(&other));
    }
}
