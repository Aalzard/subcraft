//! Symmetry in the 3D view, after Painter's Symmetry settings display options: the translucent
//! mirror plane, the mirrored brush cursor, hide-while-painting, and the axis-position
//! manipulator (drag a handle to move a mirror plane along its axis, or the radial axis across
//! its plane). The guide lines themselves (plane intersections, plane outlines, radial axis) are
//! GPU lines built from [`subcraft_app::Session::symmetry_guides`].

use egui::{Color32, Pos2, Rect, Stroke, Ui};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use subcraft_app::{Command, Session};
use subcraft_paint::Camera;

use crate::state::UiState;
use crate::theme::ACCENT;

/// Symmetry display options (Painter's Symmetry settings menu > display).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SymmetryDisplay {
    /// Translucent mirror plane (and its outline among the guide lines).
    pub show_plane: bool,
    /// Where the mirror plane cuts the surface.
    pub show_intersection: bool,
    /// Radial axis line and spokes.
    pub show_axis: bool,
    /// A second brush cursor at every mirrored / radial copy.
    pub show_cursor: bool,
    /// Hide the guides and the mirrored cursor while a stroke is being painted.
    pub hide_while_painting: bool,
    /// Draggable axis-position handles.
    pub manipulator: bool,
    /// Handle size factor.
    pub manipulator_size: f32,
}

impl Default for SymmetryDisplay {
    fn default() -> Self {
        Self {
            show_plane: true,
            show_intersection: true,
            show_axis: true,
            show_cursor: true,
            hide_while_painting: false,
            manipulator: false,
            manipulator_size: 1.0,
        }
    }
}

impl SymmetryDisplay {
    pub const SIZE_RANGE: std::ops::RangeInclusive<f32> = 0.25..=4.0;

    /// Handle size factor, kept in range.
    pub fn size(&self) -> f32 {
        if self.manipulator_size.is_finite() { self.manipulator_size.clamp(*Self::SIZE_RANGE.start(), *Self::SIZE_RANGE.end()) } else { 1.0 }
    }
}

/// What the uploaded guide lines were built from: a new key means upload again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuideKey {
    mesh: usize,
    symmetry: [bool; 3],
    planes: [u32; 3],
    radial: subcraft_paint::RadialSymmetry,
    kinds: [bool; 3],
}

impl GuideKey {
    pub fn new(session: &Session, display: &SymmetryDisplay) -> Self {
        let options = &session.brush.options;
        Self {
            mesh: std::sync::Arc::as_ptr(&session.scene().mesh) as usize,
            symmetry: options.symmetry,
            planes: options.mirror_offset.map(f32::to_bits),
            radial: options.radial,
            kinds: [display.show_intersection, display.show_plane, display.show_axis],
        }
    }
}

/// The guide lines to upload for the chosen display options.
pub fn guide_lines(session: &Session, display: &SymmetryDisplay) -> Vec<[Vec3; 2]> {
    session.symmetry_guides().segments(display.show_intersection, display.show_plane, display.show_axis)
}

/// Whether the guides show now: symmetry on, guides on, and not hidden by an ongoing stroke.
fn guides_shown(state: &UiState) -> bool {
    let view = &state.recovery.prefs.viewport;
    view.symmetry_guides && state.session.brush.options.has_symmetry() && !(view.symmetry_display.hide_while_painting && state.painting)
}

fn to_pos(rect: Rect, pixel_scale: Vec2, p: Vec3) -> Pos2 {
    rect.min + egui::vec2(p.x / pixel_scale.x, p.y / pixel_scale.y)
}

fn axis_color(axis: usize) -> Color32 {
    [Color32::from_rgb(232, 84, 84), Color32::from_rgb(112, 200, 92), Color32::from_rgb(84, 144, 240)][axis.min(2)]
}

// ----- math -------------------------------------------------------------------------------------

/// Parameter `t` of the point `p + t * e` on a line closest to the ray `o + s * d` (None when the
/// ray runs along the line).
pub fn closest_on_line(o: Vec3, d: Vec3, p: Vec3, e: Vec3) -> Option<f32> {
    let (e, d) = (e.normalize_or_zero(), d.normalize_or_zero());
    let b = e.dot(d);
    let denom = 1.0 - b * b;
    if denom < 1e-6 {
        return None;
    }
    let w = p - o;
    Some((b * d.dot(w) - e.dot(w)) / denom)
}

/// Where the ray `o + s * d` (s >= 0) meets the plane through `p0` with normal `n`.
pub fn ray_plane(o: Vec3, d: Vec3, p0: Vec3, n: Vec3) -> Option<Vec3> {
    let denom = n.dot(d);
    if denom.abs() < 1e-6 {
        return None;
    }
    let s = n.dot(p0 - o) / denom;
    (s >= 0.0).then(|| o + d * s)
}

// ----- manipulator ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    /// The mirror plane of this axis, moved along it.
    Mirror(usize),
    /// The radial axis, moved across the plane normal to it.
    Radial,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    handle: Handle,
    /// Mirror: plane coordinate minus the grabbed point's; radial: pivot minus the grabbed point.
    grab: Vec3,
}

/// One handle: its anchor (object space) and arrow tips along the directions it moves.
struct HandleShape {
    handle: Handle,
    anchor: Vec3,
    arms: Vec<(Vec3, usize)>,
}

fn handle_shapes(state: &UiState) -> Vec<HandleShape> {
    let display = &state.recovery.prefs.viewport.symmetry_display;
    let options = &state.session.brush.options;
    if !display.manipulator || !options.has_symmetry() {
        return Vec::new();
    }
    let bounds = &state.session.scene().mesh.bounds;
    let center = bounds.center();
    let len = bounds.diagonal().max(1e-6) * 0.12 * display.size();
    let mut shapes = Vec::new();
    for axis in 0..3 {
        if options.symmetry[axis] {
            let mut anchor = center;
            anchor[axis] = options.mirror_plane(axis, center);
            let mut e = Vec3::ZERO;
            e[axis] = len;
            shapes.push(HandleShape { handle: Handle::Mirror(axis), anchor, arms: vec![(anchor + e, axis), (anchor - e, axis)] });
        }
    }
    let mirrors = options.symmetry.iter().filter(|&&m| m).count();
    if options.radial.effective_count(mirrors) > 1 {
        let axis = (options.radial.axis as usize).min(2);
        let anchor = options.radial_pivot(center);
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut arms = Vec::new();
        for (a, s) in [(u, 1.0), (u, -1.0), (v, 1.0), (v, -1.0)] {
            let mut e = Vec3::ZERO;
            e[a] = s * len * 0.75;
            arms.push((anchor + e, a));
        }
        shapes.push(HandleShape { handle: Handle::Radial, anchor, arms });
    }
    shapes
}

/// Screen distance from `p` to the segment `a`-`b`.
fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t).distance(p)
}

/// The handle under `pointer`, if any.
fn hit(shapes: &[HandleShape], camera: &Camera, rect: Rect, pixel_scale: Vec2, pointer: Pos2, size: f32) -> Option<Handle> {
    let reach = 7.0 * size.sqrt();
    shapes.iter().find_map(|s| {
        let anchor = to_pos(rect, pixel_scale, camera.project(s.anchor)?);
        let near = anchor.distance(pointer) < reach + 3.0
            || s.arms.iter().any(|(tip, _)| camera.project(*tip).is_some_and(|t| segment_distance(pointer, anchor, to_pos(rect, pixel_scale, t)) < reach));
        near.then_some(s.handle)
    })
}

/// The point a handle drag follows for the ray through `screen`: on the axis line for a mirror
/// plane, on the plane normal to the radial axis for the radial pivot.
fn drag_point(state: &UiState, handle: Handle, camera: &Camera, screen: Vec2) -> Option<Vec3> {
    let (o, d) = camera.ray(screen);
    let options = &state.session.brush.options;
    let center = state.session.scene().mesh.bounds.center();
    match handle {
        Handle::Mirror(axis) => {
            let mut p = center;
            p[axis] = options.mirror_plane(axis, center);
            let mut e = Vec3::ZERO;
            e[axis] = 1.0;
            closest_on_line(o, d, p, e).map(|t| p + e * t)
        }
        Handle::Radial => ray_plane(o, d, options.radial_pivot(center), options.radial.axis_dir()),
    }
}

/// Handle input before painting: a primary press on a handle starts dragging it, and the drag
/// moves the plane or axis until release. Returns whether the pointer belongs to the
/// manipulator this frame (the view then ignores the press).
#[allow(clippy::too_many_arguments)]
pub fn manipulate(state: &mut UiState, ui: &Ui, response: &egui::Response, rect: Rect, pixel_scale: Vec2, camera: &Camera, primary_pressed: bool, primary_down: bool, pointer: Option<Pos2>) -> bool {
    let id = egui::Id::new("symmetry_manipulator_drag");
    let mut drag: Option<Drag> = ui.data(|d| d.get_temp(id));
    let screen = |p: Pos2| Vec2::new((p.x - rect.min.x) * pixel_scale.x, (p.y - rect.min.y) * pixel_scale.y);
    if drag.is_some() && !primary_down {
        drag = None;
    }
    let mut grabbed = false;
    if drag.is_none() && primary_pressed && response.hovered() && let Some(p) = pointer {
        let shapes = handle_shapes(state);
        let size = state.recovery.prefs.viewport.symmetry_display.size();
        if let Some(handle) = hit(&shapes, camera, rect, pixel_scale, p, size)
            && let Some(at) = drag_point(state, handle, camera, screen(p))
        {
            let options = &state.session.brush.options;
            let center = state.session.scene().mesh.bounds.center();
            let grab = match handle {
                Handle::Mirror(axis) => {
                    let mut g = Vec3::ZERO;
                    g[axis] = options.mirror_plane(axis, center) - at[axis];
                    g
                }
                Handle::Radial => options.radial_pivot(center) - at,
            };
            drag = Some(Drag { handle, grab });
            grabbed = true;
        }
    } else if let Some(Drag { handle, grab }) = drag
        && let Some(p) = pointer
        && let Some(at) = drag_point(state, handle, camera, screen(p))
    {
        let mut options = state.session.brush.options;
        let center = state.session.scene().mesh.bounds.center();
        match handle {
            Handle::Mirror(axis) => options.mirror_offset[axis] = at[axis] + grab[axis] - center[axis],
            Handle::Radial => {
                let axis = (options.radial.axis as usize).min(2);
                let pivot = at + grab;
                for a in [(axis + 1) % 3, (axis + 2) % 3] {
                    options.radial.center_offset[a] = pivot[a] - center[a];
                }
            }
        }
        if options != state.session.brush.options && options.mirror_offset.iter().chain(&options.radial.center_offset).all(|v| v.is_finite()) {
            let _ = state.session.execute(Command::SetStrokeOptions(options));
        }
    }
    ui.data_mut(|d| match drag {
        Some(v) => {
            d.insert_temp(id, v);
        }
        None => d.remove::<Drag>(id),
    });
    if drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    grabbed || drag.is_some()
}

// ----- drawing ----------------------------------------------------------------------------------

/// Plane fill and manipulator handles (call after the 3D image, before other overlays).
pub fn overlay(state: &UiState, ui: &Ui, painter: &egui::Painter, rect: Rect, pixel_scale: Vec2, camera: &Camera, enabled: bool) {
    if !enabled || !state.session.brush.options.has_symmetry() {
        return;
    }
    let display = state.recovery.prefs.viewport.symmetry_display;
    if guides_shown(state) && display.show_plane {
        let fill = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 22);
        for plane in state.session.symmetry_guides().planes {
            let points: Option<Vec<Pos2>> = plane.iter().map(|c| camera.project(*c).map(|s| to_pos(rect, pixel_scale, s))).collect();
            if let Some(points) = points {
                painter.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
            }
        }
    }
    if state.painting {
        return;
    }
    let shapes = handle_shapes(state);
    if shapes.is_empty() {
        return;
    }
    let size = display.size();
    let dragging: Option<Drag> = ui.data(|d| d.get_temp(egui::Id::new("symmetry_manipulator_drag")));
    let hovered = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p)).and_then(|p| hit(&shapes, camera, rect, pixel_scale, p, size));
    for s in &shapes {
        let Some(anchor) = camera.project(s.anchor).map(|a| to_pos(rect, pixel_scale, a)) else { continue };
        let active = dragging.is_some_and(|d| d.handle == s.handle) || hovered == Some(s.handle);
        let width = if active { 3.0 } else { 2.0 } * size.sqrt();
        for (tip, axis) in &s.arms {
            let Some(tip) = camera.project(*tip).map(|t| to_pos(rect, pixel_scale, t)) else { continue };
            painter.line_segment([anchor, tip], Stroke::new(width + 2.0, Color32::from_black_alpha(140)));
            painter.line_segment([anchor, tip], Stroke::new(width, axis_color(*axis)));
            painter.circle_filled(tip, 3.5 * size.sqrt(), axis_color(*axis));
        }
        let r = 5.0 * size.sqrt();
        painter.circle_filled(anchor, r, if active { ACCENT } else { Color32::from_gray(235) });
        painter.circle_stroke(anchor, r, Stroke::new(1.5, Color32::from_black_alpha(160)));
    }
    if hovered.is_some() && dragging.is_none() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// The brush cursor repeated at every copy of the stroke point `p` (view depth `depth`, cursor
/// radius `r_px` in points). A copy hidden behind the surface is drawn faint.
#[allow(clippy::too_many_arguments)]
pub fn mirrored_cursor(state: &UiState, painter: &egui::Painter, rect: Rect, pixel_scale: Vec2, camera: &Camera, p: Vec3, depth: f32, r_px: f32, color: Color32) {
    let display = &state.recovery.prefs.viewport.symmetry_display;
    if !display.show_cursor || (display.hide_while_painting && state.painting) {
        return;
    }
    let ppu = camera.pixels_per_unit(depth);
    if !(ppu.is_finite() && ppu > 0.0) {
        return;
    }
    let tolerance = state.session.scene().mesh.bounds.diagonal() * 2e-3;
    for q in state.session.symmetry_copies(p) {
        let Some(s) = camera.project(q) else { continue };
        let r = r_px * camera.pixels_per_unit(s.z) / ppu;
        if !(r.is_finite() && r > 0.5) {
            continue;
        }
        let visible = state.session.pick(camera, Vec2::new(s.x, s.y)).is_none_or(|hit| hit.depth >= s.z - tolerance);
        let (shadow, line) = if visible {
            (Color32::from_black_alpha(120), color.gamma_multiply(0.8))
        } else {
            (Color32::from_black_alpha(40), color.gamma_multiply(0.3))
        };
        let at = to_pos(rect, pixel_scale, s);
        painter.circle_stroke(at, r, Stroke::new(1.5, shadow));
        painter.circle_stroke(at, r, Stroke::new(1.0, line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_closest_point_on_an_axis_follows_the_ray() {
        let (o, d) = (Vec3::new(2.0, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0));
        let t = closest_on_line(o, d, Vec3::ZERO, Vec3::X).unwrap();
        assert!((t - 2.0).abs() < 1e-5, "{t}");
        // A slanted ray: the closest point is still the X coordinate where it passes the axis.
        let d = Vec3::new(-0.5, 0.0, -1.0).normalize();
        let t = closest_on_line(Vec3::new(3.0, 0.0, 2.0), d, Vec3::ZERO, Vec3::X).unwrap();
        assert!((t - 2.0).abs() < 1e-4, "{t}");
        assert!(closest_on_line(o, Vec3::X, Vec3::ZERO, Vec3::X).is_none(), "a ray along the axis has no single closest point");
    }

    #[test]
    fn rays_meet_the_radial_plane_in_front_only() {
        let hit = ray_plane(Vec3::new(1.0, 5.0, 2.0), Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::Y).unwrap();
        assert!(hit.distance(Vec3::new(1.0, 1.0, 2.0)) < 1e-6);
        assert!(ray_plane(Vec3::new(0.0, 5.0, 0.0), Vec3::Y, Vec3::ZERO, Vec3::Y).is_none(), "behind the ray");
        assert!(ray_plane(Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Y).is_none(), "parallel");
    }

    #[test]
    fn display_defaults_and_size_bounds() {
        let d = SymmetryDisplay::default();
        assert!(d.show_plane && d.show_intersection && d.show_axis && d.show_cursor && !d.hide_while_painting && !d.manipulator);
        assert_eq!(SymmetryDisplay { manipulator_size: f32::NAN, ..d }.size(), 1.0);
        assert_eq!(SymmetryDisplay { manipulator_size: 99.0, ..d }.size(), 4.0);
        // Older preference files have no display options: they read as the defaults.
        let old: SymmetryDisplay = serde_json::from_str("{}").unwrap();
        assert_eq!(old, d);
    }

    #[test]
    fn segment_distance_clamps_to_the_ends() {
        let (a, b) = (Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0));
        assert_eq!(segment_distance(Pos2::new(5.0, 3.0), a, b), 3.0);
        assert_eq!(segment_distance(Pos2::new(-4.0, 3.0), a, b), 5.0);
    }
}
