//! Assimp `.assbin` (Painter `paint/scene.assbin`). Public Assimp 6.x only accepts format 1.0;
//! Painter 11 writes **1.7** with a 12-byte payload prelude and a custom node/mesh layout.

mod header;
mod painter;
mod painter_blob;

use glam::{Mat3, Mat4};

use crate::import::ImportError;
use crate::mesh::{Corner, MeshBuilder};
use header::{AssbinHeader, payload_offset};

pub use header::assbin_header;

/// Parse header + prelude; does not load geometry yet.
pub fn probe(bytes: &[u8]) -> Result<AssbinHeader, ImportError> {
    assbin_header(bytes)
}

/// Byte offset after the flat node tree + metadata (start of mesh/material section).
pub fn painter_v17_tail_offset(bytes: &[u8]) -> Result<usize, ImportError> {
    let header = probe(bytes)?;
    if !header.is_painter_v17() {
        return Err(ImportError::Assbin("not painter 1.7".into()));
    }
    let off = payload_offset(&header);
    if bytes.len() < off + 4 {
        return Err(ImportError::Assbin("painter block length truncated".into()));
    }
    let block_len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    Ok(off + 4 + block_len as usize)
}

/// Load mesh from in-memory Painter `scene.assbin`.
/// Rust-only Painter 1.7 path (no `assimp.dll`).
pub fn import_assbin_pure(bytes: &[u8]) -> Result<crate::import::ImportReport, ImportError> {
    let header = probe(bytes)?;
    if header.is_painter_v17() {
        return painter::import_painter_v17(bytes, &header);
    }
    Err(ImportError::Assbin("not Painter assbin 1.7".into()))
}

pub fn import_assbin(bytes: &[u8]) -> Result<crate::import::ImportReport, ImportError> {
    import_assbin_pure(bytes)
}

pub(crate) fn scene_to_mesh(
    name: &str,
    instances: &[(Mat4, u32)],
    meshes: &[painter::ParsedMesh],
) -> Result<crate::import::ImportReport, ImportError> {
    let mut b = MeshBuilder::new(name);
    let mut warnings = Vec::new();
    for (world, mesh_idx) in instances {
        let mesh = meshes
            .get(*mesh_idx as usize)
            .ok_or_else(|| ImportError::Assbin(format!("node references missing mesh {mesh_idx}")))?;
        b.object(&mesh.mesh_name);
        let mat = b.material(&mesh.material_name);
        let normal_m = Mat3::from_mat4(*world).inverse().transpose();
        let flip = world.determinant() < 0.0;
        for tri in &mesh.triangles {
            let corner = |i: u32| {
                let i = i as usize;
                Corner {
                    position: world.transform_point3(mesh.positions[i]),
                    normal: mesh.normals.get(i).map(|n| (normal_m * *n).normalize_or_zero()),
                    uv: mesh.uvs.get(i).copied(),
                }
            };
            let c = [corner(tri[0]), corner(tri[1]), corner(tri[2])];
            b.push_triangle(mat, if flip { [c[0], c[2], c[1]] } else { c });
        }
    }
    let stats_pre = b.stats;
    let mesh = b.build()?;
    if mesh.normals.is_empty() {
        warnings.push("assbin: generated normals".into());
    }
    Ok(crate::import::ImportReport {
        mesh,
        stats: stats_pre,
        warnings,
    })
}
