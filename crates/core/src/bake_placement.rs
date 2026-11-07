//! Where the high-poly bake source sits relative to the low poly.
//!
//! AI-generated or hand-sculpted high polys rarely line up with the low poly. A
//! [`BakePlacement`] is a small, saved description of a transform applied to the *loaded* high
//! poly just before baking (and in the overlay); the imported file is never modified.
//!
//! The transform maps a high-poly point `p` to
//! `anchor + offset + R * S * (p - pivot)` where `pivot` is the point of the high poly that
//! stays put under rotation / scaling (its own origin or its bounding-box centre), `anchor` is
//! where that pivot is placed (unchanged, the low poly's bounding-box centre or its origin),
//! `S` is the uniform scale times the per-axis scale and `R` is an XYZ Euler rotation in degrees.
//! The default is the identity.

use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

/// Which point of the high poly is the pivot for rotation and scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacePivot {
    /// The high poly's own origin (as exported).
    #[default]
    Origin,
    /// The centre of the high poly's bounding box.
    BoundsCenter,
}

/// Where the pivot is put.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceAnchor {
    /// Nowhere new: the pivot stays where the file has it.
    #[default]
    Keep,
    /// The centre of the low poly's bounding box.
    LowCenter,
    /// The low poly's origin.
    LowOrigin,
}

/// How `auto_fit_scale` compares the two bounding boxes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    /// Make the heights (Y extents) equal.
    Height,
    /// Make the longest sides equal.
    #[default]
    LongestAxis,
    /// Largest scale at which the high poly still fits inside the low poly's box on every axis.
    FitBoth,
}

impl FitMode {
    pub const ALL: [Self; 3] = [Self::Height, Self::LongestAxis, Self::FitBoth];
    pub fn label(self) -> &'static str {
        match self { Self::Height => "Match height", Self::LongestAxis => "Match longest side", Self::FitBoth => "Fit inside (all axes)" }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BakePlacement {
    pub pivot: PlacePivot,
    pub anchor: PlaceAnchor,
    /// Extra move in world units (after anchoring).
    pub offset: [f32; 3],
    /// Uniform scale (1 = unchanged).
    pub scale: f32,
    /// Extra per-axis scale multiplied with `scale` (1 = unchanged).
    pub axis_scale: [f32; 3],
    /// XYZ Euler rotation in degrees.
    pub rotation_deg: [f32; 3],
}

impl Default for BakePlacement {
    fn default() -> Self {
        Self { pivot: PlacePivot::Origin, anchor: PlaceAnchor::Keep, offset: [0.; 3], scale: 1., axis_scale: [1.; 3], rotation_deg: [0.; 3] }
    }
}

/// An axis-aligned box as `(min, max)`.
pub type Box3 = (Vec3, Vec3);

fn center(b: Box3) -> Vec3 { (b.0 + b.1) * 0.5 }
fn extent(b: Box3) -> Vec3 { (b.1 - b.0).max(Vec3::ZERO) }

impl BakePlacement {
    pub fn is_identity(&self) -> bool { *self == Self::default() }

    /// Everything back to the identity.
    pub fn reset(&mut self) { *self = Self::default(); }

    pub fn offset_vec(&self) -> Vec3 { Vec3::from(self.offset) }

    fn linear(&self) -> Mat4 {
        let r = Quat::from_euler(EulerRot::XYZ, self.rotation_deg[0].to_radians(), self.rotation_deg[1].to_radians(), self.rotation_deg[2].to_radians());
        Mat4::from_quat(r) * Mat4::from_scale(self.scale_vec())
    }

    pub fn scale_vec(&self) -> Vec3 { Vec3::from(self.axis_scale) * self.scale }

    /// The matrix applied to high-poly points, given the high poly's and low poly's boxes.
    pub fn matrix(&self, low: Box3, high: Box3) -> Mat4 {
        let pivot = match self.pivot { PlacePivot::Origin => Vec3::ZERO, PlacePivot::BoundsCenter => center(high) };
        let anchor = match self.anchor { PlaceAnchor::Keep => pivot, PlaceAnchor::LowCenter => center(low), PlaceAnchor::LowOrigin => Vec3::ZERO };
        Mat4::from_translation(anchor + self.offset_vec()) * self.linear() * Mat4::from_translation(-pivot)
    }

    /// Box of the high poly after the placement (exact for the 8 corners of its box).
    pub fn placed_bounds(&self, low: Box3, high: Box3) -> Box3 {
        let m = self.matrix(low, high);
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for i in 0..8 {
            let c = Vec3::new(if i & 1 == 0 { high.0.x } else { high.1.x }, if i & 2 == 0 { high.0.y } else { high.1.y }, if i & 4 == 0 { high.0.z } else { high.1.z });
            let p = m.transform_point3(c);
            lo = lo.min(p); hi = hi.max(p);
        }
        (lo, hi)
    }

    /// The uniform `scale` that matches the placed high poly's box to the low poly's box
    /// (keeping the current rotation and per-axis scale), or `None` for a degenerate box.
    pub fn auto_fit_scale(&self, low: Box3, high: Box3, mode: FitMode) -> Option<f32> {
        let unit = Self { scale: 1., offset: [0.; 3], anchor: PlaceAnchor::Keep, pivot: PlacePivot::BoundsCenter, ..self.clone() };
        let h = extent(unit.placed_bounds(low, high));
        let l = extent(low);
        let ratio = |a: f32, b: f32| (b > 1e-9 && a > 1e-9).then_some(a / b);
        let s = match mode {
            FitMode::Height => ratio(l.y, h.y),
            FitMode::LongestAxis => ratio(l.max_element(), h.max_element()),
            FitMode::FitBoth => {
                let rs: Vec<f32> = (0..3).filter_map(|i| ratio(l[i], h[i])).collect();
                rs.into_iter().reduce(f32::min)
            }
        };
        s.filter(|s| s.is_finite() && *s > 0.)
    }

    /// Centre on the low poly (bounding boxes) with the high poly's centre as pivot, clearing the offset.
    pub fn center_on_low(&mut self) { self.pivot = PlacePivot::BoundsCenter; self.anchor = PlaceAnchor::LowCenter; self.offset = [0.; 3]; }

    /// Put the high poly's origin on the low poly's origin, clearing the offset.
    pub fn pivot_to_low_origin(&mut self) { self.pivot = PlacePivot::Origin; self.anchor = PlaceAnchor::LowOrigin; self.offset = [0.; 3]; }

    /// Centre plus auto-fit scale in one step. Returns false when the boxes are degenerate.
    pub fn auto_align(&mut self, low: Box3, high: Box3, mode: FitMode) -> bool {
        self.center_on_low();
        match self.auto_fit_scale(low, high, mode) { Some(s) => { self.scale = s; true } None => false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOW: Box3 = (Vec3::new(-1., 0., -1.), Vec3::new(1., 2., 1.));
    const HIGH: Box3 = (Vec3::new(10., 10., 10.), Vec3::new(14., 18., 12.)); // 4 x 8 x 2, centre (12, 14, 11)

    fn near(a: Vec3, b: Vec3) -> bool { (a - b).length() < 1e-4 }

    #[test]
    fn default_is_identity_and_serializes_empty() {
        let p = BakePlacement::default();
        assert!(p.is_identity());
        assert!(near(p.matrix(LOW, HIGH).transform_point3(Vec3::new(3., 4., 5.)), Vec3::new(3., 4., 5.)));
        // Old files without the field, or with a partial one, load with defaults.
        let q: BakePlacement = serde_json::from_str("{}").unwrap();
        assert_eq!(q, p);
        let q: BakePlacement = serde_json::from_str(r#"{"scale":2.0}"#).unwrap();
        assert_eq!(q.scale, 2.0);
        assert_eq!(q.axis_scale, [1.; 3]);
    }

    #[test]
    fn center_on_low_moves_box_centre_onto_low_centre() {
        let mut p = BakePlacement::default();
        p.center_on_low();
        let (lo, hi) = p.placed_bounds(LOW, HIGH);
        assert!(near((lo + hi) * 0.5, Vec3::new(0., 1., 0.)));
        assert!(near(hi - lo, Vec3::new(4., 8., 2.)), "size is unchanged");
    }

    #[test]
    fn pivot_to_low_origin_puts_high_origin_on_low_origin() {
        let mut p = BakePlacement::default();
        p.pivot_to_low_origin();
        assert!(near(p.matrix(LOW, HIGH).transform_point3(Vec3::ZERO), Vec3::ZERO));
        p.anchor = PlaceAnchor::LowOrigin; p.offset = [1., 2., 3.];
        assert!(near(p.matrix(LOW, HIGH).transform_point3(Vec3::ZERO), Vec3::new(1., 2., 3.)));
    }

    #[test]
    fn scale_is_about_the_pivot() {
        let p = BakePlacement { pivot: PlacePivot::BoundsCenter, scale: 0.5, ..Default::default() };
        let m = p.matrix(LOW, HIGH);
        assert!(near(m.transform_point3(Vec3::new(12., 14., 11.)), Vec3::new(12., 14., 11.)), "centre stays");
        assert!(near(m.transform_point3(Vec3::new(14., 14., 11.)), Vec3::new(13., 14., 11.)));
    }

    #[test]
    fn auto_fit_modes() {
        let p = BakePlacement::default();
        // Low is 2 x 2 x 2, high 4 x 8 x 2.
        assert_eq!(p.auto_fit_scale(LOW, HIGH, FitMode::Height), Some(0.25));
        assert_eq!(p.auto_fit_scale(LOW, HIGH, FitMode::LongestAxis), Some(0.25));
        assert_eq!(p.auto_fit_scale(LOW, HIGH, FitMode::FitBoth), Some(0.25));
        let wide: Box3 = (Vec3::ZERO, Vec3::new(8., 2., 2.));
        assert_eq!(p.auto_fit_scale(LOW, wide, FitMode::Height), Some(1.0));
        assert_eq!(p.auto_fit_scale(LOW, wide, FitMode::LongestAxis), Some(0.25));
        assert_eq!(p.auto_fit_scale(LOW, wide, FitMode::FitBoth), Some(0.25));
        assert_eq!(p.auto_fit_scale(LOW, (Vec3::ZERO, Vec3::ZERO), FitMode::Height), None);
    }

    #[test]
    fn auto_fit_accounts_for_rotation() {
        // Rotating 90° about Z turns the 4 x 8 box into 8 x 4.
        let p = BakePlacement { rotation_deg: [0., 0., 90.], ..Default::default() };
        let s = p.auto_fit_scale(LOW, HIGH, FitMode::Height).unwrap();
        assert!((s - 0.5).abs() < 1e-4, "{s}");
    }

    #[test]
    fn auto_align_centres_and_fits() {
        let mut p = BakePlacement::default();
        assert!(p.auto_align(LOW, HIGH, FitMode::LongestAxis));
        let (lo, hi) = p.placed_bounds(LOW, HIGH);
        assert!(near((lo + hi) * 0.5, Vec3::new(0., 1., 0.)));
        assert!(near(hi - lo, Vec3::new(1., 2., 0.5)));
        assert!(!p.auto_align(LOW, (Vec3::ZERO, Vec3::ZERO), FitMode::Height));
    }
}
