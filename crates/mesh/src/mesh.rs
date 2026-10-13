//! Canonical mesh representation.
//!
//! Every importer produces raw triangle *corners* through [`MeshBuilder`]; the builder validates,
//! computes missing normals and MikkTSpace tangents, deduplicates vertices and groups triangles
//! by material slot. Nothing downstream ever sees importer-specific data (guide risk #10).
//!
//! Conventions (ADR-0005): right-handed, +Y up, counter-clockwise front faces, UV origin at the
//! *top-left* of the texture (v grows downward, glTF convention). Tangents follow MikkTSpace
//! computed in OpenGL UV space, so tangent-space normal maps are OpenGL (+Y) convention.

use std::collections::HashMap;

use glam::{Vec2, Vec3, Vec4};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) };

    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Self {
        let mut b = Aabb::EMPTY;
        for p in points {
            b.grow(p);
        }
        b
    }
    pub fn grow(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn merge(&self, o: &Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    pub fn extent(&self) -> Vec3 {
        (self.max - self.min).max(Vec3::ZERO)
    }
    pub fn diagonal(&self) -> f32 {
        self.extent().length()
    }
    pub fn is_valid(&self) -> bool {
        self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z
    }
    pub fn surface_area(&self) -> f32 {
        let e = self.extent();
        2.0 * (e.x * e.y + e.y * e.z + e.z * e.x)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaterialSlot {
    pub name: String,
}

/// Contiguous range of triangles sharing a material slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Submesh {
    pub material: u32,
    pub first_triangle: u32,
    pub triangle_count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    /// xyz = tangent, w = bitangent sign: `B = w * cross(N, T)`.
    pub tangents: Vec<Vec4>,
    pub indices: Vec<u32>,
    /// Material slot per triangle (redundant with `submeshes`, kept for O(1) lookup).
    pub tri_material: Vec<u32>,
    pub materials: Vec<MaterialSlot>,
    pub submeshes: Vec<Submesh>,
    pub bounds: Aabb,
}

impl Default for Aabb {
    fn default() -> Self {
        Aabb::EMPTY
    }
}

/// Interpolated surface attributes at a point on a triangle.
#[derive(Clone, Copy, Debug)]
pub struct SurfacePoint {
    pub position: Vec3,
    pub normal: Vec3,
    pub tangent: Vec4,
    pub uv: Vec2,
    pub geometric_normal: Vec3,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    #[inline]
    pub fn tri(&self, t: usize) -> [u32; 3] {
        [self.indices[3 * t], self.indices[3 * t + 1], self.indices[3 * t + 2]]
    }

    #[inline]
    pub fn tri_positions(&self, t: usize) -> [Vec3; 3] {
        let [a, b, c] = self.tri(t);
        [self.positions[a as usize], self.positions[b as usize], self.positions[c as usize]]
    }

    #[inline]
    pub fn tri_uvs(&self, t: usize) -> [Vec2; 3] {
        let [a, b, c] = self.tri(t);
        [self.uvs[a as usize], self.uvs[b as usize], self.uvs[c as usize]]
    }

    pub fn geometric_normal(&self, t: usize) -> Vec3 {
        let [a, b, c] = self.tri_positions(t);
        (b - a).cross(c - a).normalize_or_zero()
    }

    /// Interpolate attributes with barycentrics (w0 = 1 - u - v for vertex 0).
    pub fn interpolate(&self, t: usize, u: f32, v: f32) -> SurfacePoint {
        let [a, b, c] = self.tri(t).map(|i| i as usize);
        let w = 1.0 - u - v;
        let position = self.positions[a] * w + self.positions[b] * u + self.positions[c] * v;
        let normal = (self.normals[a] * w + self.normals[b] * u + self.normals[c] * v).normalize_or_zero();
        let tangent = self.tangents[a] * w + self.tangents[b] * u + self.tangents[c] * v;
        let tw = if self.tangents[a].w < 0.0 { -1.0 } else { 1.0 };
        let tangent = tangent.truncate().normalize_or_zero().extend(tw);
        let uv = self.uvs[a] * w + self.uvs[b] * u + self.uvs[c] * v;
        SurfacePoint { position, normal, tangent, uv, geometric_normal: self.geometric_normal(t) }
    }

    pub fn submesh_for_material(&self, material: u32) -> Option<&Submesh> {
        self.submeshes.iter().find(|s| s.material == material)
    }

    /// Triangle index range of a material slot.
    pub fn material_triangles(&self, material: u32) -> std::ops::Range<usize> {
        match self.submesh_for_material(material) {
            Some(s) => s.first_triangle as usize..(s.first_triangle + s.triangle_count) as usize,
            None => 0..0,
        }
    }

    /// Basic structural validation; returns human-readable problems.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let n = self.positions.len();
        if self.normals.len() != n || self.uvs.len() != n || self.tangents.len() != n {
            problems.push("attribute arrays have mismatched lengths".into());
        }
        if self.indices.len() % 3 != 0 {
            problems.push("index count is not a multiple of 3".into());
        }
        if self.indices.iter().any(|&i| i as usize >= n) {
            problems.push("index out of range".into());
        }
        if self.tri_material.len() != self.triangle_count() {
            problems.push("tri_material length mismatch".into());
        }
        if self.positions.iter().any(|p| !p.is_finite()) {
            problems.push("non-finite vertex position".into());
        }
        let covered: u32 = self.submeshes.iter().map(|s| s.triangle_count).sum();
        if covered as usize != self.triangle_count() {
            problems.push("submeshes do not cover all triangles".into());
        }
        problems
    }
}

/// One triangle corner as delivered by an importer (already in canonical conventions).
#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub position: Vec3,
    pub normal: Option<Vec3>,
    pub uv: Option<Vec2>,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("mesh has no valid triangles")]
    NoTriangles,
    #[error("mesh has no UV coordinates; SubCraft requires UVs to paint (automatic unwrapping is not available yet)")]
    NoUvs,
}

/// Accumulates triangles from importers and produces a validated [`Mesh`].
#[derive(Default)]
pub struct MeshBuilder {
    name: String,
    materials: Vec<MaterialSlot>,
    material_lookup: HashMap<String, u32>,
    corners: Vec<Corner>,
    tri_material: Vec<u32>,
    pub stats: BuildStats,
}

impl MeshBuilder {
    /// Take accumulated corners (3 per triangle), leaving the builder empty.
    pub(crate) fn take_corners(&mut self) -> Vec<Corner> {
        self.tri_material.clear();
        std::mem::take(&mut self.corners)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildStats {
    pub degenerate_dropped: usize,
    pub non_finite_dropped: usize,
    pub missing_uv_triangles: usize,
    pub generated_normals: bool,
}

impl MeshBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    /// Get or create a material slot by name.
    pub fn material(&mut self, name: &str) -> u32 {
        let name = if name.trim().is_empty() { "DefaultMaterial" } else { name };
        if let Some(&i) = self.material_lookup.get(name) {
            return i;
        }
        let i = self.materials.len() as u32;
        self.materials.push(MaterialSlot { name: name.to_string() });
        self.material_lookup.insert(name.to_string(), i);
        i
    }

    pub fn push_triangle(&mut self, material: u32, c: [Corner; 3]) {
        if c.iter().any(|c| !c.position.is_finite()) {
            self.stats.non_finite_dropped += 1;
            return;
        }
        let area = (c[1].position - c[0].position).cross(c[2].position - c[0].position).length();
        if area <= 1e-12 {
            self.stats.degenerate_dropped += 1;
            return;
        }
        if c.iter().any(|c| c.uv.is_none()) {
            self.stats.missing_uv_triangles += 1;
        }
        self.corners.extend_from_slice(&c);
        self.tri_material.push(material);
    }

    pub fn triangle_count(&self) -> usize {
        self.tri_material.len()
    }

    pub fn build(mut self) -> Result<Mesh, BuildError> {
        let tri_count = self.tri_material.len();
        if tri_count == 0 {
            return Err(BuildError::NoTriangles);
        }
        if self.stats.missing_uv_triangles == tri_count {
            return Err(BuildError::NoUvs);
        }

        // Sort triangles by material so each texture set is one contiguous draw range.
        let mut order: Vec<usize> = (0..tri_count).collect();
        order.sort_by_key(|&t| self.tri_material[t]);
        let mut corners = Vec::with_capacity(self.corners.len());
        let mut tri_material = Vec::with_capacity(tri_count);
        for &t in &order {
            corners.extend_from_slice(&self.corners[3 * t..3 * t + 3]);
            tri_material.push(self.tri_material[t]);
        }

        // Normals: fill missing ones with angle-weighted smooth normals over welded positions.
        let mut normals: Vec<Vec3> = Vec::with_capacity(corners.len());
        let any_missing = corners.iter().any(|c| c.normal.is_none_or(|n| !n.is_finite() || n.length_squared() < 1e-12));
        if any_missing {
            self.stats.generated_normals = true;
            let smooth = smooth_corner_normals(&corners);
            for (i, c) in corners.iter().enumerate() {
                normals.push(match c.normal {
                    Some(n) if n.is_finite() && n.length_squared() > 1e-12 => n.normalize(),
                    _ => smooth[i],
                });
            }
        } else {
            normals.extend(corners.iter().map(|c| c.normal.expect("checked").normalize()));
        }
        let uvs: Vec<Vec2> = corners.iter().map(|c| c.uv.unwrap_or(Vec2::ZERO)).collect();
        let positions: Vec<Vec3> = corners.iter().map(|c| c.position).collect();

        let tangents = crate::tangent::compute_corner_tangents(&positions, &normals, &uvs);

        // Deduplicate identical corners into shared vertices.
        let mut mesh = Mesh { name: self.name, materials: self.materials, ..Default::default() };
        let mut lookup: HashMap<[u32; 12], u32> = HashMap::with_capacity(corners.len());
        mesh.indices.reserve(corners.len());
        for i in 0..corners.len() {
            let (p, n, uv, t) = (positions[i], normals[i], uvs[i], tangents[i]);
            let key = [
                p.x.to_bits(),
                p.y.to_bits(),
                p.z.to_bits(),
                n.x.to_bits(),
                n.y.to_bits(),
                n.z.to_bits(),
                uv.x.to_bits(),
                uv.y.to_bits(),
                t.x.to_bits(),
                t.y.to_bits(),
                t.z.to_bits(),
                t.w.to_bits(),
            ];
            let idx = *lookup.entry(key).or_insert_with(|| {
                mesh.positions.push(p);
                mesh.normals.push(n);
                mesh.uvs.push(uv);
                mesh.tangents.push(t);
                (mesh.positions.len() - 1) as u32
            });
            mesh.indices.push(idx);
        }
        mesh.tri_material = tri_material;
        mesh.bounds = Aabb::from_points(mesh.positions.iter().copied());

        // Submesh ranges.
        let mut start = 0usize;
        while start < tri_count {
            let m = mesh.tri_material[start];
            let mut end = start;
            while end < tri_count && mesh.tri_material[end] == m {
                end += 1;
            }
            mesh.submeshes.push(Submesh {
                material: m,
                first_triangle: start as u32,
                triangle_count: (end - start) as u32,
            });
            start = end;
        }
        // Drop material slots that ended up with no triangles, remapping indices.
        let used: Vec<u32> = mesh.submeshes.iter().map(|s| s.material).collect();
        if used.len() != mesh.materials.len() {
            let mut remap = vec![u32::MAX; mesh.materials.len()];
            let mut mats = Vec::new();
            for &m in &used {
                remap[m as usize] = mats.len() as u32;
                mats.push(mesh.materials[m as usize].clone());
            }
            mesh.materials = mats;
            for m in mesh.tri_material.iter_mut() {
                *m = remap[*m as usize];
            }
            for s in mesh.submeshes.iter_mut() {
                s.material = remap[s.material as usize];
            }
        }
        Ok(mesh)
    }
}

/// Angle-weighted smooth normals, welding corners with bit-identical positions.
fn smooth_corner_normals(corners: &[Corner]) -> Vec<Vec3> {
    let mut acc: HashMap<[u32; 3], Vec3> = HashMap::new();
    let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    for tri in corners.chunks_exact(3) {
        let p = [tri[0].position, tri[1].position, tri[2].position];
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        for k in 0..3 {
            let e1 = (p[(k + 1) % 3] - p[k]).normalize_or_zero();
            let e2 = (p[(k + 2) % 3] - p[k]).normalize_or_zero();
            let angle = e1.dot(e2).clamp(-1.0, 1.0).acos();
            *acc.entry(key(p[k])).or_insert(Vec3::ZERO) += n * angle;
        }
    }
    corners
        .iter()
        .map(|c| acc.get(&key(c.position)).copied().unwrap_or(Vec3::Y).normalize_or(Vec3::Y))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corner(p: [f32; 3], uv: [f32; 2]) -> Corner {
        Corner { position: Vec3::from(p), normal: None, uv: Some(Vec2::from(uv)) }
    }

    #[test]
    fn builds_quad_with_shared_vertices() {
        let mut b = MeshBuilder::new("quad");
        let m = b.material("mat");
        let c = [
            corner([0.0, 0.0, 0.0], [0.0, 1.0]),
            corner([1.0, 0.0, 0.0], [1.0, 1.0]),
            corner([1.0, 1.0, 0.0], [1.0, 0.0]),
            corner([0.0, 1.0, 0.0], [0.0, 0.0]),
        ];
        b.push_triangle(m, [c[0], c[1], c[2]]);
        b.push_triangle(m, [c[0], c[2], c[3]]);
        let mesh = b.build().unwrap();
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 2);
        for n in &mesh.normals {
            assert!((n.z - 1.0).abs() < 1e-5, "generated normal should face +Z: {n}");
        }
        for t in &mesh.tangents {
            assert!((t.truncate().length() - 1.0).abs() < 1e-4);
            assert!(t.truncate().dot(Vec3::Z).abs() < 1e-4, "tangent must be orthogonal to normal");
        }
    }

    #[test]
    fn drops_degenerate_and_groups_materials() {
        let mut b = MeshBuilder::new("t");
        let a = b.material("A");
        let bm = b.material("B");
        let t = |x: f32| [corner([x, 0.0, 0.0], [0.0, 0.0]), corner([x + 1.0, 0.0, 0.0], [1.0, 0.0]), corner([x, 1.0, 0.0], [0.0, 1.0])];
        b.push_triangle(bm, t(0.0));
        b.push_triangle(a, t(2.0));
        b.push_triangle(bm, t(4.0));
        b.push_triangle(a, [corner([0.0; 3], [0.0; 2]), corner([0.0; 3], [0.0; 2]), corner([1.0, 0.0, 0.0], [0.0; 2])]);
        let mesh = b.build().unwrap();
        assert_eq!(mesh.triangle_count(), 3);
        assert_eq!(mesh.submeshes.len(), 2);
        assert_eq!(mesh.material_triangles(0), 0..1);
        assert_eq!(mesh.material_triangles(1), 1..3);
    }

    #[test]
    fn rejects_mesh_without_uvs() {
        let mut b = MeshBuilder::new("t");
        let m = b.material("");
        let c = |p: [f32; 3]| Corner { position: Vec3::from(p), normal: None, uv: None };
        b.push_triangle(m, [c([0.0, 0.0, 0.0]), c([1.0, 0.0, 0.0]), c([0.0, 1.0, 0.0])]);
        assert!(matches!(b.build(), Err(BuildError::NoUvs)));
    }
}
