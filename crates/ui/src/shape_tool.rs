//! Shape fill tool: drag a rectangle or ellipse in the 3D or 2D view; it is filled once with the
//! brush material ([`subcraft_app::shape_fill`]). Shift keeps it square / round, Ctrl drags it
//! from its centre, Esc cancels the drag.

use egui::{Color32, Pos2, Rect, Stroke, Ui, Vec2};
use subcraft_paint::shape::{Shape, ShapeKind};

use crate::state::UiState;
use crate::widgets::{hint, prop_row_reset, slider_row_reset};

/// The box a drag from `start` to `end` makes: `square` gives it equal sides (the longer drag
/// side wins, the corner keeps its direction), `centred` makes `start` its centre.
pub fn drag_box(start: Vec2, end: Vec2, square: bool, centred: bool) -> (Vec2, Vec2) {
    let mut d = end - start;
    if square {
        let side = d.x.abs().max(d.y.abs());
        d = Vec2::new(side.copysign(d.x), side.copysign(d.y));
    }
    let (a, b) = if centred { (start - d, start + d) } else { (start, start + d) };
    (a.min(b), a.max(b))
}

/// Points of the shape's outline in the box `min..max` (for the drag preview), as fractions of
/// the box mapped through `to_screen`, so a turned 2D view draws it turned with the texture.
pub fn outline(shape: &Shape, min: Vec2, max: Vec2, to_screen: impl Fn(Vec2) -> Pos2) -> Vec<Pos2> {
    let c = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    const N: usize = 96;
    let point = |t: f32| -> Vec2 {
        let (s, co) = (t * std::f32::consts::TAU).sin_cos();
        let dir = Vec2::new(co, s);
        match shape.kind {
            ShapeKind::Ellipse => c + dir * half,
            ShapeKind::Rectangle => {
                // Walk out from the centre to the (rounded) edge along `dir`.
                let r = shape.corner.clamp(0.0, 1.0) * half.min_elem();
                let inner = half - Vec2::splat(r);
                let ray = |k: f32| {
                    let p = (dir * k).abs();
                    let q = p - inner;
                    q.max(Vec2::ZERO).length() + q.max_elem().min(0.0) - r
                };
                let (mut lo, mut hi) = (0.0f32, half.length() * 2.0);
                for _ in 0..30 {
                    let mid = (lo + hi) * 0.5;
                    if ray(mid) > 0.0 { hi = mid } else { lo = mid }
                }
                c + dir * lo
            }
        }
    };
    let mut pts: Vec<Pos2> = (0..N).map(|i| to_screen(point(i as f32 / N as f32))).collect();
    if shape.kind == ShapeKind::Rectangle && shape.corner <= 0.0 {
        // Square corners: draw the exact box instead of sampled points.
        pts = [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)].into_iter().map(&to_screen).collect();
    }
    pts
}

/// Draw the shape being dragged.
pub fn draw_preview(painter: &egui::Painter, points: Vec<Pos2>, bounds: Rect) {
    let accent = crate::theme::ACCENT();
    if points.len() < 3 {
        return;
    }
    painter.add(egui::Shape::convex_polygon(points.clone(), Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 40), Stroke::NONE));
    painter.add(egui::Shape::closed_line(points, Stroke::new(1.5, accent)));
    painter.text(bounds.right_bottom() + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, format!("{:.0} × {:.0}", bounds.width(), bounds.height()), crate::theme::font(11.0), accent);
}

/// The tool's options, at the top of the brush Properties.
pub fn options(state: &mut UiState, ui: &mut Ui) {
    let d = Shape::default();
    let s = &mut state.shape;
    prop_row_reset(ui, "Shape", &mut s.kind, d.kind, |ui, v| {
        ui.horizontal(|ui| {
            for k in ShapeKind::ALL {
                let label = match k {
                    ShapeKind::Rectangle => crate::i18n::tr("Rectangle"),
                    ShapeKind::Ellipse => crate::i18n::tr("Ellipse"),
                };
                ui.selectable_value(v, k, label);
            }
        })
        .response
    });
    if s.kind == ShapeKind::Rectangle {
        let mut corner = s.corner * 100.0;
        if slider_row_reset(ui, "Corners", &mut corner, d.corner * 100.0, |v| egui::Slider::new(v, 0.0..=100.0).max_decimals(0).suffix("%"))
            .on_hover_text(crate::i18n::tr("Rounded corners: 100 % rounds the shorter sides fully"))
            .changed()
        {
            s.corner = corner / 100.0;
        }
    }
    let mut feather = s.feather * 100.0;
    if slider_row_reset(ui, "Feather", &mut feather, d.feather * 100.0, |v| egui::Slider::new(v, 0.0..=100.0).max_decimals(0).suffix("%"))
        .on_hover_text(crate::i18n::tr("Soft edge inside the shape, as a share of half its shorter side"))
        .changed()
    {
        s.feather = feather / 100.0;
    }
    hint(ui, "Drag in the 3D or 2D view to fill the shape with the brush material, in one undo step. Shift: square / circle. Ctrl: from the centre. In the 3D view the shape covers what the box shows; start it with its centre over the model.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_box_squares_and_centres() {
        let (a, b) = drag_box(Vec2::new(10.0, 10.0), Vec2::new(40.0, 20.0), false, false);
        assert_eq!((a, b), (Vec2::new(10.0, 10.0), Vec2::new(40.0, 20.0)));
        // Dragging up-left: the box is normalised.
        let (a, b) = drag_box(Vec2::new(40.0, 20.0), Vec2::new(10.0, 10.0), false, false);
        assert_eq!((a, b), (Vec2::new(10.0, 10.0), Vec2::new(40.0, 20.0)));
        let (a, b) = drag_box(Vec2::new(10.0, 10.0), Vec2::new(40.0, 20.0), true, false);
        assert_eq!((a, b), (Vec2::new(10.0, 10.0), Vec2::new(40.0, 40.0)));
        let (a, b) = drag_box(Vec2::new(10.0, 10.0), Vec2::new(0.0, 15.0), true, true);
        assert_eq!((a, b), (Vec2::new(0.0, 0.0), Vec2::new(20.0, 20.0)));
    }

    #[test]
    fn outlines_follow_the_shape() {
        let id = |v: Vec2| Pos2::new(v.x, v.y);
        let (min, max) = (Vec2::new(0.0, 0.0), Vec2::new(40.0, 20.0));
        let rect = outline(&Shape::default(), min, max, id);
        assert_eq!(rect.len(), 4);
        let ellipse = outline(&Shape { kind: ShapeKind::Ellipse, ..Shape::default() }, min, max, id);
        assert!(ellipse.iter().all(|p| {
            let q = (Vec2::new(p.x, p.y) - Vec2::new(20.0, 10.0)) / Vec2::new(20.0, 10.0);
            (q.length() - 1.0).abs() < 1e-3
        }));
        let rounded = outline(&Shape { corner: 1.0, ..Shape::default() }, min, max, id);
        // Every point lies on the box or inside it, and a corner is cut.
        assert!(rounded.iter().all(|p| p.x >= -0.01 && p.x <= 40.01 && p.y >= -0.01 && p.y <= 20.01));
        assert!(rounded.iter().all(|p| Vec2::new(p.x, p.y).length() > 2.0));
    }
}
