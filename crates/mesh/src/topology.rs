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
}
