//! Manual UV edits: move, rotate, scale and flip whole UV islands (see [`crate::topology::uv_islands`]).
//!
//! The result is a new mesh with the edited corners, recomputed tangents and vertices split where
//! corners no longer share a UV. The painted work is moved onto it by the caller (the app's mesh
//! reload Reproject path), like Auto UV unwrap.

use std::collections::{BTreeSet, HashMap};

use glam::{Vec2, Vec3};

use crate::Mesh;

/// Where an island transform turns and scales about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UvPivot {
    /// The centre of the bounding box of every selected island together.
    #[default]
    Selection,
    /// Each island about the centre of its own bounding box.
    EachIsland,
}

/// One transform for the selected islands. Applied as scale (with flips), then rotation, about the
/// pivot, then the offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvTransform {
    /// Added to every UV after scaling and rotating (1.0 = one UDIM tile).
    pub offset: Vec2,
    /// Counter-clockwise in UV space (V up), in degrees.
    pub rotation_deg: f32,
    /// Per axis; negative values are not used (see `flip_u` / `flip_v`).
    pub scale: Vec2,
    pub flip_u: bool,
    pub flip_v: bool,
    pub pivot: UvPivot,
}

impl Default for UvTransform {
    fn default() -> Self {
        Self { offset: Vec2::ZERO, rotation_deg: 0.0, scale: Vec2::ONE, flip_u: false, flip_v: false, pivot: UvPivot::Selection }
    }
}

impl UvTransform {
    /// True when applying it would change nothing.
    pub fn is_identity(&self) -> bool {
        self.offset == Vec2::ZERO && self.rotation_deg.rem_euclid(360.0) == 0.0 && self.scale == Vec2::ONE && !self.flip_u && !self.flip_v
    }

    /// Where `uv` goes when turned about `pivot`.
    pub fn apply(&self, uv: Vec2, pivot: Vec2) -> Vec2 {
        let s = Vec2::new(if self.flip_u { -self.scale.x } else { self.scale.x }, if self.flip_v { -self.scale.y } else { self.scale.y });
        let (sin, cos) = self.rotation_deg.to_radians().sin_cos();
        let p = (uv - pivot) * s;
        pivot + Vec2::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos) + self.offset
    }

    /// True when the transform mirrors the islands (one flip, or a negative scale on one axis):
    /// their triangles then wind the other way in UV space.
    pub fn mirrors(&self) -> bool {
        (self.flip_u ^ self.flip_v) ^ ((self.scale.x < 0.0) ^ (self.scale.y < 0.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UvEditError {
    NothingSelected,
    /// `islands` does not have one entry per triangle.
    StaleIslands,
    /// The scale is zero or not finite: the islands would collapse.
    BadScale,
}

impl std::fmt::Display for UvEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            UvEditError::NothingSelected => "no UV island is selected",
            UvEditError::StaleIslands => "the island list does not match the mesh",
            UvEditError::BadScale => "the scale must be a positive number",
        })
    }
}

/// The pivot each triangle of the selection turns about (`None` for unselected triangles).
fn pivots(mesh: &Mesh, islands: &[u32], selected: &BTreeSet<u32>, pivot: UvPivot) -> Vec<Option<Vec2>> {
    let mut boxes: HashMap<u32, (Vec2, Vec2)> = HashMap::new();
    for (t, island) in islands.iter().enumerate() {
        if !selected.contains(island) {
            continue;
        }
        let key = match pivot {
            UvPivot::Selection => 0,
            UvPivot::EachIsland => *island,
        };
        let b = boxes.entry(key).or_insert((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)));
        for uv in mesh.tri_uvs(t) {
            b.0 = b.0.min(uv);
            b.1 = b.1.max(uv);
        }
    }
    islands
        .iter()
        .map(|island| {
            selected.contains(island).then(|| {
                let key = match pivot {
                    UvPivot::Selection => 0,
                    UvPivot::EachIsland => *island,
                };
                let (lo, hi) = boxes[&key];
                (lo + hi) * 0.5
            })
        })
        .collect()
}

/// The UV triangles of the selected islands after `t`, without building a mesh: the 2D view's
/// preview while the transform is edited.
pub fn preview(mesh: &Mesh, islands: &[u32], selected: &BTreeSet<u32>, t: &UvTransform) -> Vec<[Vec2; 3]> {
    if islands.len() != mesh.triangle_count() {
        return Vec::new();
    }
    let pivots = pivots(mesh, islands, selected, t.pivot);
    pivots.iter().enumerate().filter_map(|(tri, p)| p.map(|p| mesh.tri_uvs(tri).map(|uv| t.apply(uv, p)))).collect()
}

/// `mesh` with the islands in `selected` (ids from `islands`, one per triangle) transformed by `t`.
/// The UV set each triangle is painted with (`uv_sets`, when the file has several) gets the same
/// edit, so switching sets later keeps it.
pub fn transform_islands(mesh: &Mesh, islands: &[u32], selected: &BTreeSet<u32>, t: &UvTransform) -> Result<Mesh, UvEditError> {
    let tris = mesh.triangle_count();
    if islands.len() != tris {
        return Err(UvEditError::StaleIslands);
    }
    if !(t.scale.x.is_finite() && t.scale.y.is_finite() && t.scale.x > 0.0 && t.scale.y > 0.0) {
        return Err(UvEditError::BadScale);
    }
    if !islands.iter().any(|i| selected.contains(i)) {
        return Err(UvEditError::NothingSelected);
    }
    let pivots = pivots(mesh, islands, selected, t.pivot);
    let corner_uv: Vec<Vec2> = (0..3 * tris)
        .map(|c| {
            let uv = mesh.uvs[mesh.indices[c] as usize];
            pivots[c / 3].map_or(uv, |p| t.apply(uv, p))
        })
        .collect();
    let corner_pos: Vec<Vec3> = mesh.indices.iter().map(|&i| mesh.positions[i as usize]).collect();
    let corner_nrm: Vec<Vec3> = mesh.indices.iter().map(|&i| mesh.normals[i as usize]).collect();
    let tangents = crate::tangent::compute_corner_tangents(&corner_pos, &corner_nrm, &corner_uv);
    let mut out = Mesh { positions: Vec::new(), normals: Vec::new(), uvs: Vec::new(), tangents: Vec::new(), indices: Vec::new(), ..mesh.clone() };
    let mut lookup: HashMap<(u32, [u32; 2], [u32; 4]), u32> = HashMap::with_capacity(mesh.positions.len());
    for (c, &orig) in mesh.indices.iter().enumerate() {
        let (uv, tangent) = (corner_uv[c], tangents[c]);
        let key = (orig, uv.to_array().map(f32::to_bits), tangent.to_array().map(f32::to_bits));
        let idx = *lookup.entry(key).or_insert_with(|| {
            out.positions.push(mesh.positions[orig as usize]);
            out.normals.push(mesh.normals[orig as usize]);
            out.uvs.push(uv);
            out.tangents.push(tangent);
            (out.positions.len() - 1) as u32
        });
        out.indices.push(idx);
    }
    // Keep the file's UV sets in step: the edited triangles' own set takes the new corners.
    if out.uv_sets.iter().all(|s| s.corners.len() == 3 * tris) {
        for tri in (0..tris).filter(|&tri| pivots[tri].is_some()) {
            let set = mesh.tri_uv_set(tri);
            if let Some(s) = out.uv_sets.get_mut(set) {
                s.corners[3 * tri..3 * tri + 3].copy_from_slice(&corner_uv[3 * tri..3 * tri + 3]);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{primitives, topology::uv_islands};

    fn approx(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn the_transform_scales_turns_and_moves_about_the_pivot() {
        let t = UvTransform { offset: Vec2::new(0.1, 0.0), rotation_deg: 90.0, scale: Vec2::splat(2.0), ..Default::default() };
        let p = Vec2::splat(0.5);
        assert!(approx(t.apply(p, p), Vec2::new(0.6, 0.5)), "the pivot only moves by the offset");
        // (0.6, 0.5) is 0.1 right of the pivot: scaled to 0.2, turned to 0.2 up, then moved.
        assert!(approx(t.apply(Vec2::new(0.6, 0.5), p), Vec2::new(0.6, 0.7)));
        let flip = UvTransform { flip_u: true, ..Default::default() };
        assert!(approx(flip.apply(Vec2::new(0.2, 0.3), p), Vec2::new(0.8, 0.3)));
        assert!(flip.mirrors() && !t.mirrors() && UvTransform::default().is_identity() && !t.is_identity());
    }

    #[test]
    fn only_the_selected_island_moves_and_tangents_follow() {
        let mesh = primitives::sample_crate();
        let islands = uv_islands(&mesh);
        let count = islands.iter().max().unwrap() + 1;
        assert!(count >= 2, "the crate has several islands");
        let selected = BTreeSet::from([0u32]);
        let t = UvTransform { rotation_deg: 90.0, offset: Vec2::new(1.0, 0.0), ..Default::default() };
        let out = transform_islands(&mesh, &islands, &selected, &t).unwrap();
        assert_eq!(out.triangle_count(), mesh.triangle_count());
        let pivots = pivots(&mesh, &islands, &selected, t.pivot);
        for tri in 0..mesh.triangle_count() {
            let (before, after) = (mesh.tri_uvs(tri), out.tri_uvs(tri));
            // Same positions in the same order.
            assert_eq!(mesh.tri_positions(tri), out.tri_positions(tri));
            for k in 0..3 {
                let want = if islands[tri] == 0 { t.apply(before[k], pivots[tri].unwrap()) } else { before[k] };
                assert!(approx(after[k], want), "tri {tri} corner {k}: {:?} vs {want:?}", after[k]);
            }
        }
        // A quarter turn of the UVs turns the tangent of the moved faces a quarter turn about the normal.
        let tri = islands.iter().position(|&i| i == 0).unwrap();
        let (a, b) = (mesh.tangents[mesh.indices[3 * tri] as usize].truncate(), out.tangents[out.indices[3 * tri] as usize].truncate());
        assert!(a.dot(b).abs() < 1e-3, "tangent must turn with the UVs: {a} vs {b}");
        let other = islands.iter().position(|&i| i != 0).unwrap();
        let (a, b) = (mesh.tangents[mesh.indices[3 * other] as usize], out.tangents[out.indices[3 * other] as usize]);
        assert!((a - b).length() < 1e-4, "untouched islands keep their tangents");
    }

    #[test]
    fn each_island_turns_about_its_own_centre() {
        let mesh = primitives::sample_crate();
        let islands = uv_islands(&mesh);
        let selected: BTreeSet<u32> = islands.iter().copied().collect();
        let t = UvTransform { rotation_deg: 180.0, pivot: UvPivot::EachIsland, ..Default::default() };
        let out = transform_islands(&mesh, &islands, &selected, &t).unwrap();
        // Every island keeps its bounding box: a half turn about its centre maps the box onto itself.
        for island in &selected {
            let bbox = |m: &Mesh| {
                let mut b = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                for tri in (0..m.triangle_count()).filter(|&tri| islands[tri] == *island) {
                    for uv in m.tri_uvs(tri) {
                        b = (b.0.min(uv), b.1.max(uv));
                    }
                }
                b
            };
            let (a, b) = (bbox(&mesh), bbox(&out));
            assert!(approx(a.0, b.0) && approx(a.1, b.1), "island {island}: {a:?} vs {b:?}");
        }
    }

    #[test]
    fn the_preview_matches_the_result_and_bad_input_is_refused() {
        let mesh = primitives::sample_crate();
        let islands = uv_islands(&mesh);
        let selected = BTreeSet::from([1u32]);
        let t = UvTransform { scale: Vec2::new(0.5, 0.75), rotation_deg: 30.0, flip_v: true, ..Default::default() };
        let out = transform_islands(&mesh, &islands, &selected, &t).unwrap();
        let shown = preview(&mesh, &islands, &selected, &t);
        let moved: Vec<[Vec2; 3]> = (0..out.triangle_count()).filter(|&tri| islands[tri] == 1).map(|tri| out.tri_uvs(tri)).collect();
        assert_eq!(shown.len(), moved.len());
        assert!(shown.iter().flatten().zip(moved.iter().flatten()).all(|(a, b)| approx(*a, *b)));
        assert_eq!(transform_islands(&mesh, &islands, &BTreeSet::new(), &t).unwrap_err(), UvEditError::NothingSelected);
        assert_eq!(transform_islands(&mesh, &islands[1..], &selected, &t).unwrap_err(), UvEditError::StaleIslands);
        let flat = UvTransform { scale: Vec2::new(0.0, 1.0), ..Default::default() };
        assert_eq!(transform_islands(&mesh, &islands, &selected, &flat).unwrap_err(), UvEditError::BadScale);
    }

    #[test]
    fn the_painted_uv_set_takes_the_edit() {
        let mut mesh = primitives::sample_crate();
        let tris = mesh.triangle_count();
        let corners: Vec<Vec2> = mesh.indices.iter().map(|&i| mesh.uvs[i as usize]).collect();
        mesh.uv_sets = vec![
            crate::UvSet { name: "UV0".into(), corners: corners.clone() },
            crate::UvSet { name: "UV1".into(), corners: corners.iter().map(|c| *c * 0.5).collect() },
        ];
        let islands = uv_islands(&mesh);
        let selected = BTreeSet::from([0u32]);
        let t = UvTransform { offset: Vec2::new(0.25, 0.0), ..Default::default() };
        let out = transform_islands(&mesh, &islands, &selected, &t).unwrap();
        for tri in 0..tris {
            for k in 0..3 {
                let c = 3 * tri + k;
                let moved = if islands[tri] == 0 { Vec2::new(0.25, 0.0) } else { Vec2::ZERO };
                assert!(approx(out.uv_sets[0].corners[c], corners[c] + moved));
                assert!(approx(out.uv_sets[1].corners[c], corners[c] * 0.5), "the other set is left alone");
            }
        }
    }
}
