//! Border edges of a mesh part, for the outline the 3D view draws around the part a Ctrl + drop
//! will land on.

use std::collections::HashMap;

use glam::Vec3;

use crate::Session;
use crate::part_workflow::PartKey;

impl Session {
    /// Whether mesh triangle `t` belongs to part `key`.
    pub fn part_contains(&self, key: PartKey, t: usize) -> bool {
        self.scene().geometry.get(0, 1).triangle_region(t, key.kind) == Some(key.id)
    }

    /// Border edges of part `key` (object space): edges with only one of their triangles in the
    /// part. Vertices split at UV seams are matched by position, so seams inside do not show.
    pub fn part_outline(&self, key: PartKey) -> Vec<[Vec3; 2]> {
        let mesh = &self.scene().mesh;
        let geo = self.scene().geometry.get(0, 1);
        let bits = |p: Vec3| p.to_array().map(f32::to_bits);
        let mut edges: HashMap<([u32; 3], [u32; 3]), (u32, [Vec3; 2])> = HashMap::new();
        for t in 0..mesh.triangle_count() {
            if geo.triangle_region(t, key.kind) != Some(key.id) {
                continue;
            }
            let p = [0, 1, 2].map(|k| mesh.positions[mesh.indices[3 * t + k] as usize]);
            for (a, b) in [(p[0], p[1]), (p[1], p[2]), (p[2], p[0])] {
                let (ka, kb) = (bits(a), bits(b));
                let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
                edges.entry(k).or_insert((0, [a, b])).0 += 1;
            }
        }
        edges.into_values().filter(|(n, _)| *n == 1).map(|(_, e)| e).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_core::PartKind;

    #[test]
    fn a_closed_part_has_no_border_and_an_open_one_is_outlined() {
        let cube = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
        let id = cube.scene().geometry.get(0, 1).triangle_region(0, PartKind::Connected).expect("a part");
        let key = PartKey { kind: PartKind::Connected, id };
        assert!(cube.part_contains(key, 0));
        assert!(cube.part_outline(key).is_empty(), "a closed cube has no border edge");

        let plane = Session::new(subcraft_mesh::primitives::plane(2.0, 1), 64);
        let id = plane.scene().geometry.get(0, 1).triangle_region(0, PartKind::Connected).expect("a part");
        let outline = plane.part_outline(PartKey { kind: PartKind::Connected, id });
        assert_eq!(outline.len(), 4, "the square's rim: {outline:?}");
        assert!(outline.iter().flatten().all(|p| (p.x.abs() - 1.0).abs() < 1e-5 || (p.z.abs() - 1.0).abs() < 1e-5));
    }
}
