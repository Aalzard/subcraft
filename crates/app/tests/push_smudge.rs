//! Push smudge (liquify): the paint under the brush moves with it and stays sharp, instead of
//! smearing a carried colour.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;
use subcraft_paint::{BrushMaterial, StrokeMode};

const RES: u32 = 256;

fn session() -> Session {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s.brush.settings.size = 6.0;
    s.brush.settings.hardness = 1.0;
    s
}
fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}
fn screen(cam: &Camera, x: f32, z: f32) -> Vec2 {
    cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate()
}
fn stroke(s: &mut Session, cam: &Camera, points: &[(f32, f32)]) {
    s.begin_stroke().unwrap();
    for &(x, z) in points {
        s.stroke_to(cam, screen(cam, x, z), false).unwrap();
    }
    s.end_stroke();
}
fn raster(s: &Session) -> TiledImage {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    p.channels[&K::BaseColor].clone()
}
fn alpha(img: &TiledImage, x: f32, z: f32) -> f32 {
    img.get(((x / 2.0 + 0.5) * RES as f32) as u32, ((z / 2.0 + 0.5) * RES as f32) as u32)[3]
}
/// Rightmost painted x on the row at `z` (alpha > 0.5).
fn right_edge(img: &TiledImage, z: f32) -> f32 {
    let y = ((z / 2.0 + 0.5) * RES as f32) as u32;
    let x = (0..RES).rev().find(|&x| img.get(x, y)[3] > 0.5).expect("paint on the row");
    (x as f32 + 0.5) / RES as f32 * 2.0 - 1.0
}
/// Texels on the row at `z` that are neither clear nor opaque.
fn soft_texels(img: &TiledImage, z: f32) -> usize {
    let y = ((z / 2.0 + 0.5) * RES as f32) as u32;
    (0..RES).filter(|&x| (0.1..0.9).contains(&img.get(x, y)[3])).count()
}

/// A thin vertical red strip at x = 0, then a smudge stroke across its middle (rightwards).
fn smudged(push: bool) -> (Session, TiledImage, TiledImage) {
    let mut s = session();
    let cam = camera();
    stroke(&mut s, &cam, &[(0.0, -0.8), (0.0, -0.4), (0.0, 0.0), (0.0, 0.4), (0.0, 0.8)]);
    let before = raster(&s);
    s.brush.mode = StrokeMode::Smudge;
    s.brush.settings.size = 16.0;
    s.brush.settings.hardness = 0.5;
    s.brush.settings.smudge_push = push;
    stroke(&mut s, &cam, &[(-0.2, 0.0), (0.0, 0.0), (0.2, 0.0), (0.4, 0.0)]);
    let after = raster(&s);
    (s, before, after)
}

#[test]
fn push_moves_the_strip_with_the_brush_and_keeps_it_sharp() {
    let (_, before, after) = smudged(true);
    let edge0 = right_edge(&before, 0.0);
    let edge1 = right_edge(&after, 0.0);
    assert!(edge1 > edge0 + 0.1, "the strip bent rightwards under the stroke: {edge0} -> {edge1}");
    // Away from the stroke the strip has not moved.
    assert_eq!(right_edge(&after, 0.75), right_edge(&before, 0.75));
    assert_eq!(alpha(&after, 0.0, 0.75), alpha(&before, 0.0, 0.75));
    // The colour is carried unchanged (no blending with clear surface makes it darker).
    let y = RES / 2;
    for x in 0..RES {
        let px = after.get(x, y);
        if px[3] > 0.5 {
            assert!(px[0] > 0.99 && px[1] < 0.01, "pushed paint keeps its colour: {px:?}");
        }
    }
    // A push keeps the edge sharp; a smear of the same stroke leaves a long soft trail.
    let (_, _, smeared) = smudged(false);
    let (sharp, soft) = (soft_texels(&after, 0.0), soft_texels(&smeared, 0.0));
    assert!(sharp * 2 < soft, "push: {sharp} soft texels, smear: {soft}");
}

#[test]
fn a_push_stroke_is_one_undo_step_and_deterministic() {
    let (mut s, before, after) = smudged(true);
    assert_eq!(after, smudged(true).2, "same stroke, same result");
    assert_ne!(after, before);
    s.execute(subcraft_app::Command::Undo).unwrap();
    assert_eq!(raster(&s), before, "one undo restores the strip");
}

#[test]
fn push_keeps_normals_unit_length() {
    let mut s = session();
    s.brush.material = BrushMaterial::with(&[(K::Normal, ChannelValue::rgb(0.9, 0.5, 0.8))]);
    let cam = camera();
    stroke(&mut s, &cam, &[(0.0, -0.5), (0.0, 0.0), (0.0, 0.5)]);
    s.brush.mode = StrokeMode::Smudge;
    s.brush.settings.size = 16.0;
    s.brush.settings.smudge_push = true;
    stroke(&mut s, &cam, &[(-0.2, 0.0), (0.1, 0.0), (0.4, 0.0)]);
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    let n = &p.channels[&K::Normal];
    let mut checked = 0;
    for y in (RES / 2 - 10)..(RES / 2 + 10) {
        for x in 0..RES {
            let px = n.get(x, y);
            if px[3] > 0.01 {
                let v = subcraft_core::normal::decode([px[0], px[1], px[2]]);
                assert!((v.length() - 1.0).abs() < 0.02, "unit normal at ({x},{y}): {v}");
                checked += 1;
            }
        }
    }
    assert!(checked > 50);
}
