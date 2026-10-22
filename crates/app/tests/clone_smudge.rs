//! Clone and Smudge strokes, and camera-projected brush images, on a flat plane.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_image::{Bitmap, TiledImage};
use subcraft_mesh::primitives;
use subcraft_paint::{BrushMaterial, PaintError, StrokeMode};

const RES: u32 = 256;

fn session() -> Session {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s.brush.settings.size = 18.0;
    s.brush.settings.hardness = 1.0;
    s
}
/// Looking straight down at the plane; `up` decides how the plane is rotated on screen.
fn camera(up: Vec3) -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, up, 40.0, 0.1, 100.0, [512, 512])
}
fn screen(cam: &Camera, x: f32, z: f32) -> Vec2 { cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate() }
fn stroke(s: &mut Session, cam: &Camera, points: &[(f32, f32)]) {
    s.begin_stroke().unwrap();
    for &(x, z) in points { s.stroke_to(cam, screen(cam, x, z), false).unwrap(); }
    s.end_stroke();
}
fn raster(s: &Session, k: K) -> TiledImage {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    p.channels[&k].clone()
}
/// Texel at world `(x, z)` of the plane (u = x/2 + .5, v = z/2 + .5).
fn at(img: &TiledImage, x: f32, z: f32) -> [f32; 4] {
    img.get(((x / 2.0 + 0.5) * RES as f32) as u32, ((z / 2.0 + 0.5) * RES as f32) as u32)
}

#[test]
fn smudge_drags_paint_into_clean_surface_in_one_undo_step_and_leaves_the_rest_alone() {
    let mut s = session();
    let cam = camera(Vec3::NEG_Z);
    stroke(&mut s, &cam, &[(-0.8, 0.0), (-0.5, 0.0), (-0.2, 0.0)]);
    let painted = raster(&s, K::BaseColor);
    assert!(at(&painted, 0.3, 0.0)[3] < 1e-4 && at(&painted, -0.5, 0.0)[3] > 0.99);
    let (doc, entries) = (s.document().clone(), s.history().entries().len());

    s.brush.mode = StrokeMode::Smudge;
    stroke(&mut s, &cam, &[(-0.4, 0.0), (-0.2, 0.0), (0.0, 0.0), (0.2, 0.0), (0.4, 0.0), (0.6, 0.0)]);
    let after = raster(&s, K::BaseColor);
    let dragged = at(&after, 0.3, 0.0);
    assert!(dragged[3] > 0.05 && dragged[0] > 0.9 && dragged[1] < 0.05, "paint reaches clean surface: {dragged:?}");
    // The start of the stroke thins out as paint is carried away only gradually; far texels are exact.
    for (x, z) in [(0.0, 0.8), (-0.9, -0.9), (0.9, 0.9)] {
        assert_eq!(at(&after, x, z), at(&painted, x, z), "untouched texel ({x},{z}) changed");
    }
    assert_eq!(s.history().entries().len(), entries + 1, "one smudge stroke is one history entry");
    s.execute(subcraft_app::Command::Undo).unwrap();
    assert_eq!(s.document(), &doc);
}

#[test]
fn smudge_is_deterministic_and_works_on_mask_rasters() {
    let run = || {
        let mut s = session();
        let cam = camera(Vec3::NEG_Z);
        stroke(&mut s, &cam, &[(-0.7, 0.0), (-0.4, 0.0)]);
        s.brush.mode = StrokeMode::Smudge;
        stroke(&mut s, &cam, &[(-0.5, 0.0), (-0.2, 0.0), (0.1, 0.0)]);
        raster(&s, K::BaseColor)
    };
    assert_eq!(run(), run());
    // A mask: white strip painted, smudged onto black.
    let mut s = session();
    let layer = s.selection().layer.unwrap();
    s.execute(subcraft_app::Command::AddMask { layer, white: false }).unwrap();
    s.brush.target = subcraft_paint::PaintTarget::MaskBase;
    let cam = camera(Vec3::NEG_Z);
    stroke(&mut s, &cam, &[(-0.7, 0.0), (-0.4, 0.0)]);
    s.brush.mode = StrokeMode::Smudge;
    stroke(&mut s, &cam, &[(-0.5, 0.0), (-0.2, 0.0), (0.1, 0.0)]);
    let mask = &s.selected_layer().unwrap().mask.as_ref().unwrap().base;
    assert!(at(mask, -0.1, 0.0)[0] > 0.05, "mask value dragged along: {:?}", at(mask, -0.1, 0.0));
    assert!(at(mask, 0.8, 0.0)[0] < 1e-4);
}

#[test]
fn clone_copies_the_paint_at_the_source_to_where_the_stroke_goes_and_keeps_its_offset() {
    let mut s = session();
    let cam = camera(Vec3::NEG_Z);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(0.1, 0.9, 0.1))]);
    stroke(&mut s, &cam, &[(-0.5, 0.0)]);
    assert!(at(&raster(&s, K::BaseColor), 0.5, 0.0)[3] < 1e-4);

    // Without a source the stroke cannot start.
    s.brush.mode = StrokeMode::Clone;
    assert_eq!(s.begin_stroke().unwrap_err(), PaintError::NoCloneSource);
    assert!(s.set_clone_source(&cam, screen(&cam, -0.5, 0.0)));
    assert_eq!(s.clone_offset(), None, "the first dab fixes the offset");
    stroke(&mut s, &cam, &[(0.5, 0.0)]);
    let copy = at(&raster(&s, K::BaseColor), 0.5, 0.0);
    assert!(copy[3] > 0.95 && copy[1] > 0.5 && copy[0] < 0.3, "the green dab was cloned: {copy:?}");
    let offset = s.clone_offset().expect("offset kept after the first stroke");
    let expected = screen(&cam, -0.5, 0.0) - screen(&cam, 0.5, 0.0);
    assert!((offset - expected).length() < 1.0, "{offset:?} vs {expected:?}");

    // A later stroke keeps the offset (aligned): painting at z = .7 copies from x = -.5 shifted too.
    s.brush.mode = StrokeMode::Paint;
    stroke(&mut s, &cam, &[(-0.5, 0.7)]);
    s.brush.mode = StrokeMode::Clone;
    stroke(&mut s, &cam, &[(0.5, 0.7)]);
    assert_eq!(s.clone_offset(), Some(offset));
    let second = at(&raster(&s, K::BaseColor), 0.5, 0.7);
    assert!(second[3] > 0.95 && second[1] > 0.5, "aligned second clone: {second:?}");
    // Picking a new source forgets the old offset.
    assert!(s.set_clone_source(&cam, screen(&cam, 0.2, 0.2)));
    assert_eq!(s.clone_offset(), None);
}

#[test]
fn clone_does_not_copy_empty_surface_over_existing_paint() {
    let mut s = session();
    let cam = camera(Vec3::NEG_Z);
    stroke(&mut s, &cam, &[(0.5, 0.0)]);
    let before = at(&raster(&s, K::BaseColor), 0.5, 0.0);
    s.brush.mode = StrokeMode::Clone;
    assert!(s.set_clone_source(&cam, screen(&cam, -0.5, 0.0))); // nothing painted there
    stroke(&mut s, &cam, &[(0.5, 0.0)]);
    assert_eq!(at(&raster(&s, K::BaseColor), 0.5, 0.0), before, "an empty source leaves the destination as it was");
}

#[test]
fn camera_projection_maps_the_image_from_the_view_not_through_the_uvs() {
    let split = |camera_projection: bool, up: Vec3| {
        let mut s = session();
        s.brush.settings.size = 45.0;
        // Left half red, right half blue (2 x 1 image).
        s.set_brush_channel_bitmap(K::BaseColor, "split", Bitmap::new_u8(2, 1, 3, vec![255, 0, 0, 0, 0, 255])).unwrap();
        s.brush.material.camera_projection = camera_projection;
        s.brush.material.texture_tiling = false;
        let cam = camera(up);
        let line: Vec<(f32, f32)> = (0..=24).map(|i| (-0.6, -0.9 + 0.075 * i as f32)).collect();
        stroke(&mut s, &cam, &line);
        (raster(&s, K::BaseColor), cam)
    };
    // Plane rotated a quarter turn on screen: screen-x follows world z, UV u follows world x.
    let (projected, cam) = split(true, Vec3::X);
    let (a, b) = ((-0.6f32, -0.6f32), (-0.6f32, 0.6f32));
    let left_is_a = screen(&cam, a.0, a.1).x < screen(&cam, b.0, b.1).x;
    let (pa, pb) = (at(&projected, a.0, a.1), at(&projected, b.0, b.1));
    let (red_side, blue_side) = if left_is_a { (pa, pb) } else { (pb, pa) };
    assert!(red_side[0] > 0.8 && red_side[2] < 0.2, "left of the view is red: {red_side:?}");
    assert!(blue_side[2] > 0.8 && blue_side[0] < 0.2, "right of the view is blue: {blue_side:?}");
    // Through the UVs both points share u = 0.2, so they get the same colour.
    let (uv_mode, _) = split(false, Vec3::X);
    let (ua, ub) = (at(&uv_mode, a.0, a.1), at(&uv_mode, b.0, b.1));
    assert!((ua[0] - ub[0]).abs() < 0.05 && (ua[2] - ub[2]).abs() < 0.05, "UV mode ignores the view: {ua:?} {ub:?}");
}
