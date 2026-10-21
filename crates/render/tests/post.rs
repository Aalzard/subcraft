//! Numeric GPU acceptance for tone mapping, bloom and ambient occlusion.
use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::{PostParams, ToneMap, ViewParams, ViewportRenderer, headless};

const N: u32 = 192;

struct Scene {
    device: subcraft_render::wgpu_api::Device,
    queue: subcraft_render::wgpu_api::Queue,
    renderer: ViewportRenderer,
    params: ViewParams,
}

fn scene(mesh: &subcraft_mesh::Mesh, eye: Vec3, orthographic: bool) -> Option<Scene> {
    let (_, device, queue) = headless::device()?;
    let mut renderer = ViewportRenderer::new(&device, &queue);
    renderer.set_mesh(&device, mesh);
    for slot in 0..mesh.materials.len() as u32 {
        renderer.upload_channel(&device, &queue, slot, ChannelKind::BaseColor, &TiledImage::new(64, 64, 4, [0.8, 0.8, 0.8, 1.0]), None);
    }
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let proj = if orthographic {
        Mat4::orthographic_rh(-2.0, 2.0, -2.0, 2.0, 0.1, 100.0)
    } else {
        Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0)
    };
    let mut params = ViewParams::new(view, proj);
    params.antialias = false;
    Some(Scene { device, queue, renderer, params })
}

impl Scene {
    fn frame(&mut self, post: PostParams) -> Vec<u8> {
        self.params.post = post;
        self.renderer.render(&self.device, &self.queue, [N, N], &self.params).read_rgba8(&self.device, &self.queue)
    }
}

fn luma(image: &[u8]) -> Vec<f32> {
    image.as_chunks::<4>().0.iter().map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0).collect()
}
fn total(image: &[u8]) -> f32 { luma(image).iter().sum() }

#[test]
fn tone_maps_differ_keep_the_default_and_order_highlights() {
    let mesh = primitives::cube(2.0);
    let Some(mut s) = scene(&mesh, Vec3::new(3.0, 2.5, 4.0), false) else { return };
    s.params.light_intensity = 6.0; // push highlights well past 1.0
    let aces = s.frame(PostParams::default());
    assert_eq!(aces, s.frame(PostParams { tone_map: ToneMap::Aces, ..PostParams::default() }), "ACES is the default look");
    let mut frames = Vec::new();
    for tone_map in ToneMap::ALL {
        frames.push((tone_map, s.frame(PostParams { tone_map, ..PostParams::default() })));
    }
    for (i, (a, fa)) in frames.iter().enumerate() {
        for (b, fb) in &frames[i + 1..] {
            assert_ne!(fa, fb, "{a:?} and {b:?} must look different");
        }
    }
    let by = |m: ToneMap| total(&frames.iter().find(|f| f.0 == m).unwrap().1);
    assert!(by(ToneMap::Linear) > by(ToneMap::Reinhard), "Reinhard compresses highlights below a hard clip");
}

#[test]
fn bloom_adds_glow_around_bright_areas_and_nowhere_else() {
    let mesh = primitives::cube(2.0);
    let Some(mut s) = scene(&mesh, Vec3::new(3.0, 2.5, 4.0), false) else { return };
    s.params.light_intensity = 5.0;
    let plain = s.frame(PostParams::default());
    let glow = s.frame(PostParams { bloom: 1.0, bloom_threshold: 0.3, ..PostParams::default() });
    let (a, b) = (luma(&plain), luma(&glow));
    assert!(b.iter().zip(&a).all(|(g, p)| g + 0.5 >= *p), "bloom never darkens");
    assert!(total(&glow) > total(&plain) * 1.01, "glow adds light: {} vs {}", total(&glow), total(&plain));
    // Background next to the cube gets glow, the far corner stays exactly as it was.
    let lit_background = a.iter().zip(&b).filter(|(p, g)| **g > **p + 3.0 && **p < 45.0).count();
    assert!(lit_background > 40, "background pixels around the cube glow: {lit_background}");
    assert_eq!(plain[..4], glow[..4], "far corner untouched");
    // Stronger bloom, more light; a threshold above everything, none.
    let more = s.frame(PostParams { bloom: 2.0, bloom_threshold: 0.3, ..PostParams::default() });
    assert!(total(&more) > total(&glow));
    let none = s.frame(PostParams { bloom: 1.0, bloom_threshold: 3.0, ..PostParams::default() });
    assert!((total(&none) - total(&plain)).abs() < total(&plain) * 0.002, "nothing is bright enough");
}

#[test]
fn ambient_occlusion_darkens_creases_never_brightens_and_leaves_the_background() {
    let mesh = primitives::sample_crate();
    for orthographic in [false, true] {
        let Some(mut s) = scene(&mesh, Vec3::new(2.3, 1.6, 2.6), orthographic) else { return };
        let plain = s.frame(PostParams::default());
        let ao = s.frame(PostParams { ssao: 1.0, ssao_radius: 0.12, ..PostParams::default() });
        let (a, b) = (luma(&plain), luma(&ao));
        assert!(b.iter().zip(&a).all(|(o, p)| *o <= *p + 1.0), "AO only darkens (orthographic: {orthographic})");
        assert!(total(&ao) < total(&plain) * 0.995, "AO darkens the crate overall");
        let darker = a.iter().zip(&b).filter(|(p, o)| **p > 30.0 && **o < **p * 0.9).count();
        assert!(darker > 60, "creases between bands, posts and body darken: {darker}");
        assert_eq!(plain[..4], ao[..4], "background pixel unchanged");
        let wide = s.frame(PostParams { ssao: 1.0, ssao_radius: 0.4, ..PostParams::default() });
        assert!(total(&wide) < total(&ao), "a wider radius occludes more");
        let weak = s.frame(PostParams { ssao: 0.3, ssao_radius: 0.12, ..PostParams::default() });
        assert!(total(&weak) > total(&ao) && total(&weak) < total(&plain), "strength scales the darkening");
    }
}

#[test]
fn a_flat_open_surface_gets_no_occlusion() {
    let mesh = primitives::plane(4.0, 2);
    let Some(mut s) = scene(&mesh, Vec3::new(0.0, 4.0, 2.0), false) else { return };
    let plain = s.frame(PostParams::default());
    let ao = s.frame(PostParams { ssao: 1.0, ssao_radius: 0.3, ..PostParams::default() });
    let (a, b) = (luma(&plain), luma(&ao));
    let worst = a.iter().zip(&b).map(|(p, o)| p - o).fold(0.0f32, f32::max);
    assert!(worst < 6.0, "a plane must not darken itself (max drop {worst})");
}

fn emissive_scene(color: [f32; 4]) -> Option<Scene> {
    let mesh = primitives::cube(2.0);
    let mut s = scene(&mesh, Vec3::new(0.0, 0.0, 7.0), false)?;
    s.params.environment_intensity = 0.0;
    s.params.light_intensity = 0.0;
    s.renderer.upload_channel(&s.device, &s.queue, 0, ChannelKind::Emissive, &TiledImage::new(64, 64, 4, color), None);
    Some(s)
}

/// Mean brightness of a square probe around `(x, y)` pixels.
fn probe(image: &[u8], x: i32, y: i32) -> f32 {
    let l = luma(image);
    let mut sum = 0.0;
    for dy in -3..=3 {
        for dx in -3..=3 {
            sum += l[((y + dy) as u32 * N + (x + dx) as u32) as usize];
        }
    }
    sum / 49.0
}

#[test]
fn emission_glow_only_appears_with_an_emissive_channel_and_spreads_with_radius() {
    // No emissive channel: the default glow changes nothing.
    let mesh = primitives::cube(2.0);
    let Some(mut plain_scene) = scene(&mesh, Vec3::new(0.0, 0.0, 7.0), false) else { return };
    let base = plain_scene.frame(PostParams { emission_glow: 0.0, ..PostParams::default() });
    assert_eq!(base, plain_scene.frame(PostParams::default()), "default glow is invisible without emission");

    let Some(mut s) = emissive_scene([4.0, 1.0, 0.4, 1.0]) else { return };
    let off = s.frame(PostParams { emission_glow: 0.0, ..PostParams::default() });
    let glow = s.frame(PostParams { emission_glow: 1.0, ..PostParams::default() });
    assert!(total(&glow) > total(&off) * 1.01, "emission glows: {} vs {}", total(&glow), total(&off));
    assert_eq!(off[..4], glow[..4], "the far corner stays dark");
    // Pixels just outside the cube gain light, the cube itself is not dimmed.
    let (a, b) = (luma(&off), luma(&glow));
    assert!(b.iter().zip(&a).all(|(g, p)| g + 1.0 >= *p));
    let wide = s.frame(PostParams { emission_glow: 1.0, glow_radius: 4.0, ..PostParams::default() });
    let reach = |image: &[u8]| luma(image).iter().zip(&a).filter(|(g, p)| **g > **p + 2.0).count();
    assert!(reach(&wide) > reach(&glow), "a larger radius lights more of the background ({} vs {})", reach(&wide), reach(&glow));
    // Default strength is subtle but visible.
    let subtle = s.frame(PostParams::default());
    assert!(total(&subtle) > total(&off) && total(&subtle) < total(&glow));
}

#[test]
fn glare_streaks_follow_the_chosen_direction() {
    let Some(mut s) = emissive_scene([6.0, 6.0, 6.0, 1.0]) else { return };
    let base = PostParams { emission_glow: 0.0, ..PostParams::default() };
    let off = s.frame(base);
    let half = (N as f32 * 0.5) as i32;
    // The front face reaches about 44 px from the centre of the 192 px image; probe just beyond it.
    let edge = 60;
    let right = |image: &[u8]| probe(image, half + edge, half);
    let above = |image: &[u8]| probe(image, half, half - edge);
    let diagonal = |image: &[u8]| probe(image, half + edge, half - edge);
    let (r0, a0, d0) = (right(&off), above(&off), diagonal(&off));

    let h = s.frame(PostParams { glare: 1.5, glare_mode: subcraft_render::GlareMode::Horizontal, glare_length: 0.8, ..base });
    assert!(right(&h) > r0 + 3.0 && above(&h) < a0 + 1.0, "horizontal streaks reach sideways only: right {} above {}", right(&h), above(&h));
    let v = s.frame(PostParams { glare: 1.5, glare_mode: subcraft_render::GlareMode::Vertical, glare_length: 0.8, ..base });
    assert!(above(&v) > a0 + 3.0 && right(&v) < r0 + 1.0, "vertical streaks reach up and down only");
    let both = s.frame(PostParams { glare: 1.5, glare_mode: subcraft_render::GlareMode::Both, glare_length: 0.8, ..base });
    assert!(right(&both) > r0 + 3.0 && above(&both) > a0 + 3.0);
    let star = s.frame(PostParams { glare: 1.5, glare_mode: subcraft_render::GlareMode::Star, glare_length: 0.8, ..base });
    assert!(diagonal(&star) > d0 + 2.0, "a starburst also reaches the diagonals: {} vs {}", diagonal(&star), d0);
    assert!(diagonal(&h) < d0 + 1.0, "horizontal streaks leave the diagonal alone");
    // Longer streaks reach further.
    let short = s.frame(PostParams { glare: 1.5, glare_mode: subcraft_render::GlareMode::Horizontal, glare_length: 0.0, ..base });
    assert!(total(&h) > total(&short));
}
