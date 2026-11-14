//! Connectivity queries: welded positions, UV islands, mesh elements, UV wireframe edges.

use std::collections::HashMap;

use glam::Vec2;

use crate::mesh::Mesh;

/// Union-find with path halving.
struct Dsu(Vec<u32>);

impl Dsu {
    fn new(n: usize) -> Self {
        Self((0..n as u32).collect())
    }
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            let p = self.0[self.0[x as usize] as usize];
            self.0[x as usize] = p;
            x = p;
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra as usize] = rb;
        }
    }
}

/// Map each vertex to a welded position id (bit-exact positions share an id).
pub fn weld_positions(mesh: &Mesh) -> Vec<u32> {
    let mut map: HashMap<[u32; 3], u32> = HashMap::with_capacity(mesh.positions.len());
    mesh.positions
        .iter()
        .map(|p| {
            // `+ 0.0` turns -0.0 into 0.0, which is the same position with different bits.
            let k = [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()];
            let next = map.len() as u32;
            *map.entry(k).or_insert(next)
        })
        .collect()
}

fn components_by(mesh: &Mesh, vertex_key: &[u64]) -> Vec<u32> {
    let tri_count = mesh.triangle_count();
    let mut dsu = Dsu::new(tri_count);
    let mut first_tri: HashMap<u64, u32> = HashMap::new();
    for t in 0..tri_count {
        for v in mesh.tri(t) {
            let k = vertex_key[v as usize];
            match first_tri.get(&k) {
                Some(&o) => dsu.union(t as u32, o),
                None => {
                    first_tri.insert(k, t as u32);
                }
            }
        }
    }
    let mut ids: HashMap<u32, u32> = HashMap::new();
    (0..tri_count as u32)
        .map(|t| {
            let r = dsu.find(t);
            let next = ids.len() as u32;
            *ids.entry(r).or_insert(next)
        })
        .collect()
}

/// Connected mesh element per triangle (triangles sharing a welded vertex are connected).
pub fn mesh_elements(mesh: &Mesh) -> Vec<u32> {
    let weld = weld_positions(mesh);
    let keys: Vec<u64> = weld.iter().map(|&w| w as u64).collect();
    components_by(mesh, &keys)
}

/// UV island per triangle: triangles connected through vertices with identical position *and* UV.
pub fn uv_islands(mesh: &Mesh) -> Vec<u32> {
    let weld = weld_positions(mesh);
    let mut uv_ids: HashMap<(u32, [u32; 2]), u64> = HashMap::new();
    let keys: Vec<u64> = (0..mesh.vertex_count())
        .map(|v| {
            let k = (weld[v], [mesh.uvs[v].x.to_bits(), mesh.uvs[v].y.to_bits()]);
            let next = uv_ids.len() as u64;
            *uv_ids.entry(k).or_insert(next)
        })
        .collect();
    components_by(mesh, &keys)
}

/// Unique UV-space edges of the triangles in `tris` (for the 2D view wireframe).
pub fn uv_edges(mesh: &Mesh, tris: std::ops::Range<usize>) -> Vec<[Vec2; 2]> {
    let mut seen: HashMap<([u32; 2], [u32; 2]), ()> = HashMap::new();
    let mut out = Vec::new();
    for t in tris {
        let uv = mesh.tri_uvs(t);
        for k in 0..3 {
            let (a, b) = (uv[k], uv[(k + 1) % 3]);
            let ka = [a.x.to_bits(), a.y.to_bits()];
            let kb = [b.x.to_bits(), b.y.to_bits()];
            let key = if ka <= kb { (ka, kb) } else { (kb, ka) };
            if seen.insert(key, ()).is_none() {
                out.push([a, b]);
            }
        }
    }
    out
}

/// Recovered polygon diagonals (see [`polygon_edges`]): each `(t, diagonal)` joins triangle `t`
/// and `t + 1` into one polygon across the welded edge `diagonal`.
fn polygon_merges(mesh: &Mesh, weld: &[u32], max_angle_deg: f32) -> Vec<(usize, (u32, u32))> {
    let tri_count = mesh.triangle_count();
    let key = |a: u32, b: u32| if a <= b { (a, b) } else { (b, a) };
    let corners = |t: usize| mesh.tri(t).map(|v| weld[v as usize]);
    let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
    for t in 0..tri_count {
        let c = corners(t);
        for k in 0..3 {
            *uses.entry(key(c[k], c[(k + 1) % 3])).or_default() += 1;
        }
    }
    let cos_limit = max_angle_deg.to_radians().cos();
    let mut merges = Vec::new();
    let mut previous: Option<(usize, (u32, u32))> = None;
    for t in 0..tri_count.saturating_sub(1) {
        let (a, b) = (corners(t), corners(t + 1));
        let shared: Vec<u32> = a.iter().copied().filter(|v| b.contains(v)).collect();
        if shared.len() != 2 || shared[0] == shared[1] {
            continue;
        }
        let diagonal = key(shared[0], shared[1]);
        // A triangle already closed a polygon with its predecessor only continues it as a fan
        // (every diagonal meets at one corner); otherwise a border between two cells is not a diagonal.
        if let Some((pt, prev)) = previous
            && pt + 1 == t
            && ![prev.0, prev.1].iter().any(|v| *v == diagonal.0 || *v == diagonal.1)
        {
            continue;
        }
        let same_group = mesh.tri_material.get(t) == mesh.tri_material.get(t + 1) && mesh.tri_object.get(t) == mesh.tri_object.get(t + 1);
        if same_group && uses.get(&diagonal) == Some(&2) && mesh.geometric_normal(t).dot(mesh.geometric_normal(t + 1)) >= cos_limit {
            merges.push((t, diagonal));
            previous = Some((t, diagonal));
        }
    }
    merges
}

/// Polygon (quad / n-gon) id per triangle, recovered as in [`polygon_edges`]: the triangles an
/// importer made from one face share an id. Ids are dense, in first-triangle order.
pub fn polygons(mesh: &Mesh, max_angle_deg: f32) -> Vec<u32> {
    let weld = weld_positions(mesh);
    let tri_count = mesh.triangle_count();
    let mut dsu = Dsu::new(tri_count);
    for (t, _) in polygon_merges(mesh, &weld, max_angle_deg) {
        dsu.union(t as u32 + 1, t as u32);
    }
    let mut ids: HashMap<u32, u32> = HashMap::new();
    (0..tri_count as u32)
        .map(|t| {
            let r = dsu.find(t);
            let next = ids.len() as u32;
            *ids.entry(r).or_insert(next)
        })
        .collect()
}

/// Real polygon edges for the 3D view's wireframe overlay.
///
/// [`Mesh`] only stores triangles, so the polygons an importer triangulated (quads as two
/// triangles, n-gons as a fan around one corner) are recovered here. Consecutive triangles of the
/// same material and object that share an edge, and whose normals differ by less than
/// `max_angle_deg`, are one polygon, and the shared diagonal is not an edge. A chain of more than
/// two triangles only continues as a fan (all diagonals meet at one corner). A diagonal is only
/// dropped when exactly those two triangles share it. Edges are unique and welded by
/// position. Limit: a flat region that was authored as plain triangles in fan order looks like a
/// quad, because the file does not say which it was.
pub fn polygon_edges(mesh: &Mesh, max_angle_deg: f32) -> Vec<[glam::Vec3; 2]> {
    let weld = weld_positions(mesh);
    let key = |a: u32, b: u32| if a <= b { (a, b) } else { (b, a) };
    let corners = |t: usize| mesh.tri(t).map(|v| weld[v as usize]);
    let tri_count = mesh.triangle_count();
    let hidden: std::collections::HashSet<(u32, u32)> = polygon_merges(mesh, &weld, max_angle_deg).into_iter().map(|(_, d)| d).collect();
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for t in 0..tri_count {
        let c = corners(t);
        let v = mesh.tri(t);
        for k in 0..3 {
            let (i, j) = (k, (k + 1) % 3);
            let e = key(c[i], c[j]);
            if !hidden.contains(&e) && seen.insert(e) {
                out.push([mesh.positions[v[i] as usize], mesh.positions[v[j] as usize]]);
            }
        }
    }
    out
}

/// A welded edge: the two welded vertex ids, smaller first.
pub type WeldedEdge = (u32, u32);

/// Polygons (from [`polygons`]) as faces with welded boundary edges, for walking polygon loops
/// (Painter's Polygon loop fill).
#[derive(Clone, Debug, Default)]
pub struct PolygonTopology {
    /// Polygon of every triangle.
    pub polygon_of: Vec<u32>,
    /// Triangles of every polygon.
    pub triangles: Vec<Vec<u32>>,
    /// Boundary edges of every polygon (an internal diagonal is not one).
    pub edges: Vec<Vec<WeldedEdge>>,
    /// Position of every welded vertex.
    pub positions: Vec<glam::Vec3>,
    edge_polygons: HashMap<WeldedEdge, Vec<u32>>,
}

/// [`PolygonTopology`] of `mesh` with the polygon ids `polygon_of` (one per triangle).
pub fn polygon_topology(mesh: &Mesh, polygon_of: &[u32]) -> PolygonTopology {
    let weld = weld_positions(mesh);
    let count = polygon_of.iter().map(|&p| p as usize + 1).max().unwrap_or(0);
    let mut triangles = vec![Vec::new(); count];
    for (t, &p) in polygon_of.iter().enumerate() {
        triangles[p as usize].push(t as u32);
    }
    let mut positions = vec![glam::Vec3::ZERO; weld.iter().map(|&w| w as usize + 1).max().unwrap_or(0)];
    for (v, &w) in weld.iter().enumerate() {
        positions[w as usize] = mesh.positions[v];
    }
    let mut edges = Vec::with_capacity(count);
    let mut edge_polygons: HashMap<WeldedEdge, Vec<u32>> = HashMap::new();
    for (p, tris) in triangles.iter().enumerate() {
        // Edges used once inside the polygon are its boundary; a diagonal is used twice.
        let mut uses: Vec<(WeldedEdge, u32)> = Vec::new();
        for &t in tris {
            let c = mesh.tri(t as usize).map(|v| weld[v as usize]);
            for k in 0..3 {
                let (a, b) = (c[k], c[(k + 1) % 3]);
                if a == b {
                    continue;
                }
                let e = (a.min(b), a.max(b));
                match uses.iter_mut().find(|(f, _)| *f == e) {
                    Some(u) => u.1 += 1,
                    None => uses.push((e, 1)),
                }
            }
        }
        let boundary: Vec<WeldedEdge> = uses.into_iter().filter(|(_, n)| *n == 1).map(|(e, _)| e).collect();
        for &e in &boundary {
            edge_polygons.entry(e).or_default().push(p as u32);
        }
        edges.push(boundary);
    }
    PolygonTopology { polygon_of: polygon_of.to_vec(), triangles, edges, positions, edge_polygons }
}

impl PolygonTopology {
    pub fn polygon_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn is_quad(&self, p: u32) -> bool {
        self.edges.get(p as usize).is_some_and(|e| e.len() == 4)
    }

    /// The edge of quad `p` facing `e` (sharing no vertex with it); `None` for other polygons.
    pub fn opposite(&self, p: u32, e: WeldedEdge) -> Option<WeldedEdge> {
        if !self.is_quad(p) {
            return None;
        }
        self.edges[p as usize].iter().copied().find(|f| f.0 != e.0 && f.0 != e.1 && f.1 != e.0 && f.1 != e.1)
    }

    /// The polygon on the other side of `p`'s edge `e`, when exactly two polygons share it.
    pub fn across(&self, p: u32, e: WeldedEdge) -> Option<u32> {
        match self.edge_polygons.get(&e).map(Vec::as_slice) {
            Some([a, b]) if *a == p => Some(*b),
            Some([a, b]) if *b == p => Some(*a),
            _ => None,
        }
    }

    /// The polygons met leaving `p` through `e` and going on through each quad's opposite edge,
    /// in order (without `p`). It stops at a border, at a non-quad, at a polygon `accept`
    /// rejects or at one already met; `true` when it came back round to `p` (a closed loop).
    pub fn walk(&self, p: u32, e: WeldedEdge, accept: impl Fn(u32) -> bool) -> (Vec<u32>, bool) {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let (mut at, mut edge) = (p, e);
        while let Some(next) = self.across(at, edge) {
            if next == p {
                return (out, true);
            }
            if !accept(next) || !self.is_quad(next) || !seen.insert(next) {
                break;
            }
            out.push(next);
            let Some(o) = self.opposite(next, edge) else { break };
            at = next;
            edge = o;
        }
        (out, false)
    }

    /// The whole loop through `p` that crosses its edge `e`: forward through `e`, back through
    /// the opposite edge (when `p` is a quad). Ordered along the loop, `p` included.
    pub fn ring(&self, p: u32, e: WeldedEdge, accept: impl Fn(u32) -> bool + Copy) -> Vec<u32> {
        let (forward, closed) = self.walk(p, e, accept);
        let mut out = Vec::new();
        if !closed {
            if let Some(o) = self.opposite(p, e) {
                let (back, _) = self.walk(p, o, accept);
                out.extend(back.into_iter().rev().filter(|q| !forward.contains(q)));
            }
        }
        out.push(p);
        out.extend(forward);
        out
    }

    /// Part of the loop through `p` crossing `e`: from `p` to `end` the short way along the loop
    /// (just `p` when `end` is not on it). `both_sides` adds as many polygons on the other side of
    /// `p` (Painter's Alt while dragging).
    pub fn partial(&self, p: u32, e: WeldedEdge, end: u32, both_sides: bool, accept: impl Fn(u32) -> bool + Copy) -> Vec<u32> {
        let (forward, _) = self.walk(p, e, accept);
        let back = self.opposite(p, e).map(|o| self.walk(p, o, accept).0).unwrap_or_default();
        let (f, b) = (forward.iter().position(|&q| q == end), back.iter().position(|&q| q == end));
        let empty: &[u32] = &[];
        let (span, other): (&[u32], &[u32]) = match (f, b) {
            (Some(i), Some(j)) if j < i => (&back[..=j], &forward[..]),
            (Some(i), _) => (&forward[..=i], &back[..]),
            (None, Some(j)) => (&back[..=j], &forward[..]),
            (None, None) => (empty, empty),
        };
        let mut out = vec![p];
        out.extend_from_slice(span);
        if both_sides {
            out.extend(other.iter().take(span.len()).filter(|q| !span.contains(q) && **q != p));
        }
        out
    }
}

#[cfg(test)]
mod loop_tests {
    use super::*;
    use crate::primitives;

    fn fan_mesh(poly: &[glam::Vec3]) -> Mesh {
        let mut b = crate::MeshBuilder::new("poly");
        let m = b.material("m");
        let c = |p: glam::Vec3| crate::Corner { position: p, normal: Some(glam::Vec3::Z), uv: Some(Vec2::new(p.x, p.y)) };
        for i in 1..poly.len() - 1 {
            b.push_triangle(m, [c(poly[0]), c(poly[i]), c(poly[i + 1])]);
        }
        b.build().unwrap()
    }

    fn topology(mesh: &Mesh) -> PolygonTopology {
        polygon_topology(mesh, &polygons(mesh, 30.0))
    }

    /// Polygon whose centre is nearest `at`, and its edge whose midpoint is nearest `toward`.
    fn pick(mesh: &Mesh, t: &PolygonTopology, at: glam::Vec3, toward: glam::Vec3) -> (u32, WeldedEdge) {
        let centre = |p: u32| {
            let e = &t.edges[p as usize];
            e.iter().map(|&(a, b)| t.positions[a as usize] + t.positions[b as usize]).sum::<glam::Vec3>() / (2.0 * e.len() as f32)
        };
        let p = (0..t.polygon_count() as u32).min_by(|&a, &b| centre(a).distance(at).total_cmp(&centre(b).distance(at))).unwrap();
        let mid = |e: WeldedEdge| (t.positions[e.0 as usize] + t.positions[e.1 as usize]) * 0.5;
        let e = t.edges[p as usize].iter().copied().min_by(|&a, &b| mid(a).distance(toward).total_cmp(&mid(b).distance(toward))).unwrap();
        let _ = mesh;
        (p, e)
    }

    #[test]
    fn cube_loops_close_around_four_faces() {
        // 4 x 4 cells per face: a loop around the cube crosses 4 faces of 4 cells.
        let cube = primitives::cube(1.0);
        let t = topology(&cube);
        assert_eq!(t.polygon_count(), 96);
        assert!((0..96).all(|p| t.is_quad(p)));
        let top = glam::Vec3::new(0.125, 0.5, 0.125);
        let (p, e) = pick(&cube, &t, top, top + glam::Vec3::X);
        let ring = t.ring(p, e, |_| true);
        assert_eq!(ring.len(), 16, "{ring:?}");
        let mut unique = ring.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 16);
        let (forward, closed) = t.walk(p, e, |_| true);
        assert!(closed && forward.len() == 15);
        // The perpendicular loop is a different ring of 16 sharing only the start.
        let (p2, e2) = pick(&cube, &t, top, top + glam::Vec3::Z);
        assert_eq!(p2, p);
        let cross = t.ring(p, e2, |_| true);
        assert_eq!(cross.len(), 16);
        // They share the start and the cell right under it on the bottom face.
        assert_eq!(cross.iter().filter(|q| ring.contains(q)).count(), 2);
        // A polygon `accept` rejects ends the walk: an open loop on both sides of the start.
        let banned = ring[(ring.iter().position(|&q| q == p).unwrap() + 8) % 16];
        let cut = t.ring(p, e, |q| q != banned);
        assert_eq!(cut.len(), 15);
        assert!(!cut.contains(&banned));
    }

    #[test]
    fn plane_loops_stop_at_the_border_and_partial_loops_end_at_the_target() {
        let plane = primitives::plane(4.0, 4);
        let t = topology(&plane);
        assert_eq!(t.polygon_count(), 16);
        // Cell centres sit at -1.5, -0.5, 0.5, 1.5 on X and Z.
        let cell = |x: f32, z: f32| glam::Vec3::new(x, 0.0, z);
        let (p, e) = pick(&plane, &t, cell(-0.5, -1.5), cell(0.0, -1.5));
        let row = t.ring(p, e, |_| true);
        assert_eq!(row.len(), 4, "a row of 4 cells, open at both borders: {row:?}");
        // Partial: from the start to the cell two to the right; with Alt, one more on the left.
        let (end, _) = pick(&plane, &t, cell(1.5, -1.5), cell(1.5, -1.5));
        let span = t.partial(p, e, end, false, |_| true);
        assert_eq!(span.len(), 3);
        assert_eq!((span[0], span[2]), (p, end));
        let both = t.partial(p, e, end, true, |_| true);
        assert_eq!(both.len(), 4, "only one cell left of the start: {both:?}");
        let (left, _) = pick(&plane, &t, cell(-1.5, -1.5), cell(-1.5, -1.5));
        assert!(both.contains(&left));
        // A target off the loop: just the start.
        let (off, _) = pick(&plane, &t, cell(1.5, 1.5), cell(1.5, 1.5));
        assert_eq!(t.partial(p, e, off, true, |_| true), vec![p]);
        // Every quad has its opposite edge; a border edge has no polygon across.
        assert!(t.opposite(p, e).is_some());
        let (corner, border) = pick(&plane, &t, cell(-1.5, -1.5), cell(-2.0, -1.5));
        assert_eq!(t.across(corner, border), None);
    }

    #[test]
    fn non_quads_end_a_loop() {
        let q = fan_mesh(&[glam::Vec3::ZERO, glam::Vec3::new(2., 0., 0.), glam::Vec3::new(3., 1., 0.), glam::Vec3::new(1.5, 2., 0.), glam::Vec3::new(-1., 1., 0.)]);
        let t = topology(&q);
        assert_eq!(t.polygon_count(), 1);
        assert!(!t.is_quad(0) && t.edges[0].len() == 5);
        let e = t.edges[0][0];
        assert_eq!(t.opposite(0, e), None);
        assert_eq!(t.ring(0, e, |_| true), vec![0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn cube_has_six_islands_one_element() {
        let mesh = primitives::cube(1.0);
        let islands = uv_islands(&mesh);
        let n_islands = islands.iter().copied().max().unwrap() + 1;
        assert_eq!(n_islands, 6);
        let elems = mesh_elements(&mesh);
        assert!(elems.iter().all(|&e| e == 0));
    }

    #[test]
    fn crate_trim_has_separate_elements() {
        let mesh = primitives::sample_crate();
        let elems = mesh_elements(&mesh);
        let n = elems.iter().copied().max().unwrap() + 1;
        assert_eq!(n, 7, "body + 4 posts + 2 bands");
    }

    #[test]
    fn uv_edges_are_unique() {
        let mesh = primitives::plane(1.0, 2);
        let edges = uv_edges(&mesh, 0..mesh.triangle_count());
        // 2x2 grid: 12 axis edges + 4 diagonals.
        assert_eq!(edges.len(), 16);
    }

    fn fan_mesh(poly: &[glam::Vec3]) -> Mesh {
        let mut b = crate::MeshBuilder::new("poly");
        let m = b.material("m");
        let c = |p: glam::Vec3| crate::Corner { position: p, normal: Some(glam::Vec3::Z), uv: Some(Vec2::new(p.x, p.y)) };
        for i in 1..poly.len() - 1 {
            b.push_triangle(m, [c(poly[0]), c(poly[i]), c(poly[i + 1])]);
        }
        b.build().unwrap()
    }

    #[test]
    fn quad_has_four_edges_not_five() {
        use glam::Vec3;
        let q = fan_mesh(&[Vec3::ZERO, Vec3::X, Vec3::new(1., 1., 0.), Vec3::Y]);
        assert_eq!(q.triangle_count(), 2);
        assert_eq!(polygon_edges(&q, 30.0).len(), 4);
    }

    #[test]
    fn pentagon_fan_has_five_edges() {
        use glam::Vec3;
        let p = fan_mesh(&[Vec3::ZERO, Vec3::new(2., 0., 0.), Vec3::new(3., 1., 0.), Vec3::new(1.5, 2., 0.), Vec3::new(-1., 1., 0.)]);
        assert_eq!(p.triangle_count(), 3);
        assert_eq!(polygon_edges(&p, 30.0).len(), 5);
    }

    #[test]
    fn creased_pair_keeps_its_shared_edge() {
        use glam::Vec3;
        // Two triangles folded 90 degrees along the shared edge (0,0,0)-(1,1,0).
        let mut b = crate::MeshBuilder::new("fold");
        let m = b.material("m");
        let c = |p: Vec3| crate::Corner { position: p, normal: None, uv: Some(Vec2::ZERO) };
        b.push_triangle(m, [c(Vec3::ZERO), c(Vec3::X), c(Vec3::new(1., 1., 0.))]);
        b.push_triangle(m, [c(Vec3::ZERO), c(Vec3::new(1., 1., 0.)), c(Vec3::new(0., 1., 1.))]);
        let mesh = b.build().unwrap();
        assert_eq!(polygon_edges(&mesh, 30.0).len(), 5);
        assert_eq!(polygon_edges(&mesh, 120.0).len(), 4, "a loose angle limit merges them");
    }

    #[test]
    fn two_unrelated_triangles_share_nothing_hidden_and_edges_are_unique() {
        use glam::Vec3;
        let mut b = crate::MeshBuilder::new("two");
        let m = b.material("m");
        let c = |p: Vec3| crate::Corner { position: p, normal: None, uv: Some(Vec2::ZERO) };
        b.push_triangle(m, [c(Vec3::ZERO), c(Vec3::X), c(Vec3::Y)]);
        b.push_triangle(m, [c(Vec3::new(5., 0., 0.)), c(Vec3::new(6., 0., 0.)), c(Vec3::new(5., 1., 0.))]);
        assert_eq!(polygon_edges(&b.build().unwrap(), 30.0).len(), 6);
    }

    #[test]
    fn cube_primitive_loses_exactly_its_cell_diagonals() {
        // 6 faces x 4x4 cells. Triangle edges: 6*(40 grid + 16 diagonal) - 48 shared = 288.
        let mesh = primitives::cube(1.0);
        assert_eq!(polygon_edges(&mesh, 30.0).len(), 288 - 96);
    }

    #[test]
    fn polygons_group_the_triangles_of_each_face() {
        use glam::Vec3;
        let q = fan_mesh(&[Vec3::ZERO, Vec3::X, Vec3::new(1., 1., 0.), Vec3::Y]);
        assert_eq!(polygons(&q, 30.0), vec![0, 0], "a quad is one polygon");
        let p = fan_mesh(&[Vec3::ZERO, Vec3::new(2., 0., 0.), Vec3::new(3., 1., 0.), Vec3::new(1.5, 2., 0.), Vec3::new(-1., 1., 0.)]);
        assert_eq!(polygons(&p, 30.0), vec![0, 0, 0], "a pentagon fan is one polygon");
        // 6 faces x 16 quad cells: 96 polygons of two triangles each.
        let cube = primitives::cube(1.0);
        let ids = polygons(&cube, 30.0);
        let n = *ids.iter().max().unwrap() as usize + 1;
        assert_eq!(n, 96);
        let mut count = vec![0; n];
        for &i in &ids { count[i as usize] += 1; }
        assert!(count.iter().all(|&c| c == 2), "{count:?}");
    }
}
