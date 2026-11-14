//! Polygon-fill selection: which triangles a click or a box picks, and how a pick grows to the
//! fill mode's element (polygon, mesh element, UV island, object).

use glam::{Vec2, Vec3};
use rayon::prelude::*;
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::topology::{PolygonTopology, WeldedEdge};
use subcraft_mesh::{Bvh, Mesh};

use crate::camera::Camera;
use crate::settings::PolygonFillMode;

/// Most rays a visible-only box casts over its area (a coarse grid; triangles the grid steps
/// over are still found through their own visibility ray).
const MAX_GRID_RAYS: usize = 160 * 160;

/// Whether the 2D triangle `t` overlaps the rectangle `[min, max]` (separating axis test).
pub fn triangle_overlaps_rect(t: [Vec2; 3], min: Vec2, max: Vec2) -> bool {
    // A triangle without area covers nothing. Hidden parts are kept in the mesh as triangles
    // collapsed onto one vertex: they passed every test below (all three edges are skipped), so a
    // box drawn over that vertex selected hidden parts and seeded fills with them.
    if (t[1] - t[0]).perp_dot(t[2] - t[0]).abs() < 1e-12 {
        return false;
    }
    let tmin = t[0].min(t[1]).min(t[2]);
    let tmax = t[0].max(t[1]).max(t[2]);
    if tmax.x < min.x || tmin.x > max.x || tmax.y < min.y || tmin.y > max.y {
        return false;
    }
    let corners = [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)];
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let n = (b - a).perp();
        if n.length_squared() < 1e-20 {
            continue;
        }
        let third = t[(k + 2) % 3];
        let side = n.dot(third - a);
        // Every rectangle corner strictly on the far side of this edge: separated.
        if corners.iter().all(|c| n.dot(*c - a) * side < 0.0) {
            return false;
        }
    }
    true
}

/// The triangle clipped to the rectangle (Sutherland-Hodgman); empty when they don't overlap.
fn clip_to_rect(t: [Vec2; 3], min: Vec2, max: Vec2) -> Vec<Vec2> {
    let mut poly: Vec<Vec2> = t.to_vec();
    // (axis, bound, keep_greater)
    for (axis, bound, greater) in [(0, min.x, true), (0, max.x, false), (1, min.y, true), (1, max.y, false)] {
        if poly.is_empty() {
            break;
        }
        let inside = |p: Vec2| if greater { p[axis] >= bound } else { p[axis] <= bound };
        let mut out = Vec::with_capacity(poly.len() + 2);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (ia, ib) = (inside(a), inside(b));
            if ia {
                out.push(a);
            }
            if ia != ib {
                let s = (bound - a[axis]) / (b[axis] - a[axis]);
                out.push(a + (b - a) * s);
            }
        }
        poly = out;
    }
    poly
}

/// Triangles of material `slot` whose screen footprint overlaps the box `[min, max]` (pixels).
/// `visible_only` keeps those the camera actually sees there: a ray through the triangle's
/// part inside the box must hit it first (no back faces, nothing occluded), plus every triangle a
/// grid of rays over the box hits on their front side. Triangles crossing behind the eye are skipped.
pub fn triangles_in_screen_box(mesh: &Mesh, bvh: &Bvh, camera: &Camera, slot: u32, min: Vec2, max: Vec2, visible_only: bool) -> Vec<u32> {
    let (min, max) = (min.min(max), min.max(max));
    let eye = camera.eye();
    let ortho = camera.is_orthographic();
    let forward = camera.forward();
    let range = mesh.material_triangles(slot);
    let mut picked: Vec<u32> = (range.start as u32..range.end as u32)
        .into_par_iter()
        .filter(|&t| {
            let p = mesh.tri_positions(t as usize);
            let s = [camera.project(p[0]), camera.project(p[1]), camera.project(p[2])];
            let [Some(a), Some(b), Some(c)] = s else { return false };
            let tri2 = [a.truncate(), b.truncate(), c.truncate()];
            if !triangle_overlaps_rect(tri2, min, max) {
                return false;
            }
            if !visible_only {
                return true;
            }
            // Back faces are never visible.
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            let view = if ortho { forward } else { (p[0] + p[1] + p[2]) / 3.0 - eye };
            if n.dot(view) >= 0.0 {
                return false;
            }
            let clipped = clip_to_rect(tri2, min, max);
            if clipped.is_empty() {
                return false;
            }
            let probe = clipped.iter().copied().sum::<Vec2>() / clipped.len() as f32;
            let (o, d) = camera.ray(probe);
            bvh.intersect(o, d, 0.0, f32::INFINITY).is_some_and(|h| h.tri == t || same_surface(mesh, h.tri, t, o + d * h.t))
        })
        .collect();
    if visible_only {
        let size = (max - min).max(Vec2::ONE);
        let step = ((size.x * size.y / MAX_GRID_RAYS as f32).sqrt()).max(1.0);
        let (nx, ny) = ((size.x / step).ceil() as usize + 1, (size.y / step).ceil() as usize + 1);
        let hits: Vec<u32> = (0..nx * ny)
            .into_par_iter()
            .filter_map(|i| {
                let p = Vec2::new((min.x + (i % nx) as f32 * step).min(max.x), (min.y + (i / nx) as f32 * step).min(max.y));
                let (o, d) = camera.ray(p);
                bvh.intersect(o, d, 0.0, f32::INFINITY).filter(|h| h.front_face).map(|h| h.tri).filter(|&t| mesh.tri_material[t as usize] == slot)
            })
            .collect();
        picked.extend(hits);
        picked.sort_unstable();
        picked.dedup();
    }
    picked
}

/// A probe that hit triangle `hit` at `point` still counts for `t` when the point lies on `t`'s
/// plane (the two triangles of a flat quad meet at the probe).
fn same_surface(mesh: &Mesh, hit: u32, t: u32, point: Vec3) -> bool {
    let p = mesh.tri_positions(t as usize);
    let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
    let scale = (p[1] - p[0]).length().max((p[2] - p[0]).length()).max(1e-6);
    hit != t && (point - p[0]).dot(n).abs() < scale * 1e-4 && mesh.geometric_normal(hit as usize).dot(n) > 0.9999
}

/// Triangles of material `slot` whose UV footprint (wrapped into its UDIM tile) overlaps the UV
/// box `[min, max]`.
pub fn triangles_in_uv_box(mesh: &Mesh, slot: u32, min: Vec2, max: Vec2) -> Vec<u32> {
    let (min, max) = (min.min(max), min.max(max));
    let range = mesh.material_triangles(slot);
    (range.start as u32..range.end as u32)
        .into_par_iter()
        .filter(|&t| {
            let (q, _) = subcraft_mesh::uv_raster::wrap_to_tile(mesh.tri_uvs(t as usize));
            triangle_overlaps_rect(q, min, max)
        })
        .collect()
}

/// The edge of polygon `p` that a drag from `a` towards `b` (pixels) leaves through on screen;
/// a click (no drag) takes the edge nearest `a`. `None` when no edge is in front of the camera.
pub fn loop_edge(topology: &PolygonTopology, camera: &Camera, p: u32, a: Vec2, b: Vec2) -> Option<WeldedEdge> {
    let screen = |w: u32| camera.project(topology.positions[w as usize]).map(|v| v.truncate());
    let segments: Vec<(WeldedEdge, Vec2, Vec2)> = topology.edges.get(p as usize)?.iter().filter_map(|&e| Some((e, screen(e.0)?, screen(e.1)?))).collect();
    let d = b - a;
    if d.length() >= 3.0 {
        // The first edge the ray from `a` along the drag crosses.
        let mut best: Option<(f32, WeldedEdge)> = None;
        for &(e, p0, p1) in &segments {
            let s = p1 - p0;
            let denom = d.perp_dot(s);
            if denom.abs() < 1e-9 {
                continue;
            }
            let t = (p0 - a).perp_dot(s) / denom;
            let u = (p0 - a).perp_dot(d) / denom;
            if t > 0.0 && (0.0..=1.0).contains(&u) && best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, e));
            }
        }
        if let Some((_, e)) = best {
            return Some(e);
        }
        // `a` outside the projected polygon: the edge lying most in the drag direction.
        let dir = d.normalize_or_zero();
        let along = |s: &(WeldedEdge, Vec2, Vec2)| ((s.1 + s.2) * 0.5 - a).normalize_or_zero().dot(dir);
        return segments.iter().max_by(|x, y| along(x).total_cmp(&along(y))).map(|s| s.0);
    }
    let distance = |s: &(WeldedEdge, Vec2, Vec2)| {
        let ab = s.2 - s.1;
        let t = if ab.length_squared() > 0.0 { ((a - s.1).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        a.distance(s.1 + ab * t)
    };
    segments.iter().min_by(|x, y| distance(x).total_cmp(&distance(y))).map(|s| s.0)
}

/// Triangles of the polygon loop a drag from `a` to `b` (pixels) picks: the polygon under `a`,
/// then the loop through it in the drag direction ([`loop_edge`]); `partial` stops at the polygon
/// under `b` (and `both_sides` adds as many on the other side of the start). Only polygons of
/// material `geometry.material_slot` take part. Sorted, unique; empty when `a` misses the set.
pub fn loop_triangles(mesh: &Mesh, bvh: &Bvh, geometry: &TextureSetGeometry, camera: &Camera, a: Vec2, b: Vec2, partial: bool, both_sides: bool) -> Vec<u32> {
    let slot = geometry.material_slot;
    let hit = |p: Vec2| {
        let (o, d) = camera.ray(p);
        bvh.intersect(o, d, 0.0, f32::INFINITY).map(|h| h.tri).filter(|&t| mesh.tri_material.get(t as usize) == Some(&slot))
    };
    let Some(start_tri) = hit(a) else { return Vec::new() };
    let topology = geometry.polygon_topology();
    let start = topology.polygon_of[start_tri as usize];
    let accept = |p: u32| topology.triangles[p as usize].first().is_some_and(|&t| mesh.tri_material[t as usize] == slot);
    let polygons = match loop_edge(topology, camera, start, a, b) {
        None => vec![start],
        Some(edge) if partial => match hit(b) {
            Some(end) => topology.partial(start, edge, topology.polygon_of[end as usize], both_sides, accept),
            None => vec![start],
        },
        Some(edge) => topology.ring(start, edge, accept),
    };
    let mut tris: Vec<u32> = polygons.iter().flat_map(|&p| topology.triangles[p as usize].iter().copied()).collect();
    tris.sort_unstable();
    tris.dedup();
    tris
}

/// Group id of every triangle for `mode` (`None` for single triangles). Objects without object
/// data are one group.
fn groups<'a>(mesh: &'a Mesh, geometry: &'a TextureSetGeometry, mode: PolygonFillMode) -> Option<Box<dyn Fn(usize) -> u32 + Sync + 'a>> {
    match mode {
        PolygonFillMode::Triangle => None,
        PolygonFillMode::Polygon => { let g = geometry.triangle_polygons(); Some(Box::new(move |t| g[t])) }
        PolygonFillMode::MeshElement => { let g = geometry.triangle_parts(); Some(Box::new(move |t| g[t])) }
        PolygonFillMode::UvIsland => { let g = geometry.triangle_islands(); Some(Box::new(move |t| g[t])) }
        PolygonFillMode::Object => Some(Box::new(move |t| mesh.tri_object.get(t).copied().flatten().unwrap_or(u32::MAX))),
    }
}

/// `seeds` grown to whole elements of `mode`, restricted to material `geometry.material_slot`;
/// sorted, unique.
pub fn grow(mesh: &Mesh, geometry: &TextureSetGeometry, seeds: &[u32], mode: PolygonFillMode) -> Vec<u32> {
    let slot = geometry.material_slot;
    let mut seeds: Vec<u32> = seeds.iter().copied().filter(|&t| mesh.tri_material.get(t as usize) == Some(&slot)).collect();
    seeds.sort_unstable();
    seeds.dedup();
    let Some(group) = groups(mesh, geometry, mode) else { return seeds };
    if seeds.is_empty() {
        return seeds;
    }
    let ids: std::collections::HashSet<u32> = seeds.iter().map(|&t| group(t as usize)).collect();
    mesh.material_triangles(slot).filter(|&t| ids.contains(&group(t))).map(|t| t as u32).collect()
}
