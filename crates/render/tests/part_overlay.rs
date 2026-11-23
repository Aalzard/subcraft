//! The part overlay of the viewport shader (geometry mask and Polygon Fill feedback). Skips
//! (passes with a message) when no adapter is available.

use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_render::{ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 64;

#[test]
fn the_overlay_greys_and_tints_its_texture_set_and_goes_away_again() {
    let Some((_, device, queue)) = headless::device() else { eprintln!("no adapter: skipping"); return };
    let mut r = ViewportRenderer::new(&device, &queue);
    r.set_mesh(&device, &primitives::cube(2.0));
    r.upload_channel(&device, &queue, 0, ChannelKind::BaseColor, &TiledImage::new(64, 64, 4, [0.1, 0.5, 0.2, 1.0]), None);
    let mut p = ViewParams::new(Mat4::look_at_rh(Vec3::new(3.0, 2.5, 4.0), Vec3::ZERO, Vec3::Y), Mat4::perspective_rh(45f32.to_radians(), 1.0, 0.1, 100.0));
    p.antialias = false;
    let whole = |rg: [u8; 2]| rg.repeat(16);
    let mut shot = |overlay: Option<(u32, [u8; 2])>| {
        let texels = overlay.map(|(slot, rg)| (slot, whole(rg)));
        r.set_part_overlay(&device, &queue, texels.as_ref().map(|(slot, t)| (*slot, 4, t.as_slice())));
        r.render(&device, &queue, [SIZE, SIZE], &p).read_rgba8(&device, &queue)
    };
    let plain = shot(None);
    let empty = shot(Some((0, [0, 0])));
    let greyed = shot(Some((0, [255, 0])));
    let tinted = shot(Some((0, [0, 255])));
    let other = shot(Some((7, [255, 255])));
    let back = shot(None);
    assert_eq!(empty, plain, "an empty overlay changes nothing");
    assert_eq!(other, plain, "an overlay for a slot the mesh does not use changes nothing");
    assert_eq!(back, plain, "the overlay goes away again");
    let centre = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    let px = |img: &[u8]| [img[centre] as i32, img[centre + 1] as i32, img[centre + 2] as i32];
    let (a, g, t) = (px(&plain), px(&greyed), px(&tinted));
    assert!(a[1] > a[0] + 20, "the plain surface is green: {a:?}");
    assert!((g[1] - g[0]) * 3 < a[1] - a[0], "left-out geometry loses most of its colour: {a:?} -> {g:?}");
    assert_ne!(g, a, "and it is drawn differently");
    assert!(t[0] > a[0] + 20 && t[0] > t[2], "the hovered element turns warm: {a:?} -> {t:?}");
    assert_eq!(plain[0..4], greyed[0..4], "the background is untouched");
}
