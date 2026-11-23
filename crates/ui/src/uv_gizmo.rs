//! UV frame manipulator in the 2D view (Painter's 2D view manipulator), shown while the
//! selected layer is a fill layer in UV projection. It draws the frame of the texture's unit
//! tile in UV space and edits the projection's 2D Tiling, Rotation and Offset, the same values
//! as Properties > UV transformations.
//!
//! * Move: drag the centre handle (Shift: along one axis only).
//! * Rotate: drag just outside a corner; the frame turns about its centre (Shift: 45 degree steps).
//! * Scale: drag a corner or edge handle; the opposite side stays (Ctrl: about the centre,
//!   Shift: keep the ratio).
//!
//! Every drag is one undo step. Q (the projection manipulator's Show) hides it.

use egui::{Color32, Pos2, Stroke, Ui};
use glam::Vec2;
use subcraft_app::Command;
use subcraft_core::LayerId;
use subcraft_doc::{LayerContent, Projection, ProjectionMode};

use crate::panels::uv_view::Frame;
use crate::state::{Tool, UiState};

/// The centre of the texture's unit tile.
const CENTRE: Vec2 = Vec2::new(0.5, 0.5);

/// Scale handles in texture coordinates: the corners, then the edge middles.
pub const HANDLES: [Vec2; 8] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(1.0, 1.0),
    Vec2::new(0.0, 1.0),
    Vec2::new(0.5, 0.0),
    Vec2::new(1.0, 0.5),
    Vec2::new(0.5, 1.0),
    Vec2::new(0.0, 0.5),
];

const HANDLE_RADIUS: f32 = 7.0;
/// Rotation is grabbed this far (screen points) outside a corner.
const ROTATE_REACH: f32 = 28.0;

/// Texture coordinates of UV position `uv` through the projection's 2D transform.
pub fn to_tex(p: &Projection, uv: Vec2) -> Vec2 {
    Vec2::from(p.transform_2d(uv.to_array()))
}

/// UV position of texture coordinates `tex`: the inverse of [`to_tex`]; `None` for a zero tiling.
pub fn to_uv(p: &Projection, tex: Vec2) -> Option<Vec2> {
    if p.scale[0].abs() < 1e-6 || p.scale[1].abs() < 1e-6 {
        return None;
    }
    let (s, c) = p.rotation_sin_cos();
    let d = tex - Vec2::from(p.offset);
    Some(Vec2::new((d.x * c + d.y * s) / p.scale[0], (-d.x * s + d.y * c) / p.scale[1]))
}

/// The 2D transform without its offset (scale, then rotation).
fn linear(p: &Projection, v: Vec2) -> Vec2 {
    let (s, c) = p.rotation_sin_cos();
    let (x, y) = (v.x * p.scale[0], v.y * p.scale[1]);
    Vec2::new(x * c - y * s, x * s + y * c)
}

/// `p` with the offset that maps UV position `uv` onto texture coordinates `tex`.
fn keeping(mut p: Projection, tex: Vec2, uv: Vec2) -> Projection {
    let m = linear(&p, uv);
    p.offset = [tex.x - m.x, tex.y - m.y];
    p
}

/// The frame moved by `delta` in UV space.
pub fn translated(start: &Projection, delta: Vec2) -> Projection {
    let mut p = *start;
    let d = linear(start, delta);
    p.offset = [start.offset[0] - d.x, start.offset[1] - d.y];
    p
}

/// The frame turned by `deg` about its centre (positive from UV +X toward +Y, clockwise on
/// screen).
pub fn rotated(start: &Projection, deg: f32) -> Projection {
    let Some(centre) = to_uv(start, CENTRE) else { return *start };
    let mut p = *start;
    p.rotation_deg = (start.rotation_deg - deg + 180.0).rem_euclid(360.0) - 180.0;
    keeping(p, CENTRE, centre)
}

/// The frame scaled by dragging handle `handle` (texture coordinates, see [`HANDLES`]) to UV
/// position `pointer`: the opposite side stays, or the centre with `about_centre`; `keep_ratio`
/// scales both axes alike. The frame never turns inside out.
pub fn scaled(start: &Projection, handle: Vec2, pointer: Vec2, about_centre: bool, keep_ratio: bool) -> Projection {
    let t = to_tex(start, pointer);
    let fixed = if about_centre { CENTRE } else { Vec2::ONE - handle };
    let factor = |h: f32, q: f32, t: f32| ((h - q).abs() > 1e-6).then(|| ((t - q) / (h - q)).max(0.01));
    let (fx, fy) = match (factor(handle.x, fixed.x, t.x), factor(handle.y, fixed.y, t.y)) {
        (Some(a), Some(b)) if keep_ratio => ((a + b) / 2.0, (a + b) / 2.0),
        (Some(a), None) if keep_ratio => (a, a),
        (None, Some(b)) if keep_ratio => (b, b),
        (a, b) => (a.unwrap_or(1.0), b.unwrap_or(1.0)),
    };
    let Some(fixed_uv) = to_uv(start, fixed) else { return *start };
    let mut p = *start;
    p.scale = [start.scale[0] / fx, start.scale[1] / fy];
    keeping(p, fixed, fixed_uv)
}

/// The texture mirrored along the frame's X (`axis` 0) or Y (1), about the frame's centre
/// (Painter's Mirror on X / Y).
pub fn mirrored(start: &Projection, axis: usize) -> Projection {
    let Some(centre) = to_uv(start, CENTRE) else { return *start };
    let mut p = *start;
    p.scale[axis.min(1)] = -start.scale[axis.min(1)];
    keeping(p, CENTRE, centre)
}

/// What a press grabs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grab {
    Move,
    Rotate,
    /// The scale handle at these texture coordinates.
    Scale(Vec2),
}

/// The handle under `pointer` (screen points), given where texture coordinates land on screen.
pub fn hit(pointer: Pos2, to_screen: impl Fn(Vec2) -> Option<Pos2>) -> Option<Grab> {
    let at = |tex: Vec2| to_screen(tex).map(|s| s.distance(pointer));
    if let Some(h) = HANDLES.iter().copied().find(|&h| at(h).is_some_and(|d| d <= HANDLE_RADIUS)) {
        return Some(Grab::Scale(h));
    }
    if at(CENTRE).is_some_and(|d| d <= HANDLE_RADIUS + 2.0) {
        return Some(Grab::Move);
    }
    // Just outside a corner: away from the centre, past the handle.
    let centre = to_screen(CENTRE)?;
    let outside = HANDLES[..4].iter().filter_map(|&h| to_screen(h)).any(|corner| {
        let d = corner.distance(pointer);
        d > HANDLE_RADIUS && d <= ROTATE_REACH && pointer.distance(centre) > corner.distance(centre)
    });
    outside.then_some(Grab::Rotate)
}

/// The selected fill layer in UV projection, while the manipulator is shown.
fn active(state: &UiState) -> Option<(LayerId, Projection)> {
    if !state.gizmo.visible || state.tool == Tool::Pen || state.picking_parts {
        return None;
    }
    let layer = state.session.selected_layer()?;
    match &layer.content {
        LayerContent::Fill(f) if f.projection.mode == ProjectionMode::Uv && f.projection.physical_size.is_none() => Some((layer.id, f.projection)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
struct Gesture {
    grab: Grab,
    start: Projection,
    from: Pos2,
    /// The frame's centre on screen at the press.
    centre: Pos2,
}

fn gesture_id() -> egui::Id {
    egui::Id::new("uv_gizmo_gesture")
}

/// Manipulator input before the 2D view's tools. Returns whether the pointer belongs to it this
/// frame (the view then ignores the press).
pub fn input(state: &mut UiState, ui: &Ui, response: &egui::Response, frame: Frame) -> bool {
    let Some((layer, projection)) = active(state) else {
        ui.data_mut(|d| d.remove::<Gesture>(gesture_id()));
        return false;
    };
    let (pointer, pressed, down, mods) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
    let screen = |tex: Vec2| to_uv(&projection, tex).map(|uv| frame.to_px(uv));
    let mut gesture: Option<Gesture> = ui.data(|d| d.get_temp(gesture_id()));
    if gesture.is_none() && pressed && response.hovered() && !mods.alt && let Some(p) = pointer && let Some(grab) = hit(p, screen) {
        gesture = Some(Gesture { grab, start: projection, from: p, centre: screen(CENTRE).unwrap_or(p) });
    }
    let Some(g) = gesture else { return false };
    if let Some(p) = pointer.filter(|p| p.distance(g.from) > 1.0) {
        let new = match g.grab {
            Grab::Move => {
                let mut d = frame.to_uv(p) - frame.to_uv(g.from);
                if mods.shift {
                    if d.x.abs() >= d.y.abs() { d.y = 0.0 } else { d.x = 0.0 }
                }
                translated(&g.start, d)
            }
            Grab::Rotate => {
                let mut deg = crate::projection_gizmo::screen_turn(g.centre, g.from, p);
                if mods.shift {
                    deg = (deg / 45.0).round() * 45.0;
                }
                rotated(&g.start, deg)
            }
            Grab::Scale(h) => scaled(&g.start, h, frame.to_uv(p), mods.command, mods.shift),
        };
        if new.valid() && new != projection {
            let _ = state.session.execute(Command::SetFillProjection { layer, projection: new });
        }
    }
    if down {
        ui.data_mut(|d| d.insert_temp(gesture_id(), g));
    } else {
        let _ = state.session.execute(Command::Seal);
        ui.data_mut(|d| d.remove::<Gesture>(gesture_id()));
    }
    true
}

/// The frame, its handles and the centre.
pub fn overlay(state: &UiState, painter: &egui::Painter, frame: Frame) {
    let Some((_, projection)) = active(state) else { return };
    let screen = |tex: Vec2| to_uv(&projection, tex).map(|uv| frame.to_px(uv));
    let accent = crate::theme::ACCENT();
    let outline = Stroke::new(1.0, Color32::from_black_alpha(170));
    let corners: Vec<Pos2> = HANDLES[..4].iter().filter_map(|&h| screen(h)).collect();
    if corners.len() == 4 {
        painter.add(egui::Shape::closed_line(corners, Stroke::new(1.5, accent)));
    }
    for (i, &h) in HANDLES.iter().enumerate() {
        let Some(p) = screen(h) else { continue };
        let size = if i < 4 { 9.0 } else { 7.0 };
        let r = egui::Rect::from_center_size(p, egui::vec2(size, size));
        painter.rect_filled(r, 1.0, if i < 4 { accent } else { Color32::WHITE });
        painter.rect_stroke(r, 1.0, outline, egui::StrokeKind::Outside);
    }
    if let Some(c) = screen(CENTRE) {
        painter.circle_filled(c, HANDLE_RADIUS - 1.0, Color32::WHITE);
        painter.circle_stroke(c, HANDLE_RADIUS - 1.0, outline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-4
    }

    fn frame_of(p: &Projection) -> [Vec2; 4] {
        [0usize, 1, 2, 3].map(|i| to_uv(p, HANDLES[i]).unwrap())
    }

    #[test]
    fn the_frame_is_the_unit_tile_seen_through_the_transform() {
        let id = Projection::default();
        assert_eq!(frame_of(&id), [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y]);
        // Tiling 2: the tile is half the size; an offset moves it the other way.
        let tiled = Projection { scale: [2.0, 2.0], offset: [0.5, 0.0], ..id };
        assert!(near(to_uv(&tiled, Vec2::ZERO).unwrap(), Vec2::new(-0.25, 0.0)) && near(to_uv(&tiled, Vec2::ONE).unwrap(), Vec2::new(0.25, 0.5)));
        let turned = Projection { scale: [2.0, 0.5], rotation_deg: 30.0, offset: [0.1, 0.2], ..id };
        let uv = Vec2::new(0.3, 0.7);
        assert!(near(to_uv(&turned, to_tex(&turned, uv)).unwrap(), uv), "to_uv inverts to_tex");
    }

    #[test]
    fn moving_turning_scaling_and_mirroring_the_frame() {
        let id = Projection::default();
        // Move: the whole frame follows the drag.
        let moved = translated(&Projection { scale: [2.0, 2.0], rotation_deg: 20.0, ..id }, Vec2::new(0.1, -0.2));
        let before = frame_of(&Projection { scale: [2.0, 2.0], rotation_deg: 20.0, ..id });
        assert!(frame_of(&moved).iter().zip(&before).all(|(a, b)| near(*a, *b + Vec2::new(0.1, -0.2))));
        // Rotate: about the centre, which stays.
        let r = rotated(&id, 90.0);
        assert!(near(to_uv(&r, CENTRE).unwrap(), CENTRE));
        assert!(near(to_uv(&r, Vec2::ZERO).unwrap(), Vec2::new(1.0, 0.0)), "the (0,0) corner turned a quarter clockwise on screen");
        assert!((rotated(&Projection { rotation_deg: 170.0, ..id }, -20.0).rotation_deg + 170.0).abs() < 1e-3, "wrapped into -180..180");
        // Scale a corner: the opposite corner stays.
        let s = scaled(&id, Vec2::ONE, Vec2::new(2.0, 1.5), false, false);
        assert!(near(to_uv(&s, Vec2::ZERO).unwrap(), Vec2::ZERO) && near(to_uv(&s, Vec2::ONE).unwrap(), Vec2::new(2.0, 1.5)));
        // Keep ratio: both axes alike (the average); about the centre: the centre stays.
        let k = scaled(&id, Vec2::ONE, Vec2::new(2.0, 1.5), false, true);
        assert!((k.scale[0] - k.scale[1]).abs() < 1e-6 && (k.scale[0] - 1.0 / 1.75).abs() < 1e-4);
        let c = scaled(&id, Vec2::ONE, Vec2::new(1.5, 1.5), true, false);
        assert!(near(to_uv(&c, CENTRE).unwrap(), CENTRE) && near(to_uv(&c, Vec2::ONE).unwrap(), Vec2::new(1.5, 1.5)));
        // An edge handle scales one axis only; past the opposite side it does not turn inside out.
        let e = scaled(&id, Vec2::new(1.0, 0.5), Vec2::new(3.0, 9.0), false, false);
        assert!((e.scale[0] - 1.0 / 3.0).abs() < 1e-4 && e.scale[1] == 1.0);
        assert!(scaled(&id, Vec2::ONE, Vec2::new(-1.0, -1.0), false, false).scale.iter().all(|&s| s > 0.0));
        // Mirror on X: the centre stays, the frame's X runs the other way.
        let m = mirrored(&id, 0);
        assert!(m.scale[0] < 0.0 && near(to_uv(&m, CENTRE).unwrap(), CENTRE) && near(to_uv(&m, Vec2::ZERO).unwrap(), Vec2::new(1.0, 0.0)));
    }

    #[test]
    fn handles_centre_and_rotation_zone_are_hit() {
        // The unit tile drawn 100 points wide from (0, 0).
        let screen = |tex: Vec2| Some(Pos2::new(tex.x * 100.0, tex.y * 100.0));
        assert_eq!(hit(Pos2::new(98.0, 103.0), screen), Some(Grab::Scale(Vec2::ONE)));
        assert_eq!(hit(Pos2::new(51.0, 1.0), screen), Some(Grab::Scale(Vec2::new(0.5, 0.0))));
        assert_eq!(hit(Pos2::new(52.0, 49.0), screen), Some(Grab::Move));
        assert_eq!(hit(Pos2::new(115.0, 115.0), screen), Some(Grab::Rotate), "just outside a corner");
        assert_eq!(hit(Pos2::new(85.0, 85.0), screen), None, "inside, near a corner: no rotation");
        assert_eq!(hit(Pos2::new(30.0, 60.0), screen), None);
    }
}
