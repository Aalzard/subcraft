//! Automatic UV unwrapping for meshes without UVs or with unusable ones.
//!
//! Hand-written pipeline, no external crates:
//! 1. **Charts** (seams): triangles are welded by position and grown into charts per material
//!    slot. A triangle joins a chart when its normal is within the angle threshold of the
//!    chart's average normal and of its neighbour's normal. Non-manifold edges and edges whose
//!    triangles disagree on winding are always seams.
//! 2. **Flattening**: each chart is flattened with least-squares conformal maps (LSCM, two pinned
//!    vertices, conjugate gradients). A chart whose layout folds over itself or is too stretched
//!    is split in two (farthest-seed region growing) and flattened again until it is valid; a
//!    single triangle is always laid out isometrically.
//! 3. **Packing**: every chart is rotated to its smallest bounding rectangle, rectangles (padded
//!    by the island margin) are packed into the unit square by the largest uniform scale that
//!    fits. Material slots are packed separately because each is its own texture set.
//!
//! The result is a new [`Mesh`] with the same triangles, materials, objects and normals; only
//! UVs (and therefore vertex splits and tangents) change. Output is deterministic.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use glam::{Vec2, Vec3, Vec4};

use crate::mesh::{Aabb, Mesh};
use crate::tangent::compute_corner_tangents;

/// Texel density bookkeeping, packing effort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackQuality {
    /// Shelf packing, charts keep the orientation the flattening gave them.
    Fast,
    /// Skyline packing of minimum-area rotated charts.
    Balanced,
    /// Skyline packing, also trying quarter turns and several orderings, finer scale search.
    Best,
}

impl PackQuality {
    pub const ALL: [PackQuality; 3] = [PackQuality::Fast, PackQuality::Balanced, PackQuality::Best];

    pub fn label(self) -> &'static str {
        match self {
            PackQuality::Fast => "Fast",
            PackQuality::Balanced => "Balanced",
            PackQuality::Best => "Best",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnwrapSettings {
    /// Largest angle (degrees) between a triangle and its chart's average normal / its
    /// neighbour. Smaller values give more, flatter charts.
    pub angle_threshold_deg: f32,
    /// Gap between islands in texels of a `resolution` texture (half of it to the border).
    pub margin_texels: f32,
    pub resolution: u32,
    pub quality: PackQuality,
    /// Give every chart the same UV area per surface area. Off keeps the conformal scale of each
    /// chart (lengths along the pinned axis match the surface).
    pub balance_density: bool,
    /// Largest accepted singular-value ratio of a chart's worst triangle before it is split.
    pub max_stretch: f32,
}

impl Default for UnwrapSettings {
    fn default() -> Self {
        Self { angle_threshold_deg: 66.0, margin_texels: 4.0, resolution: 1024, quality: PackQuality::Balanced, balance_density: true, max_stretch: 4.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnwrapStats {
    pub charts: usize,
    pub triangles: usize,
    /// Charts that had to be split further because they did not flatten cleanly.
    pub split_charts: usize,
    /// Area-weighted mean of the singular-value ratio per triangle (1 = no angle distortion).
    pub mean_stretch: f32,
    pub max_stretch: f32,
    /// Share of the unit square covered by triangles (mean over material slots).
    pub coverage: f32,
    /// Margin actually used, in UV units (smaller than asked when there are very many charts).
    pub margin_uv: f32,
}

impl UnwrapStats {
    pub fn summary(&self) -> String {
        format!(
            "{} chart(s), {} triangle(s), coverage {:.0}%, stretch mean {:.2} / max {:.2}",
            self.charts,
            self.triangles,
            self.coverage * 100.0,
            self.mean_stretch,
            self.max_stretch
        )
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UnwrapError {
    #[error("the mesh has no triangles")]
    Empty,
    #[error("the mesh has non-finite vertex positions")]
    NonFinite,
}

const NONE: u32 = u32::MAX;
/// Triangle areas below this (relative to the mean) are laid out as degenerate.
const TINY: f32 = 1e-20;

/// Unwrap `mesh` and return a copy with new UVs.
pub fn unwrap(mesh: &Mesh, settings: &UnwrapSettings) -> Result<(Mesh, UnwrapStats), UnwrapError> {
    if mesh.triangle_count() == 0 {
        return Err(UnwrapError::Empty);
    }
    if mesh.positions.iter().any(|p| !p.is_finite()) {
        return Err(UnwrapError::NonFinite);
    }
    let prep = Prep::new(mesh);
    let cos_limit = settings.angle_threshold_deg.clamp(1.0, 179.0).to_radians().cos();
    let mut stats = UnwrapStats { triangles: mesh.triangle_count(), ..Default::default() };
    // New uv per triangle corner and the chart each triangle belongs to (globally numbered).
    let mut corner_uv = vec![Vec2::ZERO; mesh.indices.len()];
    let mut tri_chart = vec![0u32; mesh.triangle_count()];
    let mut next_chart = 0u32;
    let mut coverage_sum = 0.0f32;
    let mut slots = 0usize;
    let mut margin_used = f32::MAX;

    let slot_ranges: Vec<std::ops::Range<usize>> = if mesh.submeshes.is_empty() { vec![0..mesh.triangle_count()] } else { mesh.submeshes.iter().map(|s| s.first_triangle as usize..(s.first_triangle + s.triangle_count) as usize).collect() };
    for range in slot_ranges {
        let charts = grow_charts(&prep, range.clone(), cos_limit);
        let mut flat: Vec<FlatChart> = Vec::new();
        for tris in charts {
            flatten_valid(mesh, &prep, tris, settings, &mut flat, &mut stats.split_charts);
        }
        if flat.is_empty() {
            continue;
        }
        for c in &mut flat {
            if settings.balance_density {
                c.normalize_density();
            }
            c.fit_rotation(settings.quality);
        }
        let margin = settings.margin_texels.max(0.0) / settings.resolution.max(1) as f32;
        let packed = pack(&mut flat, margin, settings.quality);
        margin_used = margin_used.min(packed.margin);
        coverage_sum += packed.coverage;
        slots += 1;
        for c in &flat {
            for (k, &t) in c.tris.iter().enumerate() {
                tri_chart[t as usize] = next_chart;
                for j in 0..3 {
                    corner_uv[3 * t as usize + j] = c.uv[k][j];
                }
            }
            next_chart += 1;
        }
    }
    stats.charts = next_chart as usize;
    stats.coverage = if slots > 0 { coverage_sum / slots as f32 } else { 0.0 };
    stats.margin_uv = if margin_used == f32::MAX { 0.0 } else { margin_used };

    let out = rebuild(mesh, &corner_uv, &tri_chart);
    let q = measure(&out, 0..out.triangle_count());
    stats.mean_stretch = q.mean_stretch;
    stats.max_stretch = q.max_stretch;
    Ok((out, stats))
}

// ----- topology -----------------------------------------------------------------------------

struct Prep {
    /// Welded vertex id per triangle corner.
    corner_w: Vec<u32>,
    normal: Vec<Vec3>,
    area: Vec<f32>,
    /// Edge-adjacent triangle opposite each corner's edge `k -> k+1`, or `NONE`.
    neighbors: Vec<[u32; 3]>,
    positions_w: Vec<Vec3>,
}

impl Prep {
    fn new(mesh: &Mesh) -> Self {
        let key = |p: Vec3| [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()];
        let mut weld: HashMap<[u32; 3], u32> = HashMap::new();
        let mut positions_w = Vec::new();
        let vertex_w: Vec<u32> = mesh
            .positions
            .iter()
            .map(|&p| {
                *weld.entry(key(p)).or_insert_with(|| {
                    positions_w.push(p);
                    (positions_w.len() - 1) as u32
                })
            })
            .collect();
        let tri_count = mesh.triangle_count();
        let corner_w: Vec<u32> = mesh.indices.iter().map(|&i| vertex_w[i as usize]).collect();
        let mut normal = Vec::with_capacity(tri_count);
        let mut area = Vec::with_capacity(tri_count);
        for t in 0..tri_count {
            let [a, b, c] = mesh.tri_positions(t);
            let n = (b - a).cross(c - a);
            area.push(0.5 * n.length());
            normal.push(n.normalize_or_zero());
        }
        // Directed edges: an edge is shared by exactly two triangles of one material with
        // opposite directions; anything else is a seam.
        let mut edges: HashMap<(u32, u32), Vec<(u32, u8)>> = HashMap::new();
        for t in 0..tri_count {
            for k in 0..3 {
                let (a, b) = (corner_w[3 * t + k], corner_w[3 * t + (k + 1) % 3]);
                if a == b {
                    continue;
                }
                edges.entry((a.min(b), a.max(b))).or_default().push((t as u32, k as u8));
            }
        }
        let mut neighbors = vec![[NONE; 3]; tri_count];
        for list in edges.values() {
            if let [(t0, k0), (t1, k1)] = list[..] {
                let dir = |t: u32, k: u8| (corner_w[3 * t as usize + k as usize], corner_w[3 * t as usize + (k as usize + 1) % 3]);
                let (a0, _) = dir(t0, k0);
                let (a1, _) = dir(t1, k1);
                let same_material = mesh.tri_material.get(t0 as usize) == mesh.tri_material.get(t1 as usize);
                if a0 != a1 && t0 != t1 && same_material {
                    neighbors[t0 as usize][k0 as usize] = t1;
                    neighbors[t1 as usize][k1 as usize] = t0;
                }
            }
        }
        Prep { corner_w, normal, area, neighbors, positions_w }
    }

    fn degenerate(&self, t: usize) -> bool {
        self.area[t] <= TINY || self.normal[t] == Vec3::ZERO
    }

    fn corners(&self, t: u32) -> [u32; 3] {
        let t = t as usize;
        [self.corner_w[3 * t], self.corner_w[3 * t + 1], self.corner_w[3 * t + 2]]
    }
}

/// Region-grow charts over the triangles of one material slot.
fn grow_charts(prep: &Prep, range: std::ops::Range<usize>, cos_limit: f32) -> Vec<Vec<u32>> {
    let base = range.start;
    let mut assigned = vec![false; range.len()];
    let mut charts = Vec::new();
    for seed in range.clone() {
        if assigned[seed - base] {
            continue;
        }
        let mut tris = Vec::new();
        let mut chart_n = prep.normal[seed] * prep.area[seed];
        let mut heap = BinaryHeap::new();
        heap.push((Reverse(0u32), Reverse(seed as u32), seed as u32));
        while let Some((_, Reverse(t), parent)) = heap.pop() {
            let ti = t as usize;
            if assigned[ti - base] {
                continue;
            }
            let n = prep.normal[ti];
            if t as usize != seed && !prep.degenerate(ti) {
                let avg = chart_n.normalize_or_zero();
                let pn = prep.normal[parent as usize];
                if (avg != Vec3::ZERO && n.dot(avg) < cos_limit) || (pn != Vec3::ZERO && n.dot(pn) < cos_limit) {
                    continue;
                }
            }
            assigned[ti - base] = true;
            tris.push(t);
            chart_n += n * prep.area[ti];
            let avg = chart_n.normalize_or_zero();
            for &nb in &prep.neighbors[ti] {
                if nb != NONE && !assigned[nb as usize - base] {
                    let cost = (1.0 - prep.normal[nb as usize].dot(avg)).max(0.0);
                    heap.push((Reverse(cost.to_bits()), Reverse(nb), t));
                }
            }
        }
        tris.sort_unstable();
        charts.push(tris);
    }
    charts
}

// ----- flattening ---------------------------------------------------------------------------

/// A flattened chart: UV corners per triangle in chart-local units.
struct FlatChart {
    tris: Vec<u32>,
    uv: Vec<[Vec2; 3]>,
    area3d: f32,
}

impl FlatChart {
    fn area_uv(&self) -> f32 {
        self.uv.iter().map(|t| 0.5 * (t[1] - t[0]).perp_dot(t[2] - t[0]).abs()).sum()
    }

    fn normalize_density(&mut self) {
        let a = self.area_uv();
        if a > 1e-30 && self.area3d > 1e-30 {
            let s = (self.area3d / a).sqrt();
            self.map(|p| p * s);
        }
    }

    fn map(&mut self, f: impl Fn(Vec2) -> Vec2) {
        for t in &mut self.uv {
            for p in t.iter_mut() {
                *p = f(*p);
            }
        }
    }

    fn points(&self) -> Vec<Vec2> {
        self.uv.iter().flatten().copied().collect()
    }

    /// Rotate to the smallest bounding rectangle (Balanced / Best) and move the minimum to 0.
    fn fit_rotation(&mut self, quality: PackQuality) {
        if quality != PackQuality::Fast {
            let hull = convex_hull(self.points());
            let mut best = (f32::MAX, Vec2::X);
            for i in 0..hull.len() {
                let e = hull[(i + 1) % hull.len()] - hull[i];
                if e.length_squared() < 1e-24 {
                    continue;
                }
                let d = e.normalize();
                let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                for &p in &hull {
                    let q = Vec2::new(p.dot(d), p.dot(d.perp()));
                    lo = lo.min(q);
                    hi = hi.max(q);
                }
                let area = (hi.x - lo.x) * (hi.y - lo.y);
                if area < best.0 - 1e-9 * best.0.min(1e30) {
                    best = (area, d);
                }
            }
            let d = best.1;
            // Rotation, not reflection: keeps the triangle orientation.
            self.map(|p| Vec2::new(p.dot(d), p.dot(d.perp())));
            // Long side horizontal.
            let (lo, hi) = self.bounds();
            if hi.y - lo.y > hi.x - lo.x {
                self.map(|p| Vec2::new(p.y, -p.x));
            }
        }
        let (lo, _) = self.bounds();
        self.map(|p| p - lo);
    }

    fn bounds(&self) -> (Vec2, Vec2) {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in self.uv.iter().flatten() {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        (lo, hi)
    }

    fn size(&self) -> Vec2 {
        let (lo, hi) = self.bounds();
        (hi - lo).max(Vec2::ZERO)
    }
}

fn convex_hull(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut hull: Vec<Vec2> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> = if pass == 0 { Box::new(pts.iter()) } else { Box::new(pts.iter().rev()) };
        for &p in iter {
            while hull.len() >= start + 2 && (hull[hull.len() - 1] - hull[hull.len() - 2]).perp_dot(p - hull[hull.len() - 2]) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Flatten `tris`; on failure split and recurse. Appends the valid charts to `out`.
fn flatten_valid(mesh: &Mesh, prep: &Prep, tris: Vec<u32>, settings: &UnwrapSettings, out: &mut Vec<FlatChart>, splits: &mut usize) {
    let limit = sliver_limit(mesh);
    let mut stack = vec![tris];
    while let Some(tris) = stack.pop() {
        if tris.is_empty() {
            continue;
        }
        let flat = flatten_chart(mesh, prep, &tris);
        let ok = flat.as_ref().is_some_and(|f| chart_valid(mesh, f, settings, limit));
        if ok || tris.len() == 1 {
            out.push(flat.unwrap_or_else(|| isometric_single(mesh, tris[0])));
            continue;
        }
        *splits += 1;
        let (a, b) = split_chart(mesh, prep, &tris);
        // Push in reverse so the first half is processed (and numbered) first.
        stack.push(b);
        stack.push(a);
    }
}

fn isometric_single(mesh: &Mesh, t: u32) -> FlatChart {
    let [a, b, c] = mesh.tri_positions(t as usize);
    let (e1, e2) = (b - a, c - a);
    let l1 = e1.length();
    let area = 0.5 * e1.cross(e2).length();
    let uv = if l1 > 1e-18 && area > TINY {
        [Vec2::ZERO, Vec2::new(l1, 0.0), Vec2::new(e1.dot(e2) / l1, UV_SIGN * e1.cross(e2).length() / l1)]
    } else {
        let e = 1e-4 * (mesh.bounds.diagonal().max(1e-6));
        [Vec2::ZERO, Vec2::new(e, 0.0), Vec2::new(0.0, UV_SIGN * e)]
    };
    FlatChart { tris: vec![t], uv: vec![uv], area3d: area }
}

/// Split a chart in two by growing from its two most distant triangles.
fn split_chart(mesh: &Mesh, prep: &Prep, tris: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let centroid = |t: u32| {
        let [a, b, c] = mesh.tri_positions(t as usize);
        (a + b + c) / 3.0
    };
    let mean = tris.iter().map(|&t| centroid(t)).sum::<Vec3>() / tris.len() as f32;
    let far = |from: Vec3| {
        tris.iter().copied().fold((tris[0], -1.0f32), |best, t| {
            let d = centroid(t).distance_squared(from);
            if d > best.1 { (t, d) } else { best }
        })
    };
    let a = far(mean).0;
    let b = far(centroid(a)).0;
    let index: HashMap<u32, usize> = tris.iter().enumerate().map(|(i, &t)| (t, i)).collect();
    let mut label = vec![u8::MAX; tris.len()];
    let mut queues = [std::collections::VecDeque::new(), std::collections::VecDeque::new()];
    for (k, seed) in [a, if b == a { tris[tris.len() - 1] } else { b }].into_iter().enumerate() {
        if label[index[&seed]] == u8::MAX {
            label[index[&seed]] = k as u8;
            queues[k].push_back(seed);
        }
    }
    while !queues[0].is_empty() || !queues[1].is_empty() {
        for k in 0..2 {
            if let Some(t) = queues[k].pop_front() {
                for &nb in &prep.neighbors[t as usize] {
                    if let Some(&i) = index.get(&nb)
                        && label[i] == u8::MAX
                    {
                        label[i] = k as u8;
                        queues[k].push_back(nb);
                    }
                }
            }
        }
    }
    let mut parts = (Vec::new(), Vec::new());
    for (i, &t) in tris.iter().enumerate() {
        // Unreached triangles (the chart was disconnected) go with the first part.
        if label[i] == 1 { parts.1.push(t) } else { parts.0.push(t) }
    }
    if parts.1.is_empty() {
        // Degenerate seeds: halve by index so the recursion always shrinks.
        let half = tris.len() / 2;
        return (tris[..half].to_vec(), tris[half..].to_vec());
    }
    parts
}

struct Quality {
    flipped: usize,
    max_stretch: f32,
    mean_stretch: f32,
}

/// Singular value ratio (Γ/γ) of the map from UV to the surface for one triangle, and the
/// signed UV area. `None` for degenerate triangles.
fn triangle_stretch(p: [Vec3; 3], uv: [Vec2; 3]) -> Option<(f32, f32)> {
    let a = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]);
    if a.abs() < 1e-30 {
        return None;
    }
    let ss = (p[0] * (uv[1].y - uv[2].y) + p[1] * (uv[2].y - uv[0].y) + p[2] * (uv[0].y - uv[1].y)) / a;
    let st = (p[0] * (uv[2].x - uv[1].x) + p[1] * (uv[0].x - uv[2].x) + p[2] * (uv[1].x - uv[0].x)) / a;
    let (aa, bb, cc) = (ss.dot(ss), ss.dot(st), st.dot(st));
    let disc = ((aa - cc) * (aa - cc) + 4.0 * bb * bb).sqrt();
    let big = 0.5 * (aa + cc + disc);
    let small = (0.5 * (aa + cc - disc)).max(0.0);
    let ratio = if small <= 1e-30 { f32::INFINITY } else { (big / small).sqrt() };
    Some((ratio, 0.5 * a))
}

/// Triangles thinner than this area are slivers (the poles of a UV sphere): their orientation
/// is numerical noise, so they do not count as flipped or stretched.
fn sliver_limit(mesh: &Mesh) -> f32 {
    let total: f32 = (0..mesh.triangle_count()).map(|t| { let [a, b, c] = mesh.tri_positions(t); 0.5 * (b - a).cross(c - a).length() }).sum();
    total / mesh.triangle_count().max(1) as f32 * 1e-4
}

fn measure(mesh: &Mesh, range: std::ops::Range<usize>) -> Quality {
    let limit = sliver_limit(mesh);
    let (mut sum, mut weight, mut max, mut flipped) = (0.0f64, 0.0f64, 1.0f32, 0usize);
    for t in range {
        let area3d = 0.5 * (mesh.tri_positions(t)[1] - mesh.tri_positions(t)[0]).cross(mesh.tri_positions(t)[2] - mesh.tri_positions(t)[0]).length();
        if area3d <= limit {
            continue;
        }
        if let Some((ratio, signed)) = triangle_stretch(mesh.tri_positions(t), mesh.tri_uvs(t)) {
            if signed * UV_SIGN < 0.0 {
                flipped += 1;
            }
            max = max.max(ratio);
            if ratio.is_finite() {
                sum += ratio as f64 * area3d as f64;
            }
            weight += area3d as f64;
        }
    }
    Quality { flipped, max_stretch: max, mean_stretch: if weight > 0.0 { (sum / weight) as f32 } else { 1.0 } }
}

/// Sign of the UV-space signed area (`perp_dot`) of counter-clockwise (outward) triangles in
/// SubCraft's convention (v grows downward, textures are not mirrored seen from outside). Verified
/// against the box, plane and torus primitives (the UV sphere's own u axis runs mirrored).
const UV_SIGN: f32 = -1.0;

fn chart_valid(mesh: &Mesh, flat: &FlatChart, settings: &UnwrapSettings, limit: f32) -> bool {
    let mut flipped = 0;
    let mut max = 1.0f32;
    for (k, &t) in flat.tris.iter().enumerate() {
        let uv = flat.uv[k];
        if !uv.iter().all(|p| p.is_finite()) {
            return false;
        }
        let [p0, p1, p2] = mesh.tri_positions(t as usize);
        if 0.5 * (p1 - p0).cross(p2 - p0).length() <= limit {
            continue;
        }
        if let Some((ratio, signed)) = triangle_stretch([p0, p1, p2], uv) {
            if signed * UV_SIGN < 0.0 {
                flipped += 1;
            }
            max = max.max(ratio);
        }
    }
    if flipped > 0 || max > settings.max_stretch {
        return false;
    }
    flat.tris.len() < 2 || !any_overlap(&flat.uv)
}

/// Least-squares conformal flattening of one chart. `None` when the solve fails.
fn flatten_chart(mesh: &Mesh, prep: &Prep, tris: &[u32]) -> Option<FlatChart> {
    if tris.len() == 1 {
        return Some(isometric_single(mesh, tris[0]));
    }
    // Local vertices in first-appearance order.
    let mut local: HashMap<u32, usize> = HashMap::new();
    let mut verts: Vec<u32> = Vec::new();
    let mut tri_v: Vec<[usize; 3]> = Vec::with_capacity(tris.len());
    for &t in tris {
        let c = prep.corners(t).map(|w| {
            *local.entry(w).or_insert_with(|| {
                verts.push(w);
                verts.len() - 1
            })
        });
        tri_v.push(c);
    }
    let n = verts.len();
    let pos: Vec<Vec3> = verts.iter().map(|&w| prep.positions_w[w as usize]).collect();
    let area3d: f32 = tris.iter().map(|&t| prep.area[t as usize]).sum();

    // Planar initial layout in the plane of the average normal.
    let normal = tris.iter().map(|&t| prep.normal[t as usize] * prep.area[t as usize]).sum::<Vec3>().normalize_or(Vec3::Y);
    let tangent_hint = {
        let c = pos.iter().copied().sum::<Vec3>() / n as f32;
        let mut cov = [[0.0f32; 3]; 3];
        for p in &pos {
            let d = *p - c - normal * (*p - c).dot(normal);
            let a = d.to_array();
            for i in 0..3 {
                for j in 0..3 {
                    cov[i][j] += a[i] * a[j];
                }
            }
        }
        // Power iteration for the dominant in-plane direction.
        let mut v = normal.any_orthonormal_vector();
        for _ in 0..24 {
            let w = Vec3::new(
                cov[0][0] * v.x + cov[0][1] * v.y + cov[0][2] * v.z,
                cov[1][0] * v.x + cov[1][1] * v.y + cov[1][2] * v.z,
                cov[2][0] * v.x + cov[2][1] * v.y + cov[2][2] * v.z,
            );
            let w = w - normal * w.dot(normal);
            if w.length_squared() < 1e-30 {
                break;
            }
            v = w.normalize();
        }
        v
    };
    let tx = tangent_hint;
    let ty = normal.cross(tx);
    let init: Vec<Vec2> = pos.iter().map(|p| Vec2::new(p.dot(tx), p.dot(ty))).collect();

    // Pins: the two most distant vertices (two sweeps from the first one).
    let far = |from: usize| (0..n).fold((from, -1.0f32), |b, i| {
        let d = pos[i].distance_squared(pos[from]);
        if d > b.1 { (i, d) } else { b }
    }).0;
    let pa = far(far(0));
    let pb = far(pa);
    if pa == pb {
        return None;
    }
    let dir = (init[pb] - init[pa]).normalize_or(Vec2::X);
    let pin_a = init[pa];
    let pin_b = pin_a + dir * pos[pa].distance(pos[pb]);

    // Rows: two real equations per triangle, in (u0 v0 u1 v1 u2 v2) order.
    struct Row {
        v: [usize; 3],
        c: [[f64; 6]; 2],
    }
    let mut rows: Vec<Row> = Vec::with_capacity(tris.len());
    for (k, &t) in tris.iter().enumerate() {
        let [p0, p1, p2] = tri_v[k].map(|i| pos[i]);
        let e1 = p1 - p0;
        let l1 = e1.length();
        let nrm = e1.cross(p2 - p0);
        let a2 = nrm.length();
        if prep.degenerate(t as usize) || l1 < 1e-18 || a2 < 1e-18 {
            continue;
        }
        // Local isometric 2D frame of the triangle.
        let (x0, y0) = (0.0f64, 0.0f64);
        let (x1, y1) = (l1 as f64, 0.0f64);
        let (x2, y2) = ((e1.dot(p2 - p0) / l1) as f64, (a2 / l1) as f64);
        let area2 = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0); // 2A, positive
        let w = (0.5 * area2).sqrt() / area2;
        let g = [((y1 - y2) * w, (x2 - x1) * w), ((y2 - y0) * w, (x0 - x2) * w), ((y0 - y1) * w, (x1 - x0) * w)];
        let c0 = [g[0].0, -g[0].1, g[1].0, -g[1].1, g[2].0, -g[2].1];
        let c1 = [g[0].1, g[0].0, g[1].1, g[1].0, g[2].1, g[2].0];
        rows.push(Row { v: tri_v[k], c: [c0, c1] });
    }
    let apply = |x: &[f64], out: &mut [f64]| {
        for (r, o) in rows.iter().zip(out.chunks_exact_mut(2)) {
            for (row, o) in r.c.iter().zip(o.iter_mut()) {
                let mut s = 0.0;
                for j in 0..3 {
                    s += row[2 * j] * x[2 * r.v[j]] + row[2 * j + 1] * x[2 * r.v[j] + 1];
                }
                *o = s;
            }
        }
    };
    let apply_t = |y: &[f64], out: &mut [f64]| {
        out.iter_mut().for_each(|o| *o = 0.0);
        for (r, y) in rows.iter().zip(y.chunks_exact(2)) {
            for (row, y) in r.c.iter().zip(y) {
                for j in 0..3 {
                    out[2 * r.v[j]] += row[2 * j] * y;
                    out[2 * r.v[j] + 1] += row[2 * j + 1] * y;
                }
            }
        }
    };
    let mut free = vec![true; 2 * n];
    for i in [2 * pa, 2 * pa + 1, 2 * pb, 2 * pb + 1] {
        free[i] = false;
    }
    let mut x: Vec<f64> = init.iter().flat_map(|p| [p.x as f64, p.y as f64]).collect();
    x[2 * pa] = pin_a.x as f64;
    x[2 * pa + 1] = pin_a.y as f64;
    x[2 * pb] = pin_b.x as f64;
    x[2 * pb + 1] = pin_b.y as f64;
    // Jacobi preconditioner.
    let mut diag = vec![0.0f64; 2 * n];
    for r in &rows {
        for row in &r.c {
            for j in 0..3 {
                diag[2 * r.v[j]] += row[2 * j] * row[2 * j];
                diag[2 * r.v[j] + 1] += row[2 * j + 1] * row[2 * j + 1];
            }
        }
    }
    for d in &mut diag {
        if *d < 1e-30 {
            *d = 1.0;
        }
    }
    let mut res = vec![0.0f64; 2 * rows.len()];
    let mut g = vec![0.0f64; 2 * n];
    apply(&x, &mut res);
    apply_t(&res, &mut g);
    let mut r: Vec<f64> = g.iter().zip(&free).map(|(g, f)| if *f { -g } else { 0.0 }).collect();
    let mut z: Vec<f64> = r.iter().zip(&diag).map(|(r, d)| r / d).collect();
    let mut p = z.clone();
    let mut rz: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
    let r0 = r.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-300);
    let max_iter = (4 * n).clamp(200, 6000);
    let mut hp = vec![0.0f64; 2 * n];
    for _ in 0..max_iter {
        apply(&p, &mut res);
        apply_t(&res, &mut hp);
        for (h, f) in hp.iter_mut().zip(&free) {
            if !*f {
                *h = 0.0;
            }
        }
        let php: f64 = p.iter().zip(&hp).map(|(a, b)| a * b).sum();
        if php.abs() < 1e-300 {
            break;
        }
        let alpha = rz / php;
        for i in 0..2 * n {
            x[i] += alpha * p[i] * if free[i] { 1.0 } else { 0.0 };
            r[i] -= alpha * hp[i];
        }
        if r.iter().map(|v| v * v).sum::<f64>().sqrt() <= 1e-10 * r0 {
            break;
        }
        for i in 0..2 * n {
            z[i] = r[i] / diag[i];
        }
        let rz_new: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
        let beta = rz_new / rz;
        rz = rz_new;
        for i in 0..2 * n {
            p[i] = z[i] + beta * p[i];
        }
    }
    let mut uv: Vec<Vec2> = (0..n).map(|i| Vec2::new(x[2 * i] as f32, x[2 * i + 1] as f32)).collect();
    if uv.iter().any(|p| !p.is_finite()) {
        return None;
    }
    // Orientation: the majority of triangles must be counter-clockwise like the surface.
    let signed: f32 = tri_v.iter().map(|v| (uv[v[1]] - uv[v[0]]).perp_dot(uv[v[2]] - uv[v[0]])).sum();
    if signed < 0.0 {
        for p in &mut uv {
            p.y = -p.y;
        }
    }
    // Into the mesh's UV orientation convention.
    if UV_SIGN < 0.0 {
        for p in &mut uv {
            p.y = -p.y;
        }
    }
    let uvs = tri_v.iter().map(|v| [uv[v[0]], uv[v[1]], uv[v[2]]]).collect();
    Some(FlatChart { tris: tris.to_vec(), uv: uvs, area3d })
}

// ----- overlap ------------------------------------------------------------------------------

fn tris_overlap(a: &[Vec2; 3], b: &[Vec2; 3], eps: f32) -> bool {
    for tri in [a, b] {
        for k in 0..3 {
            let e = tri[(k + 1) % 3] - tri[k];
            let axis = e.perp();
            let len = axis.length();
            if len < 1e-30 {
                return false; // degenerate triangle: no area to overlap
            }
            let axis = axis / len;
            let range = |t: &[Vec2; 3]| {
                let d = t.map(|p| p.dot(axis));
                (d[0].min(d[1]).min(d[2]), d[0].max(d[1]).max(d[2]))
            };
            let (a0, a1) = range(a);
            let (b0, b1) = range(b);
            if a1 <= b0 + eps || b1 <= a0 + eps {
                return false;
            }
        }
    }
    true
}

/// Invoke `f` for every pair of triangles whose interiors overlap; stop when it returns false.
fn for_each_overlap(tris: &[[Vec2; 3]], mut f: impl FnMut(usize, usize) -> bool) {
    if tris.len() < 2 {
        return;
    }
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in tris.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let size = (hi - lo).max_element().max(1e-20);
    let eps = size * 1e-6;
    let cells = ((tris.len() as f32).sqrt().ceil() as usize).clamp(1, 512);
    let cell = |v: f32, o: f32| (((v - o) / size * cells as f32).floor().max(0.0) as usize).min(cells - 1);
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); cells * cells];
    for (i, t) in tris.iter().enumerate() {
        let (mut a, mut b) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for p in t {
            a = a.min(*p);
            b = b.max(*p);
        }
        for cy in cell(a.y, lo.y)..=cell(b.y, lo.y) {
            for cx in cell(a.x, lo.x)..=cell(b.x, lo.x) {
                grid[cy * cells + cx].push(i as u32);
            }
        }
    }
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    for list in &grid {
        for (k, &i) in list.iter().enumerate() {
            for &j in &list[k + 1..] {
                if seen.insert((i.min(j), i.max(j))) && tris_overlap(&tris[i as usize], &tris[j as usize], eps) && !f(i as usize, j as usize) {
                    return;
                }
            }
        }
    }
}

fn any_overlap(tris: &[[Vec2; 3]]) -> bool {
    let mut found = false;
    for_each_overlap(tris, |_, _| {
        found = true;
        false
    });
    found
}

// ----- packing ------------------------------------------------------------------------------

struct Packed {
    margin: f32,
    coverage: f32,
}

#[derive(Clone, Copy)]
struct Placement {
    pos: Vec2,
    quarter_turn: bool,
}

/// Pack the charts into the unit square: uniform scale found by bisection, margin fixed in UV.
fn pack(charts: &mut [FlatChart], margin_wanted: f32, quality: PackQuality) -> Packed {
    let sizes: Vec<Vec2> = charts.iter().map(|c| c.size()).collect();
    let n = charts.len();
    // Many charts leave no room for a large margin.
    let mut margin = margin_wanted.min(0.5 / (n as f32).sqrt().max(1.0));
    let quality = if n > 3000 { PackQuality::Fast } else { quality };
    let area_sum: f32 = sizes.iter().map(|s| s.x * s.y).sum::<f32>().max(1e-30);
    let try_at = |scale: f32, margin: f32| attempt(&sizes, scale, margin, quality);
    let mut result = None;
    for _ in 0..24 {
        let mut hi = (1.0 / area_sum.sqrt()) * 1.5;
        let longest = sizes.iter().map(|s| s.x.max(s.y)).fold(0.0f32, f32::max).max(1e-30);
        hi = hi.min(1.0 / longest * 2.0).max(1e-30);
        // Check that a vanishing scale fits at all (margin alone).
        let tiny = hi * 1e-4;
        let Some(first) = try_at(tiny, margin) else {
            margin *= 0.5;
            continue;
        };
        let mut best = (tiny, first);
        let mut lo = tiny;
        let iters = match quality {
            PackQuality::Fast => 14,
            PackQuality::Balanced => 22,
            PackQuality::Best => 30,
        };
        for _ in 0..iters {
            let mid = 0.5 * (lo + hi);
            match try_at(mid, margin) {
                Some(p) => {
                    best = (mid, p);
                    lo = mid;
                }
                None => hi = mid,
            }
        }
        result = Some((best.0, best.1, margin));
        break;
    }
    let Some((scale, placements, margin)) = result else {
        // Cannot happen with margin halving, but never leave UVs unplaced.
        for c in charts.iter_mut() {
            c.map(|_| Vec2::splat(0.5));
        }
        return Packed { margin: 0.0, coverage: 0.0 };
    };
    let mut covered = 0.0f32;
    for (i, c) in charts.iter_mut().enumerate() {
        let pl = placements[i];
        let size = sizes[i];
        let off = pl.pos + Vec2::splat(margin * 0.5);
        if pl.quarter_turn {
            // (x, y) -> (y, -x) + shift keeps orientation; resulting extent is (size.y, size.x).
            c.map(|p| Vec2::new(p.y, size.x - p.x) * scale + off);
        } else {
            c.map(|p| p * scale + off);
        }
        covered += c.area_uv();
    }
    // Float noise must not push a coordinate outside 0..1.
    for c in charts.iter_mut() {
        c.map(|p| p.clamp(Vec2::ZERO, Vec2::ONE));
    }
    Packed { margin, coverage: covered }
}

/// One packing attempt; `None` when the rectangles do not fit.
fn attempt(sizes: &[Vec2], scale: f32, margin: f32, quality: PackQuality) -> Option<Vec<Placement>> {
    let rects: Vec<Vec2> = sizes.iter().map(|s| *s * scale + Vec2::splat(margin)).collect();
    if rects.iter().any(|r| r.x > 1.0 || (r.y > 1.0 && r.x > 1.0)) && quality == PackQuality::Fast {
        return None;
    }
    let orders: Vec<Vec<usize>> = {
        let by = |key: &dyn Fn(Vec2) -> f32| {
            let mut o: Vec<usize> = (0..rects.len()).collect();
            o.sort_by(|&a, &b| key(rects[b]).total_cmp(&key(rects[a])).then(a.cmp(&b)));
            o
        };
        match quality {
            PackQuality::Best => vec![by(&|r| r.y), by(&|r| r.x * r.y), by(&|r| r.x.max(r.y))],
            _ => vec![by(&|r| r.y)],
        }
    };
    for order in orders {
        let placed = match quality {
            PackQuality::Fast => shelf_pack(&rects, &order),
            PackQuality::Balanced => skyline_pack(&rects, &order, false),
            PackQuality::Best => skyline_pack(&rects, &order, true),
        };
        if placed.is_some() {
            return placed;
        }
    }
    None
}

fn shelf_pack(rects: &[Vec2], order: &[usize]) -> Option<Vec<Placement>> {
    let mut out = vec![Placement { pos: Vec2::ZERO, quarter_turn: false }; rects.len()];
    let (mut x, mut y, mut row_h) = (0.0f32, 0.0f32, 0.0f32);
    for &i in order {
        let r = rects[i];
        if x + r.x > 1.0 + 1e-6 {
            y += row_h;
            x = 0.0;
            row_h = 0.0;
        }
        if r.x > 1.0 + 1e-6 || y + r.y > 1.0 + 1e-6 {
            return None;
        }
        out[i].pos = Vec2::new(x, y);
        x += r.x;
        row_h = row_h.max(r.y);
    }
    Some(out)
}

/// Bottom-left skyline packing into the unit square.
fn skyline_pack(rects: &[Vec2], order: &[usize], rotate: bool) -> Option<Vec<Placement>> {
    const E: f32 = 1e-6;
    // Segments (x, y, width) left to right covering [0, 1].
    let mut sky: Vec<(f32, f32, f32)> = vec![(0.0, 0.0, 1.0)];
    let mut out = vec![Placement { pos: Vec2::ZERO, quarter_turn: false }; rects.len()];
    for &i in order {
        let mut best: Option<(f32, f32, usize, bool, f32)> = None; // (top, x, segment, turned, y)
        for turned in [false, true] {
            if turned && !rotate {
                break;
            }
            let (w, h) = if turned { (rects[i].y, rects[i].x) } else { (rects[i].x, rects[i].y) };
            for s in 0..sky.len() {
                let x = sky[s].0;
                if x + w > 1.0 + E {
                    break;
                }
                let mut y = 0.0f32;
                let mut covered = 0.0f32;
                let mut k = s;
                while covered < w - E && k < sky.len() {
                    y = y.max(sky[k].1);
                    covered += sky[k].2;
                    k += 1;
                }
                if y + h > 1.0 + E {
                    continue;
                }
                let cand = (y + h, x, s, turned, y);
                if best.is_none_or(|b| (cand.0, cand.1) < (b.0 - 1e-9, b.1) || (cand.0 - b.0).abs() <= 1e-9 && cand.1 < b.1) {
                    best = Some(cand);
                }
            }
        }
        let (top, x, _, turned, y) = best?;
        let w = if turned { rects[i].y } else { rects[i].x };
        out[i] = Placement { pos: Vec2::new(x, y), quarter_turn: turned };
        // Raise the skyline over [x, x + w].
        let mut next: Vec<(f32, f32, f32)> = Vec::with_capacity(sky.len() + 2);
        let end = x + w;
        let mut inserted = false;
        for &(sx, sy, sw) in &sky {
            let se = sx + sw;
            if se <= x + E || sx >= end - E {
                if sx >= end - E && !inserted {
                    next.push((x, top, w));
                    inserted = true;
                }
                next.push((sx, sy, sw));
                continue;
            }
            if sx < x - E {
                next.push((sx, sy, x - sx));
            }
            if !inserted {
                next.push((x, top, w));
                inserted = true;
            }
            if se > end + E {
                next.push((end, sy, se - end));
            }
        }
        if !inserted {
            next.push((x, top, w));
        }
        // Merge neighbours of equal height.
        let mut merged: Vec<(f32, f32, f32)> = Vec::with_capacity(next.len());
        for seg in next {
            if seg.2 <= 0.0 {
                continue;
            }
            match merged.last_mut() {
                Some(last) if (last.1 - seg.1).abs() <= E => last.2 += seg.2,
                _ => merged.push(seg),
            }
        }
        sky = merged;
    }
    Some(out)
}

// ----- output mesh --------------------------------------------------------------------------

/// Copy of `mesh` with the given per-corner UVs, vertices split along chart borders.
fn rebuild(mesh: &Mesh, corner_uv: &[Vec2], tri_chart: &[u32]) -> Mesh {
    let corner_pos: Vec<Vec3> = mesh.indices.iter().map(|&i| mesh.positions[i as usize]).collect();
    let corner_nrm: Vec<Vec3> = mesh.indices.iter().map(|&i| mesh.normals[i as usize]).collect();
    let tangents = compute_corner_tangents(&corner_pos, &corner_nrm, corner_uv);
    let mut out = Mesh {
        name: mesh.name.clone(),
        tri_material: mesh.tri_material.clone(),
        tri_object: mesh.tri_object.clone(),
        objects: mesh.objects.clone(),
        materials: mesh.materials.clone(),
        submeshes: mesh.submeshes.clone(),
        ..Default::default()
    };
    let mut lookup: HashMap<(u32, u32, [u32; 4], [u32; 2]), u32> = HashMap::new();
    for (c, &orig) in mesh.indices.iter().enumerate() {
        let t = tangents[c];
        let uv = corner_uv[c];
        let key = (orig, tri_chart[c / 3], t.to_array().map(f32::to_bits), uv.to_array().map(f32::to_bits));
        let idx = *lookup.entry(key).or_insert_with(|| {
            out.positions.push(mesh.positions[orig as usize]);
            out.normals.push(mesh.normals[orig as usize]);
            out.uvs.push(uv);
            out.tangents.push(Vec4::from(t));
            (out.positions.len() - 1) as u32
        });
        out.indices.push(idx);
    }
    out.bounds = Aabb::from_points(out.positions.iter().copied());
    out
}

// ----- analysis -----------------------------------------------------------------------------

/// How usable a mesh's current UVs are for painting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UvReport {
    pub has_uvs: bool,
    /// Triangles with (near) zero UV area although they have surface area.
    pub collapsed: usize,
    /// Triangles whose UV winding is opposite to the surface's.
    pub flipped: usize,
    /// Triangles whose interior overlaps another triangle of the same material slot.
    pub overlapping: usize,
    /// Triangles with any corner outside 0..1.
    pub outside_unit_square: usize,
    pub mean_stretch: f32,
    pub max_stretch: f32,
}

impl UvReport {
    /// Whether painting would work without unwrapping.
    pub fn usable(&self) -> bool {
        self.has_uvs && self.collapsed == 0 && self.overlapping == 0 && self.outside_unit_square == 0
    }

    pub fn summary(&self) -> String {
        if !self.has_uvs {
            return "no UVs".into();
        }
        format!(
            "{} overlapping, {} flipped, {} collapsed, {} outside 0..1, stretch mean {:.2} / max {:.2}",
            self.overlapping, self.flipped, self.collapsed, self.outside_unit_square, self.mean_stretch, self.max_stretch
        )
    }
}

/// Measure the UV quality of `mesh` (overlap is per material slot).
pub fn analyze_uvs(mesh: &Mesh) -> UvReport {
    let mut report = UvReport::default();
    let Some((lo, hi)) = mesh.uv_bounds() else { return report };
    report.has_uvs = (hi - lo).length() > 1e-6;
    if !report.has_uvs {
        return report;
    }
    let slot_ranges: Vec<std::ops::Range<usize>> = if mesh.submeshes.is_empty() { vec![0..mesh.triangle_count()] } else { mesh.submeshes.iter().map(|s| s.first_triangle as usize..(s.first_triangle + s.triangle_count) as usize).collect() };
    let limit = sliver_limit(mesh);
    for range in slot_ranges {
        let tris: Vec<[Vec2; 3]> = range.clone().map(|t| mesh.tri_uvs(t)).collect();
        let mut hit = vec![false; tris.len()];
        for_each_overlap(&tris, |i, j| {
            hit[i] = true;
            hit[j] = true;
            true
        });
        report.overlapping += hit.iter().filter(|h| **h).count();
        let q = measure(mesh, range.clone());
        report.flipped += q.flipped;
        report.max_stretch = report.max_stretch.max(q.max_stretch);
        report.mean_stretch += q.mean_stretch * range.len() as f32;
        for t in range {
            let uv = mesh.tri_uvs(t);
            let area_uv = 0.5 * (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]).abs();
            let [p0, p1, p2] = mesh.tri_positions(t);
            if area_uv < 1e-10 && 0.5 * (p1 - p0).cross(p2 - p0).length() > limit {
                report.collapsed += 1;
            }
            if uv.iter().any(|p| p.x < -1e-5 || p.y < -1e-5 || p.x > 1.0 + 1e-5 || p.y > 1.0 + 1e-5) {
                report.outside_unit_square += 1;
            }
        }
    }
    report.mean_stretch /= mesh.triangle_count().max(1) as f32;
    report
}

/// Importer hook: a model with no UV layout at all is unwrapped with the default settings so it
/// can be painted. Models that have UVs (good or bad) are returned untouched.
pub fn unwrap_if_missing(mesh: Mesh, warnings: &mut Vec<String>) -> Mesh {
    if analyze_uvs(&mesh).has_uvs {
        return mesh;
    }
    match unwrap(&mesh, &UnwrapSettings::default()) {
        Ok((unwrapped, stats)) => {
            warnings.retain(|w| !w.contains("have no UVs and cannot be painted"));
            warnings.push(format!("The model has no UVs; they were generated automatically ({}).", stats.summary()));
            unwrapped
        }
        Err(e) => {
            warnings.push(format!("The model has no UVs and could not be unwrapped: {e}."));
            mesh
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Corner, MeshBuilder};
    use crate::primitives;

    /// Mesh with the UVs thrown away (all zero), as an importer would deliver a UV-less model.
    fn without_uvs(m: &Mesh) -> Mesh {
        let mut b = MeshBuilder::new(m.name.clone());
        for (i, mat) in m.materials.iter().enumerate() {
            assert_eq!(b.material(&mat.name), i as u32);
        }
        for t in 0..m.triangle_count() {
            let c = m.tri(t).map(|i| Corner { position: m.positions[i as usize], normal: Some(m.normals[i as usize]), uv: Some(Vec2::ZERO) });
            b.push_triangle(m.tri_material[t], c);
        }
        b.build_for_baking().unwrap()
    }

    fn check_valid(src: &Mesh, out: &Mesh, stats: &UnwrapStats) {
        assert!(out.validate().is_empty(), "{:?}", out.validate());
        assert_eq!(out.triangle_count(), src.triangle_count());
        for t in 0..src.triangle_count() {
            let (a, b) = (src.tri_positions(t), out.tri_positions(t));
            assert_eq!(a, b, "triangle {t} moved");
        }
        for uv in &out.uvs {
            assert!(uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0, "uv outside 0..1: {uv}");
        }
        let report = analyze_uvs(out);
        assert_eq!(report.overlapping, 0, "{}", report.summary());
        assert_eq!(report.flipped, 0, "{}", report.summary());
        assert_eq!(report.collapsed, 0);
        assert!(stats.charts >= 1);
    }

    #[test]
    fn sign_convention_matches_primitives() {
        for m in [primitives::cube(1.0), primitives::plane(1.0, 3), primitives::torus(1.0, 0.3, 12, 8)] {
            let q = measure(&m, 0..m.triangle_count());
            assert_eq!(q.flipped, 0, "{} uses the opposite UV winding", m.name);
        }
    }

    #[test]
    fn unwraps_a_cube_into_six_charts() {
        let src = without_uvs(&primitives::cube(2.0));
        assert!(!analyze_uvs(&src).has_uvs);
        let (out, stats) = unwrap(&src, &UnwrapSettings::default()).unwrap();
        check_valid(&src, &out, &stats);
        assert_eq!(stats.charts, 6, "{stats:?}");
        assert!(stats.max_stretch < 1.05, "faces are flat: {stats:?}");
        assert!(stats.coverage > 0.6, "{stats:?}");
    }

    #[test]
    fn unwraps_a_sphere_with_bounded_stretch() {
        let src = without_uvs(&primitives::uv_sphere(1.0, 48, 24));
        let (out, stats) = unwrap(&src, &UnwrapSettings::default()).unwrap();
        check_valid(&src, &out, &stats);
        assert!(stats.charts >= 3 && stats.charts < 60, "{stats:?}");
        assert!(stats.mean_stretch < 1.6, "{stats:?}");
        assert!(stats.max_stretch <= UnwrapSettings::default().max_stretch + 1e-3, "{stats:?}");
        assert!(stats.coverage > 0.35, "{stats:?}");
    }

    #[test]
    fn torus_and_all_quality_levels_work() {
        let src = without_uvs(&primitives::torus(1.0, 0.35, 24, 12));
        for quality in PackQuality::ALL {
            let (out, stats) = unwrap(&src, &UnwrapSettings { quality, ..Default::default() }).unwrap();
            check_valid(&src, &out, &stats);
            assert!(stats.max_stretch <= 4.0 + 1e-3, "{quality:?} {stats:?}");
        }
    }

    #[test]
    fn multiple_material_slots_are_packed_separately() {
        let src = without_uvs(&primitives::sample_crate());
        assert!(src.materials.len() >= 2);
        let (out, stats) = unwrap(&src, &UnwrapSettings::default()).unwrap();
        check_valid(&src, &out, &stats);
    }

    #[test]
    fn deterministic() {
        let src = without_uvs(&primitives::uv_sphere(1.0, 32, 16));
        let a = unwrap(&src, &UnwrapSettings::default()).unwrap().0;
        let b = unwrap(&src, &UnwrapSettings::default()).unwrap().0;
        assert_eq!(a.uvs, b.uvs);
        assert_eq!(a.indices, b.indices);
        assert_eq!(a.tangents, b.tangents);
    }

    #[test]
    fn margin_separates_islands() {
        let src = without_uvs(&primitives::cube(1.0));
        let settings = UnwrapSettings { margin_texels: 8.0, resolution: 256, ..Default::default() };
        let (out, stats) = unwrap(&src, &settings).unwrap();
        check_valid(&src, &out, &stats);
        let margin = 8.0 / 256.0;
        assert!((stats.margin_uv - margin).abs() < 1e-6);
        for uv in &out.uvs {
            assert!(uv.x >= margin * 0.5 - 1e-5 && uv.x <= 1.0 - margin * 0.5 + 1e-5, "{uv}");
            assert!(uv.y >= margin * 0.5 - 1e-5 && uv.y <= 1.0 - margin * 0.5 + 1e-5, "{uv}");
        }
    }

    #[test]
    fn smaller_angle_means_more_charts() {
        let src = without_uvs(&primitives::uv_sphere(1.0, 32, 16));
        let few = unwrap(&src, &UnwrapSettings { angle_threshold_deg: 80.0, ..Default::default() }).unwrap().1.charts;
        let many = unwrap(&src, &UnwrapSettings { angle_threshold_deg: 25.0, ..Default::default() }).unwrap().1.charts;
        assert!(many > few, "{few} vs {many}");
    }

    fn raw_mesh(positions: Vec<Vec3>, indices: Vec<u32>) -> Mesh {
        let n = positions.len();
        let tc = indices.len() / 3;
        Mesh {
            name: "raw".into(),
            normals: vec![Vec3::Y; n],
            uvs: vec![Vec2::ZERO; n],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); n],
            tri_material: vec![0; tc],
            tri_object: Vec::new(),
            objects: Vec::new(),
            materials: vec![crate::mesh::MaterialSlot { name: "m".into() }],
            submeshes: vec![crate::mesh::Submesh { material: 0, first_triangle: 0, triangle_count: tc as u32 }],
            bounds: Aabb::from_points(positions.iter().copied()),
            positions,
            indices,
            ..Default::default()
        }
    }

    #[test]
    fn degenerate_and_non_manifold_meshes_do_not_panic() {
        // Three triangles share one edge (non-manifold), plus a zero-area triangle, a triangle
        // with a repeated vertex and an isolated triangle.
        let p = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.5),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(5.0, 5.0, 5.0),
            Vec3::new(6.0, 5.0, 5.0),
            Vec3::new(5.0, 6.0, 5.0),
        ];
        let idx = vec![0, 1, 2, 1, 0, 3, 0, 1, 4, 0, 5, 6, 0, 0, 1, 7, 8, 9];
        let src = raw_mesh(p, idx);
        let (out, stats) = unwrap(&src, &UnwrapSettings::default()).unwrap();
        assert_eq!(out.triangle_count(), 6);
        assert!(out.validate().is_empty(), "{:?}", out.validate());
        assert!(out.uvs.iter().all(|uv| uv.is_finite() && (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)));
        assert!(stats.charts >= 2);
    }

    #[test]
    fn reports_bad_input() {
        assert_eq!(unwrap(&Mesh::default(), &UnwrapSettings::default()).unwrap_err(), UnwrapError::Empty);
        let mut m = raw_mesh(vec![Vec3::ZERO, Vec3::X, Vec3::Y], vec![0, 1, 2]);
        m.positions[1].x = f32::NAN;
        assert_eq!(unwrap(&m, &UnwrapSettings::default()).unwrap_err(), UnwrapError::NonFinite);
    }

    #[test]
    fn analysis_flags_overlap_and_missing_uvs() {
        let good = primitives::cube(1.0);
        assert!(analyze_uvs(&good).usable(), "{}", analyze_uvs(&good).summary());
        assert!(!analyze_uvs(&without_uvs(&good)).has_uvs);
        let mut piled = good.clone();
        for uv in &mut piled.uvs {
            *uv = Vec2::new((uv.x * 3.0).fract(), (uv.y * 2.0).fract());
        }
        assert!(!analyze_uvs(&piled).usable());
    }

    #[test]
    fn open_cylinder_and_saddle_flatten_without_overlap() {
        // Open tube (annulus topology) and a strongly curved saddle patch.
        let (seg, rings) = (24u32, 6u32);
        let mut p = Vec::new();
        let mut idx = Vec::new();
        for r in 0..=rings {
            for s in 0..seg {
                let a = std::f32::consts::TAU * s as f32 / seg as f32;
                p.push(Vec3::new(a.cos(), r as f32 * 0.3, a.sin()));
            }
        }
        for r in 0..rings {
            for s in 0..seg {
                let (a, b) = (r * seg + s, r * seg + (s + 1) % seg);
                let (c, d) = (a + seg, b + seg);
                idx.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
        let mut tube = raw_mesh(p, idx);
        for (n, p) in tube.normals.iter_mut().zip(&tube.positions) {
            *n = Vec3::new(p.x, 0.0, p.z).normalize();
        }
        let (out, stats) = unwrap(&tube, &UnwrapSettings::default()).unwrap();
        let report = analyze_uvs(&out);
        assert_eq!((report.overlapping, report.flipped), (0, 0), "{}", report.summary());
        assert!(out.uvs.iter().all(|uv| (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)));
        assert!(stats.max_stretch <= 4.0 + 1e-3, "{stats:?}");
    }
}

