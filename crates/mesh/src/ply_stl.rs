//! PLY (ascii, binary little/big endian) and STL (ascii, binary) readers. Hand written: both formats are small and
//! the input is untrusted, so every count is checked against the bytes that remain and nothing panics on bad data.

use std::collections::HashMap;
use std::path::Path;

use glam::{Vec2, Vec3};

use crate::mesh::{Corner, MeshBuilder};

type R<T> = Result<T, String>;

const NO_UV_WARNING: &str = "no UVs, use auto unwrap or provide UVs";

/// Corner normals of an STL average the neighbouring faces within this angle (cosine); sharper edges stay hard.
const STL_CREASE_COS: f32 = 0.5;

// ---------------------------------------------------------------- STL

pub fn load_stl(path: &Path, b: &mut MeshBuilder, warnings: &mut Vec<String>) -> R<()> {
    let bytes = subcraft_core::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Mesh");
    let tris = parse_stl(&bytes, warnings)?;
    if tris.is_empty() {
        return Err("the file contains no triangles".into());
    }
    emit_welded(&tris, name, b);
    warnings.push(format!("STL has {NO_UV_WARNING}"));
    Ok(())
}

/// Triangles as three positions each.
pub fn parse_stl(bytes: &[u8], warnings: &mut Vec<String>) -> R<Vec<[Vec3; 3]>> {
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        let need = 84usize.saturating_add(n.saturating_mul(50));
        if need == bytes.len() {
            return Ok(parse_stl_binary(bytes, n));
        }
        let looks_ascii = starts_with_solid(bytes) && find_ignore_case(bytes, b"facet").is_some();
        if !looks_ascii {
            if bytes.len() < need {
                return Err(format!("binary STL is truncated: header says {n} triangles ({need} bytes), file has {}", bytes.len()));
            }
            warnings.push(format!("binary STL has {} bytes of trailing data, ignored", bytes.len() - need));
            return Ok(parse_stl_binary(bytes, n));
        }
    }
    if starts_with_solid(bytes) || find_ignore_case(bytes, b"facet").is_some() {
        return parse_stl_ascii(bytes);
    }
    Err("not an STL file (no 'solid' header and the size does not match a binary STL)".into())
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// ASCII keywords are matched without case (the tokenizer lowercases them too).
fn find_ignore_case(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w.eq_ignore_ascii_case(needle))
}

/// An ASCII STL starts with `solid`, possibly after a byte-order mark and white space, in any case.
fn starts_with_solid(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(bytes.len());
    bytes[start..].get(..5).is_some_and(|w| w.eq_ignore_ascii_case(b"solid"))
}

fn parse_stl_binary(bytes: &[u8], n: usize) -> Vec<[Vec3; 3]> {
    let mut out = Vec::with_capacity(n.min(bytes.len() / 50));
    for rec in bytes[84..].chunks_exact(50).take(n) {
        let f = |o: usize| f32::from_le_bytes([rec[o], rec[o + 1], rec[o + 2], rec[o + 3]]);
        let v = |o: usize| Vec3::new(f(o), f(o + 4), f(o + 8));
        // Bytes 0..12 are the facet normal; it is recomputed from the winding instead.
        out.push([v(12), v(24), v(36)]);
    }
    out
}

fn parse_stl_ascii(bytes: &[u8]) -> R<Vec<[Vec3; 3]>> {
    let text = String::from_utf8_lossy(bytes);
    let mut it = text.split_ascii_whitespace();
    let mut verts: Vec<Vec3> = Vec::new();
    let mut facets = 0usize;
    while let Some(tok) = it.next() {
        match tok.to_ascii_lowercase().as_str() {
            "facet" => facets += 1,
            "vertex" => {
                let mut c = [0f32; 3];
                for slot in &mut c {
                    let t = it.next().ok_or("ascii STL ends inside a vertex")?;
                    *slot = t.parse().map_err(|_| format!("ascii STL: bad number '{}'", t.chars().take(24).collect::<String>()))?;
                }
                verts.push(Vec3::from(c));
            }
            _ => {}
        }
    }
    if verts.len() != facets * 3 {
        return Err(format!("ascii STL is truncated or malformed: {facets} facets but {} vertices", verts.len()));
    }
    Ok(verts.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect())
}

/// Weld triangles by exact position and give each corner the area-weighted normal of the neighbouring faces within the
/// crease angle, so a scanned surface is smooth and a CAD cube keeps its hard edges.
fn emit_welded(tris: &[[Vec3; 3]], name: &str, b: &mut MeshBuilder) {
    // `+ 0.0` turns -0.0 into 0.0, which is the same position with different bits.
    let key = |p: Vec3| [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()];
    let mut index: HashMap<[u32; 3], u32> = HashMap::new();
    let mut ids: Vec<[u32; 3]> = Vec::with_capacity(tris.len());
    for t in tris {
        let mut id = [0u32; 3];
        for (k, p) in t.iter().enumerate() {
            let next = index.len() as u32;
            id[k] = *index.entry(key(*p)).or_insert(next);
        }
        ids.push(id);
    }
    let face_n: Vec<Vec3> = tris.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0])).collect();
    let mut around: Vec<Vec<u32>> = vec![Vec::new(); index.len()];
    for (f, id) in ids.iter().enumerate() {
        for &v in id {
            around[v as usize].push(f as u32);
        }
    }
    b.object(name);
    let mat = b.material("DefaultMaterial");
    for (f, t) in tris.iter().enumerate() {
        let own = face_n[f].normalize_or_zero();
        let corner = |k: usize| {
            let mut n = Vec3::ZERO;
            for &g in &around[ids[f][k] as usize] {
                let gn = face_n[g as usize];
                if own.dot(gn.normalize_or_zero()) >= STL_CREASE_COS {
                    n += gn;
                }
            }
            let n = n.normalize_or_zero();
            Corner { position: t[k], normal: (n != Vec3::ZERO).then_some(n), uv: None }
        };
        b.push_triangle(mat, [corner(0), corner(1), corner(2)]);
    }
}

// ---------------------------------------------------------------- PLY

#[derive(Clone, Copy, PartialEq)]
enum Ty {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Ty {
    fn parse(s: &str) -> R<Ty> {
        Ok(match s {
            "char" | "int8" => Ty::I8,
            "uchar" | "uint8" => Ty::U8,
            "short" | "int16" => Ty::I16,
            "ushort" | "uint16" => Ty::U16,
            "int" | "int32" => Ty::I32,
            "uint" | "uint32" => Ty::U32,
            "float" | "float32" => Ty::F32,
            "double" | "float64" => Ty::F64,
            _ => return Err(format!("unknown PLY property type '{s}'")),
        })
    }
    fn size(self) -> usize {
        match self {
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 => 4,
            Ty::F64 => 8,
        }
    }
}

struct Prop {
    name: String,
    ty: Ty,
    /// Count type when the property is a list.
    list: Option<Ty>,
}

struct Element {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

enum Src<'a> {
    Ascii(std::str::SplitAsciiWhitespace<'a>),
    Bin { data: &'a [u8], pos: usize, big: bool },
}

impl Src<'_> {
    fn scalar(&mut self, ty: Ty) -> R<f64> {
        match self {
            Src::Ascii(it) => {
                let t = it.next().ok_or("PLY data ends early (truncated file)")?;
                t.parse::<f64>().map_err(|_| format!("PLY: bad number '{}'", t.chars().take(24).collect::<String>()))
            }
            Src::Bin { data, pos, big } => {
                let n = ty.size();
                let raw = data.get(*pos..*pos + n).ok_or("PLY data ends early (truncated file)")?;
                *pos += n;
                let mut a = [0u8; 8];
                a[..n].copy_from_slice(raw);
                if *big {
                    a[..n].reverse();
                }
                Ok(match ty {
                    Ty::I8 => a[0] as i8 as f64,
                    Ty::U8 => a[0] as f64,
                    Ty::I16 => i16::from_le_bytes([a[0], a[1]]) as f64,
                    Ty::U16 => u16::from_le_bytes([a[0], a[1]]) as f64,
                    Ty::I32 => i32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
                    Ty::U32 => u32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
                    Ty::F32 => f32::from_le_bytes([a[0], a[1], a[2], a[3]]) as f64,
                    Ty::F64 => f64::from_le_bytes(a),
                })
            }
        }
    }

    /// Upper bound on how many more values the file can hold; a count above it is corrupt.
    /// Text has no cheap bound (counting the remaining tokens for every face made the import
    /// quadratic); reading past the end fails there anyway.
    fn remaining(&self) -> usize {
        match self {
            Src::Ascii(_) => usize::MAX,
            Src::Bin { data, pos, .. } => data.len().saturating_sub(*pos),
        }
    }

    /// Read a list count and check it against what is left of the file.
    fn list_len(&mut self, ct: Ty) -> R<usize> {
        let n = self.scalar(ct)?;
        if !(0.0..=1.0e7).contains(&n) || n as usize > self.remaining() {
            return Err("PLY list is longer than the file (truncated or corrupt)".into());
        }
        Ok(n as usize)
    }
}

pub fn load_ply(path: &Path, b: &mut MeshBuilder, warnings: &mut Vec<String>) -> R<()> {
    let bytes = subcraft_core::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Mesh");
    parse_ply(&bytes, name, b, warnings)
}

pub fn parse_ply(bytes: &[u8], name: &str, b: &mut MeshBuilder, warnings: &mut Vec<String>) -> R<()> {
    if !bytes.starts_with(b"ply") {
        return Err("not a PLY file (missing 'ply' magic)".into());
    }
    let end = find(bytes, b"end_header").ok_or("PLY header has no end_header")?;
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| "PLY header is not text")?;
    let mut body_start = end + "end_header".len();
    if bytes.get(body_start) == Some(&b'\r') {
        body_start += 1;
    }
    if bytes.get(body_start) == Some(&b'\n') {
        body_start += 1;
    }
    let body = &bytes[body_start.min(bytes.len())..];

    let mut format = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in header.lines().skip(1) {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["format", f, _] => format = Some(*f),
            ["element", n, c] => {
                let count = c.parse::<usize>().map_err(|_| format!("bad PLY element count '{c}'"))?;
                elements.push(Element { name: n.to_string(), count, props: Vec::new() });
            }
            ["property", "list", ct, it, n] => {
                let e = elements.last_mut().ok_or("PLY property before any element")?;
                e.props.push(Prop { name: n.to_string(), ty: Ty::parse(it)?, list: Some(Ty::parse(ct)?) });
            }
            ["property", t, n] => {
                let e = elements.last_mut().ok_or("PLY property before any element")?;
                e.props.push(Prop { name: n.to_string(), ty: Ty::parse(t)?, list: None });
            }
            _ => {}
        }
    }
    let mut src = match format.ok_or("PLY header has no format line")? {
        "ascii" => Src::Ascii(std::str::from_utf8(body).map_err(|_| "ascii PLY body is not text")?.split_ascii_whitespace()),
        "binary_little_endian" => Src::Bin { data: body, pos: 0, big: false },
        "binary_big_endian" => Src::Bin { data: body, pos: 0, big: true },
        other => return Err(format!("unknown PLY format '{other}'")),
    };

    let mut positions: Vec<Vec3> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut uvs: Vec<Vec2> = Vec::new();
    let mut has_color = false;
    // Each face: vertex indices, plus per-corner UVs when the face carries a texcoord list.
    let mut faces: Vec<(Vec<u32>, Option<Vec<Vec2>>)> = Vec::new();

    for el in &elements {
        match el.name.as_str() {
            "vertex" => {
                let idx = |names: &[&str]| el.props.iter().position(|p| p.list.is_none() && names.contains(&p.name.as_str()));
                let (Some(ix), Some(iy), Some(iz)) = (idx(&["x"]), idx(&["y"]), idx(&["z"])) else {
                    return Err("PLY vertex element has no x/y/z".into());
                };
                let inn = (idx(&["nx"]), idx(&["ny"]), idx(&["nz"]));
                let iuv = (idx(&["s", "u", "texture_u", "texture_s"]), idx(&["t", "v", "texture_v", "texture_t"]));
                has_color = idx(&["red", "r"]).is_some() && idx(&["green", "g"]).is_some();
                if matches!(src, Src::Bin { .. }) && el.count > src.remaining() {
                    return Err(format!("PLY says {} vertices but the file is too small (truncated)", el.count));
                }
                let mut vals = vec![0f64; el.props.len()];
                for _ in 0..el.count {
                    for (k, p) in el.props.iter().enumerate() {
                        if let Some(ct) = p.list {
                            for _ in 0..src.list_len(ct)? {
                                src.scalar(p.ty)?;
                            }
                        } else {
                            vals[k] = src.scalar(p.ty)?;
                        }
                    }
                    positions.push(Vec3::new(vals[ix] as f32, vals[iy] as f32, vals[iz] as f32));
                    if let (Some(a), Some(b2), Some(c)) = inn {
                        normals.push(Vec3::new(vals[a] as f32, vals[b2] as f32, vals[c] as f32));
                    }
                    if let (Some(u), Some(v)) = iuv {
                        // Same bottom-left origin as OBJ.
                        uvs.push(Vec2::new(vals[u] as f32, 1.0 - vals[v] as f32));
                    }
                }
            }
            "face" => {
                if matches!(src, Src::Bin { .. }) && el.count > src.remaining() {
                    return Err(format!("PLY says {} faces but the file is too small (truncated)", el.count));
                }
                for _ in 0..el.count {
                    let mut vi: Option<Vec<u32>> = None;
                    let mut tc: Option<Vec<Vec2>> = None;
                    for p in &el.props {
                        let Some(ct) = p.list else {
                            src.scalar(p.ty)?;
                            continue;
                        };
                        let n = src.list_len(ct)?;
                        let mut list = Vec::with_capacity(n);
                        for _ in 0..n {
                            list.push(src.scalar(p.ty)?);
                        }
                        match p.name.as_str() {
                            // A negative, fractional or NaN index is out of range (`as u32` alone would turn -1 into vertex 0).
                            "vertex_indices" | "vertex_index" => vi = Some(list.iter().map(|&x| if x >= 0.0 && x.fract() == 0.0 && x < u32::MAX as f64 { x as u32 } else { u32::MAX }).collect()),
                            "texcoord" => {
                                tc = Some(list.chunks_exact(2).map(|c| Vec2::new(c[0] as f32, 1.0 - c[1] as f32)).collect())
                            }
                            _ => {}
                        }
                    }
                    if let Some(v) = vi {
                        faces.push((v, tc));
                    }
                }
            }
            _ => {
                // Unknown element (edges, materials ...): read and discard so later elements stay aligned.
                for _ in 0..el.count {
                    for p in &el.props {
                        if let Some(ct) = p.list {
                            for _ in 0..src.list_len(ct)? {
                                src.scalar(p.ty)?;
                            }
                        } else {
                            src.scalar(p.ty)?;
                        }
                    }
                }
            }
        }
    }

    if positions.is_empty() {
        return Err("PLY has no vertices".into());
    }
    if faces.is_empty() {
        return Err("PLY has no faces (a point cloud cannot be painted)".into());
    }
    let has_n = normals.len() == positions.len();
    let has_vuv = uvs.len() == positions.len();
    let mut skipped = 0usize;
    let mut face_uv_used = false;
    b.object(name);
    let mat = b.material("DefaultMaterial");
    for (poly, tc) in &faces {
        if poly.len() < 3 || poly.iter().any(|&i| i as usize >= positions.len()) {
            skipped += 1;
            continue;
        }
        let use_tc = tc.as_ref().filter(|t| t.len() == poly.len());
        face_uv_used |= use_tc.is_some();
        let corner = |k: usize| {
            let i = poly[k] as usize;
            Corner {
                position: positions[i],
                normal: has_n.then(|| normals[i]),
                uv: use_tc.map(|t| t[k]).or_else(|| has_vuv.then(|| uvs[i])),
            }
        };
        for k in 1..poly.len() - 1 {
            b.push_triangle(mat, [corner(0), corner(k), corner(k + 1)]);
        }
    }
    if skipped > 0 {
        warnings.push(format!("skipped {skipped} PLY faces with fewer than 3 vertices or out-of-range indices"));
    }
    if !has_vuv && !face_uv_used {
        warnings.push(format!("PLY has {NO_UV_WARNING}"));
    }
    if has_color {
        warnings.push("PLY vertex colors were read but SubCraft meshes do not store them; they are ignored".into());
    }
    Ok(())
}
