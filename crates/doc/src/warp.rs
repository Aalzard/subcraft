//! Warp projection grid (Painter's Warp projection): a planar projection whose plane is a grid of
//! movable points. A texel is mapped through the grid cell that covers it seen down the
//! projection axis, so moving points bends and stretches the projected image; outside the grid
//! nothing is projected.
//!
//! Points live in the normalized bounding-cube frame the other 3D projections use (mesh bounds
//! min corner = 0, divided by the largest extent). The default grid is the planar projection's
//! square at the middle of the cube, so an untouched Warp projection equals Planar.

use serde::{Deserialize, Serialize};

/// Most points per grid side (6 × 6 cells).
pub const MAX_WARP_SIDE: usize = 7;
/// Room for the largest grid, row by row.
pub const MAX_WARP_POINTS: usize = MAX_WARP_SIDE * MAX_WARP_SIDE;
/// Cells per side of a new grid.
pub const DEFAULT_WARP_CELLS: u8 = 3;
/// Points stay within this distance of the origin, in units of the model's bounding cube (which
/// spans 0 to 1). Far larger values overflowed the cell maths to NaN texture coordinates.
pub const MAX_WARP_COORD: f32 = 4.0;

/// 2D coordinates of a normalized position seen down `axis` (the planar projection's frame:
/// X → (z, 1−y), Y → (x, z), Z → (x, 1−y)).
#[inline]
pub fn plane_coords(pos: [f32; 3], axis: u8) -> [f32; 2] {
    match axis {
        0 => [pos[2], 1.0 - pos[1]],
        1 => [pos[0], pos[2]],
        _ => [pos[0], 1.0 - pos[1]],
    }
}

/// The normalized position with plane coordinates `q` at depth `depth` along `axis`
/// (inverse of [`plane_coords`]).
#[inline]
pub fn from_plane_coords(q: [f32; 2], axis: u8, depth: f32) -> [f32; 3] {
    match axis {
        0 => [depth, 1.0 - q[1], q[0]],
        1 => [q[0], depth, q[1]],
        _ => [q[0], 1.0 - q[1], depth],
    }
}

/// A grid of `(rows + 1) × (cols + 1)` points, row by row. Each column and row has an image
/// coordinate (`us`, `vs`, from 0 at the first to 1 at the last), so a split adds points
/// without moving the image.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "WarpGridFile", into = "WarpGridFile")]
pub struct WarpGrid {
    cols: u8,
    rows: u8,
    points: [[f32; 3]; MAX_WARP_POINTS],
    us: [f32; MAX_WARP_SIDE],
    vs: [f32; MAX_WARP_SIDE],
}

/// File form: only the used points and coordinates, checked on load.
#[derive(Serialize, Deserialize)]
struct WarpGridFile {
    cols: u8,
    rows: u8,
    points: Vec<[f32; 3]>,
    us: Vec<f32>,
    vs: Vec<f32>,
}

impl TryFrom<WarpGridFile> for WarpGrid {
    type Error = String;
    fn try_from(f: WarpGridFile) -> Result<Self, String> {
        let max = (MAX_WARP_SIDE - 1) as u8;
        if !(1..=max).contains(&f.cols) || !(1..=max).contains(&f.rows) {
            return Err(format!("warp grid of {} × {} cells (1 to {max} per side)", f.cols, f.rows));
        }
        let n = (f.cols as usize + 1) * (f.rows as usize + 1);
        if f.points.len() != n {
            return Err(format!("warp grid needs {n} points, found {}", f.points.len()));
        }
        if !f.points.iter().flatten().all(|v| v.is_finite() && v.abs() <= MAX_WARP_COORD) {
            return Err(format!("warp grid point outside ±{MAX_WARP_COORD} or not a number"));
        }
        let increasing = |v: &[f32]| v.iter().all(|x| x.is_finite()) && v.windows(2).all(|w| w[0] < w[1]);
        if f.us.len() != f.cols as usize + 1 || f.vs.len() != f.rows as usize + 1 || !increasing(&f.us) || !increasing(&f.vs) {
            return Err("warp grid image coordinates must increase, one per column and row".into());
        }
        let mut points = [[0.0; 3]; MAX_WARP_POINTS];
        points[..n].copy_from_slice(&f.points);
        let (mut us, mut vs) = ([0.0; MAX_WARP_SIDE], [0.0; MAX_WARP_SIDE]);
        us[..f.us.len()].copy_from_slice(&f.us);
        vs[..f.vs.len()].copy_from_slice(&f.vs);
        Ok(Self { cols: f.cols, rows: f.rows, points, us, vs })
    }
}

impl From<WarpGrid> for WarpGridFile {
    fn from(g: WarpGrid) -> Self {
        Self { cols: g.cols, rows: g.rows, points: g.points().to_vec(), us: g.us[..g.width()].to_vec(), vs: g.vs[..g.rows as usize + 1].to_vec() }
    }
}

impl WarpGrid {
    /// The flat grid of the planar projection along `axis`: the unit square at depth 0.5,
    /// `cols × rows` cells (clamped to 1..=6).
    pub fn planar(axis: u8, cols: u8, rows: u8) -> Self {
        let max = (MAX_WARP_SIDE - 1) as u8;
        let (cols, rows) = (cols.clamp(1, max), rows.clamp(1, max));
        let mut points = [[0.0; 3]; MAX_WARP_POINTS];
        let (mut us, mut vs) = ([0.0; MAX_WARP_SIDE], [0.0; MAX_WARP_SIDE]);
        for c in 0..=cols as usize {
            us[c] = c as f32 / cols as f32;
        }
        for r in 0..=rows as usize {
            vs[r] = r as f32 / rows as f32;
        }
        for r in 0..=rows as usize {
            for c in 0..=cols as usize {
                points[r * (cols as usize + 1) + c] = from_plane_coords([us[c], vs[r]], axis, 0.5);
            }
        }
        Self { cols, rows, points, us, vs }
    }

    pub fn cols(&self) -> u8 {
        self.cols
    }

    pub fn rows(&self) -> u8 {
        self.rows
    }

    /// Points per row.
    pub fn width(&self) -> usize {
        self.cols as usize + 1
    }

    /// The used points, row by row.
    pub fn points(&self) -> &[[f32; 3]] {
        &self.points[..self.width() * (self.rows as usize + 1)]
    }

    /// Index of the point at row `r`, column `c`.
    pub fn index(&self, r: usize, c: usize) -> usize {
        r * self.width() + c
    }

    /// Image coordinates (0..1 each) of point `i`.
    pub fn grid_coords(&self, i: usize) -> [f32; 2] {
        let (r, c) = (i / self.width(), i % self.width());
        [self.us[c], self.vs[r]]
    }

    /// Move point `i` (kept within ±[`MAX_WARP_COORD`]); refused (false) for an index outside
    /// the grid or a non-finite position.
    pub fn set_point(&mut self, i: usize, p: [f32; 3]) -> bool {
        if i >= self.points().len() || !p.iter().all(|v| v.is_finite()) {
            return false;
        }
        self.points[i] = p.map(|v| v.clamp(-MAX_WARP_COORD, MAX_WARP_COORD));
        true
    }

    /// Whether the grid is still the flat, evenly divided grid of `axis` (rows and columns can
    /// then still be changed, as in Painter).
    pub fn is_flat(&self, axis: u8) -> bool {
        *self == Self::planar(axis, self.cols, self.rows)
            || {
                let flat = Self::planar(axis, self.cols, self.rows);
                let near = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-6);
                self.points().iter().zip(flat.points()).all(|(a, b)| near(a, b)) && near(&self.us, &flat.us) && near(&self.vs, &flat.vs)
            }
    }

    /// Insert a column of points through the middle of cell column `cell` (Split vertically).
    /// Returns false when the grid is already at its widest or `cell` is out of range.
    pub fn split_column(&mut self, cell: usize) -> bool {
        if self.width() >= MAX_WARP_SIDE || cell >= self.cols as usize {
            return false;
        }
        let (old_w, rows) = (self.width(), self.rows as usize + 1);
        let old = self.points;
        let new_w = old_w + 1;
        for r in 0..rows {
            for c in 0..new_w {
                self.points[r * new_w + c] = match c.cmp(&(cell + 1)) {
                    std::cmp::Ordering::Less => old[r * old_w + c],
                    std::cmp::Ordering::Equal => mid(old[r * old_w + cell], old[r * old_w + cell + 1]),
                    std::cmp::Ordering::Greater => old[r * old_w + c - 1],
                };
            }
        }
        let us = self.us;
        for c in (cell + 1..new_w).rev() {
            self.us[c] = if c == cell + 1 { (us[cell] + us[cell + 1]) * 0.5 } else { us[c - 1] };
        }
        self.cols += 1;
        true
    }

    /// Insert a row of points through the middle of cell row `cell` (Split horizontally).
    pub fn split_row(&mut self, cell: usize) -> bool {
        if self.rows as usize + 1 >= MAX_WARP_SIDE || cell >= self.rows as usize {
            return false;
        }
        let (w, rows) = (self.width(), self.rows as usize + 1);
        let old = self.points;
        for r in 0..=rows {
            for c in 0..w {
                self.points[r * w + c] = match r.cmp(&(cell + 1)) {
                    std::cmp::Ordering::Less => old[r * w + c],
                    std::cmp::Ordering::Equal => mid(old[cell * w + c], old[(cell + 1) * w + c]),
                    std::cmp::Ordering::Greater => old[(r - 1) * w + c],
                };
            }
        }
        let vs = self.vs;
        for r in (cell + 1..=rows).rev() {
            self.vs[r] = if r == cell + 1 { (vs[cell] + vs[cell + 1]) * 0.5 } else { vs[r - 1] };
        }
        self.rows += 1;
        true
    }

    /// Grid coordinates of the normalized position `pos` seen down `axis`, and whether a grid
    /// cell covers it. Outside the grid the nearest cell's mapping is extended (continuous at
    /// the border); callers treat those texels as uncovered.
    pub fn coords(&self, axis: u8, pos: [f32; 3]) -> ([f32; 2], bool) {
        let p = plane_coords(pos, axis);
        let w = self.width();
        let q = |r: usize, c: usize| plane_coords(self.points[r * w + c], axis);
        let g = |r: usize, c: usize| [self.us[c], self.vs[r]];
        let mut best: Option<(f32, [f32; 2])> = None;
        for r in 0..self.rows as usize {
            for c in 0..self.cols as usize {
                for tri in [[(r, c), (r, c + 1), (r + 1, c + 1)], [(r, c), (r + 1, c + 1), (r + 1, c)]] {
                    let [a, b, d] = tri.map(|(r, c)| q(r, c));
                    let Some(l) = barycentric(p, a, b, d) else { continue };
                    let [ga, gb, gd] = tri.map(|(r, c)| g(r, c));
                    let uv = [l[0] * ga[0] + l[1] * gb[0] + l[2] * gd[0], l[0] * ga[1] + l[1] * gb[1] + l[2] * gd[1]];
                    let outside = -l[0].min(l[1]).min(l[2]);
                    if outside <= 1e-5 {
                        return (uv, true);
                    }
                    if best.is_none_or(|(o, _)| outside < o) {
                        best = Some((outside, uv));
                    }
                }
            }
        }
        (best.map_or([-1.0, -1.0], |(_, uv)| uv), false)
    }
}

fn mid(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5]
}

/// Barycentric weights of `p` in triangle `(a, b, c)`; `None` for a degenerate triangle.
fn barycentric(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> Option<[f32; 3]> {
    let (v0, v1, v2) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
    let den = v0[0] * v1[1] - v1[0] * v0[1];
    if den.abs() <= 1e-12 {
        return None;
    }
    let l1 = (v2[0] * v1[1] - v1[0] * v2[1]) / den;
    let l2 = (v0[0] * v2[1] - v2[0] * v0[1]) / den;
    Some([1.0 - l1 - l2, l1, l2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    #[test]
    fn the_flat_grid_maps_like_the_planar_projection_on_every_axis() {
        for axis in 0..3u8 {
            let g = WarpGrid::planar(axis, 3, 2);
            assert!(g.is_flat(axis));
            for pos in [[0.1, 0.2, 0.3], [0.9, 0.5, 0.05], [0.5, 0.5, 0.5]] {
                let (uv, inside) = g.coords(axis, pos);
                assert!(inside && close(uv, plane_coords(pos, axis)), "axis {axis} {pos:?}: {uv:?}");
            }
            let (_, inside) = g.coords(axis, from_plane_coords([1.3, 0.5], axis, 0.2));
            assert!(!inside, "outside the square nothing is projected");
        }
    }

    /// QA 2026-11: a point such as 3e38 loaded (only finiteness was checked) and overflowed the
    /// cell maths to NaN texture coordinates.
    #[test]
    fn grid_points_stay_near_the_model() {
        let file = |x: f32| format!(r#"{{"cols":1,"rows":1,"points":[[{x},0,0],[1,0,0],[0,1,0],[1,1,0]],"us":[0,1],"vs":[0,1]}}"#);
        assert!(serde_json::from_str::<WarpGrid>(&file(0.0)).is_ok());
        assert!(serde_json::from_str::<WarpGrid>(&file(3e38)).is_err());
        assert!(serde_json::from_str::<WarpGrid>(&file(-5.0)).is_err());
        let mut g = WarpGrid::planar(2, 2, 2);
        assert!(g.set_point(0, [1e30, -1e30, 0.5]));
        assert_eq!(g.points()[0], [MAX_WARP_COORD, -MAX_WARP_COORD, 0.5], "a drag far away stops at the limit");
        let (uv, _) = g.coords(2, [0.5, 0.5, 0.5]);
        assert!(uv.iter().all(|v| v.is_finite()), "{uv:?}");
    }

    #[test]
    fn moving_a_point_bends_the_mapping_inside_its_cells_only() {
        let mut g = WarpGrid::planar(2, 2, 2);
        let centre = g.index(1, 1);
        // Pull the centre point right: what sat at x = 0.6 now reads grid u < 0.6.
        assert!(g.set_point(centre, [0.7, 0.5, 0.5]));
        assert!(!g.is_flat(2));
        let (uv, inside) = g.coords(2, [0.6, 0.5, 0.2]);
        assert!(inside && uv[0] < 0.59 && (uv[1] - 0.5).abs() < 1e-5, "{uv:?}");
        // The centre's new position reads the centre of the image.
        assert!(close(g.coords(2, [0.7, 0.5, 0.9]).0, [0.5, 0.5]));
        // Corners are untouched.
        assert!(close(g.coords(2, [0.02, 0.02, 0.5]).0, [0.02, 0.98]));
        assert!(!g.set_point(99, [0.0; 3]) && !g.set_point(0, [f32::NAN, 0.0, 0.0]));
    }

    #[test]
    fn splits_insert_midpoints_and_keep_the_mapping() {
        let mut g = WarpGrid::planar(1, 2, 2);
        let before: Vec<_> = [[0.3, 0.5, 0.7], [0.8, 0.1, 0.2]].iter().map(|p| g.coords(1, *p).0).collect();
        assert!(g.split_column(0) && g.split_row(1));
        assert_eq!((g.cols(), g.rows(), g.points().len()), (3, 3, 16));
        let after: Vec<_> = [[0.3, 0.5, 0.7], [0.8, 0.1, 0.2]].iter().map(|p| g.coords(1, *p).0).collect();
        assert!(before.iter().zip(&after).all(|(a, b)| close(*a, *b)), "{before:?} {after:?}");
        // The new column sits halfway through the first cell, at image u = 0.25.
        assert!(g.points()[1][0] > 0.24 && g.points()[1][0] < 0.26 && (g.grid_coords(1)[0] - 0.25).abs() < 1e-6);
        assert!(!g.is_flat(1), "a split grid is no longer evenly divided");
        let mut wide = WarpGrid::planar(1, 6, 6);
        assert!(!wide.split_column(0) && !wide.split_row(0), "at most 6 cells per side");
    }

    #[test]
    fn the_file_form_round_trips_and_bad_grids_are_refused() {
        let mut g = WarpGrid::planar(0, 2, 3);
        g.set_point(4, [0.4, 0.45, 0.6]);
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<WarpGrid>(&json).unwrap(), g);
        assert!(serde_json::from_str::<WarpGrid>(r#"{"cols":2,"rows":1,"points":[[0,0,0]]}"#).is_err(), "wrong point count");
        assert!(serde_json::from_str::<WarpGrid>(r#"{"cols":9,"rows":1,"points":[]}"#).is_err(), "too many cells");
    }
}
