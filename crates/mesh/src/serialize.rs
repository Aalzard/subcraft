//! Versioned binary encoding of the canonical mesh (stored inside project files).
//!
//! Layout (little endian): magic `SCMESH`, u16 version, then length-prefixed sections.
//! The format is a product contract: bump `VERSION` and keep a reader for old versions.

use glam::{Vec2, Vec3, Vec4};

use crate::mesh::{Aabb, MaterialSlot, Mesh, Submesh};

const MAGIC: &[u8; 6] = b"SCMESH";
const VERSION: u16 = 1;

#[derive(Debug, thiserror::Error)]
pub enum MeshDecodeError {
    #[error("not a SubCraft mesh blob")]
    BadMagic,
    #[error("unsupported mesh blob version {0}")]
    Version(u16),
    #[error("truncated mesh blob")]
    Truncated,
    #[error("invalid mesh: {0}")]
    Invalid(String),
}

struct W(Vec<u8>);
impl W {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32s(&mut self, v: &[f32]) {
        self.u32(v.len() as u32);
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }
    fn u32s(&mut self, v: &[u32]) {
        self.u32(v.len() as u32);
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
}

struct R<'a>(&'a [u8]);
impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], MeshDecodeError> {
        if self.0.len() < n {
            return Err(MeshDecodeError::Truncated);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u32(&mut self) -> Result<u32, MeshDecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn f32s(&mut self) -> Result<Vec<f32>, MeshDecodeError> {
        let n = self.u32()? as usize;
        let b = self.take(n.checked_mul(4).ok_or(MeshDecodeError::Truncated)?)?;
        Ok(b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().expect("4"))).collect())
    }
    fn u32s(&mut self) -> Result<Vec<u32>, MeshDecodeError> {
        let n = self.u32()? as usize;
        let b = self.take(n.checked_mul(4).ok_or(MeshDecodeError::Truncated)?)?;
        Ok(b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().expect("4"))).collect())
    }
    fn str(&mut self) -> Result<String, MeshDecodeError> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|e| MeshDecodeError::Invalid(e.to_string()))
    }
}

pub fn encode(mesh: &Mesh) -> Vec<u8> {
    let mut w = W(Vec::with_capacity(mesh.positions.len() * 64 + mesh.indices.len() * 4 + 256));
    w.0.extend_from_slice(MAGIC);
    w.0.extend_from_slice(&VERSION.to_le_bytes());
    w.str(&mesh.name);
    w.u32(mesh.materials.len() as u32);
    for m in &mesh.materials {
        w.str(&m.name);
    }
    w.f32s(bytemuck::cast_slice(&mesh.positions.iter().map(|p| p.to_array()).collect::<Vec<_>>()));
    w.f32s(bytemuck::cast_slice(&mesh.normals.iter().map(|p| p.to_array()).collect::<Vec<_>>()));
    w.f32s(bytemuck::cast_slice(&mesh.uvs.iter().map(|p| p.to_array()).collect::<Vec<_>>()));
    w.f32s(bytemuck::cast_slice(&mesh.tangents.iter().map(|p| p.to_array()).collect::<Vec<_>>()));
    w.u32s(&mesh.indices);
    w.u32s(&mesh.tri_material);
    w.u32(mesh.submeshes.len() as u32);
    for s in &mesh.submeshes {
        w.u32(s.material);
        w.u32(s.first_triangle);
        w.u32(s.triangle_count);
    }
    w.0
}

pub fn decode(bytes: &[u8]) -> Result<Mesh, MeshDecodeError> {
    let mut r = R(bytes);
    if r.take(6)? != MAGIC {
        return Err(MeshDecodeError::BadMagic);
    }
    let version = u16::from_le_bytes(r.take(2)?.try_into().expect("2"));
    if version != VERSION {
        return Err(MeshDecodeError::Version(version));
    }
    let name = r.str()?;
    let n_mat = r.u32()? as usize;
    let mut materials = Vec::with_capacity(n_mat.min(4096));
    for _ in 0..n_mat {
        materials.push(MaterialSlot { name: r.str()? });
    }
    let v3 = |v: Vec<f32>| v.chunks_exact(3).map(|c| Vec3::new(c[0], c[1], c[2])).collect::<Vec<_>>();
    let positions = v3(r.f32s()?);
    let normals = v3(r.f32s()?);
    let uvs: Vec<Vec2> = r.f32s()?.chunks_exact(2).map(|c| Vec2::new(c[0], c[1])).collect();
    let tangents: Vec<Vec4> = r.f32s()?.chunks_exact(4).map(|c| Vec4::new(c[0], c[1], c[2], c[3])).collect();
    let indices = r.u32s()?;
    let tri_material = r.u32s()?;
    let n_sub = r.u32()? as usize;
    let mut submeshes = Vec::with_capacity(n_sub.min(4096));
    for _ in 0..n_sub {
        submeshes.push(Submesh { material: r.u32()?, first_triangle: r.u32()?, triangle_count: r.u32()? });
    }
    let bounds = Aabb::from_points(positions.iter().copied());
    let mesh = Mesh { name, positions, normals, uvs, tangents, indices, tri_material, materials, submeshes, bounds };
    let problems = mesh.validate();
    if !problems.is_empty() {
        return Err(MeshDecodeError::Invalid(problems.join("; ")));
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives;

    #[test]
    fn roundtrip_exact() {
        let m = primitives::sample_crate();
        let bytes = encode(&m);
        let back = decode(&bytes).unwrap();
        assert_eq!(back.positions, m.positions);
        assert_eq!(back.indices, m.indices);
        assert_eq!(back.tangents, m.tangents);
        assert_eq!(back.materials, m.materials);
        assert_eq!(back.submeshes, m.submeshes);
    }

    #[test]
    fn rejects_truncated_and_garbage() {
        let bytes = encode(&primitives::cube(1.0));
        assert!(decode(&bytes[..bytes.len() / 2]).is_err());
        assert!(matches!(decode(b"NOTMESH!"), Err(MeshDecodeError::BadMagic)));
    }
}
