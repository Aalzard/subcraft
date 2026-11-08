//! `Session::stroke_to_many` (all of a frame's pen/mouse positions in one pass) against the
//! one-position-per-call `stroke_to` the viewport used before.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_mesh::primitives;

fn camera() -> Camera {
    Camera::orthographic(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y, 1.3, 0.01, 20.0, [512, 512])
}

fn painted(many: bool, per_frame: usize) -> Vec<[f32; 4]> {
    let mut s = Session::new(primitives::uv_sphere(1.0, 32, 32), 256);
    s.brush.settings.size = 12.0;
    s.brush.settings.hardness = 0.3;
    s.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.2, 0.1));
    let cam = camera();
    let events: Vec<Vec2> = (0..120).map(|i| Vec2::new(120.0 + 2.6 * i as f32, 256.0 + 60.0 * (i as f32 * 0.07).sin())).collect();
    s.begin_stroke().unwrap();
    for frame in events.chunks(per_frame) {
        if many {
            s.stroke_to_many(&cam, frame).unwrap();
        } else {
            for &p in frame {
                s.stroke_to(&cam, p, false).unwrap();
            }
        }
    }
    s.end_stroke();
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    let img = &p.channels[&ChannelKind::BaseColor];
    (0..256u32).flat_map(|y| (0..256u32).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)).collect()
}

#[test]
fn one_position_per_frame_is_exactly_stroke_to() {
    assert_eq!(painted(true, 1), painted(false, 1));
}

#[test]
fn several_positions_per_frame_paint_the_same_stroke_as_one_call_each() {
    // Orthographic camera: batching changes nothing about the stamps (documented on add_points).
    for per_frame in [3, 8, 120] {
        let (a, b) = (painted(true, per_frame), painted(false, per_frame));
        assert!(a.iter().filter(|p| p[3] > 0.0).count() > 400);
        assert_eq!(a, b, "{per_frame} positions per frame");
    }
}
