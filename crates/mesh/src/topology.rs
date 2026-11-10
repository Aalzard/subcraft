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
            let k = [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
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
