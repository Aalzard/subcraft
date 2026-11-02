//! Substance Painter stores the project's mesh as a binary dump written by an Assimp-derived
//! exporter (`paint/scene.assbin`). The layout below was worked out from real projects: a header
//! of 512 bytes, a node tree, then one record per mesh. All numbers are little endian.
//!
//! Mesh record: `u32 count-or-0`, `u32 record length` (counted from the start of the record),
//! `u32 0`, `u32 name length` + name, `u32 primitive type` (4 = triangles), a `u32` that this
//! reader ignores, and a `u32` of format bits: `0x1` positions, `0x2` normals, `0x4` tangents and
//! bitangents, `0x100 << n` UV channel n, `0x10000 << n` colour channel n. Then, in that order,
//! `u32 n` + `n*3` f32 for positions, normals, tangents and bitangents, `u32 n` + `n*4` f32 per
//! colour channel, and per UV channel `u32 components` + `u32 n` + `n*3` f32 (u, v, 0). Finally
//! `u32 faces` followed by `faces` entries of `u32 flag, u32 3, u32 i0, u32 i1, u32 i2`.

use glam::{Vec2, Vec3};

#[derive(Debug, thiserror::Error)]
pub enum AssbinError {
    #[error("not an Assimp binary dump")]
    BadHeader,
    #[error("unexpected end of mesh data ({0})")]
    Truncated(&'static str),
    #[error("unsupported mesh record: {0}")]
    Unsupported(String),
}

#[derive(Debug, Clone)]
pub struct DumpMesh {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Texture coordinates with the origin at the bottom-left (v up).
    pub uvs: Vec<Vec2>,
    pub triangles: Vec<[u32; 3]>,
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn u32(&mut self, what: &'static str) -> Result<u32, AssbinError> {
        let s = self.b.get(self.p..self.p + 4).ok_or(AssbinError::Truncated(what))?;
        self.p += 4;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    /// Skip `n * floats` f32 after a `u32 n` count (data this reader does not use).
    fn skip_floats(&mut self, floats: usize, what: &'static str) -> Result<(), AssbinError> {
        let n = self.u32(what)? as usize;
        let bytes = n.checked_mul(4 * floats).ok_or(AssbinError::Truncated(what))?;
        if self.p + bytes > self.b.len() {
            return Err(AssbinError::Truncated(what));
        }
        self.p += bytes;
        Ok(())
    }

    fn vec3s(&mut self, what: &'static str) -> Result<Vec<Vec3>, AssbinError> {
        let n = self.u32(what)? as usize;
        let bytes = n.checked_mul(12).ok_or(AssbinError::Truncated(what))?;
        let s = self.b.get(self.p..self.p + bytes).ok_or(AssbinError::Truncated(what))?;
        self.p += bytes;
        Ok(s.chunks_exact(12)
            .map(|c| {
                let f = |i: usize| f32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]);
                Vec3::new(f(0), f(4), f(8))
            })
            .collect())
    }
}

/// Parse every mesh in a `scene.assbin` dump.
pub fn parse(b: &[u8]) -> Result<Vec<DumpMesh>, AssbinError> {
    if !b.starts_with(b"ASSIMP.binary-dump") || b.len() < 540 {
        return Err(AssbinError::BadHeader);
    }
    let node_len = u32::from_le_bytes([b[524], b[525], b[526], b[527]]) as usize;
    let mut pos = 524 + node_len;
    let mut r = Reader { b, p: pos };
    let count = r.u32("mesh count")? as usize;
    if count == 0 || count > 100_000 {
        return Err(AssbinError::Unsupported(format!("mesh count {count}")));
    }
    let mut meshes = Vec::with_capacity(count);
    for _ in 0..count {
        let mut r = Reader { b, p: pos };
        let _count_or_zero = r.u32("record header")?;
        let len = r.u32("record length")? as usize;
        if len < 8 || pos + len > b.len() {
            return Err(AssbinError::Truncated("record length"));
        }
        let _zero = r.u32("record")?;
        let name_len = r.u32("name length")? as usize;
        let name = b.get(r.p..r.p + name_len).ok_or(AssbinError::Truncated("name"))?;
        let name = String::from_utf8_lossy(name).into_owned();
        r.p += name_len;
        let primitive = r.u32("primitive type")?;
        if primitive != 4 {
            return Err(AssbinError::Unsupported(format!("primitive type {primitive} in '{name}'")));
        }
        r.u32("record")?;
        let bits = r.u32("format bits")?;
        if bits & 1 == 0 {
            return Err(AssbinError::Unsupported(format!("mesh '{name}' has no positions")));
        }
        let positions = r.vec3s("positions")?;
        let normals = if bits & 2 != 0 { r.vec3s("normals")? } else { Vec::new() };
        if bits & 4 != 0 {
            r.skip_floats(3, "tangents")?;
            r.skip_floats(3, "bitangents")?;
        }
        for c in 0..8 {
            if bits & (0x10000 << c) != 0 {
                r.skip_floats(4, "colors")?;
            }
        }
        let mut uvs: Vec<Vec2> = Vec::new();
        for c in 0..8 {
            if bits & (0x100 << c) != 0 {
                r.u32("uv components")?;
                let set: Vec<Vec2> = r.vec3s("uvs")?.into_iter().map(|v| Vec2::new(v.x, v.y)).collect();
                if c == 0 {
                    uvs = set;
                }
            }
        }
        let faces = r.u32("face count")? as usize;
        let mut triangles = Vec::with_capacity(faces);
        for _ in 0..faces {
            let _flag = r.u32("faces")?;
            let n = r.u32("faces")?;
            if n != 3 {
                return Err(AssbinError::Unsupported(format!("face with {n} indices in '{name}'")));
            }
            let t = [r.u32("faces")?, r.u32("faces")?, r.u32("faces")?];
            if t.iter().any(|&i| i as usize >= positions.len()) {
                return Err(AssbinError::Unsupported(format!("face index out of range in '{name}'")));
            }
            triangles.push(t);
        }
        if r.p != pos + len {
            return Err(AssbinError::Unsupported(format!("record '{name}' has {} unread bytes", (pos + len) as i64 - r.p as i64)));
        }
        meshes.push(DumpMesh { name, positions, normals, uvs, triangles });
        pos += len;
    }
    Ok(meshes)
}
