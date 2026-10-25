//! Painter 11 assbin 1.7 — node tree at payload start; geometry decode in progress.

use glam::{Mat4, Vec2, Vec3};

use super::header::{AssbinHeader, payload_offset};
use super::scene_to_mesh;
use crate::import::ImportError;

#[derive(Debug)]
pub(crate) struct ParsedMesh {
    pub mesh_name: String,
    pub material_name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub triangles: Vec<[u32; 3]>,
}

pub(crate) fn import_painter_v17(bytes: &[u8], header: &AssbinHeader) -> Result<crate::import::ImportReport, ImportError> {
    let off = payload_offset(header);
    if bytes.len() < off + 8 {
        return Err(ImportError::Assbin("painter assbin payload truncated".into()));
    }
    let mesh_start = super::painter_v17_tail_offset(bytes)?;
    let meshes = read_mesh_bank(bytes, mesh_start)?;
    if meshes.is_empty() {
        return Err(ImportError::Assbin(
            "Painter assbin 1.7: no mesh blobs decoded".into(),
        ));
    }
    let (root_name, instances) = match read_scene_graph_nodes(bytes, off, header) {
        Ok((_, name, inst)) if inst.len() >= meshes.len() => (name, inst),
        _ => {
            let instances = meshes
                .iter()
                .enumerate()
                .map(|(i, _)| (Mat4::IDENTITY, i as u32))
                .collect();
            ("PainterScene".to_string(), instances)
        }
    };
    scene_to_mesh(&root_name, &instances, &meshes)
}

/// Flat assimp-style node (no per-node chunk wrapper in Painter 1.7).
fn read_node(
    bytes: &[u8],
    off: usize,
    is_root: bool,
    child_table_prelude: bool,
    parent_world: Mat4,
    graph_end: usize,
) -> Result<(usize, String, Vec<(Mat4, u32)>, Vec<(Mat4, u32)>), ImportError> {
    let (mut off, name) = if is_root {
        read_painter_root_name(bytes, off)?
    } else {
        read_ai_string(bytes, off)?
    };
    let local = read_matrix(bytes, off)?;
    off += 64;
    let world = parent_world * local;
    let num_children = read_u32(bytes, off)?;
    off += 4;
    let num_meshes = read_u32(bytes, off)?;
    off += 4;
    let num_meta = read_u32(bytes, off)?;
    off += 4;

    let mut own_meshes = Vec::new();
    for _ in 0..num_meshes as usize {
        let idx = read_u32(bytes, off)?;
        off += 4;
        own_meshes.push((world, idx));
    }

    let mut child_instances = Vec::new();
    if child_table_prelude && num_children > 0 && off + 12 <= bytes.len() {
        let maybe_count = read_u32(bytes, off)?;
        if maybe_count == num_children {
            off += 12;
        }
    }
    for _ in 0..num_children as usize {
        let (child_off, _child_name, child_own, child_nested) =
            read_node(bytes, off, false, false, world, graph_end)?;
        off = child_off;
        child_instances.extend(child_own);
        child_instances.extend(child_nested);
    }

    off = skip_painter_metadata(bytes, off, num_meta).or_else(|e| {
        if graph_end > off && graph_end <= bytes.len() {
            Ok(graph_end)
        } else {
            Err(e)
        }
    })?;
    if off > graph_end && graph_end > 0 {
        off = graph_end;
    }

    let nested = child_instances;
    Ok((off, name, own_meshes, nested))
}

/// Painter 1.7 root node: `u32` zero, `u32` name byte length, raw name bytes (not `aiString`).
fn read_painter_root_name(bytes: &[u8], off: usize) -> Result<(usize, String), ImportError> {
    let _zero = read_u32(bytes, off)?;
    let nlen = read_u32(bytes, off + 4)? as usize;
    let start = off + 8;
    let end = start + nlen;
    if end > bytes.len() {
        return Err(ImportError::Assbin("root name truncated".into()));
    }
    let s = String::from_utf8_lossy(&bytes[start..end])
        .trim_end_matches('\0')
        .to_string();
    Ok((end, s))
}

fn read_scene_graph_nodes(
    bytes: &[u8],
    off: usize,
    header: &AssbinHeader,
) -> Result<(usize, String, Vec<(Mat4, u32)>), ImportError> {
    let (block_len, off) = if header.is_painter_v17() {
        let block_len = read_u32(bytes, off)?;
        (block_len, off + 4)
    } else {
        (0, off)
    };
    let graph_end = if block_len > 0 {
        off + block_len as usize
    } else {
        bytes.len()
    };
    let (mut off, root_name, own, nested) =
        read_node(bytes, off, true, true, Mat4::IDENTITY, graph_end)?;
    if graph_end > off && graph_end <= bytes.len() {
        off = graph_end;
    }
    let instances: Vec<(Mat4, u32)> = own.into_iter().chain(nested).collect();
    let mesh_off = if graph_end > 0 && graph_end <= bytes.len() {
        graph_end
    } else {
        off
    };
    Ok((mesh_off, root_name, instances))
}

#[allow(dead_code)]
pub(crate) fn read_scene_graph_public(
    bytes: &[u8],
    off: usize,
    header: &AssbinHeader,
) -> Result<(usize, String, Vec<(Mat4, u32)>, Vec<ParsedMesh>), ImportError> {
    let (mesh_off, root_name, instances) = read_scene_graph_nodes(bytes, off, header)?;
    let meshes = read_mesh_bank(bytes, mesh_off)?;
    Ok((mesh_off, root_name, instances, meshes))
}

fn read_mesh_bank(bytes: &[u8], off: usize) -> Result<Vec<ParsedMesh>, ImportError> {
    super::painter_blob::read_painter_mesh_blobs(bytes, off)
}

fn skip_painter_metadata(bytes: &[u8], off: usize, count: u32) -> Result<usize, ImportError> {
    let mut off = off;
    for _ in 0..count as usize {
        let (o, _key) = read_ai_string(bytes, off)?;
        off = o;
        if off + 4 > bytes.len() {
            return Err(ImportError::Assbin("metadata truncated".into()));
        }
        let a = read_u16(bytes, off)?;
        off += 2;
        let b = read_u16(bytes, off)?;
        off += 2;
        if a == 1 && b == 2 {
            off = skip_painter_meta_blob(bytes, off)?;
        } else {
            off = skip_assimp_meta_value(bytes, off, a)?;
        }
    }
    Ok(off)
}

fn skip_painter_meta_blob(bytes: &[u8], mut off: usize) -> Result<usize, ImportError> {
    let start = off;
    let limit = (start + 4096).min(bytes.len());
    while off + 4 <= limit {
        let n = read_u32(bytes, off)? as usize;
        if n == 0 || n > 512 {
            break;
        }
        off += 4;
        if off + n > bytes.len() {
            return Err(ImportError::Assbin("painter metadata blob truncated".into()));
        }
        off += n;
    }
    Ok(off)
}

fn skip_assimp_meta_value(bytes: &[u8], off: usize, kind: u16) -> Result<usize, ImportError> {
    let mut off = off;
    match kind {
        0 => off += 4,
        1 => off += 4,
        2 => off += 8,
        3 => off += 4,
        4 => off += 8,
        5 => {
            let (o, _) = read_ai_string(bytes, off)?;
            off = o;
        }
        6 => off += 12,
        _ => return Err(ImportError::Assbin(format!("unknown metadata type {kind}"))),
    }
    Ok(off)
}

fn read_ai_string(bytes: &[u8], off: usize) -> Result<(usize, String), ImportError> {
    let n = read_u32(bytes, off)? as usize;
    let start = off + 4;
    let end = start + n;
    if end > bytes.len() {
        return Err(ImportError::Assbin("string extends past EOF".into()));
    }
    let s = String::from_utf8_lossy(&bytes[start..end]).into_owned();
    Ok((end, s))
}

fn read_matrix(bytes: &[u8], off: usize) -> Result<Mat4, ImportError> {
    if off + 64 > bytes.len() {
        return Err(ImportError::Assbin("matrix truncated".into()));
    }
    let mut cols = [0.0f32; 16];
    for i in 0..16 {
        cols[i] = read_f32(bytes, off + i * 4)?;
    }
    Ok(mat4_from_assimp_rows(&cols))
}

fn mat4_from_assimp_rows(m: &[f32; 16]) -> Mat4 {
    Mat4::from_cols(
        glam::Vec4::new(m[0], m[4], m[8], m[12]),
        glam::Vec4::new(m[1], m[5], m[9], m[13]),
        glam::Vec4::new(m[2], m[6], m[10], m[14]),
        glam::Vec4::new(m[3], m[7], m[11], m[15]),
    )
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, ImportError> {
    if off + 4 > bytes.len() {
        return Err(ImportError::Assbin("unexpected EOF".into()));
    }
    Ok(u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()))
}

fn read_u16(bytes: &[u8], off: usize) -> Result<u16, ImportError> {
    if off + 2 > bytes.len() {
        return Err(ImportError::Assbin("unexpected EOF".into()));
    }
    Ok(u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap()))
}

fn read_f32(bytes: &[u8], off: usize) -> Result<f32, ImportError> {
    if off + 4 > bytes.len() {
        return Err(ImportError::Assbin("unexpected EOF".into()));
    }
    Ok(f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()))
}
