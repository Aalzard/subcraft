//! Numeric GPU acceptance: guide visibility, axis colors, depth and projection/AA modes.
use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::{GridParams, ViewParams, ViewportRenderer, headless};

fn axis_pixels(image: &[u8], axis: usize) -> usize {
    image
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| {
            (0..3)
                .filter(|&c| c != axis)
                .all(|c| p[axis] > p[c].saturating_add(6))
        })
        .count()
}

#[test]
fn grid_axes_hide_exactly_and_remain_behind_mesh_in_both_projection_and_aa_modes() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no GPU adapter: skipping grid acceptance");
        return;
    };
    let mesh = primitives::cube(2.0);
    let view = Mat4::look_at_rh(Vec3::new(3.0, 2.5, 4.0), Vec3::ZERO, Vec3::Y);
    let perspective = Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0);
    let orthographic = Mat4::orthographic_rh(-3.0, 3.0, -3.0, 3.0, 0.1, 100.0);
    for proj in [perspective, orthographic] {
        for antialias in [false, true] {
            let mut renderer = ViewportRenderer::new(&device, &queue);
            let mut p = ViewParams::new(view, proj);
            p.antialias = antialias;
            let plain = renderer
                .render(&device, &queue, [256, 256], &p)
                .read_rgba8(&device, &queue);
            p.grid = Some(GridParams::new(mesh.bounds));
            let floor = renderer
                .render(&device, &queue, [256, 256], &p)
                .read_rgba8(&device, &queue);
            assert_ne!(floor, plain);
            assert!(axis_pixels(&floor, 0) > 8, "red X guide");
            assert!(axis_pixels(&floor, 2) > 8, "blue Z guide");
            assert_eq!(axis_pixels(&floor, 1), 0, "Y must start hidden");
            p.grid.as_mut().unwrap().axes[1] = true;
            let up = renderer
                .render(&device, &queue, [256, 256], &p)
                .read_rgba8(&device, &queue);
            assert!(axis_pixels(&up, 1) > 8, "optional green upward guide");
            p.grid.as_mut().unwrap().axes = [false; 3];
            let gray = renderer
                .render(&device, &queue, [256, 256], &p)
                .read_rgba8(&device, &queue);
            assert!(
                (0..3).all(|axis| axis_pixels(&gray, axis) == 0),
                "axis toggles leave only the neutral grid"
            );
            p.grid = None;
            assert_eq!(
                renderer
                    .render(&device, &queue, [256, 256], &p)
                    .read_rgba8(&device, &queue),
                plain,
                "off restores exact background"
            );
            renderer.set_mesh(&device, &mesh);
            renderer.upload_channel(
                &device,
                &queue,
                0,
                ChannelKind::BaseColor,
                &TiledImage::new(64, 64, 4, [0.6, 0.6, 0.6, 1.0]),
                None,
            );
            let object = renderer
                .render(&device, &queue, [256, 256], &p)
                .read_rgba8(&device, &queue);
            p.grid = Some(GridParams::new(mesh.bounds));
            let guided = renderer
                .render(&device, &queue, [256, 256], &p)
                .read_rgba8(&device, &queue);
            assert_ne!(guided, object, "floor visible around the model");
            let mut checked = 0;
            let object_pixels = object.as_chunks::<4>().0;
            let guided_pixels = guided.as_chunks::<4>().0;
            for y in 2..254 {
                for x in 2..254 {
                    // Silhouette MSAA samples may correctly show guides through the
                    // uncovered part of an edge pixel; erode the foreground mask once.
                    let opaque = (y - 1..=y + 1).all(|ny| {
                        (x - 1..=x + 1)
                            .all(|nx| object_pixels[ny * 256 + nx][..3].iter().all(|&c| c > 100))
                    });
                    if opaque {
                        assert_eq!(
                            object_pixels[y * 256 + x],
                            guided_pixels[y * 256 + x],
                            "mesh depth occludes guides"
                        );
                        checked += 1;
                    }
                }
            }
            assert!(checked > 1000);
        }
    }
}
