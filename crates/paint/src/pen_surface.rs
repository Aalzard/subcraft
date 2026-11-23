//! Stamped pen paths laid on the model's surface instead of in UV space
//! ([`subcraft_doc::PathStamp::surface`]). The path's curve is taken to 3D; each stamp is a
//! square of the tip lying on the surface at its centre, painted into every texel of the
//! texture set near it. So stitches keep their size where the UV scale changes, and continue
//! over UV seams: a segment whose curve leaves its UV island (its two points are on different
//! islands) runs straight over the surface between its two points.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_doc::PenPath;
use subcraft_core::IRect;
use subcraft_image::{Bitmap, TILE_SIZE, TILE_TEXELS};
use subcraft_mesh::uv_raster::{bary_point, raster_triangle, triangle_rect, wrap_to_tile};
use subcraft_mesh::{Bvh, Mesh};

use crate::kernel::KERNEL_EXPAND;

/// Most stamps one path may place.
const MAX_STAMPS: usize = 200_000;
/// Curve samples per path segment.
const STEPS: usize = 64;
/// A stamp paints surfaces facing within 60 degrees of its own.
const FACING: f32 = 0.5;

#[derive(Clone, Copy, Debug)]
struct SurfaceStamp {
    center: Vec3,
    normal: Vec3,
    /// Along the path.
    tangent: Vec3,
    /// To the right of the path, seen from outside the surface (the tip image's down).
    side: Vec3,
}

/// A point of the path on the surface.
#[derive(Clone, Copy, Debug)]
struct OnSurface {
    position: Vec3,
    normal: Vec3,
    tri: u32,
}

/// The stamps of a surface path and the tiles they touch.
pub struct SurfaceStamps {
    mesh: Arc<Mesh>,
    resolution: u32,
    /// Stamp radius in object units.
    radius: f32,
    stamps: Vec<SurfaceStamp>,
    /// Tile -> triangles to rasterize and stamps to test.
    bins: BTreeMap<usize, (BTreeSet<u32>, BTreeSet<u32>)>,
}

/// Finds the triangle of a texture set under a UV position.
struct UvLocator<'a> {
    mesh: &'a Mesh,
    cells: Vec<Vec<u32>>,
}

const GRID: usize = 128;

impl<'a> UvLocator<'a> {
    fn new(mesh: &'a Mesh, slot: u32) -> Self {
        let mut cells = vec![Vec::new(); GRID * GRID];
        for t in 0..mesh.triangle_count() {
            if mesh.tri_material[t] != slot {
                continue;
            }
            let (uv, _) = wrap_to_tile(mesh.tri_uvs(t));
            let lo = (uv[0].min(uv[1]).min(uv[2]) * GRID as f32).floor().max(Vec2::ZERO);
            let hi = (uv[0].max(uv[1]).max(uv[2]) * GRID as f32).floor().min(Vec2::splat(GRID as f32 - 1.0));
            if !(lo.is_finite() && hi.is_finite()) {
                continue;
            }
            for y in lo.y as usize..=hi.y as usize {
                for x in lo.x as usize..=hi.x as usize {
                    cells[y * GRID + x].push(t as u32);
                }
            }
        }
        Self { mesh, cells }
    }

    fn locate(&self, uv: Vec2) -> Option<OnSurface> {
        if !(uv.is_finite() && uv.x >= 0.0 && uv.y >= 0.0 && uv.x <= 1.0 && uv.y <= 1.0) {
            return None;
        }
        let cell = (uv * GRID as f32).floor().min(Vec2::splat(GRID as f32 - 1.0));
        for &t in &self.cells[cell.y as usize * GRID + cell.x as usize] {
            let (p, _) = wrap_to_tile(self.mesh.tri_uvs(t as usize));
            let area = (p[1] - p[0]).perp_dot(p[2] - p[0]);
            if area.abs() < 1e-14 {
                continue;
            }
            let u = (uv - p[0]).perp_dot(p[2] - p[0]) / area;
            let v = (p[1] - p[0]).perp_dot(uv - p[0]) / area;
            if u >= -1e-4 && v >= -1e-4 && u + v <= 1.0 + 1e-4 {
                let sp = self.mesh.interpolate(t as usize, u, v);
                return Some(OnSurface { position: sp.position, normal: surface_normal(&sp), tri: t });
            }
        }
        None
    }
}

fn surface_normal(sp: &subcraft_mesh::SurfacePoint) -> Vec3 {
    if sp.normal.length_squared() > 0.5 { sp.normal } else { sp.geometric_normal }
}

/// UV units per object unit on a triangle (0 for a degenerate one).
fn uv_per_unit(mesh: &Mesh, t: u32) -> f32 {
    let p = mesh.tri_positions(t as usize);
    let uv = mesh.tri_uvs(t as usize);
    let world = (p[1] - p[0]).cross(p[2] - p[0]).length();
    let area = (uv[1] - uv[0]).perp_dot(uv[2] - uv[0]).abs();
    if world > 1e-12 { (area / world).sqrt() } else { 0.0 }
}

/// Append the surface route from `a` (already in `out`) to `b`: `b` itself when it is within
/// `gap`, otherwise through the surface point under their middle, on both halves.
fn bridge(a: OnSurface, b: OnSurface, gap: f32, depth: u32, project: &impl Fn(Vec3) -> Option<OnSurface>, budget: &mut usize, out: &mut Vec<OnSurface>) {
    let distance = a.position.distance(b.position);
    if distance > gap && depth > 0 && *budget > 0
        && let Some(m) = project((a.position + b.position) * 0.5)
        // The middle has to make progress, or the two sides cannot be joined (a hole).
        && m.position.distance(a.position) > distance * 0.05 && m.position.distance(b.position) > distance * 0.05
    {
        *budget -= 1;
        bridge(a, m, gap, depth - 1, project, budget, out);
        bridge(m, b, gap, depth - 1, project, budget, out);
        return;
    }
    out.push(b);
}

impl SurfaceStamps {
    /// The surface stamps of `path` on the triangles of material `slot`; `None` when the path
    /// is not a stamped surface path or none of it lies on the texture set.
    pub fn build(path: &PenPath, mesh: &Arc<Mesh>, bvh: &Bvh, slot: u32, resolution: u32) -> Result<Option<Self>, &'static str> {
        let Some(stamp) = path.stamp.filter(|s| s.surface && path.kind == subcraft_doc::PathKind::Paint) else { return Ok(None) };
        let locator = UvLocator::new(mesh, slot);
        // The object-space radius, from the UV scale under the first point on the surface.
        let first = path.anchors.iter().find_map(|a| locator.locate(a.position));
        let Some(first) = first else { return Ok(None) };
        let scale = uv_per_unit(mesh, first.tri);
        if scale <= 0.0 {
            return Ok(None);
        }
        let radius = (path.radius / scale).max(1e-9);
        let bezier = |c: &[Vec2; 4], t: f32| {
            let u = 1.0 - t;
            c[0] * (u * u * u) + c[1] * (3.0 * u * u * t) + c[2] * (3.0 * u * t * t) + c[3] * (t * t * t)
        };
        // Runs of surface points; a run ends where the path leaves the texture set.
        let mut runs: Vec<Vec<OnSurface>> = vec![Vec::new()];
        let segments = path.segment_count();
        if segments == 0 {
            runs[0].push(first);
        }
        for i in 0..segments {
            let Some(c) = path.segment(i) else { continue };
            let uvs: Vec<Vec2> = (0..=STEPS).map(|k| bezier(&c, k as f32 / STEPS as f32)).collect();
            let points: Vec<Option<OnSurface>> = uvs.iter().map(|&uv| locator.locate(uv)).collect();
            // On one island the curve's 3D steps match its UV steps; a gap or a jump means the
            // curve left the island.
            let continuous = points.iter().all(Option::is_some)
                && points.windows(2).zip(uvs.windows(2)).all(|(p, uv)| {
                    let (a, b) = (p[0].unwrap(), p[1].unwrap());
                    let expected = (uv[1] - uv[0]).length() / uv_per_unit(mesh, a.tri).max(1e-12);
                    a.position.distance(b.position) <= expected * 6.0 + radius * 0.05
                });
            let run = runs.last_mut().unwrap();
            if continuous {
                run.extend(points.into_iter().flatten().skip(usize::from(!run.is_empty())));
                continue;
            }
            // Straight over the surface between the segment's two points.
            let (Some(a), Some(b)) = (points[0], points[STEPS]) else {
                if !run.is_empty() {
                    runs.push(Vec::new());
                }
                continue;
            };
            let length = a.position.distance(b.position);
            let count = ((length / (radius * 0.5)).ceil() as usize).clamp(8, 4096);
            if run.is_empty() {
                run.push(a);
            }
            let project = |guess: Vec3| {
                bvh.closest_point(guess, length.max(radius), |_| true).map(|hit| {
                    let sp = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                    OnSurface { position: sp.position, normal: surface_normal(&sp), tri: hit.tri }
                })
            };
            // A coarse route: the chord between the two points pulled onto the surface, then
            // tightened like a string (each point moves to the surface under the middle of its
            // neighbours), which takes it close to the shortest way over the surface.
            let coarse = count.min(24);
            let mut route: Vec<OnSurface> = (0..=coarse)
                .map(|k| match k {
                    0 => a,
                    k if k == coarse => b,
                    k => project(a.position.lerp(b.position, k as f32 / coarse as f32)).unwrap_or(a),
                })
                .collect();
            for _ in 0..48 {
                for k in 1..coarse {
                    if let Some(m) = project((route[k - 1].position + route[k + 1].position) * 0.5) {
                        route[k] = m;
                    }
                }
            }
            // Between the coarse points the route is made whole: where two neighbours are far
            // apart (around an edge), the surface point under their middle joins them.
            let mut budget = 16384usize;
            for w in route.windows(2) {
                bridge(w[0], w[1], radius * 0.5, 12, &project, &mut budget, run);
            }
        }
        // Stamps along the runs, by distance on the surface. The spacing is fitted to each run:
        // a whole number of steps from its first point to its last, so the stitches end on the
        // last point and a closed path joins without a short or long last gap.
        let step = (stamp.spacing * 2.0 * radius).max(radius * 0.02);
        let closed = path.is_closed() && runs.len() == 1;
        let mut stamps: Vec<SurfaceStamp> = Vec::new();
        for run in runs.iter().filter(|r| !r.is_empty()) {
            let place = |at: Vec3, normal: Vec3, dir: Vec3, stamps: &mut Vec<SurfaceStamp>| {
                let mut tangent = dir - normal * dir.dot(normal);
                if tangent.length_squared() < 1e-12 {
                    tangent = normal.any_orthonormal_vector();
                }
                let tangent = tangent.normalize();
                let (tangent, side) = if stamp.follow { (tangent, tangent.cross(normal)) } else {
                    // Upright: the tip keeps the surface's own frame.
                    let t = normal.any_orthonormal_vector();
                    (t, t.cross(normal))
                };
                stamps.push(SurfaceStamp { center: at, normal, tangent, side });
            };
            let length: f32 = run.windows(2).map(|w| w[0].position.distance(w[1].position)).sum();
            if run.len() == 1 || length <= 1e-12 {
                place(run[0].position, run[0].normal, Vec3::X, &mut stamps);
                continue;
            }
            let steps = ((length / step).round() as usize).max(1);
            if steps > MAX_STAMPS || stamps.len() + steps > MAX_STAMPS {
                return Err("too many stamps on this path: raise the spacing or the width");
            }
            let fitted = length / steps as f32;
            // A closed run's last point is its first: no stamp there twice.
            let count = if closed { steps } else { steps + 1 };
            let (mut k, mut walked) = (0usize, 0.0f32);
            for w in run.windows(2) {
                let d = w[1].position - w[0].position;
                let len = d.length();
                if len <= 1e-12 {
                    continue;
                }
                while k < count && k as f32 * fitted <= walked + len + length * 1e-5 {
                    let t = ((k as f32 * fitted - walked) / len).clamp(0.0, 1.0);
                    // Onto the surface: between two route points around an edge the straight
                    // step cuts the corner.
                    let (mut at, mut normal) = (w[0].position + d * t, w[0].normal.lerp(w[1].normal, t).normalize_or(w[0].normal));
                    if let Some(hit) = bvh.closest_point(at, radius, |_| true) {
                        let sp = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                        (at, normal) = (sp.position, surface_normal(&sp));
                    }
                    place(at, normal, d / len, &mut stamps);
                    k += 1;
                }
                walked += len;
            }
        }
        // Alternating offset: every second stamp moves across the path.
        if stamp.alternate != 0.0 {
            for s in stamps.iter_mut().skip(1).step_by(2) {
                s.center += s.side * (stamp.alternate * radius);
            }
        }
        if stamps.is_empty() {
            return Ok(None);
        }
        // Bin: the triangles of the texture set near each stamp, and the tiles its footprint
        // can reach on each of them.
        let reach = radius * std::f32::consts::SQRT_2;
        let res = resolution as f32;
        let across = resolution.div_ceil(TILE_SIZE) as i32;
        let mut bins: BTreeMap<usize, (BTreeSet<u32>, BTreeSet<u32>)> = BTreeMap::new();
        for (si, s) in stamps.iter().enumerate() {
            for t in bvh.query_sphere(s.center, reach) {
                if mesh.tri_material[t as usize] != slot || mesh.geometric_normal(t as usize).dot(s.normal) < FACING {
                    continue;
                }
                let p = mesh.tri_positions(t as usize);
                let (uv, _) = wrap_to_tile(mesh.tri_uvs(t as usize));
                let (e1, e2) = (p[1] - p[0], p[2] - p[0]);
                let n = e1.cross(e2);
                let n2 = n.length_squared();
                let whole = triangle_rect(mesh.tri_uvs(t as usize), resolution, resolution, KERNEL_EXPAND);
                let rect = if n2 <= 1e-20 { whole } else {
                    // The affine object -> UV map of the triangle bounds the stamp's image.
                    let (d1, d2) = (e2.cross(n) / n2, n.cross(e1) / n2);
                    let u = d1 * (uv[1].x - uv[0].x) + d2 * (uv[2].x - uv[0].x);
                    let v = d1 * (uv[1].y - uv[0].y) + d2 * (uv[2].y - uv[0].y);
                    let delta = s.center - p[0];
                    let c = (uv[0] + Vec2::new(u.dot(delta), v.dot(delta))) * res;
                    let r = Vec2::new(u.length(), v.length()) * reach * res + Vec2::splat(KERNEL_EXPAND + 1.0);
                    IRect::new((c.x - r.x).floor() as i32, (c.y - r.y).floor() as i32, (c.x + r.x).ceil() as i32, (c.y + r.y).ceil() as i32).intersect(&whole)
                };
                if rect.is_empty() {
                    continue;
                }
                let ts = TILE_SIZE as i32;
                for ty in rect.y0 / ts..=(rect.y1 - 1) / ts {
                    for tx in rect.x0 / ts..=(rect.x1 - 1) / ts {
                        let bin = bins.entry((ty * across + tx) as usize).or_default();
                        bin.0.insert(t);
                        bin.1.insert(si as u32);
                    }
                }
            }
        }
        Ok(Some(Self { mesh: mesh.clone(), resolution, radius, stamps, bins }))
    }

    /// Tiles the stamps can touch, in order.
    pub fn tiles(&self) -> Vec<usize> {
        self.bins.keys().copied().collect()
    }

    pub fn stamp_count(&self) -> usize {
        self.stamps.len()
    }

    /// Coverage of one tile (`TILE_TEXELS` values): each texel takes the strongest stamp over it.
    pub fn coverage(&self, tile: usize, alpha: Option<&Bitmap>, hardness: f32) -> Vec<f32> {
        let mut coverage = vec![0.0f32; TILE_TEXELS];
        let Some((tris, stamps)) = self.bins.get(&tile) else { return coverage };
        let res = self.resolution;
        let across = res.div_ceil(TILE_SIZE) as usize;
        let (x0, y0) = ((tile % across) as i32 * TILE_SIZE as i32, (tile / across) as i32 * TILE_SIZE as i32);
        let clip = IRect::new(x0, y0, x0 + TILE_SIZE as i32, y0 + TILE_SIZE as i32);
        let near: Vec<&SurfaceStamp> = stamps.iter().map(|&i| &self.stamps[i as usize]).collect();
        let r = self.radius;
        for &t in tris {
            let pos = self.mesh.tri_positions(t as usize);
            let ng = self.mesh.geometric_normal(t as usize);
            raster_triangle(self.mesh.tri_uvs(t as usize), res, res, clip, KERNEL_EXPAND, |h| {
                let p = bary_point(&pos, h.bary);
                let mut best = 0.0f32;
                for s in &near {
                    if ng.dot(s.normal) < FACING {
                        continue;
                    }
                    let d = p - s.center;
                    if d.dot(s.normal).abs() > r {
                        continue;
                    }
                    let local = Vec2::new(d.dot(s.tangent), d.dot(s.side)) / r;
                    if local.length_squared() > 2.0 {
                        continue;
                    }
                    best = best.max(crate::stamp::tip_footprint(alpha, hardness, 0.0, 1.0, local));
                }
                let slot = &mut coverage[((h.y - y0) as u32 * TILE_SIZE + (h.x - x0) as u32) as usize];
                if best > *slot {
                    *slot = best;
                }
            });
        }
        coverage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_core::{ChannelKind, ChannelValue};
    use subcraft_doc::{PathKind, PathStamp, PenAnchor};

    fn path(points: &[Vec2], radius: f32, surface: bool) -> PenPath {
        let values = [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 1.0, 1.0))].into_iter().collect();
        let stamp = Some(PathStamp { alpha: None, spacing: 1.0, follow: true, alternate: 0.0, surface });
        PenPath { anchors: points.iter().map(|&p| PenAnchor::new(p)).collect(), closed: false, radius, hardness: 1.0, opacity: 1.0, values, kind: PathKind::Paint, stamp, ribbon: None }
    }

    /// UV of the cube's surface point under a ray.
    fn uv_at(mesh: &Mesh, bvh: &Bvh, from: Vec3, to: Vec3) -> Vec2 {
        let hit = bvh.intersect(from, (to - from).normalize(), 0.0, 100.0).unwrap();
        mesh.interpolate(hit.tri as usize, hit.u, hit.v).uv
    }

    #[test]
    fn a_path_over_two_faces_of_a_cube_is_one_unbroken_line_of_stamps() {
        let mesh = Arc::new(subcraft_mesh::primitives::cube(2.0));
        let bvh = Bvh::build(&mesh);
        // One point on the front face (+z), one on the top face (+y): two UV islands.
        let front = uv_at(&mesh, &bvh, Vec3::new(0.0, -0.4, 5.0), Vec3::new(0.0, -0.4, 0.0));
        let top = uv_at(&mesh, &bvh, Vec3::new(0.0, 5.0, -0.4), Vec3::new(0.0, 0.0, -0.4));
        let p = path(&[front, top], 0.01, true);
        let s = SurfaceStamps::build(&p, &mesh, &bvh, 0, 256).unwrap().expect("a surface path");
        // The way over the edge is 1.4 up the front face and 1.4 back on the top face.
        let step = 2.0 * s.radius;
        let length = 2.8;
        let expected = (length / step) as usize;
        assert!(s.stamps.len().abs_diff(expected) <= expected / 8 + 2, "{} stamps for a route of {length} at {step}", s.stamps.len());
        for w in s.stamps.windows(2) {
            let d = w[0].center.distance(w[1].center);
            assert!(d <= step * 1.05 && d >= step * 0.6, "neighbours {d} apart at a step of {step}");
        }
        // Every stamp lies on the cube, on the front face first and on the top face at the end.
        for st in &s.stamps {
            let on_front = (st.center.z - 1.0).abs() < 1e-3;
            let on_top = (st.center.y - 1.0).abs() < 1e-3;
            assert!(on_front || on_top, "{:?} is on neither face", st.center);
            assert!(st.tangent.dot(st.normal).abs() < 1e-3 && (st.side.length() - 1.0).abs() < 1e-3);
        }
        assert!((s.stamps[0].center.z - 1.0).abs() < 1e-3 && (s.stamps.last().unwrap().center.y - 1.0).abs() < 1e-3);
        // Both faces are painted: texels under a stamp on each side are covered.
        let covered = |at: Vec3, n: Vec3| {
            let uv = uv_at(&mesh, &bvh, at + n * 3.0, at);
            let (x, y) = ((uv.x * 256.0) as u32, (uv.y * 256.0) as u32);
            let tile = (y / TILE_SIZE * 256u32.div_ceil(TILE_SIZE) + x / TILE_SIZE) as usize;
            s.coverage(tile, None, 1.0)[((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize]
        };
        let (first, last) = (s.stamps[1], s.stamps[s.stamps.len() - 2]);
        assert!(covered(first.center, Vec3::Z) > 0.9 && covered(last.center, Vec3::Y) > 0.9);
        // Away from the path nothing is painted.
        assert_eq!(covered(Vec3::new(0.8, 0.0, 1.0), Vec3::Z), 0.0);
    }

    #[test]
    fn on_one_island_the_surface_path_follows_the_uv_curve_at_the_same_size() {
        // A 2 x 2 plane over the whole UV square: 1 UV unit is 2 object units.
        let mesh = Arc::new(subcraft_mesh::primitives::plane(2.0, 4));
        let bvh = Bvh::build(&mesh);
        let p = path(&[Vec2::new(0.2, 0.5), Vec2::new(0.8, 0.5)], 0.02, true);
        let s = SurfaceStamps::build(&p, &mesh, &bvh, 0, 256).unwrap().unwrap();
        assert!((s.radius - 0.04).abs() < 1e-4, "radius {} in object units", s.radius);
        // 0.6 UV long at a step of 0.04 UV: 16 stamps (15 steps and the first).
        assert_eq!(s.stamp_count(), 16);
        let total: f32 = s.tiles().iter().map(|&t| s.coverage(t, None, 1.0).iter().sum::<f32>()).sum();
        // 16 discs of radius 0.02 x 256 texels.
        let discs = 16.0 * std::f32::consts::PI * (0.02f32 * 256.0).powi(2);
        assert!((total - discs).abs() < discs * 0.1, "{total} covered texels for {discs}");
        // Not a surface path, or off the texture set: nothing to lay on the surface.
        assert!(SurfaceStamps::build(&path(&[Vec2::new(0.2, 0.5), Vec2::new(0.8, 0.5)], 0.02, false), &mesh, &bvh, 0, 256).unwrap().is_none());
        assert!(SurfaceStamps::build(&p, &mesh, &bvh, 7, 256).unwrap().is_none());
    }
    /// The spacing is fitted to the path: an open path ends with a stitch on its last point,
    /// and a closed path's last gap is the same as every other.
    #[test]
    fn stamps_are_spread_evenly_from_the_first_point_to_the_last() {
        let mesh = Arc::new(subcraft_mesh::primitives::plane(2.0, 4));
        let bvh = Bvh::build(&mesh);
        // 0.63 UV = 1.26 units at a step of 0.08: 15.75 steps, fitted to 16 of 0.07875.
        let open = path(&[Vec2::new(0.2, 0.5), Vec2::new(0.83, 0.5)], 0.02, true);
        let s = SurfaceStamps::build(&open, &mesh, &bvh, 0, 256).unwrap().unwrap();
        assert_eq!(s.stamp_count(), 17);
        let (first, last) = (s.stamps[0].center, s.stamps[16].center);
        assert!((first.distance(last) - 1.26).abs() < 1e-3, "from the first point to the last: {}", first.distance(last));
        for w in s.stamps.windows(2) {
            assert!((w[0].center.distance(w[1].center) - 0.07875).abs() < 1e-3);
        }
        // A closed triangle: every gap the same, the one from the last stamp back to the first too.
        let mut closed = path(&[Vec2::new(0.2, 0.2), Vec2::new(0.8, 0.2), Vec2::new(0.5, 0.75)], 0.02, true);
        closed.closed = true;
        let s = SurfaceStamps::build(&closed, &mesh, &bvh, 0, 256).unwrap().unwrap();
        let n = s.stamps.len();
        // Gaps along a side are the step; across a corner the chord is shorter, never longer.
        let gaps: Vec<f32> = (0..n).map(|k| s.stamps[k].center.distance(s.stamps[(k + 1) % n].center)).collect();
        let step = gaps.iter().copied().fold(0.0f32, f32::max);
        assert!((step - 0.08).abs() < 0.004, "fitted step {step}");
        assert!(gaps[n - 1] > step * 0.6 && gaps[n - 1] <= step + 1e-4, "the closing gap {} at a step of {step}", gaps[n - 1]);
        assert!(s.stamps[0].center.distance(s.stamps[n - 1].center) > step * 0.6, "no stamp twice on the first point");
    }
}
