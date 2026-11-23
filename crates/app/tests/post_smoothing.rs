//! Stabilizer post-smoothing: a finished stroke is painted again along a smoothed path.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_mesh::primitives;
use subcraft_paint::BrushMaterial;

const RES: u32 = 256;

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512])
}

fn session(post: f32) -> Session {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 1.0, 1.0))]);
    s.brush.settings.size = 1.5;
    s.brush.settings.hardness = 1.0;
    let options = subcraft_paint::StrokeOptions { post_smoothing: post, ..s.brush.options };
    s.execute(Command::SetStrokeOptions(options)).unwrap();
    s
}

/// A left-to-right stroke across the middle with a 6 px zigzag every point.
fn zigzag(s: &mut Session, line_at_end: bool) {
    let cam = camera();
    let a = cam.project(Vec3::new(-0.7, 0.0, 0.0)).unwrap().truncate();
    let b = cam.project(Vec3::new(0.7, 0.0, 0.0)).unwrap().truncate();
    s.begin_stroke().unwrap();
    let n = 120;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let jitter = if i == 0 || i == n { 0.0 } else if i % 2 == 0 { 6.0 } else { -6.0 };
        s.stroke_to(&cam, a.lerp(b, t) + Vec2::new(0.0, jitter), false).unwrap();
    }
    if line_at_end {
        s.stroke_to(&cam, b + Vec2::new(0.0, 40.0), true).unwrap();
    }
    s.end_stroke();
}

fn alpha(s: &Session) -> Vec<f32> {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    p.channels.get(&K::BaseColor).map_or(vec![0.0; (RES * RES) as usize], |img| img.to_dense().chunks(4).map(|px| px[3]).collect())
}

/// Per texel column across the middle of the stroke: how far the painted band reaches from the
/// stroke's line (max distance of a painted texel from the centre row), averaged.
fn spread(a: &[f32]) -> f32 {
    let mid = RES as f32 / 2.0;
    let cols: Vec<f32> = (RES / 4..RES * 3 / 4)
        .map(|x| (0..RES).filter(|&y| a[(y * RES + x) as usize] > 0.5).map(|y| (y as f32 + 0.5 - mid).abs()).fold(0.0, f32::max))
        .collect();
    cols.iter().sum::<f32>() / cols.len() as f32
}

#[test]
fn post_smoothing_straightens_a_shaky_stroke_and_is_one_undo_step() {
    let mut raw = session(0.0);
    zigzag(&mut raw, false);
    let mut smooth = session(1.0);
    let before = alpha(&smooth);
    zigzag(&mut smooth, false);
    let (r, sm) = (spread(&alpha(&raw)), spread(&alpha(&smooth)));
    assert!(sm < r * 0.75, "smoothed band {sm} vs raw band {r}");
    assert!(sm > 0.5, "the smoothed stroke was painted: {sm}");
    // The stroke still starts and ends where it was drawn.
    let a = alpha(&smooth);
    let painted_x = |x: u32| (0..RES).any(|y| a[(y * RES + x) as usize] > 0.5);
    let (x0, x1) = (((-0.7f32 / 2.0 + 0.5) * RES as f32) as u32, ((0.7f32 / 2.0 + 0.5) * RES as f32) as u32);
    assert!(painted_x(x0 + 1) && painted_x(x1 - 1));
    assert_eq!(smooth.history().entries().len(), raw.history().entries().len(), "one history step either way");
    smooth.execute(Command::Undo).unwrap();
    assert_eq!(alpha(&smooth), before);
}

#[test]
fn a_stroke_with_a_straight_line_is_not_replayed() {
    let mut raw = session(0.0);
    zigzag(&mut raw, true);
    let mut smooth = session(1.0);
    zigzag(&mut smooth, true);
    assert_eq!(alpha(&raw), alpha(&smooth));
}
