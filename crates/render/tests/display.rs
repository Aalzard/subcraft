//! Numeric GPU checks for the Display Settings: the environment drawn behind the model.
use glam::{Mat4, Vec3};
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

/// An environment whose two halves have different colors (the seam faces +X) and whose blurred
/// level is one flat gray, with a camera at the origin looking along +X (screen right is +Z).
fn scene() -> Option<(subcraft_render::wgpu_api::Device, subcraft_render::wgpu_api::Queue, ViewportRenderer, ViewParams)> {
    let (_, device, queue) = headless::device()?;
    let mut renderer = ViewportRenderer::new(&device, &queue);
    let (w, h) = (64u32, 32u32);
    let mut sharp = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..h {
        for x in 0..w {
            let c = if x < w / 2 { LEFT } else { RIGHT };
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
