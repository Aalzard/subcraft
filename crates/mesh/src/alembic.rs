//! Alembic (`.abc`) mesh import for the Ogawa container (the default since Alembic 1.5).
//!
//! The old HDF5 container is detected and reported, not read. Static import only: every
//! property is evaluated at its first sample. Transforms (`IXform`) are accumulated into world
//! matrices, every `IPolyMesh` / `ISubD` becomes one object, n-gons are fan-triangulated.
//! Cameras, lights, curves, points and NURBS are skipped and listed as notes.

use glam::{Mat3, Mat4, Vec2, Vec3};

use crate::mesh::{Corner, MeshBuilder};

const DATA_BIT: u64 = 1 << 63;
const HDF5_SIGNATURE: [u8; 8] = [0x89, b'H', b'D', b'F', 0x0d, 0x0a, 0x1a, 0x0a];
const MAX_DEPTH: usize = 512;
const MAX_OBJECTS: usize = 1 << 20;

type R<T> = Result<T, String>;

fn bad<T>(what: &str) -> R<T> {
    Err(format!("corrupt Alembic file: {what}"))
}

/// A child reference inside an Ogawa group: a file position, with the top bit marking raw data.
#[derive(Clone, Copy)]
struct Ref(u64);

impl Ref {
    fn is_data(self) -> bool {
        self.0 & DATA_BIT != 0
    }
    fn pos(self) -> u64 {
        self.0 & !DATA_BIT
    }
    fn is_empty(self) -> bool {
        self.pos() == 0
    }
}

struct Ogawa<'a> {
    buf: &'a [u8],
}

impl<'a> Ogawa<'a> {
    fn open(buf: &'a [u8]) -> R<(Self, Ref)> {
        if buf.len() >= 8 && buf[..8] == HDF5_SIGNATURE {
            return Err("this Alembic file uses the older HDF5 container, which SubCraft cannot read; re-save it as Ogawa (for example with abcconvert -toOgawa)".into());
        }
        if buf.len() < 16 || &buf[..5] != b"Ogawa" {
            return Err("not an Alembic (Ogawa) file".into());
        }
        if buf[5] != 0xff {
            return Err("the Alembic file was not closed properly (it is truncated or still being written)".into());
        }
        let root = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        if root == 0 || root >= buf.len() as u64 {
            return bad("root group outside the file");
        }
        Ok((Self { buf }, Ref(root)))
    }

    fn u64_at(&self, pos: u64) -> R<u64> {
        let p = usize::try_from(pos).map_err(|_| "corrupt Alembic file: position overflow".to_string())?;
        match self.buf.get(p..p.wrapping_add(8)) {
            Some(s) if p.checked_add(8).is_some() => Ok(u64::from_le_bytes(s.try_into().unwrap())),
            _ => bad("read past the end of the file"),
        }
    }

    fn children(&self, g: Ref) -> R<Vec<Ref>> {
        if g.is_empty() {
            return Ok(Vec::new());
        }
        if g.is_data() {
            return bad("expected a group, found data");
        }
        let pos = g.pos();
        let n = self.u64_at(pos)?;
        let room = (self.buf.len() as u64).saturating_sub(pos + 8) / 8;
        if n > room {
            return bad("group larger than the file");
        }
        (0..n).map(|i| self.u64_at(pos + 8 + 8 * i).map(Ref)).collect()
    }

    fn data(&self, d: Ref) -> R<&'a [u8]> {
        if d.is_empty() {
            return Ok(&[]);
        }
        if !d.is_data() {
            return bad("expected data, found a group");
        }
        let pos = d.pos();
        let size = self.u64_at(pos)?;
        let start = pos + 8;
        if size > (self.buf.len() as u64).saturating_sub(start) {
            return bad("data block past the end of the file");
        }
        Ok(&self.buf[start as usize..(start + size) as usize])
    }

    /// First sample of a scalar or array property: sample data is preceded by a 16 byte hash.
    /// Array properties store their dimensions in the next child, which is not needed here.
    fn sample0(&self, r: Ref) -> R<&'a [u8]> {
        let d = if r.is_data() {
            r
        } else {
            match self.children(r)?.first() {
                Some(&c) => c,
                None => return Ok(&[]),
            }
        };
        Ok(self.data(d)?.get(16..).unwrap_or(&[]))
    }
}

struct Cursor<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        let s = self.b.get(self.p..self.p.saturating_add(n)).filter(|_| self.p.checked_add(n).is_some());
        match s {
            Some(s) => {
                self.p += n;
                Ok(s)
            }
            None => bad("header data ends early"),
        }
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    /// An integer whose width (1, 2 or 4 bytes) is chosen per property by a size hint.
    fn hinted(&mut self, hint: u32) -> R<u32> {
        match hint {
            0 => Ok(self.u8()? as u32),
            1 => Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as u32),
            2 => self.u32(),
            _ => bad("bad integer size hint"),
        }
    }
    fn string(&mut self, n: usize) -> R<String> {
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Compound,
    Scalar,
    Array,
}

#[derive(Clone, Debug)]
struct PropHeader {
    name: String,
    kind: Kind,
    pod: u8,
    samples: u32,
    meta: String,
}

fn read_indexed_metadata(og: &Ogawa, root: &[Ref]) -> R<Vec<String>> {
    let mut out = vec![String::new()];
    if let Some(&r) = root.get(5) {
        let data = og.data(r)?;
        let mut c = Cursor { b: data, p: 0 };
        while c.p < data.len() {
            let n = c.u8()? as usize;
            out.push(c.string(n)?);
        }
    }
    Ok(out)
}

fn meta_from(c: &mut Cursor, idx: u8, hint: Option<u32>, indexed: &[String]) -> R<String> {
    if idx == 0xff {
        let n = match hint {
            Some(h) => c.hinted(h)?,
            None => c.u32()?,
        };
        c.string(n as usize)
    } else {
        indexed.get(idx as usize).cloned().ok_or_else(|| "corrupt Alembic file: metadata index out of range".to_string())
    }
}

fn read_prop_headers(data: &[u8], count: usize, indexed: &[String]) -> R<Vec<PropHeader>> {
    let mut c = Cursor { b: data, p: 0 };
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let info = c.u32()?;
        let kind = match info & 3 {
            0 => Kind::Compound,
            1 => Kind::Scalar,
            _ => Kind::Array,
        };
        let hint = (info >> 2) & 3;
        let pod = ((info >> 4) & 0xf) as u8;
        let mut samples = 0;
        if kind != Kind::Compound {
            samples = c.hinted(hint)?;
            if info & 0x200 != 0 {
                c.hinted(hint)?;
                c.hinted(hint)?;
            }
            if info & 0x100 != 0 {
                c.hinted(hint)?;
            }
        }
        let n = c.hinted(hint)?;
        if n == 0 {
            return bad("property without a name");
        }
        let name = c.string(n as usize)?;
        let meta = meta_from(&mut c, ((info >> 20) & 0xff) as u8, Some(hint), indexed)?;
        out.push(PropHeader { name, kind, pod, samples, meta });
    }
    Ok(out)
}

struct ObjHeader {
    name: String,
    meta: String,
}

fn read_object_headers(data: &[u8], indexed: &[String]) -> R<Vec<ObjHeader>> {
    // The block ends with two 16 byte hashes.
    let body = data.len().saturating_sub(32);
    let mut c = Cursor { b: &data[..body], p: 0 };
    let mut out = Vec::new();
    while c.p < body {
        let n = c.u32()?;
        if n == 0 {
            return bad("object without a name");
        }
        let name = c.string(n as usize)?;
        let idx = c.u8()?;
        let meta = meta_from(&mut c, idx, None, indexed)?;
        out.push(ObjHeader { name, meta });
    }
    Ok(out)
}

struct Object {
    name: String,
    meta: String,
    group: Ref,
}

impl Ogawa<'_> {
    fn object_children(&self, obj: Ref, indexed: &[String]) -> R<Vec<Object>> {
        let ch = self.children(obj)?;
        let n = ch.len();
        if n < 2 || !ch[n - 1].is_data() {
            return Ok(Vec::new());
        }
        let headers = read_object_headers(self.data(ch[n - 1])?, indexed)?;
        if headers.len() != n - 2 {
            return bad("object header count does not match the child objects");
        }
        Ok(headers.into_iter().zip(&ch[1..n - 1]).map(|(h, &g)| Object { name: h.name, meta: h.meta, group: g }).collect())
    }

    /// The properties of a compound property group (an object's first child is one).
    fn compound(&self, g: Ref, indexed: &[String]) -> R<Vec<(PropHeader, Ref)>> {
        let ch = self.children(g)?;
        let Some((&last, props)) = ch.split_last() else { return Ok(Vec::new()) };
        let headers = read_prop_headers(self.data(last)?, props.len(), indexed)?;
        Ok(headers.into_iter().zip(props.iter().copied()).collect())
    }
}

fn find<'a>(props: &'a [(PropHeader, Ref)], name: &str) -> Option<&'a (PropHeader, Ref)> {
    props.iter().find(|(h, _)| h.name == name)
}

fn pod_size(pod: u8) -> R<usize> {
    Ok(match pod {
        0..=2 => 1,
        3 | 4 | 9 => 2,
        5 | 6 | 10 => 4,
        7 | 8 | 11 => 8,
        _ => return bad("unsupported property data type"),
    })
}

fn floats(bytes: &[u8], pod: u8) -> R<Vec<f64>> {
    match pod {
        10 => Ok(bytes.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64).collect()),
        11 => Ok(bytes.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()),
        _ => bad("expected floating point data"),
    }
}

fn ints(bytes: &[u8], pod: u8) -> R<Vec<i64>> {
    let size = pod_size(pod)?;
    let signed = matches!(pod, 2 | 4 | 6 | 8);
    if matches!(pod, 9..=11) {
        return bad("expected integer data");
    }
    Ok(bytes
        .chunks_exact(size)
        .map(|c| match (size, signed) {
            (1, false) => c[0] as i64,
            (1, true) => c[0] as i8 as i64,
            (2, false) => u16::from_le_bytes([c[0], c[1]]) as i64,
            (2, true) => i16::from_le_bytes([c[0], c[1]]) as i64,
            (4, false) => u32::from_le_bytes(c.try_into().unwrap()) as i64,
            (4, true) => i32::from_le_bytes(c.try_into().unwrap()) as i64,
            (_, false) => u64::from_le_bytes(c.try_into().unwrap()).min(i64::MAX as u64) as i64,
            (_, true) => i64::from_le_bytes(c.try_into().unwrap()),
        })
        .collect())
}

/// A geometry parameter: values plus optional indices and the declared scope.
struct Param {
    vals: Vec<f64>,
    indices: Option<Vec<i64>>,
    scope: Option<String>,
}

fn read_param(og: &Ogawa, entry: &(PropHeader, Ref), indexed: &[String]) -> R<Option<Param>> {
    let (hdr, r) = entry;
    let scope = hdr.meta.split(';').find_map(|kv| kv.strip_prefix("geoScope=")).map(str::to_string);
    let (vals_entry, idx_entry);
    let sub;
    if hdr.kind == Kind::Compound {
        sub = og.compound(*r, indexed)?;
        vals_entry = find(&sub, ".vals").cloned();
        idx_entry = find(&sub, ".indices").cloned();
    } else {
        vals_entry = Some(entry.clone());
        idx_entry = None;
    }
    let Some((vh, vr)) = vals_entry else { return Ok(None) };
    let vals = floats(og.sample0(vr)?, vh.pod)?;
    let indices = match idx_entry {
        Some((ih, ir)) => Some(ints(og.sample0(ir)?, ih.pod)?),
        None => None,
    };
    Ok(Some(Param { vals, indices, scope }))
}

/// Per-corner (indexed by the original corner order) values of a parameter, or `None` with a reason.
fn per_corner(p: &Param, comps: usize, corner_point: &[u32], corner_face: &[u32], faces: usize) -> Result<Vec<usize>, String> {
    let n = p.vals.len() / comps;
    let len = p.indices.as_ref().map_or(n, Vec::len);
    let points = corner_point.iter().copied().max().map_or(0, |m| m as usize + 1);
    let nc = corner_point.len();
    let scope = match p.scope.as_deref() {
        Some("fvr") => "fvr",
        Some("vtx") | Some("var") => "vtx",
        Some("uni") => "uni",
        Some("con") => "con",
        _ if len == nc => "fvr",
        _ if len >= points && len != faces => "vtx",
        _ if len == faces => "uni",
        _ => "?",
    };
    let slot = |i: usize| -> Result<usize, String> {
        let v = match &p.indices {
            Some(ix) => *ix.get(i).ok_or("index out of range")?,
            None => i as i64,
        };
        usize::try_from(v).ok().filter(|&v| v < n).ok_or_else(|| "value index out of range".to_string())
    };
    (0..nc)
        .map(|c| match scope {
            "fvr" if len == nc => slot(c),
            "vtx" if len > corner_point[c] as usize => slot(corner_point[c] as usize),
            "uni" if len > corner_face[c] as usize => slot(corner_face[c] as usize),
            "con" if len >= 1 => slot(0),
            _ => Err("size does not match the mesh".to_string()),
        })
        .collect()
}

fn xform_matrix(og: &Ogawa, props: &[(PropHeader, Ref)], indexed: &[String]) -> R<(Mat4, bool, bool)> {
    let Some((_, xr)) = find(props, ".xform") else { return Ok((Mat4::IDENTITY, true, false)) };
    let sub = og.compound(*xr, indexed)?;
    let inherits = match find(&sub, ".inherits") {
        Some((_, r)) => og.sample0(*r)?.first().is_none_or(|&b| b != 0),
        None => true,
    };
    let ops: Vec<u8> = match find(&sub, ".ops") {
        Some((_, r)) => og.sample0(*r)?.to_vec(),
        None => Vec::new(),
    };
    let (vals, animated) = match find(&sub, ".vals") {
        Some((h, r)) => (floats(og.sample0(*r)?, h.pod)?, h.samples > 1),
        None => (Vec::new(), false),
    };
    let mut m = Mat4::IDENTITY;
    let mut at = 0usize;
    for code in ops {
        let need = match code >> 4 {
            0 | 1 => 3,
            2 => 4,
            3 => 16,
            4..=6 => 1,
            _ => return bad("unknown transform operation"),
        };
        let v = vals.get(at..at + need).ok_or("corrupt Alembic file: transform values missing")?;
        at += need;
        let f = |i: usize| v[i] as f32;
        let op = match code >> 4 {
            0 => Mat4::from_scale(Vec3::new(f(0), f(1), f(2))),
            1 => Mat4::from_translation(Vec3::new(f(0), f(1), f(2))),
            2 => {
                let axis = Vec3::new(f(0), f(1), f(2)).normalize_or_zero();
                if axis == Vec3::ZERO { Mat4::IDENTITY } else { Mat4::from_axis_angle(axis, f(3).to_radians()) }
            }
            // Alembic stores matrices in row-vector order; reading the 16 values as glam columns
            // gives the column-vector matrix.
            3 => Mat4::from_cols_array(&std::array::from_fn(|i| v[i] as f32)),
            4 => Mat4::from_rotation_x(f(0).to_radians()),
            5 => Mat4::from_rotation_y(f(0).to_radians()),
            _ => Mat4::from_rotation_z(f(0).to_radians()),
        };
        m *= op;
    }
    Ok((m, inherits, animated))
}

fn schema_kind(meta: &str) -> &'static str {
    const KINDS: [(&str, &str); 9] = [
        ("AbcGeom_Xform", "xform"),
        ("AbcGeom_PolyMesh", "mesh"),
        ("AbcGeom_SubD", "mesh"),
        ("AbcGeom_Camera", "camera"),
        ("AbcGeom_Light", "light"),
        ("AbcGeom_Curves", "curves"),
        ("AbcGeom_Points", "points"),
        ("AbcGeom_NuPatch", "NURBS patch"),
        ("AbcGeom_FaceSet", "faceset"),
    ];
    KINDS.iter().find(|(s, _)| meta.contains(s)).map_or("other", |k| k.1)
}

/// Read an Alembic archive into the builder; mutates `warnings` with notes for skipped content.
pub(crate) fn load(bytes: &[u8], b: &mut MeshBuilder, warnings: &mut Vec<String>) -> R<()> {
    let (og, root) = Ogawa::open(bytes)?;
    let root_children = og.children(root)?;
    if root_children.len() < 3 {
        return bad("archive has no top-level object");
    }
    let indexed = read_indexed_metadata(&og, &root_children)?;

    let mut skipped: Vec<(&'static str, usize)> = Vec::new();
    let mut meshes = 0usize;
    let mut animated = false;
    let mut seen = 0usize;
    let mut stack: Vec<(Object, Mat4, usize)> = Vec::new();
    for o in og.object_children(root_children[2], &indexed)?.into_iter().rev() {
        stack.push((o, Mat4::IDENTITY, 1));
    }
    while let Some((obj, parent, depth)) = stack.pop() {
        seen += 1;
        if seen > MAX_OBJECTS || depth > MAX_DEPTH {
            return bad("object hierarchy too large or too deep");
        }
        let props = {
            let ch = og.children(obj.group)?;
            match ch.first() {
                Some(&p) if !p.is_data() => og.compound(p, &indexed)?,
                _ => Vec::new(),
            }
        };
        let mut world = parent;
        match schema_kind(&obj.meta) {
            "xform" => {
                let (local, inherits, anim) = xform_matrix(&og, &props, &indexed)?;
                animated |= anim;
                world = if inherits { parent * local } else { local };
            }
            "mesh" => {
                animated |= load_mesh(&og, &obj, &props, world, &indexed, b, warnings)?.1;
                meshes += 1;
            }
            "faceset" => {}
            kind @ ("camera" | "light" | "curves" | "points" | "NURBS patch") => match skipped.iter_mut().find(|s| s.0 == kind) {
                Some(s) => s.1 += 1,
                None => skipped.push((kind, 1)),
            },
            _ => {}
        }
        for c in og.object_children(obj.group, &indexed)?.into_iter().rev() {
            stack.push((c, world, depth + 1));
        }
    }
    if meshes == 0 {
        return Err("the Alembic file contains no polygon meshes".into());
    }
    for (kind, n) in skipped {
        warnings.push(format!("skipped {n} Alembic {kind} object{} (only meshes are imported)", if n == 1 { "" } else { "s" }));
    }
    if animated {
        warnings.push("the Alembic file is animated; imported the first sample only".into());
    }
    Ok(())
}

/// Returns (triangles pushed, animated).
fn load_mesh(
    og: &Ogawa,
    obj: &Object,
    props: &[(PropHeader, Ref)],
    world: Mat4,
    indexed: &[String],
    b: &mut MeshBuilder,
    warnings: &mut Vec<String>,
) -> R<(usize, bool)> {
    let Some((_, gr)) = find(props, ".geom") else {
        warnings.push(format!("'{}' has no geometry data; skipped", obj.name));
        return Ok((0, false));
    };
    let geom = og.compound(*gr, indexed)?;
    let get = |name: &str| find(&geom, name);
    let (Some(p), Some(fi), Some(fc)) = (get("P"), get(".faceIndices"), get(".faceCounts")) else {
        warnings.push(format!("'{}' is missing positions or face data; skipped", obj.name));
        return Ok((0, false));
    };
    let animated = p.0.samples > 1;
    let pos_raw = floats(og.sample0(p.1)?, p.0.pod)?;
    if pos_raw.len() % 3 != 0 {
        return bad("position array is not a multiple of 3");
    }
    let positions: Vec<Vec3> = pos_raw.chunks_exact(3).map(|c| world.transform_point3(Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32))).collect();
    let face_indices = ints(og.sample0(fi.1)?, fi.0.pod)?;
    let face_counts = ints(og.sample0(fc.1)?, fc.0.pod)?;
    let mut corner_point = Vec::with_capacity(face_indices.len());
    for &i in &face_indices {
        match u32::try_from(i) {
            Ok(i) if (i as usize) < positions.len() => corner_point.push(i),
            _ => return bad("face index outside the point array"),
        }
    }
    let mut corner_face = Vec::with_capacity(face_indices.len());
    let mut total = 0usize;
    for (f, &n) in face_counts.iter().enumerate() {
        let n = usize::try_from(n).map_err(|_| "corrupt Alembic file: negative face size".to_string())?;
        total = total.checked_add(n).filter(|&t| t <= corner_point.len()).ok_or("corrupt Alembic file: face counts exceed the face indices")?;
        corner_face.extend(std::iter::repeat_n(f as u32, n));
    }
    if total != corner_point.len() {
        return bad("face counts do not add up to the face index count");
    }

    let resolve = |name: &str, comps: usize, warnings: &mut Vec<String>| -> R<Option<(Vec<f64>, Vec<usize>)>> {
        let Some(entry) = get(name) else { return Ok(None) };
        let Some(param) = read_param(og, entry, indexed)? else { return Ok(None) };
        if param.vals.len() % comps != 0 {
            return bad("geometry parameter has a partial value");
        }
        match per_corner(&param, comps, &corner_point, &corner_face, face_counts.len()) {
            Ok(slots) => Ok(Some((param.vals, slots))),
            Err(e) => {
                warnings.push(format!("'{}': ignored '{name}' ({e})", obj.name));
                Ok(None)
            }
        }
    };
    let uvs = resolve("uv", 2, warnings)?;
    let normals = resolve("N", 3, warnings)?;

    let normal_m = Mat3::from_mat4(world).inverse().transpose();
    let flip = world.determinant() < 0.0;
    b.object(&obj.name);
    let mat = b.material("DefaultMaterial");
    let corner = |c: usize| Corner {
        position: positions[corner_point[c] as usize],
        normal: normals.as_ref().map(|(v, s)| {
            let k = 3 * s[c];
            (normal_m * Vec3::new(v[k] as f32, v[k + 1] as f32, v[k + 2] as f32)).normalize_or_zero()
        }),
        // Alembic UVs have their origin at the bottom-left.
        uv: uvs.as_ref().map(|(v, s)| Vec2::new(v[2 * s[c]] as f32, 1.0 - v[2 * s[c] + 1] as f32)),
    };
    let mut tris = 0;
    let mut start = 0usize;
    for &n in &face_counts {
        let n = n as usize;
        // Alembic winds faces clockwise; walk each face backwards for SubCraft's CCW front faces.
        let order: Vec<usize> = (start..start + n).rev().collect();
        start += n;
        for k in 1..n.saturating_sub(1) {
            let c = [corner(order[0]), corner(order[k]), corner(order[k + 1])];
            b.push_triangle(mat, if flip { [c[0], c[2], c[1]] } else { c });
            tris += 1;
        }
    }
    Ok((tris, animated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{ImportError, import_mesh};
    use crate::mesh::BuildError;

    // ---- a tiny Ogawa writer, enough to build fixtures ----

    struct W {
        buf: Vec<u8>,
    }

    impl W {
        fn new() -> Self {
            let mut buf = b"Ogawa\xff\x00\x00".to_vec();
            buf.extend_from_slice(&[0; 8]);
            Self { buf }
        }
        fn data(&mut self, bytes: &[u8]) -> u64 {
            if bytes.is_empty() {
                return DATA_BIT;
            }
            let pos = self.buf.len() as u64;
            self.buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            self.buf.extend_from_slice(bytes);
            pos | DATA_BIT
        }
        fn group(&mut self, ch: &[u64]) -> u64 {
            let pos = self.buf.len() as u64;
            self.buf.extend_from_slice(&(ch.len() as u64).to_le_bytes());
            for c in ch {
                self.buf.extend_from_slice(&c.to_le_bytes());
            }
            pos
        }
        fn finish(mut self, root: u64) -> Vec<u8> {
            self.buf[8..16].copy_from_slice(&root.to_le_bytes());
            self.buf
        }
    }

    fn keyed(bytes: &[u8]) -> Vec<u8> {
        let mut v = vec![0xab; 16];
        v.extend_from_slice(bytes);
        v
    }

    fn array(w: &mut W, bytes: &[u8], count: u64) -> u64 {
        let d = w.data(&keyed(bytes));
        let dims = w.data(&count.to_le_bytes());
        w.group(&[d, dims])
    }

    fn scalar(w: &mut W, bytes: &[u8]) -> u64 {
        let d = w.data(&keyed(bytes));
        w.group(&[d])
    }

    fn f32s(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }
    fn f64s(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }
    fn i32s(v: &[i32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }
    fn u32s(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    const BOOL: u32 = 0;
    const U8: u32 = 1;
    const U32: u32 = 5;
    const I32: u32 = 6;
    const F32: u32 = 10;
    const F64: u32 = 11;

    /// Header of one property. `hint` selects the integer width (0 = u8, 1 = u16, 2 = u32).
    fn ph(kind: u32, pod: u32, extent: u32, name: &str, meta: &str, hint: u32) -> Vec<u8> {
        let mut info = kind | (hint << 2) | (pod << 4) | (extent << 12) | (0xff << 20);
        if kind != 0 {
            info |= 0x800;
        }
        let mut v = info.to_le_bytes().to_vec();
        let put = |v: &mut Vec<u8>, n: usize| match hint {
            0 => v.push(n as u8),
            1 => v.extend_from_slice(&(n as u16).to_le_bytes()),
            _ => v.extend_from_slice(&(n as u32).to_le_bytes()),
        };
        if kind != 0 {
            put(&mut v, 1);
        }
        put(&mut v, name.len());
        v.extend_from_slice(name.as_bytes());
        put(&mut v, meta.len());
        v.extend_from_slice(meta.as_bytes());
        v
    }

    fn compound(w: &mut W, props: &[(Vec<u8>, u64)]) -> u64 {
        let headers: Vec<u8> = props.iter().flat_map(|p| p.0.clone()).collect();
        let h = w.data(&headers);
        let mut ch: Vec<u64> = props.iter().map(|p| p.1).collect();
        ch.push(h);
        w.group(&ch)
    }

    fn object(w: &mut W, props: u64, children: &[(&str, &str, u64)]) -> u64 {
        let mut ch = vec![props];
        let mut headers = Vec::new();
        for (name, meta, g) in children {
            ch.push(*g);
            headers.extend_from_slice(&(name.len() as u32).to_le_bytes());
            headers.extend_from_slice(name.as_bytes());
            headers.push(0xff);
            headers.extend_from_slice(&(meta.len() as u32).to_le_bytes());
            headers.extend_from_slice(meta.as_bytes());
        }
        if !children.is_empty() {
            headers.extend_from_slice(&[0; 32]);
            ch.push(w.data(&headers));
        }
        w.group(&ch)
    }

    fn archive(w: &mut W, top: u64) -> Vec<u8> {
        let v = w.data(&0u32.to_le_bytes());
        let fv = w.data(&1u32.to_le_bytes());
        let e = w.data(&[]);
        let root = w.group(&[v, fv, top, e, e, e]);
        std::mem::replace(w, W::new()).finish(root)
    }

    struct MeshSpec<'a> {
        p: Vec<f32>,
        counts: Vec<i32>,
        idx: Vec<i32>,
        uv: Option<(Vec<f32>, Option<Vec<u32>>, &'a str)>,
        n: Option<(Vec<f32>, &'a str)>,
    }

    fn mesh_object(w: &mut W, s: &MeshSpec) -> u64 {
        let p = array(w, &f32s(&s.p), (s.p.len() / 3) as u64);
        let fi = array(w, &i32s(&s.idx), s.idx.len() as u64);
        let fc = array(w, &i32s(&s.counts), s.counts.len() as u64);
        let mut props = vec![
            (ph(2, F32, 3, "P", "", 0), p),
            (ph(2, I32, 1, ".faceIndices", "", 0), fi),
            (ph(2, I32, 1, ".faceCounts", "", 0), fc),
        ];
        if let Some((v, ix, scope)) = &s.uv {
            let vals = array(w, &f32s(v), (v.len() / 2) as u64);
            let mut sub = vec![(ph(2, F32, 2, ".vals", "", 0), vals)];
            if let Some(ix) = ix {
                let a = array(w, &u32s(ix), ix.len() as u64);
                sub.push((ph(2, U32, 1, ".indices", "", 0), a));
            }
            let c = compound(w, &sub);
            props.push((ph(0, 0, 0, "uv", &format!("geoScope={scope};"), 1), c));
        }
        if let Some((v, scope)) = &s.n {
            let vals = array(w, &f32s(v), (v.len() / 3) as u64);
            let c = compound(w, &[(ph(2, F32, 3, ".vals", "", 0), vals)]);
            props.push((ph(0, 0, 0, "N", &format!("geoScope={scope};"), 2), c));
        }
        let geom = compound(w, &props);
        let top = compound(w, &[(ph(0, 0, 0, ".geom", "schema=AbcGeom_PolyMesh_v1;", 0), geom)]);
        object(w, top, &[])
    }

    fn xform_object(w: &mut W, ops: &[u8], vals: &[f64], inherits: bool, children: &[(&str, &str, u64)]) -> u64 {
        let o = array(w, ops, ops.len() as u64);
        let v = array(w, &f64s(vals), vals.len() as u64);
        let i = scalar(w, &[inherits as u8]);
        let x = compound(
            w,
            &[(ph(2, U8, 1, ".ops", "", 0), o), (ph(2, F64, 1, ".vals", "", 0), v), (ph(1, BOOL, 1, ".inherits", "", 0), i)],
        );
        let top = compound(w, &[(ph(0, 0, 0, ".xform", "schema=AbcGeom_Xform_v3;", 0), x)]);
        object(w, top, children)
    }

    const MESH: &str = "schema=AbcGeom_PolyMesh_v1;schemaBaseType=AbcGeom_GeomBase;";
    const XFORM: &str = "schema=AbcGeom_Xform_v3;";

    fn empty_props(w: &mut W) -> u64 {
        compound(w, &[])
    }

    fn write_tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("subcraft-abc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn quad() -> MeshSpec<'static> {
        // Listed clockwise seen from +Y, as Alembic winds front faces.
        MeshSpec {
            p: vec![0., 0., 0., 1., 0., 0., 1., 0., 1., 0., 0., 1.],
            counts: vec![4],
            idx: vec![0, 1, 2, 3],
            uv: Some((vec![0., 0., 1., 0., 1., 1., 0., 1.], None, "fvr")),
            n: Some((vec![0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1., 0.], "vtx")),
        }
    }

    fn single_mesh_file(spec: &MeshSpec) -> Vec<u8> {
        let mut w = W::new();
        let m = mesh_object(&mut w, spec);
        let props = empty_props(&mut w);
        let top = object(&mut w, props, &[("quad", MESH, m)]);
        archive(&mut w, top)
    }

    #[test]
    fn quad_is_triangulated_with_uvs_normals_and_name() {
        let path = write_tmp("quad.abc", &single_mesh_file(&quad()));
        let rep = import_mesh(&path).unwrap();
        assert_eq!(rep.mesh.triangle_count(), 2);
        assert_eq!(rep.mesh.objects.len(), 1);
        assert_eq!(rep.mesh.objects[0].name, "quad");
        assert!(rep.mesh.normals.iter().all(|n| (n - Vec3::Y).length() < 1e-5));
        let (mut lo, mut hi) = (Vec2::splat(9.), Vec2::splat(-9.));
        for uv in &rep.mesh.uvs {
            lo = lo.min(*uv);
            hi = hi.max(*uv);
        }
        // V is flipped from Alembic's bottom-left origin.
        assert_eq!((lo, hi), (Vec2::ZERO, Vec2::ONE));
        // The clockwise Alembic face ends up counter-clockwise around +Y.
        let t = rep.mesh.tri_positions(0);
        assert!((t[1] - t[0]).cross(t[2] - t[0]).y > 0.0);
    }

    #[test]
    fn transforms_accumulate_and_inherit() {
        let mut w = W::new();
        let m = mesh_object(&mut w, &quad());
        // child: translate (0,10,0); parent: scale 2 then translate (1,2,3) listed as [translate, scale].
        let child = xform_object(&mut w, &[0x10], &[0., 10., 0.], true, &[("quad", MESH, m)]);
        let parent = xform_object(&mut w, &[0x10, 0x00], &[1., 2., 3., 2., 2., 2.], true, &[("child", XFORM, child)]);
        let props = empty_props(&mut w);
        let top = object(&mut w, props, &[("parent", XFORM, parent)]);
        let path = write_tmp("xf.abc", &archive(&mut w, top));
        let rep = import_mesh(&path).unwrap();
        let bb = rep.mesh.bounds;
        // local (0..1) -> scaled (0..2) -> child translate happens before the parent scale: y = (0 + 10) * 2 + 2.
        assert!((bb.min - Vec3::new(1., 22., 3.)).length() < 1e-4, "{:?}", bb.min);
        assert!((bb.max - Vec3::new(3., 22., 5.)).length() < 1e-4, "{:?}", bb.max);
    }

    #[test]
    fn non_inheriting_xform_and_rotation_op() {
        let mut w = W::new();
        let m = mesh_object(&mut w, &quad());
        // rotate 90 degrees about Z (op 6), not inheriting the parent's translation
        let child = xform_object(&mut w, &[0x60], &[90.], false, &[("quad", MESH, m)]);
        let parent = xform_object(&mut w, &[0x10], &[100., 0., 0.], true, &[("child", XFORM, child)]);
        let props = empty_props(&mut w);
        let top = object(&mut w, props, &[("parent", XFORM, parent)]);
        let path = write_tmp("xf2.abc", &archive(&mut w, top));
        let bb = import_mesh(&path).unwrap().mesh.bounds;
        // (x, y, z) -> (-y, x, z): x range becomes 0, y range 0..1, no +100 offset.
        assert!(bb.max.x.abs() < 1e-4 && bb.min.x.abs() < 1e-4, "{bb:?}");
        assert!((bb.max.y - 1.).abs() < 1e-4);
    }

    #[test]
    fn indexed_vertex_scope_uvs_and_multiple_meshes() {
        let mut w = W::new();
        let tri = MeshSpec {
            p: vec![0., 0., 0., 0., 0., 1., 1., 0., 0.],
            counts: vec![3],
            idx: vec![0, 1, 2],
            // two distinct values referenced through vertex-scope indices
            uv: Some((vec![0.25, 0.5, 0.75, 0.5], Some(vec![0, 1, 1]), "vtx")),
            n: None,
        };
        let a = mesh_object(&mut w, &tri);
        let b = mesh_object(&mut w, &tri);
        let props = empty_props(&mut w);
        let top = object(&mut w, props, &[("a", MESH, a), ("b", MESH, b)]);
        let path = write_tmp("two.abc", &archive(&mut w, top));
        let rep = import_mesh(&path).unwrap();
        assert_eq!(rep.mesh.objects.len(), 2);
        assert_eq!(rep.mesh.triangle_count(), 2);
        assert!(rep.mesh.uvs.iter().all(|uv| (0.25..=0.75).contains(&uv.x) && (uv.y - 0.5).abs() < 1e-6));
    }

    #[test]
    fn ngon_fan_and_counts() {
        let spec = MeshSpec {
            p: vec![0., 0., 0., 0., 0., 1., 1., 0., 1., 2., 0., 0.5, 1., 0., 0.],
            counts: vec![5],
            idx: vec![0, 1, 2, 3, 4],
            uv: Some((vec![0., 0., 0., 1., 1., 1., 2., 0.5, 1., 0.], None, "vtx")),
            n: None,
        };
        let rep = import_mesh(&write_tmp("ngon.abc", &single_mesh_file(&spec))).unwrap();
        assert_eq!(rep.mesh.triangle_count(), 3);
        assert_eq!(rep.mesh.positions.len(), 5);
    }

    #[test]
    fn missing_uvs_follow_existing_behaviour() {
        let mut spec = quad();
        spec.uv = None;
        let path = write_tmp("nouv.abc", &single_mesh_file(&spec));
        assert!(matches!(import_mesh(&path), Err(ImportError::Build(BuildError::NoUvs))));
        assert_eq!(crate::import_mesh_for_baking(&path).unwrap().mesh.triangle_count(), 2);
    }

    #[test]
    fn non_mesh_objects_are_listed_as_notes() {
        let mut w = W::new();
        let m = mesh_object(&mut w, &quad());
        let cam_props = empty_props(&mut w);
        let cam = object(&mut w, cam_props, &[]);
        let props = empty_props(&mut w);
        let top = object(&mut w, props, &[("quad", MESH, m), ("cam", "schema=AbcGeom_Camera_v1;", cam)]);
        let rep = import_mesh(&write_tmp("cam.abc", &archive(&mut w, top))).unwrap();
        assert!(rep.warnings.iter().any(|w| w.contains("camera")), "{:?}", rep.warnings);
    }

    #[test]
    fn broken_files_are_errors_not_panics() {
        let good = single_mesh_file(&quad());
        for len in 0..good.len() {
            let mut b = MeshBuilder::new("t");
            let _ = load(&good[..len], &mut b, &mut Vec::new());
        }
        // every single-byte corruption must stay a clean result
        for i in 16..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 0xff;
            let mut b = MeshBuilder::new("t");
            let _ = load(&bad, &mut b, &mut Vec::new());
        }
        let mut b = MeshBuilder::new("t");
        assert!(load(&good[..good.len() - 20], &mut b, &mut Vec::new()).is_err());
        assert!(load(b"not an alembic file at all", &mut b, &mut Vec::new()).is_err());
        let mut unfrozen = good.clone();
        unfrozen[5] = 0;
        assert!(load(&unfrozen, &mut b, &mut Vec::new()).unwrap_err().contains("not closed"));
    }

    #[test]
    fn hdf5_container_gets_a_clear_message() {
        let mut b = MeshBuilder::new("t");
        let mut bytes = HDF5_SIGNATURE.to_vec();
        bytes.extend_from_slice(&[0; 64]);
        assert!(load(&bytes, &mut b, &mut Vec::new()).unwrap_err().contains("HDF5"));
    }

    #[test]
    fn bad_indices_are_rejected() {
        let mut spec = quad();
        spec.idx = vec![0, 1, 2, 9];
        let mut b = MeshBuilder::new("t");
        assert!(load(&single_mesh_file(&spec), &mut b, &mut Vec::new()).is_err());
        let mut spec = quad();
        spec.counts = vec![5];
        assert!(load(&single_mesh_file(&spec), &mut b, &mut Vec::new()).is_err());
    }
}
