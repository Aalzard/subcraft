//! Settings > General > Show world axes: a small X / Y / Z gizmo in the corner of the 3D view that
//! turns with the camera (Painter's world-axes display). The projection is pure and unit-tested.

use glam::{Mat4, Vec2, Vec3};

/// One axis of the gizmo: which (0 = X, 1 = Y, 2 = Z), its screen direction (y down, length up to
/// 1) and how far it points away from the viewer (positive = into the screen).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub index: usize,
    pub dir: Vec2,
    pub depth: f32,
}

/// The three world axes seen through `view`, farthest first (so nearer ones are drawn on top).
pub fn axes(view: Mat4) -> [Axis; 3] {
    let mut out = [Vec3::X, Vec3::Y, Vec3::Z].map(|w| {
        let v = view.transform_vector3(w);
        Axis { index: 0, dir: Vec2::new(v.x, -v.y), depth: -v.z }
    });
    for (i, a) in out.iter_mut().enumerate() {
        a.index = i;
    }
    out.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    out
}

/// Draw the gizmo centred at `center` with arms `radius` long.
pub fn draw(painter: &egui::Painter, center: egui::Pos2, radius: f32, view: Mat4) {
    const COLORS: [egui::Color32; 3] = [egui::Color32::from_rgb(230, 80, 80), egui::Color32::from_rgb(110, 200, 90), egui::Color32::from_rgb(80, 140, 240)];
    const NAMES: [&str; 3] = ["X", "Y", "Z"];
    painter.circle_filled(center, radius + 9.0, egui::Color32::from_black_alpha(70));
    for a in axes(view) {
        let tip = center + egui::vec2(a.dir.x, a.dir.y) * radius;
        // Axes pointing away from the viewer are drawn dimmer.
        let color = if a.depth > 0.05 { COLORS[a.index].gamma_multiply(0.55) } else { COLORS[a.index] };
        painter.line_segment([center, tip], egui::Stroke::new(2.0, color));
        painter.circle_filled(tip, 6.5, color);
        painter.text(tip, egui::Align2::CENTER_CENTER, NAMES[a.index], egui::FontId::proportional(9.5), egui::Color32::BLACK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camera_looking_down_minus_z_sees_x_right_y_up_and_z_towards_it() {
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let a = axes(view);
        let by = |i: usize| *a.iter().find(|x| x.index == i).unwrap();
        assert!((by(0).dir - Vec2::new(1.0, 0.0)).length() < 1e-5, "X points right");
        assert!((by(1).dir - Vec2::new(0.0, -1.0)).length() < 1e-5, "Y points up (screen y is down)");
        assert!(by(2).dir.length() < 1e-5 && by(2).depth < 0.0, "Z points at the viewer");
        assert_eq!(a[2].index, 2, "the axis nearest the viewer is drawn last");
    }

    #[test]
    fn the_gizmo_turns_with_the_camera() {
        // From the +X side looking at the origin, the camera's right is -Z.
        let view = Mat4::look_at_rh(Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Y);
        let a = axes(view);
        let by = |i: usize| *a.iter().find(|x| x.index == i).unwrap();
        assert!(by(0).depth < 0.0, "X now points at the viewer");
        assert!((by(2).dir - Vec2::new(-1.0, 0.0)).length() < 1e-5, "seen from +X, +Z points to the left");
        assert!(a.windows(2).all(|w| w[0].depth >= w[1].depth), "farthest first");
    }
}
