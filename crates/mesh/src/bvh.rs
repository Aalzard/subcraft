//! Bounding volume hierarchy over triangles (binned SAH build, stack traversal).
//!
//! Used for viewport picking, brush footprint queries and the CPU baker. The structure is
//! immutable after build and `Sync`, so baking threads share it freely.

use glam::Vec3;

use crate::mesh::{Aabb, Mesh};

#[derive(Clone, Copy, Debug)]
struct Node {
    min: Vec3,
    /// Leaf: index of first entry in `tri_ids`. Interior: index of left child (right = left + 1).
    left_first: u32,
    max: Vec3,
    /// Leaf: number of triangles (> 0). Interior: 0.
    count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub tri: u32,
    pub t: f32,
    /// Barycentric weight of vertex 1.
    pub u: f32,
    /// Barycentric weight of vertex 2.
    pub v: f32,
    /// True if the ray hit the triangle's front (counter-clockwise) side.
    pub front_face: bool,
}

/// A [`Bvh`] as plain arrays, see [`Bvh::flatten`].
#[derive(Clone, Debug, Default)]
pub struct FlatBvh {
    pub nodes: Vec<[u32; 8]>,
    pub tris: Vec<[f32; 12]>,
    /// Mesh triangle index of each entry of `tris`.
    pub tri_ids: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    tri_ids: Vec<u32>,
    v0: Vec<Vec3>,
    e1: Vec<Vec3>,
    e2: Vec<Vec3>,
}

const BINS: usize = 16;
const MAX_LEAF: u32 = 4;

impl Bvh {
    pub fn build(mesh: &Mesh) -> Self {
        let tris: Vec<[Vec3; 3]> = (0..mesh.triangle_count()).map(|t| mesh.tri_positions(t)).collect();
        Self::from_triangles(&tris)
    }

    pub fn from_triangles(tris: &[[Vec3; 3]]) -> Self {
        // Hidden viewport faces retain their IDs as zero-area triangles. Keep them out of
        // volume queries too, so an expanded brush ray cannot visit thousands of hidden faces.
        let tri_ids: Vec<u32> = tris.iter().enumerate().filter(|(_, t)| (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 0.0).map(|(i, _)| i as u32).collect();
        let n = tri_ids.len();
        if n == 0 { return Self::default(); }
        let mut bvh = Bvh {
            nodes: Vec::with_capacity(2 * n.max(1)),
            tri_ids,
            v0: tris.iter().map(|t| t[0]).collect(),
            e1: tris.iter().map(|t| t[1] - t[0]).collect(),
            e2: tris.iter().map(|t| t[2] - t[0]).collect(),
        };
        if n == 0 {
            return bvh;
        }
        let centroids: Vec<Vec3> = tris.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
        let boxes: Vec<Aabb> = tris.iter().map(|t| Aabb::from_points(t.iter().copied())).collect();

        bvh.nodes.push(Node { min: Vec3::ZERO, left_first: 0, max: Vec3::ZERO, count: n as u32 });
        let mut stack = vec![0usize];
        while let Some(ni) = stack.pop() {
            let (first, count) = (bvh.nodes[ni].left_first as usize, bvh.nodes[ni].count as usize);
            let ids = &bvh.tri_ids[first..first + count];
            let mut nb = Aabb::EMPTY;
            let mut cb = Aabb::EMPTY;
            for &t in ids {
                nb = nb.merge(&boxes[t as usize]);
                cb.grow(centroids[t as usize]);
            }
            bvh.nodes[ni].min = nb.min;
            bvh.nodes[ni].max = nb.max;
            if count as u32 <= MAX_LEAF {
                continue;
            }
            // Binned SAH split search.
            let mut best = (f32::INFINITY, 0usize, 0.0f32);
            let ext = cb.extent();
            for axis in 0..3 {
                if ext[axis] <= 1e-12 {
                    continue;
                }
                let mut bin_box = [Aabb::EMPTY; BINS];
                let mut bin_cnt = [0u32; BINS];
                let scale = BINS as f32 / ext[axis];
                for &t in ids {
                    let b = (((centroids[t as usize][axis] - cb.min[axis]) * scale) as usize).min(BINS - 1);
                    bin_cnt[b] += 1;
                    bin_box[b] = bin_box[b].merge(&boxes[t as usize]);
                }
                let mut left_area = [0.0f32; BINS - 1];
                let mut left_cnt = [0u32; BINS - 1];
                let mut acc = Aabb::EMPTY;
                let mut c = 0;
                for i in 0..BINS - 1 {
                    acc = acc.merge(&bin_box[i]);
                    c += bin_cnt[i];
                    left_area[i] = if c > 0 { acc.surface_area() } else { 0.0 };
                    left_cnt[i] = c;
                }
                let mut acc = Aabb::EMPTY;
                let mut c = 0;
                for i in (1..BINS).rev() {
                    acc = acc.merge(&bin_box[i]);
                    c += bin_cnt[i];
                    let cost = left_cnt[i - 1] as f32 * left_area[i - 1] + c as f32 * if c > 0 { acc.surface_area() } else { 0.0 };
                    if left_cnt[i - 1] > 0 && c > 0 && cost < best.0 {
                        best = (cost, axis, cb.min[axis] + i as f32 / scale);
                    }
                }
            }
            let leaf_cost = count as f32 * nb.surface_area();
            let (axis, split) = if best.0.is_finite() && best.0 < leaf_cost {
                (best.1, best.2)
            } else if count as u32 > MAX_LEAF * 4 {
                // Degenerate distribution (e.g. all centroids equal): median split by index.
                (usize::MAX, 0.0)
            } else {
                continue;
            };
            let mid = if axis == usize::MAX {
                count / 2
            } else {
                let slice = &mut bvh.tri_ids[first..first + count];
                let mut i = 0;
                let mut j = slice.len();
                while i < j {
                    if centroids[slice[i] as usize][axis] < split {
                        i += 1;
                    } else {
                        j -= 1;
                        slice.swap(i, j);
                    }
                }
                if i == 0 || i == count { count / 2 } else { i }
            };
            let left = bvh.nodes.len();
            bvh.nodes.push(Node { min: Vec3::ZERO, left_first: first as u32, max: Vec3::ZERO, count: mid as u32 });
            bvh.nodes.push(Node {
                min: Vec3::ZERO,
                left_first: (first + mid) as u32,
                max: Vec3::ZERO,
                count: (count - mid) as u32,
            });
            bvh.nodes[ni].left_first = left as u32;
            bvh.nodes[ni].count = 0;
            stack.push(left);
            stack.push(left + 1);
        }
        bvh
    }

    /// Plain arrays for uploading the tree to a GPU. `nodes`: 8 words per node
    /// (`min.xyz` as f32 bits, `left_first`, `max.xyz` as f32 bits, `count`; leaf when `count > 0`,
    /// otherwise the right child is `left_first + 1`). `tris`: 12 floats per entry
    /// (`v0.xyz, 0, e1.xyz, 0, e2.xyz, 0`), re-ordered into leaf order so a leaf covers
    /// `tris[left_first..left_first + count]` directly.
    pub fn flatten(&self) -> FlatBvh {
        let nodes = self
            .nodes
            .iter()
            .map(|n| {
                [n.min.x.to_bits(), n.min.y.to_bits(), n.min.z.to_bits(), n.left_first, n.max.x.to_bits(), n.max.y.to_bits(), n.max.z.to_bits(), n.count]
            })
            .collect();
        let tris = self
            .tri_ids
            .iter()
            .map(|&t| {
                let (v, a, b) = (self.v0[t as usize], self.e1[t as usize], self.e2[t as usize]);
                [v.x, v.y, v.z, 0.0, a.x, a.y, a.z, 0.0, b.x, b.y, b.z, 0.0]
            })
            .collect();
        FlatBvh { nodes, tris, tri_ids: self.tri_ids.clone() }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn bounds(&self) -> Aabb {
        self.nodes.first().map(|n| Aabb { min: n.min, max: n.max }).unwrap_or(Aabb::EMPTY)
    }

    /// Closest intersection in `(t_min, t_max)`. Both triangle sides are hit.
    pub fn intersect(&self, origin: Vec3, dir: Vec3, t_min: f32, t_max: f32) -> Option<RayHit> {
        self.intersect_filtered(origin, dir, t_min, t_max, |_| true)
    }

    /// Closest intersection accepting only triangles for which `accept(tri)` is true.
    pub fn intersect_filtered(
        &self,
        origin: Vec3,
        dir: Vec3,
        t_min: f32,
        t_max: f32,
        accept: impl Fn(u32) -> bool,
    ) -> Option<RayHit> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = dir.recip();
        let mut best: Option<RayHit> = None;
        let mut t_best = t_max;
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if slab(node.min, node.max, origin, inv, t_best).is_none() {
                continue;
            }
            if node.count > 0 {
                for &t in &self.tri_ids[node.left_first as usize..(node.left_first + node.count) as usize] {
                    if !accept(t) {
                        continue;
                    }
                    if let Some(h) = self.tri_hit(t, origin, dir, t_min, t_best) {
                        t_best = h.t;
                        best = Some(h);
                    }
                }
            } else {
                let l = node.left_first as usize;
                let dl = slab(self.nodes[l].min, self.nodes[l].max, origin, inv, t_best);
                let dr = slab(self.nodes[l + 1].min, self.nodes[l + 1].max, origin, inv, t_best);
                match (dl, dr) {
                    (Some(a), Some(b)) => {
                        // Push far child first so the near one is processed next.
                        if a <= b {
                            stack[sp] = l as u32 + 1;
                            stack[sp + 1] = l as u32;
                        } else {
                            stack[sp] = l as u32;
                            stack[sp + 1] = l as u32 + 1;
                        }
                        sp += 2;
                    }
                    (Some(_), None) => {
                        stack[sp] = l as u32;
                        sp += 1;
                    }
                    (None, Some(_)) => {
                        stack[sp] = l as u32 + 1;
                        sp += 1;
                    }
                    (None, None) => {}
                }
                if sp >= stack.len() - 2 {
                    // Pathologically deep tree; fall back to a heap-allocated traversal.
                    return self.intersect_slow(origin, dir, t_min, t_max, &accept);
                }
            }
        }
        best
    }

    fn intersect_slow(&self, origin: Vec3, dir: Vec3, t_min: f32, t_max: f32, accept: &dyn Fn(u32) -> bool) -> Option<RayHit> {
        let inv = dir.recip();
        let mut best: Option<RayHit> = None;
        let mut t_best = t_max;
        let mut stack = vec![0u32];
        while let Some(ni) = stack.pop() {
            let node = &self.nodes[ni as usize];
            if slab(node.min, node.max, origin, inv, t_best).is_none() {
                continue;
            }
            if node.count > 0 {
                for &t in &self.tri_ids[node.left_first as usize..(node.left_first + node.count) as usize] {
                    if accept(t)
                        && let Some(h) = self.tri_hit(t, origin, dir, t_min, t_best)
                    {
                        t_best = h.t;
                        best = Some(h);
                    }
                }
            } else {
                stack.push(node.left_first);
                stack.push(node.left_first + 1);
            }
        }
        best
    }

    /// True if anything is hit in `(t_min, t_max)`.
    pub fn occluded(&self, origin: Vec3, dir: Vec3, t_min: f32, t_max: f32) -> bool {
        self.occluded_filtered(origin, dir, t_min, t_max, |_| true)
    }

    /// True if a triangle accepted by `accept(tri)` is hit in `(t_min, t_max)`.
    pub fn occluded_filtered(&self, origin: Vec3, dir: Vec3, t_min: f32, t_max: f32, accept: impl Fn(u32) -> bool) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let inv = dir.recip();
        let mut stack = vec![0u32];
        while let Some(ni) = stack.pop() {
            let node = &self.nodes[ni as usize];
            if slab(node.min, node.max, origin, inv, t_max).is_none() {
                continue;
            }
            if node.count > 0 {
                for &t in &self.tri_ids[node.left_first as usize..(node.left_first + node.count) as usize] {
                    if accept(t) && self.tri_hit(t, origin, dir, t_min, t_max).is_some() {
                        return true;
                    }
                }
            } else {
                stack.push(node.left_first);
                stack.push(node.left_first + 1);
            }
        }
        false
    }

    /// Visit every triangle whose leaf box passes `test` (conservative volume query).
    pub fn query(&self, mut test: impl FnMut(&Aabb) -> bool, mut visit: impl FnMut(u32)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0u32];
        while let Some(ni) = stack.pop() {
            let node = &self.nodes[ni as usize];
            if !test(&Aabb { min: node.min, max: node.max }) {
                continue;
            }
            if node.count > 0 {
                for &t in &self.tri_ids[node.left_first as usize..(node.left_first + node.count) as usize] {
                    visit(t);
                }
            } else {
                stack.push(node.left_first);
                stack.push(node.left_first + 1);
            }
        }
    }

    /// Triangles whose bounds intersect a sphere.
    pub fn query_sphere(&self, center: Vec3, radius: f32) -> Vec<u32> {
        let mut out = Vec::new();
        let r2 = radius * radius;
        self.query(
            |b| {
                let d = center.clamp(b.min, b.max) - center;
                d.length_squared() <= r2
            },
            |t| {
                let (a, b, c) = (self.v0[t as usize], self.v0[t as usize] + self.e1[t as usize], self.v0[t as usize] + self.e2[t as usize]);
                let tb = Aabb::from_points([a, b, c]);
                let d = center.clamp(tb.min, tb.max) - center;
                if d.length_squared() <= r2 {
                    out.push(t);
                }
            },
        );
        out
    }

    /// Closest surface point to `p` within `max_distance`, over triangles for which
    /// `accept(tri)` is true (nearest-first traversal with box-distance pruning).
    pub fn closest_point(&self, p: Vec3, max_distance: f32, accept: impl Fn(u32) -> bool) -> Option<ClosestPoint> {
        if self.nodes.is_empty() {
            return None;
        }
        let box_d2 = |n: &Node| (p.clamp(n.min, n.max) - p).length_squared();
        let mut best: Option<ClosestPoint> = None;
        let mut best_d2 = if max_distance.is_finite() { max_distance * max_distance } else { f32::INFINITY };
        let mut stack: Vec<(u32, f32)> = vec![(0, box_d2(&self.nodes[0]))];
        while let Some((ni, d2)) = stack.pop() {
            if d2 > best_d2 {
                continue;
            }
            let node = &self.nodes[ni as usize];
            if node.count > 0 {
                for &t in &self.tri_ids[node.left_first as usize..(node.left_first + node.count) as usize] {
                    if !accept(t) {
                        continue;
                    }
                    let ti = t as usize;
                    let (u, v, q) = closest_on_triangle(p, self.v0[ti], self.e1[ti], self.e2[ti]);
                    let dd = (q - p).length_squared();
                    if dd <= best_d2 {
                        best_d2 = dd;
                        best = Some(ClosestPoint { tri: t, u, v, distance: dd.sqrt() });
                    }
                }
            } else {
                let (l, r) = (node.left_first, node.left_first + 1);
                let (dl, dr) = (box_d2(&self.nodes[l as usize]), box_d2(&self.nodes[r as usize]));
                // Nearer child on top of the stack: it is searched first and tightens the bound.
                if dl < dr {
                    stack.push((r, dr));
                    stack.push((l, dl));
                } else {
                    stack.push((l, dl));
                    stack.push((r, dr));
                }
            }
        }
        best
    }

    #[inline]
    fn tri_hit(&self, t: u32, origin: Vec3, dir: Vec3, t_min: f32, t_max: f32) -> Option<RayHit> {
        let ti = t as usize;
        let (e1, e2) = (self.e1[ti], self.e2[ti]);
        let p = dir.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-14 {
            return None;
        }
        let inv_det = 1.0 / det;
        let s = origin - self.v0[ti];
        let u = s.dot(p) * inv_det;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = dir.dot(q) * inv_det;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let dist = e2.dot(q) * inv_det;
        if dist <= t_min || dist >= t_max {
            return None;
        }
        Some(RayHit { tri: t, t: dist, u, v, front_face: det > 0.0 })
    }
}

/// Result of [`Bvh::closest_point`]: the triangle and barycentrics of the nearest point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClosestPoint {
    pub tri: u32,
    /// Barycentric weight of vertex 1.
    pub u: f32,
    /// Barycentric weight of vertex 2.
    pub v: f32,
    pub distance: f32,
}

/// Closest point on triangle `(a, a + e1, a + e2)` to `p`: barycentrics of vertices 1 and 2,
/// and the point (region tests after Ericson, Real-Time Collision Detection 5.1.5).
pub fn closest_on_triangle(p: Vec3, a: Vec3, e1: Vec3, e2: Vec3) -> (f32, f32, Vec3) {
    let ap = p - a;
    let (d1, d2) = (e1.dot(ap), e2.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return (0.0, 0.0, a);
    }
    let bp = ap - e1;
    let (d3, d4) = (e1.dot(bp), e2.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return (1.0, 0.0, a + e1);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let t = d1 / (d1 - d3);
        return (t, 0.0, a + e1 * t);
    }
    let cp = ap - e2;
    let (d5, d6) = (e1.dot(cp), e2.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return (0.0, 1.0, a + e2);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let t = d2 / (d2 - d6);
        return (0.0, t, a + e2 * t);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let t = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (1.0 - t, t, a + e1 + (e2 - e1) * t);
    }
    let denom = 1.0 / (va + vb + vc);
    let (u, v) = (vb * denom, vc * denom);
    (u, v, a + e1 * u + e2 * v)
}

/// Ray/box slab test; returns entry distance if the box is hit before `t_max`.
#[inline]
fn slab(min: Vec3, max: Vec3, origin: Vec3, inv: Vec3, t_max: f32) -> Option<f32> {
    let t0 = (min - origin) * inv;
    let t1 = (max - origin) * inv;
    let tmin = t0.min(t1);
    let tmax = t0.max(t1);
    let enter = tmin.max_element().max(0.0);
    let exit = tmax.min_element().min(t_max);
    if enter <= exit { Some(enter) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    fn brute(tris: &[[Vec3; 3]], o: Vec3, d: Vec3) -> Option<f32> {
        let bvh = Bvh::from_triangles(&tris[..]);
        let mut best: Option<f32> = None;
        for t in 0..tris.len() as u32 {
            if let Some(h) = bvh.tri_hit(t, o, d, 1e-6, f32::INFINITY) {
                best = Some(best.map_or(h.t, |b: f32| b.min(h.t)));
            }
        }
        best
    }

    #[test]
    fn matches_brute_force_on_sphere() {
        let mesh = primitives::uv_sphere(1.0, 32, 16);
        let tris: Vec<[Vec3; 3]> = (0..mesh.triangle_count()).map(|t| mesh.tri_positions(t)).collect();
        let bvh = Bvh::from_triangles(&tris);
        let mut seed = 12345u32;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        for _ in 0..300 {
            let o = Vec3::new(rnd(), rnd(), rnd()) * 3.0;
            let target = Vec3::new(rnd(), rnd(), rnd()) * 0.5;
            let d = (target - o).normalize();
            let a = bvh.intersect(o, d, 1e-6, f32::INFINITY).map(|h| h.t);
            let b = brute(&tris, o, d);
            match (a, b) {
                (Some(a), Some(b)) => assert!((a - b).abs() < 1e-4, "{a} vs {b}"),
                (None, None) => {}
                other => panic!("mismatch {other:?}"),
            }
        }
    }

    #[test]
    fn closest_point_matches_brute_force() {
        let mesh = primitives::uv_sphere(1.0, 24, 12);
        let tris: Vec<[Vec3; 3]> = (0..mesh.triangle_count()).map(|t| mesh.tri_positions(t)).collect();
        let bvh = Bvh::from_triangles(&tris);
        let mut seed = 777u32;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        for i in 0..300 {
            let p = Vec3::new(rnd(), rnd(), rnd()) * 2.0;
            // Every other query only accepts the upper half, exercising the filter.
            let accept = |t: u32| i % 2 == 0 || tris[t as usize].iter().all(|v| v.y >= 0.0);
            let hit = bvh.closest_point(p, f32::INFINITY, accept).unwrap();
            let brute = (0..tris.len() as u32)
                .filter(|&t| accept(t) && bvh.e1[t as usize].cross(bvh.e2[t as usize]).length_squared() > 0.0)
                .map(|t| {
                    let ti = t as usize;
                    let (_, _, q) = closest_on_triangle(p, bvh.v0[ti], bvh.e1[ti], bvh.e2[ti]);
                    q.distance(p)
                })
                .fold(f32::INFINITY, f32::min);
            assert!((hit.distance - brute).abs() < 1e-5, "{} vs {brute}", hit.distance);
            // The barycentrics reproduce the reported point.
            let [a, b, c] = tris[hit.tri as usize];
            let q = a * (1.0 - hit.u - hit.v) + b * hit.u + c * hit.v;
            assert!((q.distance(p) - hit.distance).abs() < 1e-4);
        }
        assert!(bvh.closest_point(Vec3::new(0.0, 5.0, 0.0), 1.0, |_| true).is_none(), "outside max distance");
    }

    #[test]
    fn ray_hits_sphere_at_radius() {
        let mesh = primitives::uv_sphere(1.0, 64, 32);
        let bvh = Bvh::build(&mesh);
        let h = bvh.intersect(Vec3::new(0.0, 0.0, 5.0), -Vec3::Z, 0.0, 100.0).unwrap();
        assert!((h.t - 4.0).abs() < 0.01, "t={}", h.t);
        assert!(h.front_face);
        assert!(bvh.occluded(Vec3::new(0.0, 0.0, 5.0), -Vec3::Z, 0.0, 100.0));
        assert!(!bvh.occluded(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, 0.0, 100.0));
    }

    #[test]
    fn sphere_query_is_conservative() {
        let mesh = primitives::uv_sphere(1.0, 32, 16);
        let bvh = Bvh::build(&mesh);
        let c = Vec3::new(0.0, 0.0, 1.0);
        let found = bvh.query_sphere(c, 0.3);
        assert!(!found.is_empty());
        for t in 0..mesh.triangle_count() {
            let p = mesh.tri_positions(t);
            if p.iter().any(|v| v.distance(c) < 0.3) {
                assert!(found.contains(&(t as u32)), "triangle {t} missing");
            }
        }
    }

    #[test]
    fn collapsed_hidden_faces_do_not_enter_queries_and_visible_triangle_ids_stay_stable() {
        let mut tris = vec![[Vec3::ZERO; 3]; 10000];
        tris.push([Vec3::new(-1.0,-1.0,0.0),Vec3::new(1.0,-1.0,0.0),Vec3::new(0.0,1.0,0.0)]);
        let bvh = Bvh::from_triangles(&tris); let mut visited = Vec::new();
        bvh.query(|_| true, |t| visited.push(t)); assert_eq!(visited, [10000]);
        assert_eq!(bvh.intersect(Vec3::Z, -Vec3::Z, 0.0, 10.0).unwrap().tri, 10000);
        let empty = Bvh::from_triangles(&tris[..10000]);
        assert!(empty.query_sphere(Vec3::ZERO, 1.0).is_empty());
        assert!(empty.intersect(Vec3::Z, -Vec3::Z, 0.0, 10.0).is_none());
    }

    #[test]
    fn filtered_queries_skip_rejected_triangles_behind_them() {
        // Two stacked triangles under a downward ray: 0 at z = 1 (closer), 1 at z = 0.
        let tri = |z: f32| [Vec3::new(-1.0, -1.0, z), Vec3::new(1.0, -1.0, z), Vec3::new(0.0, 1.0, z)];
        let bvh = Bvh::from_triangles(&[tri(1.0), tri(0.0)]);
        let (o, d) = (Vec3::new(0.0, 0.0, 5.0), -Vec3::Z);
        assert_eq!(bvh.intersect(o, d, 0.0, 10.0).unwrap().tri, 0);
        assert_eq!(bvh.intersect_filtered(o, d, 0.0, 10.0, |t| t == 1).unwrap().tri, 1);
        assert!(bvh.occluded_filtered(o, d, 0.0, 10.0, |t| t == 1));
        assert!(!bvh.occluded_filtered(o, d, 0.0, 10.0, |_| false));
        assert!(!bvh.occluded_filtered(o, d, 0.0, 4.5, |t| t == 1), "accepted triangle is beyond t_max");
    }
}
