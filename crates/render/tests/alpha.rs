//! Viewport transparency (issue #145): alpha test, alpha blending and alpha dithering of Base
//! Color coverage times the Opacity channel, per texture set. GPU checks on a cube seen from +Z
//! (its front face fills the centre; back faces are drawn too, culling is off).
use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_doc::{AlphaMode, AlphaSettings};
use subcraft_image::TiledImage;
use subcraft_render::{PostParams, ViewParams, ViewportRenderer, headless};

const N: u32 = 64;

type Scene = (subcraft_render::wgpu_api::Device, subcraft_render::wgpu_api::Queue, ViewportRenderer, ViewParams);

fn cube_scene() -> Option<Scene> {
    let (_, device, queue) = headless::device()?;
    let mut renderer = ViewportRenderer::new(&device, &queue);
    renderer.set_mesh(&device, &subcraft_mesh::primitives::cube(2.0));
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::Y);
    let mut params = ViewParams::new(view, Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    params.antialias = false;
    params.background = [0.30, 0.02, 0.02];
    params.post = PostParams { emission_glow: 0.0, ..PostParams::default() };
    Some((device, queue, renderer, params))
}

fn pixel(image: &[u8], x: u32, y: u32) -> [i32; 3] {
    let i = ((y * N + x) * 4) as usize;
    [image[i] as i32, image[i + 1] as i32, image[i + 2] as i32]
}

fn same(a: [i32; 3], b: [i32; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= 1)
}

fn alpha(mode: AlphaMode, threshold: f32, dithering: bool) -> AlphaSettings {
    AlphaSettings { mode, threshold, dithering }
}

#[test]
fn alpha_modes_cut_blend_and_dither_the_surface() {
    let Some((device, queue, mut renderer, params)) = cube_scene() else { return };
    let base = TiledImage::new(64, 64, 4, [0.8, 0.8, 0.8, 1.0]);
    renderer.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
    let mut opacity = |r: &mut ViewportRenderer, v: f32| r.upload_channel(&device, &queue, 0, ChannelKind::Opacity, &TiledImage::new(64, 64, 1, [v; 4]), None);
    let frame = |r: &mut ViewportRenderer| r.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue);

    opacity(&mut renderer, 1.0);
    let solid = frame(&mut renderer);
    let background = pixel(&solid, 1, 1);
    let centre = pixel(&solid, 32, 32);
    assert!(!same(centre, background), "the cube covers the centre");

    // Opaque (the default) ignores opacity.
    opacity(&mut renderer, 0.0);
    assert!(same(pixel(&frame(&mut renderer), 32, 32), centre), "opaque mode draws every texel");

    // Alpha test: below the threshold the surface (front and back) is cut out.
    renderer.set_alpha(0, alpha(AlphaMode::Test, 0.33, false));
    opacity(&mut renderer, 0.25);
    assert!(same(pixel(&frame(&mut renderer), 32, 32), background), "0.25 < 0.33: cut out");
    renderer.set_alpha(0, alpha(AlphaMode::Test, 0.2, false));
    assert!(same(pixel(&frame(&mut renderer), 32, 32), centre), "0.25 >= 0.2: drawn solid");

    // Alpha blending: 0 is invisible, 1 is the solid surface, 0.5 lies between.
    renderer.set_alpha(0, alpha(AlphaMode::Blend, 0.33, false));
    opacity(&mut renderer, 0.0);
    assert!(same(pixel(&frame(&mut renderer), 32, 32), background), "alpha 0 blends to the background");
    opacity(&mut renderer, 1.0);
    assert!(same(pixel(&frame(&mut renderer), 32, 32), centre), "alpha 1 is opaque");
    opacity(&mut renderer, 0.5);
    let half = pixel(&frame(&mut renderer), 32, 32);
    assert!(!same(half, centre) && !same(half, background), "{half:?} between {centre:?} and {background:?}");
    assert!(half[1] > background[1] + 2, "green rises from the background towards the grey surface: {half:?}");

    // Base Color coverage counts like opacity (a transparent texture set's empty texels).
    opacity(&mut renderer, 1.0);
    renderer.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &TiledImage::new(64, 64, 4, [0.8, 0.8, 0.8, 0.0]), None);
    assert!(same(pixel(&frame(&mut renderer), 32, 32), background), "unpainted texels of a transparent set are invisible");
    renderer.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);

    // Dithering: about half of the pixels of a 0.5 surface are drawn, the rest show the background.
    renderer.set_alpha(0, alpha(AlphaMode::Blend, 0.33, true));
    opacity(&mut renderer, 0.5);
    let image = frame(&mut renderer);
    let (mut shown, mut total) = (0, 0);
    for y in 24..40 {
        for x in 24..40 {
            total += 1;
            shown += !same(pixel(&image, x, y), background) as i32;
        }
    }
    assert_eq!(shown * 2, total, "a 4x4 Bayer pattern keeps exactly half at 0.5");
}

#[test]
fn single_channel_views_ignore_the_alpha_mode() {
    let Some((device, queue, mut renderer, mut params)) = cube_scene() else { return };
    renderer.upload_channel(&device, &queue, 0, ChannelKind::Opacity, &TiledImage::new(64, 64, 1, [0.0; 4]), None);
    renderer.set_alpha(0, AlphaSettings { mode: AlphaMode::Test, threshold: 0.5, dithering: false });
    params.solo_channel = Some(ChannelKind::Opacity);
    let image = renderer.render(&device, &queue, [N, N], &params).read_rgba8(&device, &queue);
    assert_eq!(pixel(&image, 32, 32), [0, 0, 0], "the Opacity channel itself shows, black, not cut out");
}
