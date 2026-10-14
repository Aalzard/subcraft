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
