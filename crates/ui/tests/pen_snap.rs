//! Ctrl angle snapping when adding a pen point: the direction from the previous point turns to
//! the nearest 15 degree step at the same distance.
use egui::Pos2;
use subcraft_ui::viewport::snap_angle;

#[test]
fn snapping_turns_the_direction_to_the_nearest_step_and_keeps_the_distance() {
    let step = 15f32.to_radians();
    let from = Pos2::new(100.0, 100.0);
    // 10 degrees below the horizontal snaps to 15.
    let to = from + egui::vec2(10f32.to_radians().cos(), 10f32.to_radians().sin()) * 50.0;
    let s = snap_angle(from, to, step);
    let angle = (s.y - from.y).atan2(s.x - from.x).to_degrees();
    assert!((angle - 15.0).abs() < 1e-3, "{angle}");
    assert!((s.distance(from) - 50.0).abs() < 1e-3);
    // 4 degrees snaps back to horizontal; a zero-length move is left alone.
    let near = from + egui::vec2(4f32.to_radians().cos(), 4f32.to_radians().sin()) * 30.0;
    assert!((snap_angle(from, near, step).y - from.y).abs() < 1e-3);
    assert_eq!(snap_angle(from, from, step), from);
}
