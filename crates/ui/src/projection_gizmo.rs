//! Manipulator of a fill layer's 3D projection in the 3D view (Painter's projection manipulator),
//! shown while the selected layer is a fill layer with a tri-planar, planar, spherical or
//! cylindrical projection. It edits the projection box's 3D Offset, Rotation and Scale
//! (`ProjectionTransform`), the same values as the "3D projection" group in Properties.
//!
//! * Translate: drag an axis handle to move along that axis of the box, a plane handle (the small
//!   squares between two axes) to move in that plane of the box, or the centre handle to move in
//!   the view plane.
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
    /// How much + / - change the handle size.
    pub const SIZE_STEP: f32 = 0.25;
    /// Pointer movement factor while Ctrl is held (Painter's precision mode).
    pub const PRECISION: f32 = 0.1;

    /// Painter's manipulator keys, while a projection manipulator is active: Q shows / hides it,
    /// W / E / R pick Move / Rotate / Scale (only while it is shown, so E stays the eraser
    /// otherwise), + and - change its size. Returns whether the key was used.
    pub fn handle_key(&mut self, key: egui::Key) -> bool {
        use egui::Key as K;
        let mode = match key {
            K::Q => {
                self.visible = !self.visible;
                return true;
            }
            K::Plus | K::Equals | K::Minus => {
                let step = if key == K::Minus { -Self::SIZE_STEP } else { Self::SIZE_STEP };
                self.size = (self.size() + step).clamp(*Self::SIZE_RANGE.start(), *Self::SIZE_RANGE.end());
                return true;
            }
            K::W => GizmoMode::Translate,
            K::E => GizmoMode::Rotate,
            K::R => GizmoMode::Scale,
            _ => return false,
        };
        if !self.visible {
            return false;
        }
        self.mode = mode;
        true
    }

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
    ray_plane(o, d, anchor, camera.forward())
}

/// Where the ray `o + t d` (t > 0) meets the plane through `anchor` with normal `n`; `None` when
/// the ray runs along the plane or away from it.
pub fn ray_plane(o: Vec3, d: Vec3, anchor: Vec3, n: Vec3) -> Option<Vec3> {
    let den = d.dot(n);
    if den.abs() < 1e-8 {
        return None;
    }
    let t = (anchor - o).dot(n) / den;
    (t.is_finite() && t > 0.0).then(|| o + d * t)
}

/// Corners of the plane handle of the box plane normal to `axis` (between the two other axes),
/// in object space.
pub fn plane_quad(lay: &Layout, axis: usize) -> [Vec3; 4] {
    let (a, b) = (lay.axes[(axis + 1) % 3] * lay.len, lay.axes[(axis + 2) % 3] * lay.len);
    let (near, far) = (PLANE_NEAR, PLANE_FAR);
    [lay.centre + a * near + b * near, lay.centre + a * far + b * near, lay.centre + a * far + b * far, lay.centre + a * near + b * far]
}

/// Whether `p` lies inside the convex screen quad `q` (either winding).
pub fn in_quad(p: Pos2, q: &[Pos2; 4]) -> bool {
    let cross = |a: Pos2, b: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let s: Vec<f32> = (0..4).map(|i| cross(q[i], q[(i + 1) % 4])).collect();
    s.iter().all(|&c| c >= 0.0) || s.iter().all(|&c| c <= 0.0)
}

/// The plane handles span this fraction of the handle length, from the centre outward.
const PLANE_NEAR: f32 = 0.22;
const PLANE_FAR: f32 = 0.42;

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
    /// The plane handle normal to that axis.
    Plane(usize),
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
            // Plane handles (Move only), then the centre.
            let in_plane = |k: usize| {
                let q = plane_quad(&lay, k).map(|o| to_pos(camera, rect, pixel_scale, o));
                q.iter().all(Option::is_some) && in_quad(p, &q.map(|o| o.unwrap_or(Pos2::ZERO)))
            };
            let plane = if mode == GizmoMode::Translate { (0..3).find(|&k| in_plane(k)).map(|k| (Handle::Plane(k), centre)) } else { None };
            axis.or(plane).or_else(|| (centre.distance(p) <= HANDLE_RADIUS + 3.0).then_some((Handle::Centre, centre)))
        };
        if let Some((handle, end)) = hit {
            let toward_viewer = matches!(handle, Handle::Ring(i) if lay.axes[i].dot(camera.forward()) < 0.0);
            gesture = Some(Gesture { handle, start: projection.transform3, lay, from: p, centre, end, toward_viewer });
        }
    }

    let Some(g) = gesture else { return false };
    let released = !primary_down;
    // Ctrl: precision mode, the handles follow a tenth of the pointer's movement.
    let precise = ui.input(|i| i.modifiers.command);
    let pointer = pointer.map(|p| if precise { g.from + (p - g.from) * GizmoEdit::PRECISION } else { p });
    if let Some(p) = pointer.filter(|p| p.distance(g.from) > 2.0 * if precise { GizmoEdit::PRECISION } else { 1.0 }) {
        let shift = ui.input(|i| i.modifiers.shift);
        let grid = shift.then(|| state.gizmo.grid());
        let new = match (g.handle, mode) {
            (Handle::Axis(i), GizmoMode::Scale) => Some(scaled(&g.start, Some(i), 1.0 + along(g.from, p, g.centre, g.end), grid)),
            (Handle::Axis(i), _) => {
                // The handle is `len` long in object space: that many bounding-box units per handle length.
                let amount = along(g.from, p, g.centre, g.end) * g.lay.len / frame.extent;
                Some(translated(&g.start, g.lay.axes[i] * snap(amount, grid), None))
            }
            (Handle::Plane(k), _) => {
                // Both pointer positions on the box plane through the centre: the move between them.
                let n = g.lay.axes[k];
                let on = |q: Pos2| {
                    let (o, d) = camera.ray(to_paint(q));
                    ray_plane(o, d, g.lay.centre, n)
                };
                on(g.from).zip(on(p)).map(|(a, b)| translated(&g.start, (b - a) / frame.extent, grid))
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
            if mode == GizmoMode::Translate {
                for k in 0..3 {
                    let q: Vec<Pos2> = plane_quad(&lay, k).iter().filter_map(|&o| pos(o)).collect();
                    if q.len() == 4 {
                        let c = AXIS_COLORS[k];
                        painter.add(egui::Shape::convex_polygon(q, Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 90), Stroke::new(1.0, c)));
                    }
                }
            }
            painter.circle_filled(centre, HANDLE_RADIUS, Color32::WHITE);
            painter.circle_stroke(centre, HANDLE_RADIUS, Stroke::new(1.0, Color32::from_black_alpha(160)));
        }
    }

    let hint = match state.gizmo.mode {
        GizmoMode::Translate => "Projection \u{00b7} Move: drag an axis, a square for two axes, or the centre for the view plane (Shift: steps, Ctrl: precise) \u{00b7} W / E / R tool, Q hide, + / - size",
        GizmoMode::Rotate => "Projection \u{00b7} Rotate: drag a ring (Shift: steps, Ctrl: precise) \u{00b7} W / E / R tool, Q hide, + / - size",
        GizmoMode::Scale => "Projection \u{00b7} Scale: drag an axis, or the centre for all three (Shift: steps, Ctrl: precise) \u{00b7} W / E / R tool, Q hide, + / - size",
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
    fn manipulator_keys_toggle_pick_the_tool_and_resize() {
        let mut g = GizmoEdit::default();
        assert!(g.handle_key(egui::Key::E) && g.mode == GizmoMode::Rotate);
        assert!(g.handle_key(egui::Key::R) && g.mode == GizmoMode::Scale);
        assert!(g.handle_key(egui::Key::W) && g.mode == GizmoMode::Translate);
        assert!(g.handle_key(egui::Key::Plus) && (g.size - 1.25).abs() < 1e-6);
        assert!(g.handle_key(egui::Key::Equals) && (g.size - 1.5).abs() < 1e-6);
        for _ in 0..10 {
            g.handle_key(egui::Key::Minus);
        }
        assert_eq!(g.size, *GizmoEdit::SIZE_RANGE.start(), "never below the smallest size");
        // Q hides it; hidden, the tool keys are left to the keymap (E is the eraser).
        assert!(g.handle_key(egui::Key::Q) && !g.visible);
        assert!(!g.handle_key(egui::Key::E) && g.mode == GizmoMode::Translate);
        assert!(g.handle_key(egui::Key::Q) && g.visible);
        assert!(!g.handle_key(egui::Key::B), "other keys are not the manipulator's");
    }

    #[test]
    fn plane_handles_sit_between_two_axes_and_drag_on_that_plane() {
        let frame = CubeFrame { min: Vec3::new(-1.0, -1.0, -1.0), extent: 2.0 };
        let lay = layout(&ProjectionTransform::default(), frame, 1.0);
        // The handle normal to Z lies in the XY plane, off the centre toward +X and +Y.
        let q = plane_quad(&lay, 2);
        assert!(q.iter().all(|c| c.z.abs() < 1e-6 && c.x > 0.0 && c.y > 0.0), "{q:?}");
        // A ray straight down onto the Y = 0 plane hits it below its origin; along it, no hit.
        let hit = ray_plane(Vec3::new(0.3, 5.0, 0.2), -Vec3::Y, Vec3::ZERO, Vec3::Y).unwrap();
        assert!(hit.distance(Vec3::new(0.3, 0.0, 0.2)) < 1e-6);
        assert!(ray_plane(Vec3::new(0.0, 5.0, 0.0), Vec3::X, Vec3::ZERO, Vec3::Y).is_none());
        assert!(ray_plane(Vec3::new(0.0, 5.0, 0.0), Vec3::Y, Vec3::ZERO, Vec3::Y).is_none(), "behind the ray");
        // A plane move changes only the two offsets of that plane.
        let moved = translated(&ProjectionTransform::default(), (Vec3::new(0.4, 0.0, -0.2)) / frame.extent, None);
        assert!(close(moved.offset, [0.2, 0.0, -0.1]));
        // Inside / outside a screen quad, in both windings.
        let sq = [Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), Pos2::new(10.0, 10.0), Pos2::new(0.0, 10.0)];
        let rev = [sq[3], sq[2], sq[1], sq[0]];
        assert!(in_quad(Pos2::new(5.0, 5.0), &sq) && in_quad(Pos2::new(5.0, 5.0), &rev));
        assert!(!in_quad(Pos2::new(11.0, 5.0), &sq) && !in_quad(Pos2::new(5.0, -1.0), &rev));
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
