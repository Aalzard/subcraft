//! Screen-space subsurface scattering (post.rs `fs_sss`): a sphere lit from the side, its
//! terminator softened where the material scatters. Skips (passes with a message) when no
//! adapter is available.

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

/// Two unit spheres side by side: slot 0 at x = -1.2, slot 1 at x = +1.2.
fn spheres() -> Mesh {
    let s = primitives::uv_sphere(1.0, 96, 48);
    let a = s.transformed(&Mat4::from_translation(Vec3::new(-1.2, 0.0, 0.0)));
    let b = s.transformed(&Mat4::from_translation(Vec3::new(1.2, 0.0, 0.0)));
    Mesh::merged(&[&a, &b])
}

fn scene(slots: [&[(ChannelKind, [f32; 4])]; 2]) -> Option<Scene> {
    let (_, device, queue) = headless::device()?;
    let mut renderer = ViewportRenderer::new(&device, &queue);
    renderer.set_mesh(&device, &spheres());
    for (slot, channels) in slots.iter().enumerate() {
        for (k, v) in channels.iter() {
            let comps = if k.components() == 3 { 4 } else { 1 };
            renderer.upload_channel(&device, &queue, slot as u32, *k, &TiledImage::new(16, 16, comps, *v), None);
        }
    }
    let mut params = ViewParams::new(Mat4::look_at_rh(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0));
    params.antialias = false;
    params.shadows.enabled = false;
    // Light from the right and little ambient light: each sphere's terminator runs down the
    // middle of its front.
    params.light_dir = Vec3::X;
    params.light_intensity = 3.0;
    params.environment_intensity = 0.15;
    Some(Scene { device, queue, renderer, params })
}

impl Scene {
    fn frame(&mut self, post: PostParams) -> Vec<[f32; 3]> {
        self.params.post = post;
        let px = self.renderer.render(&self.device, &self.queue, [N, N], &self.params).read_rgba8(&self.device, &self.queue);
        px.as_chunks::<4>().0.iter().map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect()
    }

    fn pixel(&self, p: Vec3) -> (u32, u32) {
        let c = self.params.proj * self.params.view * p.extend(1.0);
        let ndc = c.truncate() / c.w;
        (((ndc.x * 0.5 + 0.5) * N as f32) as u32, ((0.5 - ndc.y * 0.5) * N as f32) as u32)
    }

    /// Mean colour of a 3 x 3 block around the pixel of a world point.
    fn at(&self, img: &[[f32; 3]], p: Vec3) -> [f32; 3] {
        let (x, y) = self.pixel(p);
        let mut s = [0.0f32; 3];
        for dy in 0..3 {
            for dx in 0..3 {
                let c = img[((y + dy - 1) * N + x + dx - 1) as usize];
                for k in 0..3 {
                    s[k] += c[k] / 9.0;
                }
            }
        }
        s
    }
}

/// A point on the front of the sphere centred at `cx`, `dx` to the side of its middle.
fn front(cx: f32, dx: f32) -> Vec3 {
    Vec3::new(cx + dx, 0.0, (1.0 - dx * dx).sqrt())
}

fn lum(c: [f32; 3]) -> f32 {
    (c[0] + c[1] + c[2]) / 3.0
}

#[test]
fn scattering_softens_the_terminator_of_scattering_surfaces_only() {
    use ChannelKind as K;
    let rgb = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
    let skin: &[(K, [f32; 4])] = &[(K::BaseColor, rgb([0.8, 0.8, 0.8])), (K::Roughness, [0.6; 4]), (K::Scattering, [1.0; 4]), (K::ScatteringColor, rgb([1.0, 0.35, 0.2]))];
    let plain: &[(K, [f32; 4])] = &[(K::BaseColor, rgb([0.8, 0.8, 0.8])), (K::Roughness, [0.6; 4])];
    let Some(mut s) = scene([skin, plain]) else {
        eprintln!("no adapter: skipping");
        return;
    };
    let off = s.frame(PostParams::default());
    let on = s.frame(PostParams { sss: 1.0, sss_radius: 0.5, sss_samples: 32, ..PostParams::default() });
    if let Ok(dir) = std::env::var("SUBSURFACE_DUMP") {
        for (name, img) in [("sss_off", &off), ("sss_on", &on)] {
            let data: Vec<f32> = img.iter().flat_map(|c| [c[0], c[1], c[2], 1.0]).collect();
            subcraft_image::save_image(&std::path::Path::new(&dir).join(format!("{name}.png")), N, N, 4, &data, subcraft_image::FileFormat::Png8).unwrap();
        }
    }
    // Off is exactly off.
    assert!(off == s.frame(PostParams { sss: 0.0, sss_radius: 0.5, ..PostParams::default() }), "strength 0 is off");
    // The same sphere with Scattering 0, for comparison.
    let params = PostParams { sss: 1.0, sss_radius: 0.5, sss_samples: 32, ..PostParams::default() };
    s.renderer.upload_channel(&s.device, &s.queue, 0, K::Scattering, &TiledImage::new(16, 16, 1, [0.0; 4]), None);
    let none = s.frame(params);
    // Light bleeds past the terminator into the shade, red furthest.
    let dark = front(-1.2, -0.1);
    let (a, b) = (s.at(&none, dark), s.at(&on, dark));
    eprintln!("dark side of the terminator: no scattering {a:?}, scattering {b:?}");
    assert!(lum(b) > lum(a) + 0.02, "light scatters past the terminator: {a:?} -> {b:?}");
    assert!(b[0] - a[0] > (b[2] - a[2]) + 0.02, "red scatters further than blue: {a:?} -> {b:?}");
    // Light is moved, not made: the sphere as a whole is about as bright.
    let (x0, _) = s.pixel(Vec3::new(-2.3, 0.0, 0.0));
    let (x1, _) = s.pixel(Vec3::new(-0.1, 0.0, 0.0));
    let total = |img: &[[f32; 3]]| -> f32 { (0..N).flat_map(|y| (x0..x1).map(move |x| (y * N + x) as usize)).map(|i| lum(img[i])).sum() };
    let (ta, tb) = (total(&none), total(&on));
    eprintln!("whole sphere: no scattering {ta:.1}, scattering {tb:.1}");
    assert!((tb / ta - 1.0).abs() < 0.08, "about as much light: {ta} -> {tb}");
    // Deep in the lit area, where the light is even, nothing changes much.
    let (e, f) = (s.at(&none, front(-1.2, 0.7)), s.at(&on, front(-1.2, 0.7)));
    assert!((lum(e) - lum(f)).abs() < 0.03, "even lighting stays: {e:?} -> {f:?}");
    // The sphere without a Scattering channel is untouched, pixel for pixel.
    let (x0, _) = s.pixel(Vec3::new(0.15, 0.0, 0.0));
    for y in 0..N {
        for x in x0..N {
            let i = (y * N + x) as usize;
            assert!(off[i] == on[i], "pixel {x},{y} of the plain sphere changed: {:?} -> {:?}", off[i], on[i]);
        }
    }
}

#[test]
fn texture_detail_stays_sharp_under_scattering() {
    use ChannelKind as K;
    // A fine checker in the base colour: scattering spreads the light, not the texture.
    let Some(mut s) = scene([&[(K::Roughness, [0.7; 4]), (K::Scattering, [1.0; 4])], &[]]) else {
        eprintln!("no adapter: skipping");
        return;
    };
    let mut checker = TiledImage::new(64, 64, 4, [0.9, 0.9, 0.9, 1.0]);
    for y in 0..64 {
        for x in 0..64 {
            if (x / 4 + y / 4) % 2 == 0 {
                checker.set(x, y, [0.15, 0.15, 0.15, 1.0]);
            }
        }
    }
    s.renderer.upload_channel(&s.device, &s.queue, 0, K::BaseColor, &checker, None);
    s.params.light_dir = Vec3::new(0.0, 0.0, 1.0);
    let off = s.frame(PostParams::default());
    let on = s.frame(PostParams { sss: 1.0, sss_radius: 0.5, ..PostParams::default() });
    // Contrast between neighbouring pixels in the evenly lit middle of the sphere.
    let contrast = |img: &[[f32; 3]]| {
        let (cx, cy) = s.pixel(Vec3::new(-1.2, 0.0, 1.0));
        let mut sum = 0.0;
        for y in cy - 10..cy + 10 {
            for x in cx - 10..cx + 10 {
                sum += (lum(img[(y * N + x) as usize]) - lum(img[(y * N + x + 1) as usize])).abs();
            }
        }
        sum
    };
    let (a, b) = (contrast(&off), contrast(&on));
    eprintln!("checker contrast: off {a:.2}, on {b:.2}");
    assert!(b > a * 0.85, "the texture keeps its detail: {a} -> {b}");
}
