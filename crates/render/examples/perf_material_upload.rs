//! Bounded 4K stroke-like dirty uploads through the production renderer, including GPU completion.
//! cargo run -p subcraft-render --example perf_material_upload -- 4096
use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_render::{SHADED_CHANNELS, ViewParams, ViewportRenderer, headless};

fn main() {
    let resolution: u32 = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("resolution"))
        .unwrap_or(4096);
    assert!(resolution >= 64);
    let (adapter, device, queue) = headless::device().expect("GPU adapter required");
    eprintln!("{:?}", adapter.get_info());
    let mut renderer = ViewportRenderer::new(&device, &queue);
    renderer.set_mesh(&device, &subcraft_mesh::primitives::plane(2., 1));
    let mut images: Vec<_> = SHADED_CHANNELS
        .iter()
        .map(|&kind| {
            let comps = if matches!(
                kind,
                ChannelKind::BaseColor | ChannelKind::Normal | ChannelKind::Emissive
            ) {
                4
            } else {
                1
            };
            let d = kind.default_value().0;
            (
                kind,
                TiledImage::new(resolution, resolution, comps, [d[0], d[1], d[2], 1.]),
            )
        })
        .collect();
    for (kind, image) in &images {
        renderer.upload_channel(&device, &queue, 0, *kind, image, None);
    }
    let p = ViewParams::new(
        Mat4::look_at_rh(Vec3::new(2., 3., 2.), Vec3::ZERO, Vec3::Y),
        Mat4::perspective_rh(45f32.to_radians(), 640. / 480., 0.1, 20.),
    );
    renderer.render(&device, &queue, [640, 480], &p);
    device
        .poll(subcraft_render::wgpu_api::PollType::wait_indefinitely())
        .unwrap();
    let mut submit = Vec::new();
    let mut complete = Vec::new();
    for i in 0..120 {
        let start = std::time::Instant::now();
        let tile = (i * 13) % images[0].1.tile_count();
        for (kind, image) in &mut images {
            let rect = image.tile_rect(tile);
            let mut value = image.fill();
            value[0] = (i % 10) as f32 / 10.;
            image.set(rect.x0 as u32, rect.y0 as u32, value);
            renderer.upload_channel(&device, &queue, 0, *kind, image, Some(&[tile]));
        }
        renderer.render(&device, &queue, [640, 480], &p);
        submit.push(start.elapsed().as_secs_f64() * 1000.);
        device
            .poll(subcraft_render::wgpu_api::PollType::wait_indefinitely())
            .unwrap();
        complete.push(start.elapsed().as_secs_f64() * 1000.);
    }
    for (label, times) in [
        ("CPU submit", &mut submit),
        ("including GPU completion", &mut complete),
    ] {
        times.sort_by(f64::total_cmp);
        eprintln!(
            "{resolution}², 120 updates × 7 channels + 640×480 render, {label}: median {:.3} / p95 {:.3} / max {:.3} ms",
            times[60], times[114], times[119]
        );
    }
    eprintln!("Synthetic upload/render cost only; excludes compositor, UI and pointer input.");
}
