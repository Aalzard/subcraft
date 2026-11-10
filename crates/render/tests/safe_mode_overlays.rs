//! Safe mode (or a GPU without 4x MSAA) turns multisampling off while the anti-aliasing
//! preference stays on. Every overlay drawn into the viewport pass (grid, symmetry guides,
//! wireframe, bake preview) must then use its single-sample pipeline: a sample-count mismatch is
//! a validation error on every frame (issue seen in the startup log as "grid pipeline").
//! Own test binary: safe mode is process-wide.
use glam::{Mat4, Vec3};
use subcraft_mesh::primitives;
use subcraft_render::{GridParams, ViewParams, ViewportRenderer, caps, headless};

#[test]
fn overlays_follow_the_real_sample_count_in_safe_mode() {
    let Some((_, device, queue)) = headless::device() else {
        eprintln!("no GPU adapter: skipping");
        return;
    };
    caps::set_safe_mode(true);
    let mesh = primitives::cube(2.0);
    let mut renderer = ViewportRenderer::new(&device, &queue);
    renderer.set_mesh(&device, &mesh);
    renderer.set_symmetry_guides(&device, &[[Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)]]);
    renderer.set_wireframe(&device, &[[Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)]], [1.0, 1.0, 1.0], 1.0);
    renderer.set_bake_preview(&device, Some((&mesh, &mesh)));
    let view = Mat4::look_at_rh(Vec3::new(3.0, 2.5, 4.0), Vec3::ZERO, Vec3::Y);
    let proj = Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0);
    let mut p = ViewParams::new(view, proj);
    p.antialias = true; // the preference stays on; the device cannot honour it
    p.grid = Some(GridParams::new(mesh.bounds));
    p.symmetry_guides = true;
    p.wireframe = true;
    for overlay in [false, true] {
        p.bake_overlay = overlay;
        device.push_error_scope(subcraft_render::wgpu_api::ErrorFilter::Validation);
        let image = renderer.render(&device, &queue, [128, 128], &p).read_rgba8(&device, &queue);
        let error = pollster::block_on(device.pop_error_scope());
        assert!(error.is_none(), "bake overlay {overlay}: {error:?}");
        assert!(image.iter().any(|&b| b != image[0]), "something was drawn");
    }
}
