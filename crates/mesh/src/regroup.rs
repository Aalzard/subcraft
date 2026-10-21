//! Re-assigning triangles to material slots (texture sets) and reading UDIM tile layouts.
//!
//! A texture set is one material slot. Splitting a model by object, merging it into one set, or
//! giving each UDIM tile its own set is therefore a re-grouping of triangles into slots. UDIM
//! tiles are moved into the 0..1 square of their own slot (their vertices are duplicated where two
//! tiles shared one), so painting, baking and export never see more than one tile per set.

use std::collections::{BTreeMap, HashMap};

use glam::Vec2;

use crate::mesh::{Aabb, MaterialSlot, Mesh, Submesh};

/// A UDIM tile in stored UV space: `col = floor(u)`, `row = floor(v)`. Stored UVs have their origin
/// at the top-left (v grows downward), so file-style UDIM rows run the other way; see [`udim_number`].
pub type Tile = [i32; 2];

/// The 1001-style UDIM number of a tile: 1001 + column + 10 * row, rows counted upward from the
/// bottom of the image like every DCC tool does. Tiles left of or below the origin (not valid UDIM)
/// get numbers under 1001 (e.g. 1000, 991); they stay distinct tiles, see [`is_valid_udim`].
pub fn udim_number(tile: Tile) -> u32 {
    (1001 + tile[0] - 10 * tile[1]).max(0) as u32
}

/// Whether the tile is inside the UDIM range: u >= 0 and v >= 0 in image terms, at most 10 wide.
pub fn is_valid_udim(tile: Tile) -> bool {
    (0..10).contains(&tile[0]) && tile[1] <= 0
}

impl Mesh {
    /// The tile holding the centroid of triangle `t`.
    pub fn triangle_tile(&self, t: usize) -> Tile {
        let uv = self.tri_uvs(t);
        let c = (uv[0] + uv[1] + uv[2]) / 3.0;
        [c.x.floor() as i32, c.y.floor() as i32]
    }

    /// Triangle count per UDIM tile, over triangles of `slot` (or all triangles for `None`).
    pub fn udim_tiles(&self, slot: Option<u32>) -> BTreeMap<Tile, usize> {
        let mut out = BTreeMap::new();
        for t in 0..self.triangle_count() {
            if slot.is_none_or(|s| self.tri_material[t] == s) {
                *out.entry(self.triangle_tile(t)).or_insert(0) += 1;
            }
        }
        out
    }

    /// Smallest and largest UV over all vertices (`None` without vertices).
    pub fn uv_bounds(&self) -> Option<(Vec2, Vec2)> {
        let first = *self.uvs.first()?;
        Some(self.uvs.iter().fold((first, first), |(lo, hi), &uv| (lo.min(uv), hi.max(uv))))
    }

    /// A triangle whose corners lie in more than one tile cannot be painted as UDIM.
    pub fn triangles_crossing_tiles(&self) -> usize {
        // Corners exactly on a tile edge belong to either side; only a real crossing counts.
        const EDGE: f32 = 1e-4;
        (0..self.triangle_count())
            .filter(|&t| {
                let uv = self.tri_uvs(t);
                let lo = uv.iter().fold(Vec2::splat(f32::INFINITY), |a, p| a.min(*p));
                let hi = uv.iter().fold(Vec2::splat(f32::NEG_INFINITY), |a, p| a.max(*p));
                (hi.x - EDGE).floor() != (lo.x + EDGE).floor() || (hi.y - EDGE).floor() != (lo.y + EDGE).floor()
            })
            .count()
    }

    /// Rebuild the mesh with every triangle in the slot `slot_of_tri[t]` (indices into `names`).
    /// `uv_shift[slot]` is subtracted from the UVs of that slot (UDIM tiles move to 0..1). Slots
    /// without triangles are dropped; triangles are grouped by slot so each is one draw range.
    /// Returns the new mesh and `kept`: for each new slot, its index in `names`.
    pub fn regroup(&self, names: &[String], slot_of_tri: &[u32], uv_shift: &[Vec2]) -> (Mesh, Vec<u32>) {
        assert_eq!(slot_of_tri.len(), self.triangle_count());
        let mut order: Vec<usize> = (0..self.triangle_count()).collect();
        order.sort_by_key(|&t| slot_of_tri[t]);
        let mut kept: Vec<u32> = Vec::new();
        let mut new_slot_of: HashMap<u32, u32> = HashMap::new();
        for &t in &order {
            let s = slot_of_tri[t];
            new_slot_of.entry(s).or_insert_with(|| {
                kept.push(s);
                (kept.len() - 1) as u32
            });
        }
        let mut mesh = Mesh {
            name: self.name.clone(),
            objects: self.objects.clone(),
            materials: kept.iter().map(|&s| MaterialSlot { name: names[s as usize].clone() }).collect(),
            ..Default::default()
        };
        let mut lookup: HashMap<(u32, u32), u32> = HashMap::new();
        let has_objects = !self.tri_object.is_empty();
        for &t in &order {
            let new_slot = new_slot_of[&slot_of_tri[t]];
            let shift = uv_shift.get(slot_of_tri[t] as usize).copied().unwrap_or(Vec2::ZERO);
            for corner in self.tri(t) {
                let idx = *lookup.entry((corner, new_slot)).or_insert_with(|| {
                    let i = corner as usize;
                    mesh.positions.push(self.positions[i]);
                    mesh.normals.push(self.normals[i]);
                    mesh.uvs.push(self.uvs[i] - shift);
                    mesh.tangents.push(self.tangents[i]);
                    (mesh.positions.len() - 1) as u32
                });
                mesh.indices.push(idx);
            }
            mesh.tri_material.push(new_slot);
            if has_objects {
                mesh.tri_object.push(self.tri_object[t]);
            }
        }
        let mut start = 0usize;
        let total = mesh.tri_material.len();
        while start < total {
            let m = mesh.tri_material[start];
            let mut end = start;
            while end < total && mesh.tri_material[end] == m {
                end += 1;
            }
            mesh.submeshes.push(Submesh { material: m, first_triangle: start as u32, triangle_count: (end - start) as u32 });
            start = end;
        }
        mesh.bounds = Aabb::from_points(mesh.positions.iter().copied());
        (mesh, kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn udim_numbers_follow_the_1001_convention() {
        assert_eq!(udim_number([0, 0]), 1001);
        assert_eq!(udim_number([1, 0]), 1002);
        assert_eq!(udim_number([9, 0]), 1010);
        // One tile up in the image is a negative stored row (v grows downward).
        assert_eq!(udim_number([0, -1]), 1011);
        assert_eq!(udim_number([1, -1]), 1012);
    }

    #[test]
    fn regroup_moves_triangles_between_slots_and_keeps_the_mesh_valid() {
        let cube = primitives::cube(2.0);
        assert_eq!(cube.materials.len(), 1);
        let n = cube.triangle_count();
        let names = vec!["Even".to_string(), "Odd".to_string()];
        let slot_of: Vec<u32> = (0..n).map(|t| (t % 2) as u32).collect();
        let (mesh, kept) = cube.regroup(&names, &slot_of, &[]);
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
        assert_eq!(kept, vec![0, 1]);
        assert_eq!(mesh.materials.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Even", "Odd"]);
        assert_eq!(mesh.triangle_count(), n);
        assert_eq!(mesh.material_triangles(0).len() + mesh.material_triangles(1).len(), n);
        // Slots without triangles disappear.
        let (single, kept) = cube.regroup(&["A".into(), "B".into()], &vec![1; n], &[]);
        assert_eq!((single.materials.len(), kept), (1, vec![1]));
        assert_eq!(single.materials[0].name, "B");
    }

    #[test]
    fn tiles_are_found_split_and_shifted_into_the_unit_square() {
        // Two quads: one in tile (0,0), one shifted a tile right and one tile "up" in the image.
        let plane = primitives::plane(2.0, 1);
        let mut two = plane.clone();
        let (mut offset_verts, mut idx) = (Vec::new(), Vec::new());
        for i in 0..two.positions.len() {
            offset_verts.push((two.positions[i], two.normals[i], two.uvs[i] + Vec2::new(1.0, -1.0), two.tangents[i]));
        }
        let base = two.positions.len() as u32;
        for i in &plane.indices {
            idx.push(i + base);
        }
        for (p, n, uv, t) in offset_verts {
            two.positions.push(p);
            two.normals.push(n);
            two.uvs.push(uv);
            two.tangents.push(t);
        }
        two.indices.extend(idx);
        let tris = plane.triangle_count();
        two.tri_material.extend(std::iter::repeat_n(0, tris));
        two.tri_object.extend(plane.tri_object.clone());
        two.submeshes = vec![Submesh { material: 0, first_triangle: 0, triangle_count: (2 * tris) as u32 }];
        assert!(two.validate().is_empty(), "{:?}", two.validate());
        let tiles = two.udim_tiles(None);
        assert_eq!(tiles.keys().copied().collect::<Vec<_>>(), vec![[0, 0], [1, -1]]);
        assert_eq!(tiles.values().sum::<usize>(), 2 * tris);
        assert_eq!(two.triangles_crossing_tiles(), 0);
        assert_eq!(udim_number([1, -1]), 1012);
        // Split by tile: two slots, each back inside 0..1.
        let slot_of: Vec<u32> = (0..two.triangle_count()).map(|t| if two.triangle_tile(t) == [0, 0] { 0 } else { 1 }).collect();
        let (split, _) = two.regroup(&["T1001".into(), "T1012".into()], &slot_of, &[Vec2::ZERO, Vec2::new(1.0, -1.0)]);
        assert!(split.validate().is_empty());
        let (lo, hi) = split.uv_bounds().unwrap();
        assert!(lo.x >= -1e-6 && lo.y >= -1e-6 && hi.x <= 1.0 + 1e-6 && hi.y <= 1.0 + 1e-6, "{lo:?} {hi:?}");
        assert_eq!(split.udim_tiles(None).len(), 1, "every slot is inside tile 0,0 now");
        assert_eq!(split.material_triangles(0).len(), tris);
    }

    #[test]
    fn a_triangle_straddling_a_tile_border_is_reported() {
        let mut m = primitives::plane(2.0, 1);
        m.uvs[0] = Vec2::new(0.5, 0.2);
        m.uvs[1] = Vec2::new(1.6, 0.2);
        m.uvs[2] = Vec2::new(0.5, 0.8);
        assert!(m.triangles_crossing_tiles() >= 1);
    }
}
