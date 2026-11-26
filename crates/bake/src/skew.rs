//! Skew correction (Painter's Common settings > Skew correction).
//!
//! With averaged normals the projection rays leave no gap at the low poly's hard edges, but the
//! rays across a flat face lean towards the face's corners, so details on the high poly bake
//! sheared ("skewed"): round bolts turn into ellipses and straight panel lines bend. Skew
//! correction straightens the rays again: they follow the low poly's own normal (the mesh's
//! shading normal, or the flat triangle normal) instead of the averaged one. Edge protection keeps
//! the averaged rays next to the hard edges, where straight rays would open gaps, and fades to
//! straight rays over `edge_distance`.

use std::collections::HashMap;

use glam::{IVec3, Vec3};
use subcraft_mesh::Mesh;

/// The normal skew-corrected rays follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SkewBase {
    /// The low poly's interpolated shading normal.
    #[default]
    Mesh,
    /// The flat normal of each low-poly triangle.
    Triangle,
}

impl SkewBase {
    pub const ALL: [SkewBase; 2] = [SkewBase::Mesh, SkewBase::Triangle];

    pub fn label(self) -> &'static str {
        match self {
            SkewBase::Mesh => "Mesh",
            SkewBase::Triangle => "Per triangle",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SkewBase::Mesh => "mesh",
            SkewBase::Triangle => "triangle",
        }
    }
}

/// Skew correction settings. Only used with averaged normals and the distance-based cage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkewCorrection {
    pub enabled: bool,
    pub base: SkewBase,
    /// Keep the averaged rays near the low poly's hard edges.
    pub edge_protection: bool,
    /// Distance from a hard edge over which the rays turn straight, as a fraction of the diagonal.
    pub edge_distance: f32,
    /// 0: the correction grows linearly over the distance; 1: it switches on at half the distance.
    pub edge_contrast: f32,
}

impl Default for SkewCorrection {
    fn default() -> Self {
        Self { enabled: false, base: SkewBase::Mesh, edge_protection: true, edge_distance: 0.03, edge_contrast: 0.5 }
    }
}

impl SkewCorrection {
    /// Correction amount (0 = averaged rays, 1 = straight rays) at `distance` from the nearest
    /// hard edge, with `reach` the edge distance in world units.
    pub fn ramp(&self, distance: f32, reach: f32) -> f32 {
        if !self.edge_protection || reach <= 0.0 {
            return 1.0;
        }
        let t = (distance / reach).clamp(0.0, 1.0);
        let c = self.edge_contrast.clamp(0.0, 1.0);
        let (lo, hi) = (0.5 * c, 1.0 - 0.5 * c);
        if hi - lo < 1e-4 {
            return if t >= 0.5 { 1.0 } else { 0.0 };
        }
        let x = ((t - lo) / (hi - lo)).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    }
}

/// Where a low poly's rays are corrected: the hard edges in a uniform grid for distance queries.
pub(crate) struct SkewField {
    settings: SkewCorrection,
    reach: f32,
    cell: f32,
    origin: Vec3,
    segments: Vec<[Vec3; 2]>,
    grid: HashMap<IVec3, Vec<u32>>,
}

/// Grid cells allowed before the cells grow (long edges on a small edge distance).
const MAX_CELLS: usize = 1 << 22;

impl SkewField {
    pub(crate) fn new(low: &Mesh, settings: SkewCorrection, diag: f32) -> Self {
        let reach = settings.edge_distance.max(0.0) * diag;
        let segments = if settings.edge_protection && reach > 0.0 { hard_edges(low) } else { Vec::new() };
        let mut field = Self { settings, reach, cell: reach.max(diag * 1e-4), origin: low.bounds.min, segments, grid: HashMap::new() };
        if field.segments.is_empty() {
            return field;
        }
        // Each segment goes into every cell its box (grown by the reach) touches, so one cell
        // lookup finds every edge within the reach of a point.
        loop {
            let cells: usize = field.segments.iter().map(|s| field.span(s).iter().map(|r| (r.1 - r.0 + 1) as usize).product::<usize>()).sum();
            if cells <= MAX_CELLS {
                break;
            }
            field.cell *= 2.0;
        }
        for (i, s) in field.segments.iter().enumerate() {
            let [x, y, z] = field.span(s);
            for cx in x.0..=x.1 {
                for cy in y.0..=y.1 {
                    for cz in z.0..=z.1 {
                        field.grid.entry(IVec3::new(cx, cy, cz)).or_default().push(i as u32);
                    }
                }
            }
        }
        field
    }

    fn cell_of(&self, p: Vec3) -> IVec3 {
        ((p - self.origin) / self.cell).floor().as_ivec3()
    }

    fn span(&self, s: &[Vec3; 2]) -> [(i32, i32); 3] {
        let lo = self.cell_of(s[0].min(s[1]) - Vec3::splat(self.reach));
        let hi = self.cell_of(s[0].max(s[1]) + Vec3::splat(self.reach));
        [(lo.x, hi.x), (lo.y, hi.y), (lo.z, hi.z)]
    }

    /// Distance from `p` to the nearest hard edge, capped at the edge distance.
    pub(crate) fn edge_distance(&self, p: Vec3) -> f32 {
        let Some(list) = self.grid.get(&self.cell_of(p)) else { return self.reach };
        list.iter().map(|&i| segment_distance(p, self.segments[i as usize])).fold(self.reach, f32::min)
    }

    /// Correction amount at low-poly point `p`.
    pub(crate) fn amount(&self, p: Vec3) -> f32 {
        if self.segments.is_empty() {
            return 1.0;
        }
        self.settings.ramp(self.edge_distance(p), self.reach)
    }

    /// The ray direction (outwards) for a texel: the averaged normal turned towards the base.
    /// `painted` (0..1, from the skew map) scales the correction.
    pub(crate) fn direction(&self, p: Vec3, averaged: Vec3, shading: Vec3, triangle: Vec3, painted: f32) -> Vec3 {
        let base = match self.settings.base {
            SkewBase::Mesh => shading,
            SkewBase::Triangle => if triangle.dot(averaged) < 0.0 { -triangle } else { triangle },
        };
        let s = self.amount(p) * painted.clamp(0.0, 1.0);
        averaged.lerp(base, s).normalize_or(averaged)
    }
}

fn segment_distance(p: Vec3, [a, b]: [Vec3; 2]) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 0.0 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// The low poly's hard edges: edges (welded by position) whose two sides have different shading
/// normals at either end. Open boundaries do not count.
pub fn hard_edges(mesh: &Mesh) -> Vec<[Vec3; 2]> {
    let key = |p: Vec3| (p + Vec3::ZERO).to_array().map(f32::to_bits);
    // Per welded edge: its end points and each side's normals at those ends.
    let mut sides: HashMap<([u32; 3], [u32; 3]), (Vec3, Vec3, Vec<(Vec3, Vec3)>)> = HashMap::new();
    for t in 0..mesh.triangle_count() {
        let idx = mesh.tri(t).map(|i| i as usize);
        for k in 0..3 {
            let (i, j) = (idx[k], idx[(k + 1) % 3]);
            let (pi, pj) = (mesh.positions[i], mesh.positions[j]);
            let (ki, kj) = (key(pi), key(pj));
            if ki == kj {
                continue;
            }
            // Store the ends in a fixed order so both sides line up.
            let (entry, normals) = if ki < kj { ((ki, kj), (mesh.normals[i], mesh.normals[j])) } else { ((kj, ki), (mesh.normals[j], mesh.normals[i])) };
            let (a, b) = if ki < kj { (pi, pj) } else { (pj, pi) };
            sides.entry(entry).or_insert_with(|| (a, b, Vec::new())).2.push(normals);
        }
    }
    const SAME: f32 = 0.9998; // about 1 degree
    let mut out: Vec<[Vec3; 2]> = sides
        .into_values()
        .filter(|(_, _, n)| n.len() >= 2 && n.iter().skip(1).any(|m| m.0.normalize_or_zero().dot(n[0].0.normalize_or_zero()) < SAME || m.1.normalize_or_zero().dot(n[0].1.normalize_or_zero()) < SAME))
        .map(|(a, b, _)| [a, b])
        .collect();
    out.sort_by(|x, y| x[0].to_array().partial_cmp(&y[0].to_array()).unwrap_or(std::cmp::Ordering::Equal));
    out
}
