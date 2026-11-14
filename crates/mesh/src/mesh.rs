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
    /// Object index per triangle; empty in legacy meshes. Objects carry UUID identity.
    pub tri_object: Vec<Option<u32>>,
    pub objects: Vec<MeshObject>,
    pub materials: Vec<MaterialSlot>,
    pub submeshes: Vec<Submesh>,
    pub bounds: Aabb,
    /// Vertex colour (RGBA as stored in the file) of every triangle corner, three per triangle
    /// in the order of `indices`; empty when the file had none. Read by the baker (ID map from
    /// vertex colours). Not saved with projects and not kept by regrouping or unwrapping.
    pub corner_colors: Vec<[f32; 4]>,
    /// More vertex colour sets of the file (glTF `COLOR_1`, `COLOR_2`...), laid out like
    /// `corner_colors`, which is set 0 (issue #366). Empty when the file had one set or none.
    pub extra_color_sets: Vec<Vec<[f32; 4]>>,
    /// Which file each triangle came from when several files were joined with [`Mesh::merged`]
    /// (baking against several high-poly files: the ID map's "File ID" source). Empty for one file.
    pub tri_file: Vec<u32>,
    /// The file's colour of each material slot (diffuse / base colour), parallel to `materials`;
    /// empty when no slot has one. Read by the baker (ID map from material colours).
    pub material_colors: Vec<Option<[f32; 3]>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshObject {
    pub id: subcraft_core::ObjectId,
    pub name: String,
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
    /// The file's vertex colour at barycentrics `(u, v)` (weights of vertices 1 and 2) of a
    /// triangle; `None` when the mesh has no vertex colours.
    pub fn corner_color(&self, tri: usize, u: f32, v: f32) -> Option<[f32; 4]> {
        self.corner_color_in(0, tri, u, v)
    }

    /// Vertex colour set `set` (0 = the first), when the mesh has it for every triangle corner.
    pub fn color_set(&self, set: usize) -> Option<&[[f32; 4]]> {
        let colors = if set == 0 { &self.corner_colors } else { self.extra_color_sets.get(set - 1)? };
        (colors.len() == 3 * self.triangle_count() && !colors.is_empty()).then_some(colors.as_slice())
    }

    /// How many vertex colour sets the mesh has (0 = none).
    pub fn color_set_count(&self) -> usize {
        (0..=self.extra_color_sets.len()).take_while(|&s| self.color_set(s).is_some()).count()
    }

    /// [`Self::corner_color`] from vertex colour set `set`.
    pub fn corner_color_in(&self, set: usize, tri: usize, u: f32, v: f32) -> Option<[f32; 4]> {
        let c = self.color_set(set)?.get(3 * tri..3 * tri + 3)?;
        let w = 1.0 - u - v;
        Some(std::array::from_fn(|i| c[0][i] * w + c[1][i] * u + c[2][i] * v))
    }

    /// The file (0 = the first) a triangle came from; 0 for a mesh read from one file.
    pub fn file_of(&self, tri: usize) -> u32 {
        self.tri_file.get(tri).copied().unwrap_or(0)
    }

    /// How many files the mesh was joined from (1 for a single file).
    pub fn file_count(&self) -> u32 {
        self.tri_file.iter().max().map_or(1, |m| m + 1)
    }

    /// The file's colour of a triangle's material slot, when it has one.
    pub fn material_color(&self, tri: usize) -> Option<[f32; 3]> {
        let slot = *self.tri_material.get(tri)? as usize;
        self.material_colors.get(slot).copied().flatten()
    }

    /// Several meshes as one (baking against several high-poly files): vertices, triangles,
    /// materials and objects are appended in order. Nothing is welded or renamed.
    pub fn merged(parts: &[&Mesh]) -> Mesh {
        let mut out = Mesh { name: parts.first().map(|m| m.name.clone()).unwrap_or_default(), ..Default::default() };
        let any_objects = parts.iter().any(|m| !m.tri_object.is_empty());
        let any_colors = parts.iter().any(|m| !m.corner_colors.is_empty());
        let extra_sets = parts.iter().map(|m| m.extra_color_sets.len()).max().unwrap_or(0);
        out.extra_color_sets = vec![Vec::new(); extra_sets];
        let any_material_colors = parts.iter().any(|m| !m.material_colors.is_empty());
        for (file, m) in parts.iter().enumerate() {
            let (v0, t0) = (out.positions.len() as u32, out.triangle_count() as u32);
            out.tri_file.extend(std::iter::repeat_n(file as u32, m.triangle_count()));
            let (m0, o0) = (out.materials.len() as u32, out.objects.len() as u32);
            out.positions.extend_from_slice(&m.positions);
            out.normals.extend_from_slice(&m.normals);
            out.uvs.extend_from_slice(&m.uvs);
            out.tangents.extend_from_slice(&m.tangents);
            out.indices.extend(m.indices.iter().map(|i| i + v0));
            out.tri_material.extend(m.tri_material.iter().map(|slot| slot + m0));
            if any_objects {
                if m.tri_object.is_empty() {
                    out.tri_object.extend(std::iter::repeat_n(None, m.triangle_count()));
                } else {
                    out.tri_object.extend(m.tri_object.iter().map(|o| o.map(|o| o + o0)));
                }
            }
            out.objects.extend(m.objects.iter().cloned());
            out.materials.extend(m.materials.iter().cloned());
            out.submeshes.extend(m.submeshes.iter().map(|s| Submesh { material: s.material + m0, first_triangle: s.first_triangle + t0, triangle_count: s.triangle_count }));
            for (i, set) in out.extra_color_sets.iter_mut().enumerate() {
                match m.extra_color_sets.get(i).filter(|c| c.len() == 3 * m.triangle_count()) {
                    Some(colors) => set.extend_from_slice(colors),
                    None => set.extend(std::iter::repeat_n([1.0; 4], 3 * m.triangle_count())),
                }
            }
            if any_colors {
                if m.corner_colors.is_empty() {
                    out.corner_colors.extend(std::iter::repeat_n([1.0; 4], 3 * m.triangle_count()));
                } else {
                    out.corner_colors.extend_from_slice(&m.corner_colors);
                }
            }
            if any_material_colors {
                out.material_colors.extend((0..m.materials.len()).map(|i| m.material_colors.get(i).copied().flatten()));
            }
            out.bounds = out.bounds.merge(&m.bounds);
        }
        if parts.len() < 2 { out.tri_file.clear(); }
        out
    }

    /// A copy with every position moved by `m` (normals by the inverse transpose, tangent
    /// directions by the linear part, winding flipped for mirroring transforms) and the bounds
    /// recomputed. UVs, materials and object identity are untouched.
    pub fn transformed(&self, m: &glam::Mat4) -> Mesh {
        let mut out = self.clone();
        let lin = glam::Mat3::from_mat4(*m);
        let det = lin.determinant();
        let nm = if det.abs() > 1e-20 { lin.inverse().transpose() } else { lin };
        for p in &mut out.positions { *p = m.transform_point3(*p); }
        for n in &mut out.normals { *n = (nm * *n).normalize_or_zero(); }
        for t in &mut out.tangents { let d = (lin * t.truncate()).normalize_or_zero(); *t = d.extend(if det < 0. { -t.w } else { t.w }); }
        if det < 0. {
            for tri in out.indices.chunks_exact_mut(3) { tri.swap(1, 2); }
            for tri in out.corner_colors.chunks_exact_mut(3) { tri.swap(1, 2); }
            for set in &mut out.extra_color_sets {
                for tri in set.chunks_exact_mut(3) { tri.swap(1, 2); }
            }
        }
        out.bounds = Aabb::from_points(out.positions.iter().copied());
        out
    }

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
        if !self.tri_object.is_empty() && (self.tri_object.len() != self.triangle_count()
            || self.tri_object.iter().flatten().any(|&o| o as usize >= self.objects.len())) {
            problems.push("object indices are invalid".into());
        }
        let mut object_ids = std::collections::HashSet::new();
        if self.objects.iter().any(|o| !object_ids.insert(o.id)) {
            problems.push("duplicate object UUID".into());
        }
        if self.positions.iter().any(|p| !p.is_finite()) {
            problems.push("non-finite vertex position".into());
        }
        let covered: u32 = self.submeshes.iter().map(|s| s.triangle_count).sum();
        if covered as usize != self.triangle_count() {
            problems.push("submeshes do not cover all triangles".into());
        }
        // A submesh range outside the index buffer, or a material slot that does not exist,
        // would be indexed without a check by baking, unwrapping and OBJ export.
        let tris = self.triangle_count();
        if self.submeshes.iter().any(|s| {
            s.first_triangle as usize > tris || s.first_triangle.checked_add(s.triangle_count).is_none_or(|end| end as usize > tris)
        }) {
            problems.push("submesh range outside the triangle list".into());
        }
        if self.submeshes.iter().any(|s| s.material as usize >= self.materials.len())
            || self.tri_material.iter().any(|&m| m as usize >= self.materials.len())
        {
            problems.push("material index out of range".into());
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
    objects: Vec<MeshObject>,
    tri_object: Vec<Option<u32>>,
    current_object: Option<u32>,
    /// Vertex colour per accepted corner (white where a triangle had none), parallel to `corners`.
    corner_colors: Vec<[f32; 4]>,
    /// Whether any triangle came with vertex colours.
    any_corner_color: bool,
    /// Vertex colour sets after the first, each padded lazily to `corners` (white where a triangle had none).
    extra_colors: Vec<Vec<[f32; 4]>>,
    /// File colour per material slot, parallel to `materials`.
    material_colors: Vec<Option<[f32; 3]>>,
    pub stats: BuildStats,
}

impl MeshBuilder {
    /// Take accumulated corners (3 per triangle), leaving the builder empty.
    pub(crate) fn take_corners(&mut self) -> Vec<Corner> {
        self.tri_material.clear();
        self.tri_object.clear();
        self.corner_colors.clear();
        self.any_corner_color = false;
        self.extra_colors.clear();
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
        self.material_colors.push(None);
        self.material_lookup.insert(name.to_string(), i);
        i
    }

    /// The file's colour of a material slot (diffuse / base colour). The first one given stays.
    pub fn set_material_color(&mut self, material: u32, color: [f32; 3]) {
        if let Some(slot) = self.material_colors.get_mut(material as usize) {
            if slot.is_none() && color.iter().all(|c| c.is_finite()) {
                *slot = Some(color.map(|c| c.clamp(0.0, 1.0)));
            }
        }
    }

    /// [`Self::push_triangle`] with the file's vertex colour (RGBA) of each corner.
    pub fn push_triangle_colored(&mut self, material: u32, c: [Corner; 3], colors: [[f32; 4]; 3]) {
        let before = self.tri_material.len();
        self.push_triangle(material, c);
        if self.tri_material.len() > before {
            let n = self.corner_colors.len();
            for (slot, color) in self.corner_colors[n - 3..].iter_mut().zip(colors) {
                *slot = color.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 });
            }
            self.any_corner_color = true;
        }
    }

    /// [`Self::push_triangle`] with several vertex colour sets (RGBA per corner); the first is
    /// set 0. No sets is a triangle without vertex colours.
    pub fn push_triangle_color_sets(&mut self, material: u32, c: [Corner; 3], sets: &[[[f32; 4]; 3]]) {
        let Some((first, rest)) = sets.split_first() else { return self.push_triangle(material, c) };
        let before = self.corners.len();
        self.push_triangle_colored(material, c, *first);
        if self.corners.len() == before {
            return;
        }
        for (i, colors) in rest.iter().enumerate() {
            if self.extra_colors.len() <= i {
                self.extra_colors.push(Vec::new());
            }
            let set = &mut self.extra_colors[i];
            set.resize(before, [1.0; 4]);
            set.extend(colors.map(|c| c.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 })));
        }
    }

    pub fn push_triangle(&mut self, material: u32, mut c: [Corner; 3]) {
        if c.iter().any(|c| !c.position.is_finite()) {
            self.stats.non_finite_dropped += 1;
            return;
        }
        // NaN / infinite UVs (broken exporters) count as "no UV" instead of poisoning rasterization.
        for corner in &mut c {
            if corner.uv.is_some_and(|uv| !uv.is_finite()) { corner.uv = None; }
        }
        let area = (c[1].position - c[0].position).cross(c[2].position - c[0].position).length();
        // `!(area > eps)` also catches NaN / infinite areas from absurdly large coordinates.
        if !(area > 1e-12 && area.is_finite()) {
            self.stats.degenerate_dropped += 1;
            return;
        }
        if c.iter().any(|c| c.uv.is_none()) {
            self.stats.missing_uv_triangles += 1;
        }
        self.corners.extend_from_slice(&c);
        self.corner_colors.extend_from_slice(&[[1.0; 4]; 3]);
        self.tri_material.push(material);
        self.tri_object.push(self.current_object);
    }

    /// Begin an object instance. Equal display names do not merge identity.
    pub fn object(&mut self, name: &str) {
        let name = if name.trim().is_empty() { "Object" } else { name };
        self.current_object = Some(self.objects.len() as u32);
        self.objects.push(MeshObject { id: subcraft_core::ObjectId::new(), name: name.into() });
    }

    pub fn triangle_count(&self) -> usize {
        self.tri_material.len()
    }

    pub fn build(self) -> Result<Mesh, BuildError> {
        self.build_impl(true)
    }

    /// Ray-cast source geometry needs positions/normals, not a paintable UV atlas.
    /// Missing UVs remain zero; this does not unwrap the mesh for painting.
    pub fn build_for_baking(self) -> Result<Mesh, BuildError> {
        self.build_impl(false)
    }

    fn build_impl(mut self, require_uvs: bool) -> Result<Mesh, BuildError> {
        let tri_count = self.tri_material.len();
        if tri_count == 0 {
            return Err(BuildError::NoTriangles);
        }
        if require_uvs && self.stats.missing_uv_triangles == tri_count {
            return Err(BuildError::NoUvs);
        }

        // Sort triangles by material so each texture set is one contiguous draw range.
        let mut order: Vec<usize> = (0..tri_count).collect();
        order.sort_by_key(|&t| self.tri_material[t]);
        let mut corners = Vec::with_capacity(self.corners.len());
        let mut tri_material = Vec::with_capacity(tri_count);
        let mut tri_object = Vec::with_capacity(tri_count);
        let mut corner_colors = Vec::new();
        for set in &mut self.extra_colors {
            set.resize(self.corners.len(), [1.0; 4]);
        }
        let mut extra_color_sets: Vec<Vec<[f32; 4]>> = vec![Vec::with_capacity(self.corners.len()); self.extra_colors.len()];
        for &t in &order {
            for (out, set) in extra_color_sets.iter_mut().zip(&self.extra_colors) {
                out.extend_from_slice(&set[3 * t..3 * t + 3]);
            }
            corners.extend_from_slice(&self.corners[3 * t..3 * t + 3]);
            if self.any_corner_color {
                corner_colors.extend_from_slice(&self.corner_colors[3 * t..3 * t + 3]);
            }
            tri_material.push(self.tri_material[t]);
            tri_object.push(self.tri_object[t]);
        }
        let mut material_colors = std::mem::take(&mut self.material_colors);

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
        let mut mesh = Mesh { name: self.name, materials: self.materials, objects: self.objects, ..Default::default() };
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
        mesh.tri_object = tri_object;
        mesh.corner_colors = corner_colors;
        mesh.extra_color_sets = extra_color_sets;
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
            material_colors = used.iter().map(|&m| material_colors.get(m as usize).copied().flatten()).collect();
            mesh.materials = mats;
            for m in mesh.tri_material.iter_mut() {
                *m = remap[*m as usize];
            }
            for s in mesh.submeshes.iter_mut() {
                s.material = remap[s.material as usize];
            }
        }
        if material_colors.iter().any(Option::is_some) {
            mesh.material_colors = material_colors;
        }
        Ok(mesh)
    }
}

/// Angle-weighted smooth normals, welding corners with bit-identical positions.
fn smooth_corner_normals(corners: &[Corner]) -> Vec<Vec3> {
    let mut acc: HashMap<[u32; 3], Vec3> = HashMap::new();
    // `+ 0.0` turns -0.0 into 0.0, which is the same position with different bits.
    let key = |p: Vec3| [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()];
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

    #[test]
    fn hostile_input_is_dropped_or_repaired_never_poisons_the_mesh() {
        let mut b = MeshBuilder::new("hostile");
        let m = b.material("m");
        let c = |p: [f32; 3], uv: Option<[f32; 2]>, n: Option<[f32; 3]>| Corner { position: Vec3::from(p), normal: n.map(Vec3::from), uv: uv.map(Vec2::from) };
        // good triangle with a NaN normal on one corner and a NaN UV on another
        b.push_triangle(m, [c([0., 0., 0.], Some([0., 0.]), Some([f32::NAN, 0., 0.])), c([1., 0., 0.], Some([f32::NAN, 1.]), None), c([0., 1., 0.], Some([0., 1.]), None)]);
        // good triangle with valid UVs
        b.push_triangle(m, [c([0., 0., 1.], Some([0., 0.]), None), c([1., 0., 1.], Some([1., 0.]), None), c([0., 1., 1.], Some([0., 1.]), None)]);
        // zero area, NaN position, and coordinates so large the area overflows
        b.push_triangle(m, [c([0., 0., 2.], Some([0., 0.]), None); 3]);
        b.push_triangle(m, [c([f32::NAN, 0., 0.], Some([0., 0.]), None), c([1., 0., 0.], Some([1., 0.]), None), c([0., 1., 0.], Some([0., 1.]), None)]);
        b.push_triangle(m, [c([0., 0., 0.], Some([0., 0.]), None), c([3e38, 0., 0.], Some([1., 0.]), None), c([0., 3e38, 0.], Some([0., 1.]), None)]);
        assert_eq!((b.stats.degenerate_dropped, b.stats.non_finite_dropped), (2, 1));
        let mesh = b.build().expect("two good triangles remain");
        assert_eq!(mesh.indices.len(), 6);
        assert!(mesh.normals.iter().all(|n| n.is_finite() && (n.length() - 1.0).abs() < 1e-4));
        assert!(mesh.uvs.iter().all(|u| u.is_finite()) && mesh.tangents.iter().all(|t| t.is_finite()));
    }
}
