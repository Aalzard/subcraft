//! Offscreen rendering on whatever adapter exists (lavapipe in CI/cloud, the real GPU locally).
//! Skips (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::primitives;
use subcraft_render::{ViewMode, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 192;

#[test]
fn albedo_filter_preserves_nearest_palette_and_bilinear_values_without_affecting_data() {
    use subcraft_image::TextureFilter;
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::plane(2., 1));
    let mut base = TiledImage::new(2, 2, 4, [0., 0., 0., 1.]);
    base.set(1, 0, [1.; 4]);
    base.set(1, 1, [1.; 4]);
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(0., 3., 0.), Vec3::ZERO, Vec3::NEG_Z), Mat4::orthographic_rh(-1., 1., -1., 1., 0.1, 10.));
    p.antialias = false; p.view_mode = ViewMode::BaseColor;
    let bilinear = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    r.set_albedo_filter(0, TextureFilter::Nearest);
    let nearest = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    for y in 8..SIZE - 8 { for x in 8..SIZE - 8 {
        let value = px(&nearest, x, y)[0];
        assert!(value <= 1 || value >= 254, "nearest invented a gray value at {x},{y}: {value}");
    }}
    assert_eq!(px(&nearest, 80, 96)[0], 0);
    assert_eq!(px(&nearest, 112, 96)[0], 255);
    let expected = (subcraft_core::color::linear_to_srgb((80.5 / SIZE as f32) * 2. - 0.5) * 255.).round() as u8;
    assert!(px(&bilinear, 80, 96)[0].abs_diff(expected) <= 2, "bilinear must interpolate in linear color");
    assert!(diff(&bilinear, &nearest) > 4000);
    p.view_mode = ViewMode::Material;
    let shaded_nearest = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    r.set_albedo_filter(0, TextureFilter::Bilinear);
    let shaded_linear = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    assert!(diff(&shaded_linear, &shaded_nearest) > 4000, "material view honors the same filter");
    p.view_mode = ViewMode::BaseColor;
    assert_eq!(r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue), bilinear);
    for mode in [ViewMode::Normal, ViewMode::Roughness, ViewMode::Metallic, ViewMode::Height, ViewMode::AmbientOcclusion] {
        p.view_mode = mode;
        let linear = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
        r.set_albedo_filter(0, TextureFilter::Nearest);
        assert_eq!(r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue), linear, "{mode:?} changed");
        r.set_albedo_filter(0, TextureFilter::Bilinear);
    }
    r.set_albedo_filter(1, TextureFilter::Nearest); p.view_mode = ViewMode::BaseColor;
    assert_eq!(r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue), bilinear, "sets have independent filters");
}

fn params() -> ViewParams {
    let view = Mat4::look_at_rh(Vec3::new(3.0, 2.5, 4.0), Vec3::ZERO, Vec3::Y);
    let proj = Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0);
    ViewParams::new(view, proj)
}

fn solid(comps: usize, v: [f32; 4]) -> TiledImage {
    TiledImage::new(256, 256, comps, v)
}

fn px(img: &[u8], x: u32, y: u32) -> [u8; 4] {
    let o = ((y * SIZE + x) * 4) as usize;
    [img[o], img[o + 1], img[o + 2], img[o + 3]]
}

fn save(name: &str, img: &[u8]) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let data: Vec<f32> = img.iter().map(|&b| b as f32 / 255.0).collect();
    save_image(&dir.join(name), SIZE, SIZE, 4, &data, FileFormat::Png8).unwrap();
}

#[test]
fn renders_textured_cube_and_partial_uploads_match_full() {
    let Some((adapter, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    eprintln!("adapter: {:?}", adapter.get_info().name);
    let mesh = primitives::cube(2.0);

    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &mesh);
    let mut base = solid(4, [0.8, 0.05, 0.05, 1.0]);
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [0.5; 4]), None);
    let img = r.render(&device, &queue, [SIZE, SIZE], &params()).read_rgba8(&device, &queue);
    save("cube_red.png", &img);

    let c = px(&img, SIZE / 2, SIZE / 2);
    assert!(c[0] > 80 && c[0] > 2 * c[1] && c[0] > 2 * c[2], "center should be lit red: {c:?}");
    let corner = px(&img, 2, 2);
    let bg = (subcraft_core::color::linear_to_srgb(params().background[0]) * 255.0).round() as i32;
    assert!((corner[0] as i32 - bg).abs() <= 1, "corner is background ({bg}): {corner:?}");
    // Three visible faces are shaded differently.
    let top = px(&img, SIZE / 2, SIZE / 4);
    assert_ne!(top, c, "faces receive different lighting");

    // Paint two tiles green; upload only those; compare with a fresh full upload.
    for y in 0..64 {
        for x in 0..128 {
            base.set(x, y, [0.05, 0.7, 0.1, 1.0]);
        }
    }
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, Some(&[0, 1]));
    let partial = r.render(&device, &queue, [SIZE, SIZE], &params()).read_rgba8(&device, &queue);
    let mut fresh = ViewportRenderer::new(&device, &queue);
    fresh.set_mesh(&device, &mesh);
    fresh.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &base, None);
    fresh.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [0.5; 4]), None);
    let full = fresh.render(&device, &queue, [SIZE, SIZE], &params()).read_rgba8(&device, &queue);
    save("cube_partial.png", &partial);
    assert_eq!(partial, full, "tile uploads must equal a full upload");
    assert_ne!(partial, img, "the green tiles are visible");
}

/// A full upload clears to the fill on the GPU and uploads only allocated tiles: the result must
/// equal uploading every tile with the same texels.
#[test]
fn sparse_full_upload_matches_a_dense_one() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    let mesh = primitives::cube(2.0);
    let render = |base: &TiledImage, rough: &TiledImage| {
        let mut r = ViewportRenderer::new(&device, &queue);
        r.set_mesh(&device, &mesh);
        r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, base, None);
        r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, rough, None);
        r.render(&device, &queue, [SIZE, SIZE], &params()).read_rgba8(&device, &queue)
    };
    let mut base = solid(4, [0.8, 0.05, 0.3, 1.0]);
    let mut rough = solid(1, [0.35; 4]);
    for y in 64..128 {
        for x in 0..64 {
            base.set(x, y, [0.05, 0.7, 0.1, 1.0]);
            rough.set(x, y, [0.9; 4]);
        }
    }
    assert!(base.allocated_tiles() < base.tile_count());
    let dense = |img: &TiledImage| {
        let mut d = img.clone();
        for t in 0..d.tile_count() {
            let mut buf = vec![0.0; subcraft_image::TILE_TEXELS * d.comps()];
            img.read_tile(t, &mut buf);
            d.write_tile(t, &buf);
        }
        assert_eq!(d.allocated_tiles(), d.tile_count());
        d
    };
    let sparse = render(&base, &rough);
    assert_eq!(sparse, render(&dense(&base), &dense(&rough)), "cleared fill tiles equal uploaded fill tiles");
    assert_ne!(sparse, render(&solid(4, [0.8, 0.05, 0.3, 1.0]), &solid(1, [0.35; 4])), "the allocated tiles are visible");
}

#[test]
fn empty_scene_is_background_and_resizes() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    let mut r = ViewportRenderer::new(&device, &queue);
    let a = r.render(&device, &queue, [64, 32], &params()).read_rgba8(&device, &queue);
    assert_eq!(a.len(), 64 * 32 * 4);
    assert!(a.chunks_exact(4).all(|p| p == &a[..4]), "uniform background");
    let b = r.render(&device, &queue, [100, 70], &params()).read_rgba8(&device, &queue);
    assert_eq!(b.len(), 100 * 70 * 4);
}

fn render_with(device: &subcraft_render::wgpu_api::Device, queue: &subcraft_render::wgpu_api::Queue, channels: &[(ChannelKind, TiledImage)], p: &ViewParams) -> Vec<u8> {
    let mut r = ViewportRenderer::new(device, queue);
    r.set_mesh(device, &primitives::cube(2.0));
    r.upload_channel(device, queue, 0, ChannelKind::BaseColor, &solid(4, [0.6, 0.6, 0.6, 1.0]), None);
    for (k, img) in channels {
        r.upload_channel(device, queue, 0, *k, img, None);
    }
    r.render(device, queue, [SIZE, SIZE], p).read_rgba8(device, queue)
}

fn diff(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 6)).count()
}

#[test]
fn view_modes_show_one_channel_unlit_and_antialiasing_can_be_off() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    let channels = [(ChannelKind::Roughness, solid(1, [0.25; 4])), (ChannelKind::Metallic, solid(1, [1.0; 4]))];
    let shaded = render_with(&device, &queue, &channels, &params());
    let with = |mode| render_with(&device, &queue, &channels, &ViewParams { view_mode: mode, ..params() });

    // Unlit channel views: every face of the cube shows the stored value, whatever the lighting.
    let rough = with(ViewMode::Roughness);
    let (center, top) = (px(&rough, SIZE / 2, SIZE / 2), px(&rough, SIZE / 2, SIZE / 4));
    assert_eq!(center, top, "unlit: all faces equal");
    assert!(center[0].abs_diff(64) <= 2 && center[0] == center[1], "roughness 0.25 shows as gray 64: {center:?}");
    assert!(px(&with(ViewMode::Metallic), SIZE / 2, SIZE / 2)[0] >= 253, "metallic 1 is white");
    let base = px(&with(ViewMode::BaseColor), SIZE / 2, SIZE / 2);
    let expect = (subcraft_core::color::linear_to_srgb(0.6) * 255.0).round() as u8;
    assert!(base[0].abs_diff(expect) <= 2, "base color as authored ({expect}): {base:?}");
    let normal = px(&with(ViewMode::Normal), SIZE / 2, SIZE / 2);
    assert!(normal[0].abs_diff(128) <= 2 && normal[2] >= 253, "flat normal map color: {normal:?}");
    assert_eq!(with(ViewMode::Material), shaded, "Material is the normal shaded view");

    // Without MSAA the image still renders; only edge pixels may differ.
    let no_aa = render_with(&device, &queue, &channels, &ViewParams { antialias: false, ..params() });
    let changed = diff(&shaded, &no_aa);
    assert!(changed > 0 && changed < (SIZE * SIZE / 20) as usize, "{changed} pixels differ (edges only)");
}

#[test]
fn height_bumps_and_metallic_change_shading() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    let flat = render_with(&device, &queue, &[], &params());
    // A raised disc in the middle of every cube face cell.
    let mut height = solid(1, [0.0; 4]);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let (cx, cy) = ((x % 85) as f32 - 42.0, (y % 128) as f32 - 64.0);
            if (cx * cx + cy * cy).sqrt() < 25.0 {
                height.set(x, y, [0.5; 4]);
            }
        }
    }
    let bumped = render_with(&device, &queue, &[(ChannelKind::Height, height.clone())], &params());
    save("cube_height.png", &bumped);
    let changed = diff(&flat, &bumped);
    assert!(changed > 200, "height bumps should change shading ({changed} px)");
    let off = render_with(&device, &queue, &[(ChannelKind::Height, height)], &ViewParams { height_scale: 0.0, ..params() });
    assert_eq!(diff(&flat, &off), 0, "height scale 0 disables bumps");

    let metal = render_with(
        &device,
        &queue,
        &[(ChannelKind::Metallic, solid(1, [1.0; 4])), (ChannelKind::Roughness, solid(1, [0.15; 4]))],
        &params(),
    );
    save("cube_metal.png", &metal);
    let changed = diff(&flat, &metal);
    assert!(changed > 5000, "metallic + low roughness should look clearly different ({changed} px)");
}

/// Visual check: a sphere whose roughness rises left→right (top half dielectric, bottom metal).
#[test]
fn roughness_gradient_sphere_shows_sharp_to_blurry_reflections() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    let mesh = primitives::uv_sphere(1.0, 64, 32);
    let mut rough = solid(1, [0.0; 4]);
    let mut metal = solid(1, [0.0; 4]);
    for y in 0..256u32 {
        for x in 0..256u32 {
            rough.set(x, y, [0.02 + 0.9 * ((x % 128) as f32 / 127.0); 4]);
            metal.set(x, y, [if y > 128 { 1.0 } else { 0.0 }; 4]);
        }
    }
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.4, 3.2), Vec3::ZERO, Vec3::Y);
    let p = ViewParams::new(view, Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0));
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &mesh);
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.5, 0.5, 0.5, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &rough, None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Metallic, &metal, None);
    let img = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    save("sphere_roughness.png", &img);
    // Smooth vs rough strips must differ in local contrast.
    let contrast = |x0: u32| {
        let vals: Vec<i32> = (60..130).map(|y| px(&img, x0, y)[1] as i32).collect();
        vals.windows(2).map(|w| (w[0] - w[1]).abs()).sum::<i32>()
    };
    eprintln!("contrast smooth {} rough {}", contrast(96), contrast(70));
}

#[test]
fn every_mesh_map_layout_uploads_and_previews() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    use subcraft_core::MeshMapKind;
    use subcraft_render::PreviewSource;
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::cube(2.0));
    for kind in MeshMapKind::ALL {
        let c = kind.components();
        let mut img = TiledImage::new(100, 100, c, [0.25, 0.5, 0.75, 0.0]); // non-tile-multiple size
        img.set(3, 3, [1.0; 4]);
        r.upload_mesh_map(&device, &queue, 0, kind, &img);
        let preview = r.preview(&device, &queue, 0, PreviewSource::MeshMap(kind));
        assert!(preview.is_some(), "{kind:?}");
    }
    // A 3-component image in a shading slot (baked AO is 1-comp, but any RGB map must work too).
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &TiledImage::new(64, 64, 3, [0.2, 0.4, 0.6, 0.0]), None);
    let _ = r.render(&device, &queue, [32, 32], &params()).read_rgba8(&device, &queue);
}

#[test]
fn hdri_environment_shows_in_reflections() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no wgpu adapter available: skipping");
        return;
    };
    use subcraft_render::{EnvironmentParams, HdriUpload};
    // Red sky (v < 0.5), blue ground, 3 mip levels.
    let make = |w: u32, h: u32| -> Vec<f32> {
        (0..w * h).flat_map(|i| if (i / w) < h / 2 { [2.0, 0.1, 0.1, 1.0] } else { [0.1, 0.1, 2.0, 1.0] }).collect()
    };
    let (l0, l1, l2) = (make(64, 32), make(32, 16), make(16, 8));
    let levels = [(64u32, 32u32, l0.as_slice()), (32, 16, l1.as_slice()), (16, 8, l2.as_slice())];
    let mut sh = [[0.0f32; 3]; 9];
    sh[0] = [1.0 / 0.282_095, 0.3 / 0.282_095, 1.0 / 0.282_095];
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::uv_sphere(1.0, 48, 24));
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &solid(4, [0.9, 0.9, 0.9, 1.0]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Metallic, &solid(1, [1.0; 4]), None);
    r.upload_channel(&device, &queue, 0, ChannelKind::Roughness, &solid(1, [0.05; 4]), None);
    r.set_hdri(&device, &queue, Some(HdriUpload { levels: &levels, sh }));
    let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 3.2), Vec3::ZERO, Vec3::Y);
    let mut p = ViewParams::new(view, Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0));
    p.light_intensity = 0.0;
    p.environment = EnvironmentParams { use_hdri: true, ..EnvironmentParams::default() };
    let img = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    save("sphere_hdri.png", &img);
    let top = px(&img, SIZE / 2, SIZE / 2 - 50);
    let bottom = px(&img, SIZE / 2, SIZE / 2 + 50);
    let (top, bottom) = (top.map(i32::from), bottom.map(i32::from));
    assert!(top[0] > top[2] + 40, "upper sphere reflects the red sky: {top:?}");
    assert!(bottom[2] > bottom[0] + 40, "lower sphere reflects the blue ground: {bottom:?}");
    // Procedural mode ignores the HDRI.
    p.environment.use_hdri = false;
    let proc_img = r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue);
    let t2 = px(&proc_img, SIZE / 2, SIZE / 2 - 50).map(i32::from);
    assert!(t2[0] < t2[2] + 40, "studio sky is not red: {t2:?}");
}
