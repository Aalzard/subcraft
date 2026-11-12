//! Stencil through the session: blocks strokes where black, bypass (held N) ignores it.

use std::sync::Arc;

use glam::Vec3;
use subcraft_app::{Camera, Session};
use subcraft_image::Bitmap;
use subcraft_mesh::primitives;
use subcraft_paint::Stencil;

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.05, 100.0, [512, 512])
}

fn stroke(s: &mut Session) -> bool {
    let before = s.document().clone();
    let cam = camera();
    s.begin_stroke().unwrap();
    for k in 0..=10 {
        let p = cam.project(Vec3::new(-0.5 + k as f32 * 0.1, 0.0, 1.0)).unwrap().truncate();
        s.stroke_to(&cam, p, false).unwrap();
    }
    s.end_stroke();
    s.document() != &before
}

#[test]
fn black_stencil_blocks_and_bypass_paints() {
    let mut s = Session::new(primitives::cube(2.0), 128);
    let black = Arc::new(Bitmap::new_u8(64, 64, 1, vec![0; 64 * 64]));
    s.brush.stencil = Some(Arc::new(Stencil { scale: 2.0, ..Stencil::new(black) }));
    assert!(!stroke(&mut s), "fully black stencil: nothing painted");
    s.brush.stencil_bypass = true;
    assert!(stroke(&mut s), "bypass (held N) paints normally");
    s.brush.stencil_bypass = false;
    s.brush.stencil = None;
    assert!(stroke(&mut s), "no stencil paints");
}

/// A small white stencil beside the stroke: it blocks the stroke unless it repeats along the
/// axis that reaches it.
#[test]
fn tiling_modes_repeat_the_stencil_along_one_or_both_axes() {
    use subcraft_paint::StencilTiling;
    let white = Arc::new(Bitmap::new_u8(8, 8, 1, vec![255; 64]));
    // 5 % of the view, at the left edge, on the row the stroke runs along (the view's middle).
    let stencil = |tiling| Some(Arc::new(Stencil { scale: 0.05, center: glam::Vec2::new(0.05, 0.5), tiling, ..Stencil::new(white.clone()) }));
    for (tiling, paints) in [(StencilTiling::None, false), (StencilTiling::Vertical, false), (StencilTiling::Horizontal, true), (StencilTiling::Both, true)] {
        let mut s = Session::new(primitives::cube(2.0), 128);
        s.brush.stencil = stencil(tiling);
        assert_eq!(stroke(&mut s), paints, "{tiling:?}");
    }
}
