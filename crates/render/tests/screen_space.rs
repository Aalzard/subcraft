//! Screen-space contact shadows and reflections (post.rs `fs_screen`): a box standing on a floor.
//! Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::{Mesh, primitives};
use subcraft_render::{PostParams, ViewParams, ViewportRenderer, headless};

const N: u32 = 256;

struct Scene {
    device: subcraft_render::wgpu_api::Device,
    queue: subcraft_render::wgpu_api::Queue,
    renderer: ViewportRenderer,
    params: ViewParams,
}

/// Floor (slot 0, 4 x 4 at y = 0) and a 0.8 box standing on it (slot 1).
fn floor_and_box() -> Mesh {
    let floor = primitives::plane(4.0, 1);
    let cube = primitives::cube(0.8).transformed(&Mat4::from_translation(Vec3::new(0.0, 0.4, 0.0)));
    Mesh::merged(&[&floor, &cube])
}

fn scene(floor: &[(ChannelKind, [f32; 4])], cube: &[(ChannelKind, [f32; 4])]) -> Option<Scene> {
    let (_, device, queue) = headless::device()?;
    let mesh = floor_and_box();
    let mut renderer = ViewportRenderer::new(&device, &queue);
    renderer.set_mesh(&device, &mesh);
    for (slot, channels) in [floor, cube].iter().enumerate() {
        for (k, v) in channels.iter() {
            let comps = if k.components() == 3 { 4 } else { 1 };
            renderer.upload_channel(&device, &queue, slot as u32, *k, &TiledImage::new(16, 16, comps, *v), None);
        }
    }
    let eye = Vec3::new(0.0, 1.6, 3.2);
    let mut params = ViewParams::new(Mat4::look_at_rh(eye, Vec3::new(0.0, 0.2, 0.0), Vec3::Y), Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    params.antialias = false;
    params.shadows.enabled = false;
    Some(Scene { device, queue, renderer, params })
}

impl Scene {
    fn frame(&mut self, post: PostParams) -> Vec<[f32; 3]> {
        self.params.post = post;
        let px = self.renderer.render(&self.device, &self.queue, [N, N], &self.params).read_rgba8(&self.device, &self.queue);
        px.as_chunks::<4>().0.iter().map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect()
    }

    /// Pixel of a world point.
    fn pixel(&self, p: Vec3) -> (u32, u32) {
        let c = self.params.proj * self.params.view * p.extend(1.0);
        let ndc = c.truncate() / c.w;
        (((ndc.x * 0.5 + 0.5) * N as f32) as u32, ((0.5 - ndc.y * 0.5) * N as f32) as u32)
    }

    /// Mean colour of a 5 x 5 block around the pixel of a world point.
    fn at(&self, img: &[[f32; 3]], p: Vec3) -> [f32; 3] {
        let (x, y) = self.pixel(p);
        let mut s = [0.0f32; 3];
        for dy in 0..5 {
            for dx in 0..5 {
                let c = img[((y + dy - 2) * N + x + dx - 2) as usize];
                for k in 0..3 {
                    s[k] += c[k] / 25.0;
                }
            }
        }
        s
    }
}

fn rgb(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

#[test]
fn reflections_show_the_box_in_a_mirror_floor_and_nothing_where_there_is_nothing_to_reflect() {
    let Some(mut s) = scene(
        &[(ChannelKind::BaseColor, rgb([0.9; 3])), (ChannelKind::Metallic, [1.0; 4]), (ChannelKind::Roughness, [0.05; 4])],
        &[(ChannelKind::BaseColor, rgb([0.8, 0.05, 0.05])), (ChannelKind::Roughness, [0.8; 4])],
    ) else {
        eprintln!("no adapter: skipping");
        return;
    };
    s.params.light_dir = Vec3::new(0.3, 1.0, 0.6).normalize();
    let off = s.frame(PostParams::default());
    let on = s.frame(PostParams { ssr: 1.0, ssr_distance: 2.0, ..PostParams::default() });
    if let Ok(dir) = std::env::var("SCREEN_SPACE_DUMP") {
        for (name, img) in [("ssr_off", &off), ("ssr_on", &on)] {
            let data: Vec<f32> = img.iter().flat_map(|c| [c[0], c[1], c[2], 1.0]).collect();
            subcraft_image::save_image(&std::path::Path::new(&dir).join(format!("{name}.png")), N, N, 4, &data, subcraft_image::FileFormat::Png8).unwrap();
        }
    }
    // The floor just in front of the box reflects its red front face.
    let near = Vec3::new(0.0, 0.0, 0.7);
    let (a, b) = (s.at(&off, near), s.at(&on, near));
    eprintln!("floor in front of the box: off {a:?}, on {b:?}");
    let red = |c: [f32; 3]| c[0] - 0.5 * (c[1] + c[2]);
    assert!(red(b) > red(a) + 0.15, "the reflection is red: {a:?} -> {b:?}");
    // The floor beside the box, whose reflection rays go off to the side and up, stays the same.
    let side = Vec3::new(0.7, 0.0, 1.4);
    let (c, d) = (s.at(&off, side), s.at(&on, side));
    eprintln!("floor at the side: off {c:?}, on {d:?}");
    for k in 0..3 {
        assert!((c[k] - d[k]).abs() < 0.03, "nothing to reflect there: {c:?} vs {d:?}");
    }
    // The rough red box is not a mirror: its face hardly changes.
    let face = Vec3::new(0.0, 0.4, 0.41);
    let (e, f) = (s.at(&off, face), s.at(&on, face));
    assert!((e[0] - f[0]).abs() < 0.03, "rough surfaces barely reflect: {e:?} vs {f:?}");
}

#[test]
fn contact_shadows_darken_the_floor_beside_the_box_away_from_the_light_only() {
    let Some(mut s) = scene(
        &[(ChannelKind::BaseColor, rgb([0.8; 3])), (ChannelKind::Roughness, [0.9; 4])],
        &[(ChannelKind::BaseColor, rgb([0.8; 3])), (ChannelKind::Roughness, [0.9; 4])],
    ) else {
        eprintln!("no adapter: skipping");
        return;
    };
    // Low light from +X: the box shades the floor on its -X side.
    s.params.light_dir = Vec3::new(1.0, 0.45, 0.0).normalize();
    s.params.light_intensity = 3.0;
    let off = s.frame(PostParams::default());
    let on = s.frame(PostParams { contact_shadows: 1.0, contact_length: 0.6, ..PostParams::default() });
    let shade = Vec3::new(-0.5, 0.0, 0.2);
    let lit = Vec3::new(0.6, 0.0, 0.2);
    let far = Vec3::new(-1.5, 0.0, 1.0);
    let lum = |c: [f32; 3]| (c[0] + c[1] + c[2]) / 3.0;
    let (a, b) = (lum(s.at(&off, shade)), lum(s.at(&on, shade)));
    let (c, d) = (lum(s.at(&off, lit)), lum(s.at(&on, lit)));
    let (e, f) = (lum(s.at(&off, far)), lum(s.at(&on, far)));
    eprintln!("contact shadows: beside the box away from the light {a:.3} -> {b:.3}, towards the light {c:.3} -> {d:.3}, far away {e:.3} -> {f:.3}");
    assert!(b < a * 0.8, "the floor behind the box is in its contact shadow: {a} -> {b}");
    // Ambient light stays: not black.
    assert!(b > a * 0.25, "only the key light is taken away: {a} -> {b}");
    assert!((c - d).abs() < 0.02, "the lit side is unchanged: {c} -> {d}");
    assert!((e - f).abs() < 0.02, "the open floor is unchanged: {e} -> {f}");
    // Off by default, and strength 0 is exactly off.
    assert_eq!(off, s.frame(PostParams { contact_shadows: 0.0, ssr: 0.0, ..PostParams::default() }));
}

/// The redness the reflection adds along the floor row through `p`: its peak, and how many
/// pixels lie in the transition between 20% and 80% of that peak (0 for a hard outline).
fn reflection_profile(s: &Scene, off: &[[f32; 3]], on: &[[f32; 3]], p: Vec3) -> (f32, usize) {
    let (_, y) = s.pixel(p);
    let red = |c: [f32; 3]| c[0] - 0.5 * (c[1] + c[2]);
    let added: Vec<f32> = (0..N).map(|x| red(on[(y * N + x) as usize]) - red(off[(y * N + x) as usize])).collect();
    let peak = added.iter().copied().fold(0.0, f32::max);
    let soft = added.iter().filter(|&&v| v > 0.2 * peak && v < 0.8 * peak).count();
    (peak, soft)
}

#[test]
fn rough_floors_blur_the_reflection_instead_of_showing_a_faint_mirror() {
    // Issue #729: the reflection used to be sampled sharp at any roughness, so a rough floor
    // showed a faint mirror image with hard outlines.
    let mut soft = Vec::new();
    for rough in [0.05, 0.3] {
        let Some(mut s) = scene(
            &[(ChannelKind::BaseColor, rgb([0.9; 3])), (ChannelKind::Metallic, [1.0; 4]), (ChannelKind::Roughness, [rough; 4])],
            &[(ChannelKind::BaseColor, rgb([0.8, 0.05, 0.05])), (ChannelKind::Roughness, [0.8; 4])],
        ) else {
            eprintln!("no adapter: skipping");
            return;
        };
        s.params.light_dir = Vec3::new(0.3, 1.0, 0.6).normalize();
        let off = s.frame(PostParams::default());
        let on = s.frame(PostParams { ssr: 1.0, ssr_distance: 2.0, ..PostParams::default() });
        if let Ok(dir) = std::env::var("SCREEN_SPACE_DUMP") {
            let data: Vec<f32> = on.iter().flat_map(|c| [c[0], c[1], c[2], 1.0]).collect();
            subcraft_image::save_image(&std::path::Path::new(&dir).join(format!("ssr_rough_{rough}.png")), N, N, 4, &data, subcraft_image::FileFormat::Png8).unwrap();
        }
        // Far enough from the box that the reflection rays travel and the cone widens.
        let (peak, transition) = reflection_profile(&s, &off, &on, Vec3::new(0.0, 0.0, 1.1));
        eprintln!("roughness {rough}: peak added red {peak:.3}, {transition} soft pixels");
        assert!(peak > 0.04, "the box is still reflected at roughness {rough}: {peak}");
        soft.push(transition);
    }
    assert!(soft[0] <= 2, "the mirror keeps a sharp outline: {} soft pixels", soft[0]);
    assert!(soft[1] >= 8, "the rough floor's reflection has a soft outline: {} soft pixels", soft[1]);
}

#[test]
fn channel_views_get_no_screen_space_lighting() {
    // Issue #730: contact shadows, reflections and AO darkened or tinted the unlit channel views.
    let Some(mut s) = scene(
        &[(ChannelKind::BaseColor, rgb([0.9; 3])), (ChannelKind::Metallic, [1.0; 4]), (ChannelKind::Roughness, [0.05; 4])],
        &[(ChannelKind::BaseColor, rgb([0.8, 0.05, 0.05])), (ChannelKind::Roughness, [0.8; 4])],
    ) else {
        eprintln!("no adapter: skipping");
        return;
    };
    s.params.light_dir = Vec3::new(1.0, 0.45, 0.0).normalize();
    let all = PostParams { ssr: 1.0, ssr_distance: 2.0, contact_shadows: 1.0, contact_length: 0.6, ssao: 1.0, ..PostParams::default() };
    for mode in [subcraft_render::ViewMode::BaseColor, subcraft_render::ViewMode::Roughness, subcraft_render::ViewMode::AmbientOcclusion] {
        s.params.view_mode = mode;
        let off = s.frame(PostParams::default());
        let on = s.frame(all);
        assert_eq!(off, on, "{mode:?} shows the data as stored");
    }
    // The material view still gets them.
    s.params.view_mode = subcraft_render::ViewMode::Material;
    assert_ne!(s.frame(PostParams::default()), s.frame(all));
}
