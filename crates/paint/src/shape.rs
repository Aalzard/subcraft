//! Shape fill: a rectangle or ellipse dragged in a view, painted once with the brush material.
//!
//! The shape is one camera-aligned (or UV-aligned) dab whose tip is the shape: [`tip`] renders
//! it into a square bitmap and [`Shape::dab`] gives the brush radius, roundness and angle that
//! stretch that square back over the dragged box. The bitmap is drawn in shape units, so the
//! soft edge is as wide along both sides of a long thin shape.

use std::sync::Arc;

use glam::Vec2;
use serde::{Deserialize, Serialize};
use subcraft_image::Bitmap;

/// Side of the tip bitmap in pixels.
pub const TIP_SIZE: u32 = 512;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    #[default]
    Rectangle,
    Ellipse,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 2] = [ShapeKind::Rectangle, ShapeKind::Ellipse];

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
        }
    }
}

/// Shape tool options.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shape {
    pub kind: ShapeKind,
    /// Rounded corners of a rectangle, as a fraction of half its shorter side (0 = square
    /// corners, 1 = fully round ends).
    pub corner: f32,
    /// Soft edge inside the shape, as a fraction of half its shorter side (0 = a crisp edge).
    pub feather: f32,
}

impl Default for Shape {
    fn default() -> Self {
        Self { kind: ShapeKind::Rectangle, corner: 0.0, feather: 0.0 }
    }
}

/// Brush values that lay a shape tip over a box of `width` × `height` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dab {
    /// Brush size in Viewport size space: percent of the view height (the dab's diameter is the
    /// box's longer side).
    pub size: f32,
    /// Shorter side / longer side.
    pub roundness: f32,
    /// 0 for a wide box, 90 for a tall one (the tip's long axis follows the longer side).
    pub angle_deg: f32,
}

impl Shape {
    /// Brush values for a `width` × `height` pixel box in a view `view_height` pixels tall.
    /// `None` for a box with no area or a view with no height.
    pub fn dab(width: f32, height: f32, view_height: f32) -> Option<Dab> {
        let (w, h) = (width.abs(), height.abs());
        if !(w >= 1.0 && h >= 1.0 && view_height > 0.0 && w.is_finite() && h.is_finite()) {
            return None;
        }
        let (long, short) = if w >= h { (w, h) } else { (h, w) };
        Some(Dab { size: long / view_height * 100.0, roundness: short / long, angle_deg: if w >= h { 0.0 } else { 90.0 } })
    }

    /// Signed distance from `p` to the edge of the shape whose half extents are `half`
    /// (negative inside), in the same units.
    pub fn distance(&self, p: Vec2, half: Vec2) -> f32 {
        let p = p.abs();
        match self.kind {
            ShapeKind::Rectangle => {
                let r = self.corner.clamp(0.0, 1.0) * half.min_element();
                let q = p - (half - Vec2::splat(r));
                q.max(Vec2::ZERO).length() + q.max_element().min(0.0) - r
            }
            ShapeKind::Ellipse => {
                // Inigo Quilez's ellipse distance bound: exact on the axes, close elsewhere.
                let k0 = (p / half).length();
                let k1 = (p / (half * half)).length();
                if k1 < 1e-12 { -half.min_element() } else { k0 * (k0 - 1.0) / k1 }
            }
        }
    }

    /// Coverage (0..1) at `p` for a shape with half extents `half`: 1 inside, falling to 0 at the
    /// edge over the feather width (at least `aa`, one sample's width, for a smooth crisp edge).
    pub fn coverage(&self, p: Vec2, half: Vec2, aa: f32) -> f32 {
        let d = self.distance(p, half);
        let soft = (self.feather.clamp(0.0, 1.0) * half.min_element()).max(aa);
        let t = (-d / soft).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

/// The tip for a shape whose shorter side is `roundness` times its longer side, laid along x
/// (the longer side). The square bitmap spans the box: x over the long side, y over the short.
pub fn tip(shape: &Shape, roundness: f32, size: u32) -> Arc<Bitmap> {
    let n = size.max(8);
    let half = Vec2::new(1.0, roundness.clamp(0.01, 1.0));
    // One bitmap pixel along the short side, in shape units.
    let aa = 2.0 * half.y / n as f32 * 1.5;
    let mut data = vec![0.0f32; (n * n) as usize];
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            data[(y * n + x) as usize] = shape.coverage(Vec2::new(u, v) * half, half, aa);
        }
    }
    Arc::new(Bitmap::new_f32(n, n, 1, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dab_fits_the_long_side_and_turns_for_tall_boxes() {
        let d = Shape::dab(200.0, 50.0, 400.0).unwrap();
        assert_eq!((d.size, d.roundness, d.angle_deg), (50.0, 0.25, 0.0));
        let d = Shape::dab(-50.0, 200.0, 400.0).unwrap();
        assert_eq!((d.size, d.roundness, d.angle_deg), (50.0, 0.25, 90.0));
        assert!(Shape::dab(0.0, 10.0, 400.0).is_none());
        assert!(Shape::dab(10.0, 10.0, 0.0).is_none());
    }

    #[test]
    fn distances_are_exact_on_the_axes() {
        let half = Vec2::new(2.0, 1.0);
        for kind in ShapeKind::ALL {
            let s = Shape { kind, ..Shape::default() };
            assert!((s.distance(Vec2::new(3.0, 0.0), half) - 1.0).abs() < 1e-5, "{kind:?}");
            assert!((s.distance(Vec2::new(0.0, 0.5), half) + 0.5).abs() < 1e-5, "{kind:?}");
            assert!(s.distance(Vec2::ZERO, half) < 0.0);
        }
        // The rectangle's corner is in, the ellipse's is out.
        let corner = Vec2::new(1.9, 0.9);
        assert!(Shape { kind: ShapeKind::Rectangle, ..Shape::default() }.distance(corner, half) < 0.0);
        assert!(Shape { kind: ShapeKind::Ellipse, ..Shape::default() }.distance(corner, half) > 0.0);
        // Fully rounded corners make a stadium: the corner is out, the edge middle is not moved.
        let round = Shape { kind: ShapeKind::Rectangle, corner: 1.0, feather: 0.0 };
        assert!(round.distance(corner, half) > 0.0);
        assert!(round.distance(Vec2::new(0.0, 0.99), half) < 0.0);
    }

    #[test]
    fn the_tip_fills_its_shape_with_a_thin_soft_edge() {
        let n = 128;
        let at = |b: &Bitmap, x: u32, y: u32| b.sample((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32, subcraft_image::Wrap::Clamp, false)[0];
        let rect = tip(&Shape::default(), 0.5, n);
        // Crisp: the whole square is covered but its outermost ring.
        let covered = (0..n * n).filter(|i| at(&rect, i % n, i / n) > 0.99).count();
        assert!(covered as f32 > 0.9 * (n * n) as f32, "{covered}");
        assert_eq!(at(&rect, n / 2, n / 2), 1.0);
        let ell = tip(&Shape { kind: ShapeKind::Ellipse, ..Shape::default() }, 0.5, n);
        let area = (0..n * n).map(|i| at(&ell, i % n, i / n)).sum::<f32>() / (n * n) as f32;
        assert!((area - std::f32::consts::FRAC_PI_4).abs() < 0.02, "an ellipse covers pi/4 of its box: {area}");
        assert!(at(&ell, 1, 1) < 0.01 && at(&ell, n / 2, n / 2) == 1.0);
        // A feather softens inside the edge; the middle stays full.
        let soft = tip(&Shape { feather: 1.0, ..Shape::default() }, 0.5, n);
        assert!(at(&soft, n / 2, n / 2) > 0.99);
        assert!((0.2..0.8).contains(&at(&soft, n / 2, n / 4)), "{}", at(&soft, n / 2, n / 4));
        // The soft edge is as wide on the long side as on the short side (in shape units).
        let edge_x = (0..n / 2).find(|&x| at(&soft, x, n / 2) > 0.5).unwrap() as f32 / n as f32 * 2.0;
        let edge_y = (0..n / 2).find(|&y| at(&soft, n / 2, y) > 0.5).unwrap() as f32 / n as f32 * 2.0 * 0.5;
        assert!((edge_x - edge_y).abs() < 0.05, "x {edge_x}, y {edge_y}");
    }
}
