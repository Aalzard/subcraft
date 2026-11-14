//! Manipulator of a fill layer's 3D projection in the 3D view (Painter's projection manipulator),
//! shown while the selected layer is a fill layer with a tri-planar, planar, spherical or
//! cylindrical projection. It edits the projection box's 3D Offset, Rotation and Scale
//! (`ProjectionTransform`), the same values as the "3D projection" group in Properties.
//!
//! * Translate: drag an axis handle to move along that axis of the box, or the centre handle to
//!   move in the view plane.
//! * Rotate: drag a ring to turn the box about that axis.
//! * Scale: drag an axis handle to scale along that axis, or the centre handle (left / right) to
//!   scale all three.
//! * Shift while dragging snaps to the steps of the manipulator settings.
//!
//! Every drag is one undo step (the projection edit merges until the release seals it).

use egui::{Color32, Pos2, Rect, Stroke, Ui};
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command};
use subcraft_core::LayerId;
use subcraft_doc::{LayerContent, Projection, ProjectionTransform};

use crate::state::UiState;
use crate::warp_view::CubeFrame;

/// What the manipulator's handles do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GizmoMode {
    Translate,
    Rotate,
    Scale,
}

/// Manipulator state kept across frames (not saved).
#[derive(Clone, Debug, PartialEq)]
pub struct GizmoEdit {
    pub visible: bool,
    pub mode: GizmoMode,
    /// Size factor of the handles.
    pub size: f32,
    /// Shift+drag step for translation (bounding-box units) and scale.
    pub grid_step: f32,
    /// Shift+drag step for rotation, in degrees.
    pub angle_step: f32,
}

impl Default for GizmoEdit {
    fn default() -> Self {
        Self { visible: true, mode: GizmoMode::Translate, size: 1.0, grid_step: 0.05, angle_step: 15.0 }
    }
}

impl GizmoEdit {
    pub const SIZE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=2.0;

    fn size(&self) -> f32 {
        if self.size.is_finite() { self.size.clamp(*Self::SIZE_RANGE.start(), *Self::SIZE_RANGE.end()) } else { 1.0 }
    }
    fn grid(&self) -> f32 {
        if self.grid_step.is_finite() { self.grid_step.clamp(0.001, 1.0) } else { 0.05 }
    }
    fn angle(&self) -> f32 {
        if self.angle_step.is_finite() { self.angle_step.clamp(1.0, 90.0) } else { 15.0 }
    }
}

/// Manipulator settings (Painter's contextual-toolbar set): show / hide, the tool, the handle
/// size and the snapping steps.
pub fn settings_ui(ui: &mut Ui, gizmo: &mut GizmoEdit) {
    crate::widgets::prop_row(ui, "Manipulator", |ui| {
        ui.checkbox(&mut gizmo.visible, "Show").on_hover_text("Show the projection manipulator in the 3D view");
        ui.selectable_value(&mut gizmo.mode, GizmoMode::Translate, "Move").on_hover_text("Drag an axis handle, or the centre to move in the view plane");
        ui.selectable_value(&mut gizmo.mode, GizmoMode::Rotate, "Rotate").on_hover_text("Drag a ring to turn the projection about that axis");
        ui.selectable_value(&mut gizmo.mode, GizmoMode::Scale, "Scale").on_hover_text("Drag an axis handle, or the centre to scale all three axes");
    });
    crate::widgets::prop_row(ui, "Manipulator size", |ui| {
        ui.add(egui::Slider::new(&mut gizmo.size, GizmoEdit::SIZE_RANGE));
    });
    crate::widgets::prop_row(ui, "Snap steps", |ui| {
        ui.add(egui::DragValue::new(&mut gizmo.grid_step).speed(0.005).range(0.001..=1.0).max_decimals(3)).on_hover_text("Move and scale step while Shift is held");
        ui.add(egui::DragValue::new(&mut gizmo.angle_step).speed(0.5).range(1.0..=90.0).suffix("\u{00b0}")).on_hover_text("Rotation step while Shift is held");
    });
}

/// The selected fill layer whose projection has a 3D transform, if any.
pub fn active(state: &UiState) -> Option<(LayerId, Projection)> {
    let layer = state.session.selected_layer()?;
    match &layer.content {
        LayerContent::Fill(f) if f.projection.has_transform3() => Some((layer.id, f.projection)),
        _ => None,
    }
}

/// The manipulator in object space: the box centre, the box's three axes (unit length) and the
/// handle length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub centre: Vec3,
    pub axes: [Vec3; 3],
    pub len: f32,
}

pub fn layout(t: &ProjectionTransform, frame: CubeFrame, size: f32) -> Layout {
    let r = t.rotation();
    let axes = [0usize, 1, 2].map(|i| Vec3::new(r[0][i], r[1][i], r[2][i]));
    let centre = frame.to_object([0.5 + t.offset[0], 0.5 + t.offset[1], 0.5 + t.offset[2]]);
    Layout { centre, axes, len: frame.extent * 0.3 * size }
}

fn snap(v: f32, step: Option<f32>) -> f32 {
    match step {
        Some(s) if s > 0.0 => (v / s).round() * s,
        _ => v,
    }
}

/// `start` moved by `delta` (bounding-box units); with a step, each offset snaps to it.
pub fn translated(start: &ProjectionTransform, delta: Vec3, step: Option<f32>) -> ProjectionTransform {
    let m = ProjectionTransform::MAX_OFFSET;
    let d = delta.to_array();
    let mut t = *start;
    for i in 0..3 {
        let moved = start.offset[i] + d[i];
        // An axis the drag does not move keeps its value (no snapping of what was not touched).
        t.offset[i] = if d[i].abs() < 1e-9 { start.offset[i] } else { snap(moved, step).clamp(-m, m) };
    }
    t
}

/// `start` scaled by `factor` on `axis` (`None`: all three); with a step, the scale snaps to it.
pub fn scaled(start: &ProjectionTransform, axis: Option<usize>, factor: f32, step: Option<f32>) -> ProjectionTransform {
    let mut t = *start;
    for i in 0..3 {
        if axis.is_none_or(|a| a == i) {
            t.scale[i] = snap(start.scale[i] * factor.max(0.0), step).clamp(ProjectionTransform::MIN_SCALE, ProjectionTransform::MAX_SCALE);
        }
    }
    t
}

/// `start` turned by `deg` about its own `axis`; with a step, the angle snaps to it.
pub fn rotated(start: &ProjectionTransform, axis: usize, deg: f32, step: Option<f32>) -> ProjectionTransform {
    let mut t = *start;
    t.rotation_deg = start.rotated_about_local_axis(axis.min(2), snap(deg, step));
    t
}

/// How many times the segment `a -> b` the pointer moved along it from `from` to `to`.
pub fn along(from: Pos2, to: Pos2, a: Pos2, b: Pos2) -> f32 {
    let d = b - a;
    let l2 = d.length_sq();
    if l2 < 1e-6 { 0.0 } else { (to - from).dot(d) / l2 }
}

/// Signed screen angle in degrees from direction `a` to direction `b` about `centre`, in
/// -180..=180 (positive is clockwise on screen, where y grows downward).
pub fn screen_turn(centre: Pos2, a: Pos2, b: Pos2) -> f32 {
    let (u, v) = (a - centre, b - centre);
    let d = (v.y.atan2(v.x) - u.y.atan2(u.x)).to_degrees();
    (d + 180.0).rem_euclid(360.0) - 180.0
}

pub fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    let t = if l2 < 1e-9 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

fn to_pos(camera: &Camera, rect: Rect, pixel_scale: Vec2, o: Vec3) -> Option<Pos2> {
    camera.project(o).map(|s| rect.min + egui::vec2(s.x / pixel_scale.x, s.y / pixel_scale.y))
}

/// The point of the view plane through `anchor` under screen position `screen` (paint pixels).
fn on_view_plane(camera: &Camera, anchor: Vec3, screen: Vec2) -> Option<Vec3> {
    let (o, d) = camera.ray(screen);
    let n = camera.forward();
    let den = d.dot(n);
    if den.abs() < 1e-8 {
        return None;
    }
    let t = (anchor - o).dot(n) / den;
    (t.is_finite() && t > 0.0).then(|| o + d * t)
}

const RING_SEGMENTS: usize = 48;
const HANDLE_RADIUS: f32 = 6.0;

/// Screen points of the ring about `axis`.
fn ring(lay: &Layout, axis: usize, camera: &Camera, rect: Rect, pixel_scale: Vec2) -> Vec<Option<Pos2>> {
    let (b, d) = (lay.axes[(axis + 1) % 3], lay.axes[(axis + 2) % 3]);
    (0..=RING_SEGMENTS)
        .map(|k| {
            let a = k as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            to_pos(camera, rect, pixel_scale, lay.centre + (b * a.cos() + d * a.sin()) * lay.len)
        })
        .collect()
}

fn ring_distance(points: &[Option<Pos2>], p: Pos2) -> f32 {
    points
        .windows(2)
        .filter_map(|w| match (w[0], w[1]) {
            (Some(a), Some(b)) => Some(distance_to_segment(p, a, b)),
            _ => None,
        })
        .fold(f32::INFINITY, f32::min)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    Axis(usize),
    Centre,
    Ring(usize),
}

/// A drag in progress: the handle, the transform and the manipulator at the press, and where
/// the press was.
#[derive(Clone, Copy, Debug)]
struct Gesture {
    handle: Handle,
    start: ProjectionTransform,
    lay: Layout,
    from: Pos2,
    /// Screen positions of the centre and of the dragged axis' end at the press.
    centre: Pos2,
    end: Pos2,
    /// Whether the dragged ring's axis pointed toward the viewer at the press.
    toward_viewer: bool,
}

fn gesture_id() -> egui::Id {
    egui::Id::new("projection_gizmo_gesture")
}

const AXIS_COLORS: [Color32; 3] = [Color32::from_rgb(235, 80, 80), Color32::from_rgb(110, 210, 90), Color32::from_rgb(90, 140, 245)];

/// Manipulator input before painting. Returns whether the pointer belongs to the manipulator
/// this frame (the view then ignores the press).
#[allow(clippy::too_many_arguments)]
pub fn manipulate(
    state: &mut UiState,
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    pixel_scale: Vec2,
    camera: &Camera,
    primary_pressed: bool,
    primary_down: bool,
    pointer: Option<Pos2>,
) -> bool {
    let Some((layer, projection)) = active(state).filter(|_| state.gizmo.visible) else {
        ui.data_mut(|d| d.remove::<Gesture>(gesture_id()));
        return false;
    };
    let frame = CubeFrame::of(state);
    let lay = layout(&projection.transform3, frame, state.gizmo.size());
    let mode = state.gizmo.mode;
    let to_paint = |p: Pos2| Vec2::new((p.x - rect.min.x) * pixel_scale.x, (p.y - rect.min.y) * pixel_scale.y);
    let mut gesture: Option<Gesture> = ui.data(|d| d.get_temp(gesture_id()));

    if gesture.is_none() && primary_pressed && response.hovered() && let Some(p) = pointer
        && let Some(centre) = to_pos(camera, rect, pixel_scale, lay.centre)
    {
        let ends = [0usize, 1, 2].map(|i| to_pos(camera, rect, pixel_scale, lay.centre + lay.axes[i] * lay.len));
        let hit = if mode == GizmoMode::Rotate {
            (0..3)
                .map(|i| (i, ring_distance(&ring(&lay, i, camera, rect, pixel_scale), p)))
                .filter(|(_, d)| *d <= HANDLE_RADIUS)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| (Handle::Ring(i), centre))
        } else {
            let axis = (0..3)
                .filter_map(|i| ends[i].map(|e| (i, e, e.distance(p))))
                .filter(|(_, _, d)| *d <= HANDLE_RADIUS + 3.0)
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(i, e, _)| (Handle::Axis(i), e));
            axis.or_else(|| (centre.distance(p) <= HANDLE_RADIUS + 3.0).then_some((Handle::Centre, centre)))
        };
        if let Some((handle, end)) = hit {
            let toward_viewer = matches!(handle, Handle::Ring(i) if lay.axes[i].dot(camera.forward()) < 0.0);
            gesture = Some(Gesture { handle, start: projection.transform3, lay, from: p, centre, end, toward_viewer });
        }
    }

    let Some(g) = gesture else { return false };
    let released = !primary_down;
    if let Some(p) = pointer.filter(|p| p.distance(g.from) > 2.0) {
        let shift = ui.input(|i| i.modifiers.shift);
        let grid = shift.then(|| state.gizmo.grid());
        let new = match (g.handle, mode) {
            (Handle::Axis(i), GizmoMode::Scale) => Some(scaled(&g.start, Some(i), 1.0 + along(g.from, p, g.centre, g.end), grid)),
            (Handle::Axis(i), _) => {
                // The handle is `len` long in object space: that many bounding-box units per handle length.
                let amount = along(g.from, p, g.centre, g.end) * g.lay.len / frame.extent;
                Some(translated(&g.start, g.lay.axes[i] * snap(amount, grid), None))
            }
            (Handle::Centre, GizmoMode::Scale) => Some(scaled(&g.start, None, 1.0 + (p.x - g.from.x) / 120.0, grid)),
            (Handle::Centre, _) => on_view_plane(camera, g.lay.centre, to_paint(p)).map(|target| translated(&g.start, (target - g.lay.centre) / frame.extent, grid)),
            (Handle::Ring(i), _) => {
                let turn = screen_turn(g.centre, g.from, p);
                // Seen from the tip of the axis a positive turn is counter-clockwise, which on
                // screen (y down) is a negative angle.
                let deg = if g.toward_viewer { -turn } else { turn };
                Some(rotated(&g.start, i, deg, shift.then(|| state.gizmo.angle())))
            }
        };
        if let Some(new) = new.filter(|n| n.valid() && *n != projection.transform3) {
            let _ = state.session.execute(Command::SetFillProjection { layer, projection: Projection { transform3: new, ..projection } });
        }
    }
    if released {
        let _ = state.session.execute(Command::Seal);
    }
    ui.data_mut(|d| {
        if released {
            d.remove::<Gesture>(gesture_id());
        } else {
            d.insert_temp(gesture_id(), g);
        }
    });
    true
}

/// The projection box, the handles of the current tool and a hint line.
pub fn overlay(state: &UiState, painter: &egui::Painter, rect: Rect, pixel_scale: Vec2, camera: &Camera) {
    let Some((_, projection)) = active(state).filter(|_| state.gizmo.visible) else { return };
    let frame = CubeFrame::of(state);
    let t = projection.transform3;
    let lay = layout(&t, frame, state.gizmo.size());
    let pos = |o: Vec3| to_pos(camera, rect, pixel_scale, o);
    let shadow = Stroke::new(3.0, Color32::from_black_alpha(110));

    // The projection box (the unit cube of the projection, placed by the transform).
    let corner = |k: usize| {
        let mut o = lay.centre;
        for i in 0..3 {
            let side = if (k >> i) & 1 == 1 { 0.5 } else { -0.5 };
            o += lay.axes[i] * (t.scale[i] * side * frame.extent);
        }
        pos(o)
    };
    let corners: Vec<Option<Pos2>> = (0..8).map(corner).collect();
    for a in 0..8usize {
        for i in 0..3 {
            let b = a | (1 << i);
            if b != a && let (Some(p), Some(q)) = (corners[a], corners[b]) {
                painter.line_segment([p, q], Stroke::new(1.0, Color32::from_white_alpha(90)));
            }
        }
    }

    let Some(centre) = pos(lay.centre) else { return };
    match state.gizmo.mode {
        GizmoMode::Rotate => {
            for i in 0..3 {
                let points: Vec<Pos2> = ring(&lay, i, camera, rect, pixel_scale).into_iter().flatten().collect();
                if points.len() > 1 {
                    painter.add(egui::Shape::line(points.clone(), shadow));
                    painter.add(egui::Shape::line(points, Stroke::new(1.6, AXIS_COLORS[i])));
                }
            }
        }
        mode => {
            for i in 0..3 {
                let Some(end) = pos(lay.centre + lay.axes[i] * lay.len) else { continue };
                painter.line_segment([centre, end], shadow);
                painter.line_segment([centre, end], Stroke::new(1.6, AXIS_COLORS[i]));
                if mode == GizmoMode::Scale {
                    let r = Rect::from_center_size(end, egui::vec2(HANDLE_RADIUS * 1.8, HANDLE_RADIUS * 1.8));
                    painter.rect_filled(r, 1.0, AXIS_COLORS[i]);
                    painter.rect_stroke(r, 1.0, Stroke::new(1.0, Color32::from_black_alpha(160)), egui::StrokeKind::Outside);
                } else {
                    painter.circle_filled(end, HANDLE_RADIUS, AXIS_COLORS[i]);
                    painter.circle_stroke(end, HANDLE_RADIUS, Stroke::new(1.0, Color32::from_black_alpha(160)));
                }
            }
            painter.circle_filled(centre, HANDLE_RADIUS, Color32::WHITE);
            painter.circle_stroke(centre, HANDLE_RADIUS, Stroke::new(1.0, Color32::from_black_alpha(160)));
        }
    }

    let hint = match state.gizmo.mode {
        GizmoMode::Translate => "Projection \u{00b7} Move: drag an axis, or the centre for the view plane (Shift: steps)",
        GizmoMode::Rotate => "Projection \u{00b7} Rotate: drag a ring (Shift: steps)",
        GizmoMode::Scale => "Projection \u{00b7} Scale: drag an axis, or the centre for all three (Shift: steps)",
    };
    let at = rect.center_bottom() - egui::vec2(0.0, 42.0);
    let galley = painter.layout_no_wrap(hint.to_owned(), egui::FontId::proportional(12.0), Color32::WHITE);
    let bg = Rect::from_center_size(at, galley.size() + egui::vec2(14.0, 8.0));
    painter.rect_filled(bg, 4.0, Color32::from_black_alpha(170));
    painter.galley(bg.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn the_layout_follows_offset_and_rotation() {
        let frame = CubeFrame { min: Vec3::new(-1.0, -1.0, -1.0), extent: 2.0 };
        let id = ProjectionTransform::default();
        let lay = layout(&id, frame, 1.0);
        assert!(lay.centre.distance(Vec3::ZERO) < 1e-6 && (lay.len - 0.6).abs() < 1e-6);
        assert!(lay.axes[0].distance(Vec3::X) < 1e-6 && lay.axes[2].distance(Vec3::Z) < 1e-6);
        // Offset a quarter of the cube on X; a quarter turn about Z sends the box's X to +Y.
        let t = ProjectionTransform { offset: [0.25, 0.0, 0.0], rotation_deg: [0.0, 0.0, 90.0], ..id };
        let lay = layout(&t, frame, 2.0);
        assert!(lay.centre.distance(Vec3::new(0.5, 0.0, 0.0)) < 1e-6 && (lay.len - 1.2).abs() < 1e-6);
        assert!(lay.axes[0].distance(Vec3::Y) < 1e-5 && lay.axes[1].distance(-Vec3::X) < 1e-5);
    }

    #[test]
    fn drags_move_scale_and_turn_from_the_press_state() {
        let id = ProjectionTransform::default();
        // Translate: free, then snapped to steps of 0.05; untouched axes keep their value.
        let start = ProjectionTransform { offset: [0.01, 0.02, 0.03], ..id };
        assert!(close(translated(&start, Vec3::new(0.1, 0.0, 0.0), None).offset, [0.11, 0.02, 0.03]));
        assert!(close(translated(&start, Vec3::new(0.1, 0.0, 0.0), Some(0.05)).offset, [0.1, 0.02, 0.03]));
        assert!(close(translated(&start, Vec3::new(100.0, 0.0, 0.0), None).offset, [ProjectionTransform::MAX_OFFSET, 0.02, 0.03]));
        // Scale: one axis, all axes, snapped, and never below the minimum.
        assert!(close(scaled(&id, Some(1), 1.5, None).scale, [1.0, 1.5, 1.0]));
        assert!(close(scaled(&id, None, 2.0, None).scale, [2.0, 2.0, 2.0]));
        assert!(close(scaled(&id, Some(0), 1.27, Some(0.1)).scale, [1.3, 1.0, 1.0]));
        assert!(close(scaled(&id, Some(0), -3.0, None).scale, [ProjectionTransform::MIN_SCALE, 1.0, 1.0]));
        // Rotate: about the box's own axis, snapped to 15 degree steps.
        assert!(close(rotated(&id, 2, 30.0, None).rotation_deg, [0.0, 0.0, 30.0]));
        assert!(close(rotated(&id, 2, 37.0, Some(15.0)).rotation_deg, [0.0, 0.0, 30.0]));
        let turned = ProjectionTransform { rotation_deg: [0.0, 0.0, 90.0], ..id };
        assert!(close(rotated(&turned, 0, 90.0, None).rotation_deg, [90.0, 0.0, 90.0]));
        assert!(rotated(&turned, 1, 250.0, None).valid());
    }

    #[test]
    fn screen_measures() {
        let (a, b) = (Pos2::new(100.0, 100.0), Pos2::new(200.0, 100.0));
        // Half the handle's length along it; movement across it does not count.
        assert!((along(Pos2::new(10.0, 10.0), Pos2::new(60.0, 90.0), a, b) - 0.5).abs() < 1e-6);
        assert_eq!(along(a, b, a, a), 0.0);
        // A quarter turn clockwise on screen (y down) is +90.
        let c = Pos2::new(0.0, 0.0);
        assert!((screen_turn(c, Pos2::new(10.0, 0.0), Pos2::new(0.0, 10.0)) - 90.0).abs() < 1e-4);
        assert!((screen_turn(c, Pos2::new(0.0, 10.0), Pos2::new(10.0, 0.0)) + 90.0).abs() < 1e-4);
        assert!((screen_turn(c, Pos2::new(-10.0, 1.0), Pos2::new(-10.0, -1.0)).abs() - 11.42).abs() < 0.01, "no jump across the -180 / 180 seam");
        assert!((distance_to_segment(Pos2::new(150.0, 104.0), a, b) - 4.0).abs() < 1e-6);
        assert!((distance_to_segment(Pos2::new(203.0, 104.0), a, b) - 5.0).abs() < 1e-6);
        assert_eq!(ring_distance(&[Some(a), None, Some(b)], Pos2::new(150.0, 100.0)), f32::INFINITY);
        assert!((ring_distance(&[Some(a), Some(b)], Pos2::new(150.0, 103.0)) - 3.0).abs() < 1e-6);
    }
}
