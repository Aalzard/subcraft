//! Warp projection grid in the 3D view (Painter's warp manipulator), shown while the selected
//! layer is a fill layer with a Warp projection.
//!
//! * Transform warp (default): dragging any grid point moves the whole grid in the view plane.
//! * Edit vertices (Shift+V toggles): click a point to select it, Shift+click adds, Ctrl+click
//!   removes, a drag on empty space selects by rectangle (Shift adds, Ctrl removes), a click
//!   away deselects, Esc deselects. Dragging a point moves the selection: the dragged point
//!   follows the surface under the cursor, or with Ctrl the view plane through it.
//!
//! Every drag is one undo step (the projection edit merges until the release seals it).

use egui::{Color32, Pos2, Rect, Stroke, Ui};
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command};
use subcraft_core::LayerId;
use subcraft_doc::{LayerContent, Projection, ProjectionMode, WarpGrid};

use crate::state::UiState;
use crate::theme::ACCENT();

/// Warp editing state kept across frames (not saved).
#[derive(Clone, Debug, PartialEq)]
pub struct WarpEdit {
    /// Edit vertices (true) or Transform warp (false).
    pub edit_vertices: bool,
    /// Selected point indices of `layer`'s grid.
    pub selection: Vec<usize>,
    /// The layer the selection belongs to.
    pub layer: Option<LayerId>,
    /// Handle size factor (Warp projection settings).
    pub handle_size: f32,
    /// Grid line and handle colour (Warp projection settings).
    pub color: [u8; 3],
}

impl Default for WarpEdit {
    fn default() -> Self {
        Self { edit_vertices: false, selection: Vec::new(), layer: None, handle_size: 1.0, color: [90, 200, 255] }
    }
}

impl WarpEdit {
    pub const SIZE_RANGE: std::ops::RangeInclusive<f32> = 0.5..=3.0;

    fn radius(&self) -> f32 {
        let s = if self.handle_size.is_finite() { self.handle_size.clamp(*Self::SIZE_RANGE.start(), *Self::SIZE_RANGE.end()) } else { 1.0 };
        5.0 * s
    }

    fn color32(&self) -> Color32 {
        Color32::from_rgb(self.color[0], self.color[1], self.color[2])
    }
}

/// Warp projection settings (Painter's contextual-toolbar menu): edit mode, handle size and
/// grid colour.
pub fn settings_ui(ui: &mut Ui, warp: &mut WarpEdit) {
    crate::widgets::prop_row(ui, "Edit", |ui| {
        ui.selectable_value(&mut warp.edit_vertices, false, "Transform warp").on_hover_text("Drag a point to move the whole grid (Shift+V)");
        ui.selectable_value(&mut warp.edit_vertices, true, "Edit vertices").on_hover_text("Select points and move them (Shift+V)");
    });
    crate::widgets::prop_row(ui, "Handles", |ui| {
        ui.add(egui::Slider::new(&mut warp.handle_size, WarpEdit::SIZE_RANGE).text("size"));
        let mut c = warp.color32();
        if ui.color_edit_button_srgba(&mut c).on_hover_text("Grid colour").changed() {
            warp.color = [c.r(), c.g(), c.b()];
        }
    });
}

/// The selected fill layer with a Warp projection, if any.
pub fn active(state: &UiState) -> Option<(LayerId, Projection)> {
    let layer = state.session.selected_layer()?;
    match &layer.content {
        LayerContent::Fill(f) if f.projection.mode == ProjectionMode::Warp => Some((layer.id, f.projection)),
        _ => None,
    }
}

/// The normalized bounding-cube frame of the projections (min corner, largest extent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubeFrame {
    pub min: Vec3,
    pub extent: f32,
}

impl CubeFrame {
    pub fn of(state: &UiState) -> Self {
        let b = state.session.scene().mesh.bounds;
        Self { min: b.min, extent: (b.max - b.min).max_element().max(1e-12) }
    }
    pub fn to_object(&self, n: [f32; 3]) -> Vec3 {
        self.min + Vec3::from_array(n) * self.extent
    }
    pub fn to_normalized(&self, o: Vec3) -> [f32; 3] {
        ((o - self.min) / self.extent).to_array()
    }
}

/// Where a point sits in the view, in points.
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

/// `grid` with the points `indices` moved by `delta` (normalized units).
pub fn moved(grid: &WarpGrid, start: &WarpGrid, indices: &[usize], delta: Vec3) -> WarpGrid {
    let mut g = *grid;
    for &i in indices {
        if let Some(p) = start.points().get(i) {
            g.set_point(i, (Vec3::from_array(*p) + delta).to_array());
        }
    }
    g
}

/// Indices of the points whose screen positions lie inside `r`.
pub fn points_in(screen: &[Option<Pos2>], r: Rect) -> Vec<usize> {
    screen.iter().enumerate().filter_map(|(i, p)| p.filter(|p| r.contains(*p)).map(|_| i)).collect()
}

#[derive(Clone, Debug)]
enum Gesture {
    /// Moving points: the grid at the press, the dragged point, which points move, and
    /// whether the drag stays in the view plane (Ctrl).
    Points { start: WarpGrid, anchor: usize, moving: Vec<usize>, view_plane: bool, from: Pos2, remove_on_click: bool },
    /// Rectangle selection from `from`; `mode` 0 replace, 1 add, 2 remove.
    Rect { from: Pos2, mode: u8 },
}

fn gesture_id() -> egui::Id {
    egui::Id::new("warp_grid_gesture")
}

/// Screen positions of the grid points.
fn screen_points(grid: &WarpGrid, frame: CubeFrame, camera: &Camera, rect: Rect, pixel_scale: Vec2) -> Vec<Option<Pos2>> {
    grid.points().iter().map(|p| to_pos(camera, rect, pixel_scale, frame.to_object(*p))).collect()
}

fn nearest(screen: &[Option<Pos2>], p: Pos2, radius: f32) -> Option<usize> {
    screen
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.map(|s| (i, s.distance(p))))
        .filter(|(_, d)| *d <= radius + 3.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Grid input before painting. Returns whether the pointer belongs to the grid this frame (the
/// view then ignores the press).
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
    let Some((layer, projection)) = active(state) else {
        ui.data_mut(|d| d.remove::<Gesture>(gesture_id()));
        return false;
    };
    let grid = projection.warp_grid();
    if state.warp.layer != Some(layer) || state.warp.selection.iter().any(|&i| i >= grid.points().len()) {
        state.warp.layer = Some(layer);
        state.warp.selection.clear();
    }
    // Not while a text field has the keyboard (QA 2026-11: a capital V typed in a layer name
    // with the pointer over the view switched the mode and cleared the selection).
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (shift_v, escape, mods) = ui.input(|i| (i.key_pressed(egui::Key::V) && i.modifiers.shift && !i.modifiers.ctrl && !i.modifiers.command, i.key_pressed(egui::Key::Escape), i.modifiers));
    let (shift_v, escape) = (shift_v && !typing, escape && !typing);
    if response.hovered() && shift_v {
        state.warp.edit_vertices = !state.warp.edit_vertices;
        state.warp.selection.clear();
    }
    if escape && state.warp.edit_vertices {
        state.warp.selection.clear();
    }
    let frame = CubeFrame::of(state);
    let screen = screen_points(&grid, frame, camera, rect, pixel_scale);
    let to_paint = |p: Pos2| Vec2::new((p.x - rect.min.x) * pixel_scale.x, (p.y - rect.min.y) * pixel_scale.y);
    let ctrl = mods.ctrl || mods.command;
    let mut gesture: Option<Gesture> = ui.data(|d| d.get_temp(gesture_id()));

    if gesture.is_none() && primary_pressed && response.hovered() && let Some(p) = pointer {
        let hit = nearest(&screen, p, state.warp.radius());
        gesture = match (state.warp.edit_vertices, hit) {
            // Transform warp: a press on any point drags the whole grid in the view plane.
            (false, Some(i)) => Some(Gesture::Points { start: grid, anchor: i, moving: (0..grid.points().len()).collect(), view_plane: true, from: p, remove_on_click: false }),
            (false, None) => None,
            (true, Some(i)) => {
                let sel = &mut state.warp.selection;
                if mods.shift && !sel.contains(&i) {
                    sel.push(i);
                } else if !ctrl && !mods.shift && !sel.contains(&i) {
                    *sel = vec![i];
                }
                // Ctrl+click on a selected point removes it; Ctrl+drag moves in the view plane.
                let moving = if sel.contains(&i) { sel.clone() } else { vec![i] };
                Some(Gesture::Points { start: grid, anchor: i, moving, view_plane: ctrl, from: p, remove_on_click: ctrl })
            }
            (true, None) => Some(Gesture::Rect { from: p, mode: if mods.shift { 1 } else if ctrl { 2 } else { 0 } }),
        };
    }

    let Some(g) = gesture else { return false };
    let released = !primary_down;
    match &g {
        Gesture::Points { start, anchor, moving, view_plane, from, remove_on_click } => {
            let moved_far = pointer.is_some_and(|p| p.distance(*from) > 3.0);
            if moved_far && let Some(p) = pointer {
                let anchor_start = frame.to_object(start.points()[*anchor]);
                let target = if *view_plane {
                    on_view_plane(camera, anchor_start, to_paint(p))
                } else {
                    // On the surface under the cursor; off the model, the view plane.
                    state.session.pick(camera, to_paint(p)).map(|h| h.position).or_else(|| on_view_plane(camera, anchor_start, to_paint(p)))
                };
                if let Some(target) = target {
                    let delta = (target - anchor_start) / frame.extent;
                    let new = moved(&grid, start, moving, delta);
                    if new != grid {
                        let _ = state.session.execute(Command::SetFillProjection { layer, projection: Projection { mode: ProjectionMode::Warp, warp: Some(new), ..projection } });
                    }
                }
            }
            if released {
                if !moved_far && *remove_on_click {
                    state.warp.selection.retain(|&i| i != *anchor);
                }
                let _ = state.session.execute(Command::Seal);
            }
        }
        Gesture::Rect { from, mode } => {
            if released {
                let to = pointer.unwrap_or(*from);
                let r = Rect::from_two_pos(*from, to);
                let inside = if r.width().max(r.height()) < 3.0 { Vec::new() } else { points_in(&screen, r) };
                let sel = &mut state.warp.selection;
                match mode {
                    1 => sel.extend(inside.into_iter().filter(|i| !sel.contains(i)).collect::<Vec<_>>()),
                    2 => sel.retain(|i| !inside.contains(i)),
                    _ => *sel = inside,
                }
            }
        }
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

/// Grid lines, points (selected ones filled) and the rectangle being dragged.
pub fn overlay(state: &UiState, ui: &Ui, painter: &egui::Painter, rect: Rect, pixel_scale: Vec2, camera: &Camera) {
    let Some((_, projection)) = active(state) else { return };
    let grid = projection.warp_grid();
    let frame = CubeFrame::of(state);
    let screen = screen_points(&grid, frame, camera, rect, pixel_scale);
    let color = state.warp.color32();
    let w = grid.width();
    let rows = grid.points().len() / w;
    let line = |a: usize, b: usize| {
        if let (Some(a), Some(b)) = (screen[a], screen[b]) {
            painter.line_segment([a, b], Stroke::new(3.0, Color32::from_black_alpha(110)));
            painter.line_segment([a, b], Stroke::new(1.4, color));
        }
    };
    for r in 0..rows {
        for c in 0..w {
            let i = r * w + c;
            if c + 1 < w { line(i, i + 1); }
            if r + 1 < rows { line(i, i + w); }
        }
    }
    let radius = state.warp.radius();
    let edit = state.warp.edit_vertices;
    for (i, p) in screen.iter().enumerate() {
        let Some(p) = *p else { continue };
        let selected = edit && state.warp.selection.contains(&i);
        let r = if edit { radius } else { radius * 0.7 };
        painter.circle_filled(p, r, if selected { ACCENT } else { Color32::from_black_alpha(150) });
        painter.circle_stroke(p, r, Stroke::new(1.5, if selected { Color32::WHITE } else { color }));
    }
    if let Some(Gesture::Rect { from, .. }) = ui.data(|d| d.get_temp::<Gesture>(gesture_id()))
        && let Some(to) = ui.input(|i| i.pointer.hover_pos())
    {
        let r = Rect::from_two_pos(from, to);
        painter.rect_filled(r, 0.0, ACCENT.gamma_multiply(0.08));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
    }
    let mode = if edit { "Warp · Edit vertices (Shift+V: Transform warp)" } else { "Warp · Transform warp: drag a point to move the grid (Shift+V: Edit vertices)" };
    let pos = rect.center_bottom() - egui::vec2(0.0, 42.0);
    let galley = painter.layout_no_wrap(mode.to_owned(), egui::FontId::proportional(12.0), Color32::WHITE);
    let bg = Rect::from_center_size(pos, galley.size() + egui::vec2(14.0, 8.0));
    painter.rect_filled(bg, 4.0, Color32::from_black_alpha(170));
    painter.galley(bg.center() - galley.size() / 2.0, galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_points_shifts_only_the_chosen_ones_from_the_press_state() {
        let start = WarpGrid::planar(2, 2, 2);
        let delta = Vec3::new(0.1, -0.05, 0.0);
        let g = moved(&start, &start, &[0, 4], delta);
        assert!((g.points()[0][0] - 0.1).abs() < 1e-6 && (g.points()[4][1] - 0.45).abs() < 1e-6);
        assert_eq!(g.points()[1], start.points()[1]);
        // Moving again from the press state does not accumulate.
        let again = moved(&g, &start, &[0, 4], delta);
        assert_eq!(again, g);
    }

    #[test]
    fn rectangle_picks_points_inside_only() {
        let screen = vec![Some(Pos2::new(10.0, 10.0)), None, Some(Pos2::new(50.0, 50.0)), Some(Pos2::new(200.0, 5.0))];
        assert_eq!(points_in(&screen, Rect::from_two_pos(Pos2::new(0.0, 0.0), Pos2::new(60.0, 60.0))), vec![0, 2]);
        assert_eq!(nearest(&screen, Pos2::new(52.0, 49.0), 5.0), Some(2));
        assert_eq!(nearest(&screen, Pos2::new(100.0, 100.0), 5.0), None);
    }

    #[test]
    fn the_cube_frame_round_trips() {
        let f = CubeFrame { min: Vec3::new(-1.0, -2.0, 0.0), extent: 4.0 };
        let o = Vec3::new(0.5, 1.0, 3.0);
        assert!(f.to_object(f.to_normalized(o)).distance(o) < 1e-6);
    }
}
