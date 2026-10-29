//! Surface-constrained distance to convex creases. Exact on convex planar patches; conservative
//! vertex-graph paths across bends/non-convex patches. Never a UV dilation or a scene-space
//! nearest edge: disconnected objects and the back of a thin shell cannot short-circuit paths.
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use glam::{Vec2, Vec3};
use subcraft_mesh::{Bvh, Mesh};

#[derive(Debug, Clone, Copy)]
struct Segment(Vec3, Vec3);

impl Segment {
    fn distance(self, p: Vec3) -> f32 {
        let ab = self.1 - self.0;
        let t = ((p - self.0).dot(ab) / ab.length_squared().max(1e-20)).clamp(0.0, 1.0);
        p.distance(self.0 + ab * t)
    }
}

struct Union(Vec<usize>);
impl Union {
    fn root(&mut self, mut i: usize) -> usize {
        while self.0[i] != i { self.0[i] = self.0[self.0[i]]; i = self.0[i]; }
        i
    }
    fn join(&mut self, a: usize, b: usize) {
        let a = self.root(a); let b = self.root(b); self.0[b] = a;
    }
}

#[derive(Clone, Copy)]
struct Visit(f32, usize);
impl PartialEq for Visit { fn eq(&self, o: &Self) -> bool { self.0.to_bits() == o.0.to_bits() && self.1 == o.1 } }
impl Eq for Visit {}
impl PartialOrd for Visit { fn partial_cmp(&self, o: &Self) -> Option<Ordering> { Some(self.cmp(o)) } }
impl Ord for Visit {
    fn cmp(&self, o: &Self) -> Ordering { o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1)) }
}

#[derive(Debug)]
struct Patch { segments: Vec<Segment>, convex: bool }

#[derive(Debug)]
pub struct SurfaceEdges {
    positions: Vec<Vec3>,
    triangles: Vec<[usize; 3]>,
    distance: Vec<f32>,
    patch: Vec<usize>,
    patches: Vec<Patch>,
    local: Vec<Vec<Segment>>,
}

impl SurfaceEdges {
    pub fn new(mesh: &Mesh) -> Self {
        let extent = mesh.bounds.extent().max_element().max(1e-12);
        let normalize = |p: Vec3| (p - mesh.bounds.min) / extent;
        // Bound seam tolerance by the smallest real edge so thin connected shells cannot
        // collapse. Neighbor buckets handle values landing on opposite quantization borders.
        let min_edge = (0..mesh.triangle_count()).flat_map(|t| {
            let p = mesh.tri_positions(t); [p[0].distance(p[1]), p[1].distance(p[2]), p[2].distance(p[0])]
        }).filter(|d| *d > 0.0).fold(f32::INFINITY, f32::min) / extent;
        let epsilon = (min_edge * 0.01).clamp(1e-12, 1e-6);
        let mut welded = HashMap::<(Option<u32>, [i64; 3]), Vec<usize>>::new();
        let mut positions: Vec<Vec3> = Vec::new();
        let mut node_normals: Vec<Vec3> = Vec::new();
        let mut triangles = Vec::with_capacity(mesh.triangle_count());
        for t in 0..mesh.triangle_count() {
            let object = mesh.tri_object.get(t).copied().flatten();
            let normal = mesh.geometric_normal(t);
            triangles.push(mesh.tri(t).map(|v| {
                let p = normalize(mesh.positions[v as usize]);
                let cell = p.to_array().map(|x| (x / epsilon).floor() as i64);
                let mut best = None; let mut nearest = epsilon * epsilon;
                for z in -1..=1 { for y in -1..=1 { for x in -1..=1 {
                    if let Some(nodes) = welded.get(&(object, [cell[0] + x, cell[1] + y, cell[2] + z])) {
                        for &node in nodes {
                            let distance = p.distance_squared(positions[node]);
                            if distance <= nearest && node_normals[node].dot(normal) > -0.999999
                                && (distance < nearest || best.is_none_or(|old| node < old)) {
                                nearest = distance; best = Some(node);
                            }
                        }
                    }
                } } }
                best.unwrap_or_else(|| {
                    let node = positions.len(); positions.push(p); node_normals.push(normal);
                    welded.entry((object, cell)).or_default().push(node); node
                })
            }));
        }
        let normals: Vec<_> = (0..triangles.len()).map(|t| mesh.geometric_normal(t)).collect();
        let centers: Vec<_> = triangles.iter().map(|v| (positions[v[0]] + positions[v[1]] + positions[v[2]]) / 3.0).collect();
        let mut incident = BTreeMap::<(usize, usize), Vec<usize>>::new();
        for (t, v) in triangles.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (v[k].min(v[(k + 1) % 3]), v[k].max(v[(k + 1) % 3]));
                incident.entry((a, b)).or_default().push(t);
            }
        }
        let buried = Buried::new(mesh);
        let to_mesh = |p: Vec3| mesh.bounds.min + p * extent;
        let mut union = Union((0..triangles.len()).collect());
        let mut sources = Vec::new();
        let mut adjacency = vec![Vec::new(); positions.len()];
        for (&(a, b), faces) in &incident {
            let length = positions[a].distance(positions[b]);
            adjacency[a].push((b, length)); adjacency[b].push((a, length));
            if let [t, u] = faces.as_slice() {
                let cosine = normals[*t].dot(normals[*u]);
                if cosine > 0.99999 { union.join(*t, *u); }
                // Ignore tessellation of smooth surfaces; retain actual convex creases >20°.
                if cosine < 0.9396926 && (normals[*u] - normals[*t]).dot(centers[*u] - centers[*t]) > 1e-7 {
                    sources.push((a, b, *t, *u));
                }
            }
        }
        let patch: Vec<_> = (0..triangles.len()).map(|t| union.root(t)).collect();
        let mut patches: Vec<_> = (0..triangles.len()).map(|_| Patch { segments: Vec::new(), convex: false }).collect();
        let mut local = vec![Vec::new(); triangles.len()];
        let mut distance = vec![f32::INFINITY; positions.len()];
        let mut queue = BinaryHeap::new();
        for (a, b, t, u) in sources {
            // Only exposed crease spans are wear sources. Spans buried inside another part
            // (e.g. a box edge inside an overlapping post) must not start wear on visible faces.
            for segment in buried.as_ref().map_or_else(|| vec![Segment(positions[a], positions[b])],
                |buried| buried.exposed(Segment(positions[a], positions[b]), t, to_mesh)) {
                for face in [t, u] { patches[patch[face]].segments.push(segment); local[face].push(segment); }
                for node in [a, b] {
                    let d = segment.distance(positions[node]);
                    if d < distance[node] { distance[node] = d; queue.push(Visit(d, node)); }
                }
            }
        }
        let mut patch_points = vec![Vec::new(); triangles.len()];
        let mut patch_area = vec![0.0; triangles.len()];
        for (t, v) in triangles.iter().enumerate() {
            let [a, b, c] = v.map(|i| positions[i]);
            let (x, y) = normals[patch[t]].any_orthonormal_pair();
            patch_points[patch[t]].extend([a, b, c].map(|p| Vec2::new(p.dot(x), p.dot(y))));
            patch_area[patch[t]] += (b - a).cross(c - a).length() * 0.5;
        }
        for i in 0..patches.len() {
            if !patches[i].segments.is_empty() {
                let hull = hull_area(&mut patch_points[i]);
                patches[i].convex = (hull - patch_area[i]).abs() < hull.max(1e-12) * 1e-4;
            }
        }
        while let Some(Visit(d, node)) = queue.pop() {
            if d > distance[node] { continue; }
            for &(next, length) in &adjacency[node] {
                let candidate = d + length;
                if candidate < distance[next] { distance[next] = candidate; queue.push(Visit(candidate, next)); }
            }
        }
        Self { positions, triangles, distance, patch, patches, local }
    }

    /// Position must use the mesh's normalized object-space frame, matching TileGeometry.
    pub fn distance(&self, triangle: usize, point: Vec3) -> f32 {
        let patch = &self.patches[self.patch[triangle]];
        let mut d = f32::INFINITY;
        // A straight segment stays on a convex planar patch. Otherwise use only this triangle
        // and actual graph paths, avoiding shortcuts across holes/concave silhouettes.
        let sources = if patch.convex { &patch.segments } else { &self.local[triangle] };
        for &segment in sources { d = d.min(segment.distance(point)); }
        for &v in &self.triangles[triangle] { d = d.min(point.distance(self.positions[v]) + self.distance[v]); }
        d
    }
}

/// Inside test against the other separate parts of a mesh (objects/connected elements).
struct Buried { bvh: Bvh, group: Vec<usize>, bounds: Vec<(Vec3, Vec3)> }

impl Buried {
    /// `None` for a single part: nothing can bury its edges, so skip all ray work.
    fn new(mesh: &Mesh) -> Option<Self> {
        let elements = subcraft_mesh::topology::mesh_elements(mesh);
        let mut ids = HashMap::new();
        let mut bounds: Vec<(Vec3, Vec3)> = Vec::new();
        let group = (0..mesh.triangle_count()).map(|t| {
            let next = ids.len();
            let g = *ids.entry((elements[t], mesh.tri_object.get(t).copied().flatten())).or_insert(next);
            if g == bounds.len() { bounds.push((Vec3::INFINITY, Vec3::NEG_INFINITY)); }
            for p in mesh.tri_positions(t) { bounds[g] = (bounds[g].0.min(p), bounds[g].1.max(p)); }
            g
        }).collect();
        (bounds.len() > 1).then(|| Self { bvh: Bvh::build(mesh), group, bounds })
    }

    /// Inside one other part when every probe ray first meets that part's back side. Parts are
    /// tested separately: overlapping solids (a band passing through a post) must not hide each
    /// other's walls. Open sheets and coplanar contacts let a ray escape, so they stay exposed.
    fn inside(&self, p: Vec3, triangle: usize) -> bool {
        const PROBES: [Vec3; 6] = [Vec3::new(1.0, 0.013, 0.007), Vec3::new(-1.0, 0.011, -0.017), Vec3::new(0.019, 1.0, 0.005),
            Vec3::new(-0.007, -1.0, 0.013), Vec3::new(0.005, -0.017, 1.0), Vec3::new(0.011, 0.007, -1.0)];
        let own = self.group[triangle];
        self.bounds.iter().enumerate().any(|(g, &(min, max))| g != own && p.cmpge(min).all() && p.cmple(max).all()
            && PROBES.iter().all(|d| self.bvh.intersect_filtered(p, d.normalize(), 0.0, f32::INFINITY, |t| self.group[t as usize] == g)
                .is_some_and(|hit| !hit.front_face)))
    }

    /// Exposed spans of a crease segment (normalized frame), tested at <=1% of the extent.
    fn exposed(&self, segment: Segment, triangle: usize, to_mesh: impl Fn(Vec3) -> Vec3) -> Vec<Segment> {
        let pieces = (segment.0.distance(segment.1) / 0.01).ceil().clamp(1.0, 16.0) as usize;
        let at = |i: usize| segment.0.lerp(segment.1, i as f32 / pieces as f32);
        let mut out: Vec<Segment> = Vec::new();
        let (mut open, mut all) = (false, true);
        for i in 0..pieces {
            if self.inside(to_mesh((at(i) + at(i + 1)) * 0.5), triangle) { open = false; all = false; continue; }
            match out.last_mut() { Some(last) if open => last.1 = at(i + 1), _ => out.push(Segment(at(i), at(i + 1))) }
            open = true;
        }
        // Fully exposed creases keep their exact endpoints (exact planar distances).
        if all { vec![segment] } else { out }
    }
}

fn hull_area(points: &mut Vec<Vec2>) -> f32 {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 { return 0.0; }
    let mut hull: Vec<Vec2> = Vec::new();
    for &p in points.iter() {
        while hull.len() >= 2 && (hull[hull.len() - 1] - hull[hull.len() - 2]).perp_dot(p - hull[hull.len() - 1]) <= 0.0 { hull.pop(); }
        hull.push(p);
    }
    let lower = hull.len();
    for &p in points.iter().rev().skip(1) {
        while hull.len() > lower && (hull[hull.len() - 1] - hull[hull.len() - 2]).perp_dot(p - hull[hull.len() - 1]) <= 0.0 { hull.pop(); }
        hull.push(p);
    }
    hull.windows(2).map(|p| p[0].perp_dot(p[1])).sum::<f32>().abs() * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_mesh::{Corner, MeshBuilder, primitives};

    #[test]
    fn convex_planar_patch_distances_are_exact_across_subdivisions_and_uv_seams() {
        for subdivisions in [1, 4, 13] {
            let mesh = primitives::atlas_box(Vec3::splat(2.0), subdivisions, "Distance golden");
            let field = SurfaceEdges::new(&mesh);
            for t in 0..mesh.triangle_count() {
                let [a, b, c] = mesh.tri_positions(t);
                for bary in [[0.2, 0.3, 0.5], [1.0, 0.0, 0.0], [0.0, 0.5, 0.5]] {
                    let p = a * bary[0] + b * bary[1] + c * bary[2];
                    let normal = mesh.geometric_normal(t).abs();
                    let expected = [0, 1, 2].into_iter().filter(|&axis| normal[axis] < 0.5)
                        .map(|axis| (1.0 - p[axis].abs()) * 0.5).fold(f32::INFINITY, f32::min);
                    let got = field.distance(t, (p + Vec3::ONE) * 0.5);
                    assert!((got - expected).abs() < 2e-6, "subdivision {subdivisions}, triangle {t}, {p:?}: {got} != {expected}");
                }
            }
        }
    }

    #[test]
    fn thin_surfaces_and_disconnected_coincident_objects_do_not_leak_sources() {
        let slab = primitives::thin_slab(2.0, 2.0, 0.0002);
        let field = SurfaceEdges::new(&slab);
        for t in 0..slab.triangle_count() {
            if slab.geometric_normal(t).z > 0.99 {
                let [a, b, c] = slab.tri_positions(t);
                let p = (a + b + c) / 3.0;
                let expected = (1.0 - p.x.abs().max(p.y.abs())) * 0.5;
                assert!((field.distance(t, (p - slab.bounds.min) / 2.0) - expected).abs() < 2e-6);
            }
        }
        let mut builder = MeshBuilder::new("Coincident separate objects"); let material = builder.material("M");
        let mut plane_start = 0;
        for (name, source) in [("Cube", primitives::cube(2.0)), ("Plane", primitives::plane(2.0, 2))] {
            builder.object(name);
            if name == "Plane" { plane_start = builder.triangle_count(); }
            for t in 0..source.triangle_count() {
                let p = source.tri_positions(t); let uv = source.tri_uvs(t);
                builder.push_triangle(material, [0, 1, 2].map(|i| Corner { position: p[i], uv: Some(uv[i]), normal: Some(source.geometric_normal(t)) }));
            }
        }
        let mesh = builder.build().unwrap(); let field = SurfaceEdges::new(&mesh);
        for t in plane_start..mesh.triangle_count() {
            let [a, b, c] = mesh.tri_positions(t);
            assert!(field.distance(t, ((a + b + c) / 3.0 + Vec3::ONE) * 0.5).is_infinite());
        }
    }

    #[test]
    fn crease_spans_buried_inside_other_parts_are_not_wear_sources() {
        // Default crate: the body's vertical edges lie entirely inside the corner posts, and the
        // ends of its top edges run into them. Only exposed spans may start wear.
        let mesh = primitives::sample_crate();
        let field = SurfaceEdges::new(&mesh);
        let extent = mesh.bounds.extent().max_element();
        let body = mesh.objects.iter().position(|o| o.name == "Body").unwrap() as u32;
        let mut checked = 0;
        for t in 0..mesh.triangle_count() {
            if mesh.tri_object[t] != Some(body) { continue; }
            let [a, b, c] = mesh.tri_positions(t);
            for bary in [[0.2, 0.3, 0.5], [0.6, 0.2, 0.2], [1.0 / 3.0; 3]] {
                let p = a * bary[0] + b * bary[1] + c * bary[2];
                let n = mesh.geometric_normal(t);
                // Body side faces (normal ±X or ±Z): nearest exposed crease is the top/bottom rim.
                if n.y.abs() > 0.5 { continue; }
                let expected = (0.5 - p.y.abs()) / extent;
                let got = field.distance(t, (p - mesh.bounds.min) / extent);
                assert!(got >= expected - 1e-4, "{p:?}: buried vertical edge leaks wear ({got} < {expected})");
                assert!(got <= expected + 0.2 / extent, "{p:?}: exposed rim lost ({got} vs {expected})");
                checked += 1;
            }
        }
        assert!(checked > 50);
    }

    #[test]
    fn hull_test_rejects_concave_patches_and_holes() {
        let mut points = vec![Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y, Vec2::splat(0.5)];
        assert!((hull_area(&mut points) - 1.0).abs() < 1e-6);
        // Area missing from the convex hull (a cutout or a hole) must reject direct shortcuts.
        assert!((hull_area(&mut points) - 0.75).abs() > 0.01);
    }
}
