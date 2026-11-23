//! Texture set highlight in the viewport shader. Skips (passes with a message) when no adapter
//! is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::{ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 64;

#[test]
fn the_highlighted_texture_set_is_tinted_and_nothing_else_changes() {
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::cube(2.0));
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &TiledImage::new(64, 64, 4, [0.3, 0.3, 0.3, 1.0]), None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(3.0, 2.5, 4.0), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0));
    p.antialias = false;
    let mut shot = |slot: Option<u32>| {
        p.highlight_slot = slot;
        r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue)
    };
    let plain = shot(None);
    let lit = shot(Some(0));
    let other = shot(Some(7));
    let back = shot(None);
    assert_eq!(other, plain, "highlighting a slot the mesh does not use changes nothing");
    assert_eq!(back, plain, "the tint goes away again");
    let centre = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    let (a, b) = (&plain[centre..centre + 3], &lit[centre..centre + 3]);
    assert!(b[0] > a[0] + 10 && b[0] > b[2], "the highlighted set turns warm: {a:?} -> {b:?}");
    let corner = 0usize;
    assert_eq!(plain[corner..corner + 4], lit[corner..corner + 4], "the background is untouched");
}
