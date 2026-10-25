//! Painter 1.7 mesh blobs after the scene-graph block (`u32` byte length + payload).
//!
//! Layout per blob after `(0, name_len, name)`:
//! - `u32` prim_types, `u32` pad, `u32` component_mask (`0x107`), `u32` **num_vertices** (`nv`)
//! - Vertex arrays (60 B × `nv` when mask is `0x107`): positions, normals, tangents, bitangents, UV₀
//! - 20-byte channel table (e.g. `num_uv_components`) before faces
//! - `u32` **num_faces** (`nf`)
//! - `nf` faces × 20 B: `u32` flag (`0`/`1`), `u32` `3`, `u32` `i0`, `u32` `i1`, `u32` `i2`

use glam::{Vec2, Vec3};

use super::painter::ParsedMesh;
use crate::import::ImportError;

const ASSBIN_MESH_HAS_POSITIONS: u32 = 0x1;
const ASSBIN_MESH_HAS_NORMALS: u32 = 0x2;
const ASSBIN_MESH_HAS_TANGENTS_AND_BITANGENTS: u32 = 0x4;
const ASSBIN_MESH_HAS_TEXCOORD_BASE: u32 = 0x100;
const ASSBIN_MESH_HAS_COLOR_BASE: u32 = 0x10000;
const PAINTER_FACE_STRIDE: usize = 20;

pub(crate) fn read_painter_mesh_blobs(bytes: &[u8], off: usize) -> Result<Vec<ParsedMesh>, ImportError> {
    let scene_materials = extract_assbin_material_names(bytes, off);
    let mut meshes = Vec::new();
    let mut o = off;
    while o + 24 < bytes.len() {
        let Some((body, has_len_prefix)) = next_blob_start(bytes, o) else {
            break;
        };
        let end_hint = if has_len_prefix {
            let sz = read_u32(bytes, body - 4)? as usize;
            (body - 4 + sz).min(bytes.len())
        } else {
            bytes.len()
        };
        match parse_blob(bytes, body, end_hint, &scene_materials) {
            Ok(Some((mesh, consumed_end))) => {
                meshes.push(mesh);
                o = if has_len_prefix { end_hint } else { consumed_end };
            }
            Ok(None) => break,
            Err(e) => {
                log::warn!("assbin mesh blob @ {body} skipped: {e}");
                o = find_blob_marker(bytes, body + 32).unwrap_or(bytes.len());
            }
        }
    }
    Ok(meshes)
}

/// Scan the material property block at the end of Painter `scene.assbin` for `?mat.name` strings.
fn extract_assbin_material_names(bytes: &[u8], from: usize) -> Vec<String> {
    const KEY: &[u8] = b"?mat.name";
    let mut names = Vec::new();
    let start = from.min(bytes.len());
    let slice = &bytes[start..];
    let mut i = 0usize;
    while i + KEY.len() + 24 <= slice.len() {
        if &slice[i..i + KEY.len()] == KEY {
            for probe in (i + KEY.len()..(i + KEY.len() + 32).min(slice.len().saturating_sub(4))).step_by(1) {
                let slen = u32::from_le_bytes(slice[probe..probe + 4].try_into().unwrap()) as usize;
                if (2..=64).contains(&slen) && probe + 4 + slen <= slice.len() {
                    let raw = &slice[probe + 4..probe + 4 + slen];
                    let trimmed = if raw.last() == Some(&0) {
                        &raw[..raw.len() - 1]
                    } else {
                        raw
                    };
                    if !trimmed.is_empty()
                        && trimmed[0].is_ascii_alphabetic()
                        && trimmed.iter().all(|&b| (32..127).contains(&b))
                    {
                        if let Ok(s) = std::str::from_utf8(trimmed) {
                            names.push(s.to_string());
                            i = probe + 4 + slen;
                            break;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    names
}

fn next_blob_start(bytes: &[u8], o: usize) -> Option<(usize, bool)> {
    if o + 12 <= bytes.len() {
        let sz = read_u32(bytes, o).ok()? as usize;
        if sz >= 64 && o + 4 + sz <= bytes.len() && read_u32(bytes, o + 4).ok() == Some(0) {
            return Some((o + 4, true));
        }
    }
    find_blob_marker(bytes, o).map(|b| (b, false))
}

fn find_blob_marker(bytes: &[u8], from: usize) -> Option<usize> {
    for o in from..bytes.len().saturating_sub(24) {
        if read_u32(bytes, o).ok() != Some(0) {
            continue;
        }
        let nlen = read_u32(bytes, o + 4).ok().unwrap_or(0) as usize;
        if nlen < 4 || nlen > 48 || o + 8 + nlen > bytes.len() {
            continue;
        }
        let name = &bytes[o + 8..o + 8 + nlen];
        if name.iter().all(|&b| b == 0 || (32..127).contains(&b)) && name[0].is_ascii_graphic() {
            return Some(o);
        }
    }
    None
}

fn parse_blob(
    bytes: &[u8],
    body: usize,
    end_limit: usize,
    scene_materials: &[String],
) -> Result<Option<(ParsedMesh, usize)>, ImportError> {
    if body + 12 > end_limit {
        return Ok(None);
    }
    let _zero = read_u32(bytes, body)?;
    let nlen = read_u32(bytes, body + 4)? as usize;
    let name_start = body + 8;
    let name_end = name_start + nlen;
    if name_end > end_limit {
        return Err(ImportError::Assbin("mesh blob name truncated".into()));
    }
    let mesh_name = String::from_utf8_lossy(&bytes[name_start..name_end])
        .trim_end_matches('\0')
        .to_string();
    if mesh_name.starts_with('$') || mesh_name.starts_with('?') {
        return Ok(None);
    }
    let hdr = name_end;
    if hdr + 16 > end_limit {
        return Err(ImportError::Assbin("mesh blob header truncated".into()));
    }
    let _prim = read_u32(bytes, hdr)?;
    let mat_index = read_u32(bytes, hdr + 4)? as usize;
    let material_name = scene_materials
        .get(mat_index)
        .cloned()
        .unwrap_or_else(|| mesh_name.clone());
    let components = normalize_components(read_u32(bytes, hdr + 8)?);
    let num_vertices = read_u32(bytes, hdr + 12)?;
    if num_vertices == 0 {
        return Err(ImportError::Assbin(format!("mesh {mesh_name}: zero vertices")));
    }

    let mut cursor = hdr + 16;
    let mut positions = Vec::with_capacity(num_vertices as usize);
    let mut normals = Vec::with_capacity(num_vertices as usize);
    let mut uvs = Vec::with_capacity(num_vertices as usize);

    if components & ASSBIN_MESH_HAS_POSITIONS == 0 {
        return Err(ImportError::Assbin(format!("mesh {mesh_name}: no positions flag")));
    }
    for _ in 0..num_vertices {
        positions.push(read_vec3(bytes, &mut cursor, end_limit)?);
    }
    if components & ASSBIN_MESH_HAS_NORMALS != 0 {
        skip_array_len_prefix(bytes, &mut cursor, end_limit, num_vertices)?;
        for _ in 0..num_vertices {
            normals.push(read_vec3(bytes, &mut cursor, end_limit)?);
        }
    }
    if components & ASSBIN_MESH_HAS_TANGENTS_AND_BITANGENTS != 0 {
        skip_array_len_prefix(bytes, &mut cursor, end_limit, num_vertices)?;
        for _ in 0..num_vertices {
            read_vec3(bytes, &mut cursor, end_limit)?;
        }
        skip_array_len_prefix(bytes, &mut cursor, end_limit, num_vertices)?;
        for _ in 0..num_vertices {
            read_vec3(bytes, &mut cursor, end_limit)?;
        }
    }
    // Vertex color channels (`ASSBIN_MESH_HAS_COLOR(n) = 0x10000 << n`, 4 × f32 RGBA per vertex).
    for c_idx in 0..8u32 {
        if components & (ASSBIN_MESH_HAS_COLOR_BASE << c_idx) != 0 {
            skip_array_len_prefix(bytes, &mut cursor, end_limit, num_vertices)?;
            let byte_len = (num_vertices as usize) * 16;
            if cursor + byte_len > end_limit {
                return Err(ImportError::Assbin(format!(
                    "mesh {mesh_name}: color channel {c_idx} truncated"
                )));
            }
            cursor += byte_len;
        }
    }
    // UV channels (`ASSBIN_MESH_HAS_TEXCOORD(n) = 0x100 << n`, 3 × f32 aiVector3D per vertex).
    for uv_idx in 0..8u32 {
        if components & (ASSBIN_MESH_HAS_TEXCOORD_BASE << uv_idx) != 0 {
            // Painter 1.7 writes `u32 num_uv_components` (usually 2) followed by `u32 num_vertices`.
            if cursor + 8 <= end_limit {
                let maybe_comp = read_u32(bytes, cursor)?;
                let maybe_nv = read_u32(bytes, cursor + 4)?;
                if (1..=4).contains(&maybe_comp) && maybe_nv == num_vertices {
                    cursor += 8;
                } else {
                    skip_array_len_prefix(bytes, &mut cursor, end_limit, num_vertices)?;
                }
            }
            if uv_idx == 0 {
                for _ in 0..num_vertices {
                    let u = read_f32_at(bytes, &mut cursor, end_limit)?;
                    let v = read_f32_at(bytes, &mut cursor, end_limit)?;
                    read_f32_at(bytes, &mut cursor, end_limit)?;
                    uvs.push(Vec2::new(u, 1.0 - v));
                }
            } else {
                let byte_len = (num_vertices as usize) * 12;
                if cursor + byte_len > end_limit {
                    return Err(ImportError::Assbin(format!(
                        "mesh {mesh_name}: uv channel {uv_idx} truncated"
                    )));
                }
                cursor += byte_len;
            }
        }
    }
    if uvs.len() != num_vertices as usize {
        uvs.resize(num_vertices as usize, Vec2::ZERO);
    }

    let (face_count, mut face_cursor) = locate_face_table(bytes, cursor, end_limit, num_vertices)?;
    let mut triangles = Vec::with_capacity(face_count as usize);
    for _ in 0..face_count {
        if face_cursor + PAINTER_FACE_STRIDE > end_limit {
            return Err(ImportError::Assbin(format!("mesh {material_name}: face table truncated")));
        }
        let _flag = read_u32(bytes, face_cursor)?;
        let deg = read_u32(bytes, face_cursor + 4)?;
        let i0 = read_u32(bytes, face_cursor + 8)?;
        let i1 = read_u32(bytes, face_cursor + 12)?;
        let i2 = read_u32(bytes, face_cursor + 16)?;
        face_cursor += PAINTER_FACE_STRIDE;
        if deg != 3 {
            return Err(ImportError::Assbin(format!(
                "mesh {material_name}: non-triangle face (deg={deg})"
            )));
        }
        if i0 >= num_vertices || i1 >= num_vertices || i2 >= num_vertices {
            return Err(ImportError::Assbin(format!(
                "mesh {material_name}: index out of range (nv={num_vertices})"
            )));
        }
        triangles.push([i0, i1, i2]);
    }

    let mut consumed = face_cursor;
    while consumed + 4 <= end_limit && read_u32(bytes, consumed).ok() == Some(0) {
        if find_blob_marker(bytes, consumed) == Some(consumed) {
            break;
        }
        consumed += 4;
    }

    Ok(Some((
        ParsedMesh {
            mesh_name,
            material_name,
            positions,
            normals,
            uvs,
            triangles,
        },
        consumed,
    )))
}

fn skip_array_len_prefix(
    bytes: &[u8],
    cursor: &mut usize,
    end_limit: usize,
    expected_nv: u32,
) -> Result<(), ImportError> {
    if *cursor + 4 <= end_limit && read_u32(bytes, *cursor)? == expected_nv {
        *cursor += 4;
    }
    Ok(())
}

fn locate_face_table(
    bytes: &[u8],
    after_verts: usize,
    end_limit: usize,
    num_vertices: u32,
) -> Result<(u32, usize), ImportError> {
    for pad in (0..=64usize).step_by(4) {
        let pos = after_verts + pad;
        if pos + 4 + PAINTER_FACE_STRIDE > end_limit {
            break;
        }
        let nf = read_u32(bytes, pos)?;
        if nf == 0 || nf > 500_000 {
            continue;
        }
        let table = pos + 4;
        if table + nf as usize * PAINTER_FACE_STRIDE > end_limit {
            continue;
        }
        let flag = read_u32(bytes, table)?;
        let deg = read_u32(bytes, table + 4)?;
        let i0 = read_u32(bytes, table + 8)?;
        let i1 = read_u32(bytes, table + 12)?;
        let i2 = read_u32(bytes, table + 16)?;
        if (flag == 0 || flag == 1) && deg == 3 && i0 < num_vertices && i1 < num_vertices && i2 < num_vertices {
            return Ok((nf, table));
        }
    }
    Err(ImportError::Assbin("could not locate Painter 20-byte face table".into()))
}

fn normalize_components(c: u32) -> u32 {
    match c {
        0x107 | 0x0001_0700 => 0x107,
        _ => c,
    }
}

fn read_vec3(bytes: &[u8], cursor: &mut usize, end: usize) -> Result<Vec3, ImportError> {
    let x = read_f32_at(bytes, cursor, end)?;
    let y = read_f32_at(bytes, cursor, end)?;
    let z = read_f32_at(bytes, cursor, end)?;
    Ok(Vec3::new(x, y, z))
}

fn read_f32_at(bytes: &[u8], cursor: &mut usize, end: usize) -> Result<f32, ImportError> {
    let v = read_f32(bytes, *cursor, end)?;
    *cursor += 4;
    Ok(v)
}

fn read_u32(bytes: &[u8], off: usize) -> Result<u32, ImportError> {
    if off + 4 > bytes.len() {
        return Err(ImportError::Assbin("unexpected EOF".into()));
    }
    Ok(u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()))
}

fn read_f32(bytes: &[u8], off: usize, end: usize) -> Result<f32, ImportError> {
    if off + 4 > end {
        return Err(ImportError::Assbin("unexpected EOF".into()));
    }
    Ok(f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()))
}
