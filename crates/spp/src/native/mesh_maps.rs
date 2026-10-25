//! Import Painter baked mesh maps from `resources.toc` + `resources/{hash}` JPEG 2000 tiles.

use hdf5_reader::Hdf5File;

const JP2_SIG: &[u8] = b"\x00\x00\x00\x0cjP  ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PainterMeshMapKind {
    Normal,
    WorldSpaceNormal,
    Id,
    AmbientOcclusion,
    Curvature,
    Position,
    Thickness,
}

#[derive(Clone, Debug)]
pub struct CachedMeshMap {
    pub material_name: String,
    pub kind: PainterMeshMapKind,
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub pixels_f32: Vec<f32>,
}

pub fn decode_painter_mesh_maps(
    spp_path: &std::path::Path,
    max_res: u32,
) -> Result<Vec<CachedMeshMap>, String> {
    let resolved = super::container::resolve_spp_path(spp_path);
    let file = Hdf5File::open(&resolved)
        .map_err(|e| format!("HDF5 open {}: {e}", resolved.display()))?;
    let toc = match file.dataset("resources.toc") {
        Ok(ds) => ds.read_raw_bytes().map_err(|e| format!("resources.toc: {e}"))?,
        Err(_) => return Ok(Vec::new()),
    };
    let entries = parse_resource_toc(&toc)?;
    let target = max_res.max(128);
        let mut raw_entries = Vec::new();
        for (kind, material, hash) in entries {
            let ds_name = format!("resources/{hash}");
            let Ok(ds) = file.dataset(&ds_name) else {
                log::warn!("SPP mesh map missing dataset {ds_name}");
                continue;
            };
            match ds.read_raw_bytes() {
                Ok(raw) => raw_entries.push((kind, material, ds_name, raw)),
                Err(e) => log::warn!("SPP mesh map {ds_name} read failed: {e}"),
            }
        }
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().clamp(1, 8))
            .unwrap_or(4);
        let chunk_size = raw_entries.len().div_ceil(n_threads).max(1);
        let mut out = Vec::with_capacity(raw_entries.len());
        std::thread::scope(|s| {
            let mut handles = Vec::new();
            for chunk in raw_entries.chunks(chunk_size) {
                handles.push(s.spawn(move || {
                    let mut local = Vec::with_capacity(chunk.len());
                    for (kind, material, ds_name, raw) in chunk {
                        match decode_jp2_resource(raw, target) {
                            Ok((width, height, channels, pixels_f32)) => {
                                local.push(CachedMeshMap {
                                    material_name: material.clone(),
                                    kind: *kind,
                                    width,
                                    height,
                                    channels,
                                    pixels_f32,
                                });
                            }
                            Err(e) => log::warn!("SPP mesh map {ds_name} decode failed: {e}"),
                        }
                    }
                    local
                }));
            }
            for h in handles {
                if let Ok(local) = h.join() {
                    out.extend(local);
                }
            }
        });
        Ok(out)
}

fn parse_resource_toc(toc: &[u8]) -> Result<Vec<(PainterMeshMapKind, String, String)>, String> {
    let mut off = 0usize;
    let type_count = read_u32(toc, &mut off).ok_or("toc eof at type_count")?;
    if type_count == 0 || type_count > 16 {
        return Err(format!("unexpected toc type count {type_count}"));
    }
    let mut out = Vec::new();
    for _ in 0..type_count {
        let name = read_utf16be(toc, &mut off).ok_or("toc eof at type name")?;
        let count = read_u32(toc, &mut off).ok_or("toc eof at item count")?;
        if count > 10_000 {
            return Err(format!("toc item count {count} too large"));
        }
        if !name.contains("ResourceImage") {
            for _ in 0..count {
                let _ = read_utf16be(toc, &mut off);
            }
            continue;
        }
        for _ in 0..count {
            let path = read_utf16be(toc, &mut off).ok_or("toc eof at image path")?;
            if let Some(entry) = classify_mesh_map(&path) {
                out.push(entry);
            }
        }
    }
    Ok(out)
}

fn classify_mesh_map(path: &str) -> Option<(PainterMeshMapKind, String, String)> {
    let path = path.trim_start_matches('/');
    let (label, hash) = path.split_once("?version=")?;
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let lower = label.to_lowercase();
    let kind = if looks_like_ao(&lower) {
        PainterMeshMapKind::AmbientOcclusion
    } else if looks_like_curvature(&lower) {
        PainterMeshMapKind::Curvature
    } else if looks_like_world_normal(&lower) {
        PainterMeshMapKind::WorldSpaceNormal
    } else if looks_like_thickness(&lower) {
        PainterMeshMapKind::Thickness
    } else if looks_like_id(&lower) {
        PainterMeshMapKind::Id
    } else if lower.contains("position") {
        PainterMeshMapKind::Position
    } else if looks_like_tangent_normal(&lower) {
        PainterMeshMapKind::Normal
    } else {
        return None;
    };
    let material = label.split_whitespace().last()?.to_string();
    Some((kind, material, hash.to_string()))
}

fn looks_like_ao(s: &str) -> bool {
    s.contains("umgebungsverdeck")
        || s.contains("ambient occlusion")
        || s.contains("ambient_occlusion")
        || s.contains("ao from")
}

fn looks_like_curvature(s: &str) -> bool {
    s.contains("krümm") || s.contains("kruemm") || s.contains("curvature") || s.contains("krummungs")
}

fn looks_like_world_normal(s: &str) -> bool {
    s.contains("raum-normal")
        || s.contains("raumnormalen")
        || s.contains("world space")
        || s.contains("worldspace")
        || s.contains("normalws")
}

fn looks_like_thickness(s: &str) -> bool {
    s.contains("thickness") || s.contains("dicke-map") || s.contains("dicken-map")
}

fn looks_like_id(s: &str) -> bool {
    s.contains("farbmatrix") || s.contains("color from") || s.contains("id from") || s.contains("id-map")
}

fn looks_like_tangent_normal(s: &str) -> bool {
    (s.contains("normal-map") || s.contains("normal map") || s.contains("normal from"))
        && !looks_like_world_normal(s)
}

fn decode_jp2_resource(raw: &[u8], max_res: u32) -> Result<(u32, u32, u32, Vec<f32>), String> {
    let tiles = split_jp2_tiles(raw)?;
    if tiles.is_empty() {
        return Err("no jp2 tiles".into());
    }
    let decoded: Vec<(u32, u32, DecodedTile)> = tiles
        .iter()
        .map(|&(x0, y0, bytes)| Ok::<_, String>((x0, y0, decode_jp2_tile(bytes)?)))
        .collect::<Result<_, _>>()?;
    let tw = decoded[0].2.width;
    let th = decoded[0].2.height;
    let channels = decoded[0].2.channels;
    let src_w = decoded.iter().map(|(x0, _, t)| x0 + t.width).max().unwrap_or(tw);
    let src_h = decoded.iter().map(|(_, y0, t)| y0 + t.height).max().unwrap_or(th);
    let mut dense = vec![0.0f32; src_w as usize * src_h as usize * channels as usize];
    for (x0, y0, tile) in &decoded {
        if tile.width != tw || tile.height != th || tile.channels != channels {
            return Err("mesh map tile size mismatch".into());
        }
        blit_tile(&mut dense, src_w, channels, *x0, *y0, tile);
    }
    let out_w = src_w.min(max_res);
    let out_h = src_h.min(max_res);
    let pixels = downsample_dense(&dense, src_w, src_h, out_w, out_h, channels as usize);
    Ok((out_w, out_h, channels, pixels))
}

struct DecodedTile {
    width: u32,
    height: u32,
    channels: u32,
    pixels: Vec<f32>,
}

#[cfg(target_arch = "wasm32")]
fn decode_jp2_tile(_bytes: &[u8]) -> Result<DecodedTile, String> {
    Err("JPEG 2000 mesh-map decode is desktop-only".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_jp2_tile(bytes: &[u8]) -> Result<DecodedTile, String> {
    let image = jpeg2k::Image::from_bytes(bytes).map_err(|e| format!("jpeg2k: {e}"))?;
    let comps = image.components();
    if comps.is_empty() {
        return Err("jpeg2k tile has no components".into());
    }
    let width = comps[0].width();
    let height = comps[0].height();
    let n = (width as usize).saturating_mul(height as usize);
    let color: Vec<&jpeg2k::ImageComponent> = comps.iter().filter(|c| !c.is_alpha()).collect();
    let channels = match color.len() {
        1 => 1u32,
        n if n >= 3 => 3,
        other => return Err(format!("unsupported mesh-map component count {other}")),
    };
    let mut pixels = vec![0.0f32; n * channels as usize];
    for c in 0..channels as usize {
        let comp = color[c];
        if comp.width() != width || comp.height() != height {
            return Err("jpeg2k component size mismatch".into());
        }
        let prec = comp.precision().clamp(1, 16);
        let max = ((1u32 << prec) - 1) as f32;
        let data = comp.data();
        if data.len() < n {
            return Err("jpeg2k component truncated".into());
        }
        for i in 0..n {
            pixels[i * channels as usize + c] = (data[i] as f32 / max).clamp(0.0, 1.0);
        }
    }
    Ok(DecodedTile { width, height, channels, pixels })
}

fn split_jp2_tiles(raw: &[u8]) -> Result<Vec<(u32, u32, &[u8])>, String> {
    let mut tiles = Vec::new();
    let mut pos = find_jp2(raw, 0).ok_or("no jp2 signature in mesh-map resource")?;
    while pos + JP2_SIG.len() <= raw.len() {
        let end = walk_jp2(raw, pos).ok_or("truncated jp2 tile")?;
        let (x0, y0) = if pos >= 24 {
            let hdr = &raw[pos - 24..pos];
            let x0 = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
            let y0 = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
            let x1 = u32::from_le_bytes(hdr[8..12].try_into().unwrap());
            let y1 = u32::from_le_bytes(hdr[12..16].try_into().unwrap());
            if x1 > x0 && y1 > y0 && x1 <= 65536 && y1 <= 65536 {
                (x0, y0)
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };
        tiles.push((x0, y0, &raw[pos..end]));
        match find_jp2(raw, end) {
            Some(next) if next < end + 64 => pos = next,
            _ => break,
        }
        if tiles.len() > 256 {
            return Err("too many jp2 tiles".into());
        }
    }
    Ok(tiles)
}

fn find_jp2(raw: &[u8], from: usize) -> Option<usize> {
    raw[from..].windows(JP2_SIG.len()).position(|w| w == JP2_SIG).map(|i| from + i)
}

fn walk_jp2(raw: &[u8], start: usize) -> Option<usize> {
    let mut off = start;
    while off + 8 <= raw.len() {
        let len = u32::from_be_bytes(raw[off..off + 4].try_into().ok()?) as usize;
        let typ = &raw[off + 4..off + 8];
        let (hdr, box_len) = if len == 1 {
            if off + 16 > raw.len() {
                return None;
            }
            (16usize, u64::from_be_bytes(raw[off + 8..off + 16].try_into().ok()?) as usize)
        } else if len == 0 {
            return Some(raw.len());
        } else {
            (8usize, len)
        };
        if box_len < hdr || off.checked_add(box_len)? > raw.len() {
            return None;
        }
        off += box_len;
        if typ == b"jp2c" {
            return Some(off);
        }
    }
    None
}

fn blit_tile(dst: &mut [f32], dst_w: u32, channels: u32, x0: u32, y0: u32, tile: &DecodedTile) {
    let comps = channels as usize;
    let dw = dst_w as usize;
    let tw = tile.width as usize;
    for y in 0..tile.height as usize {
        let src_row = y * tw * comps;
        let dst_row = ((y0 as usize + y) * dw + x0 as usize) * comps;
        dst[dst_row..dst_row + tw * comps].copy_from_slice(&tile.pixels[src_row..src_row + tw * comps]);
    }
}

fn downsample_dense(
    src: &[f32],
    w: u32,
    h: u32,
    tw: u32,
    th: u32,
    comps: usize,
) -> Vec<f32> {
    let mut out = vec![0.0f32; tw as usize * th as usize * comps];
    let sx = (w / tw).max(1) as usize;
    let sy = (h / th).max(1) as usize;
    let w = w as usize;
    let h = h as usize;
    let tw = tw as usize;
    let th = th as usize;
    for y in 0..th {
        let sy0 = y * sy;
        for x in 0..tw {
            let sx0 = x * sx;
            let mut acc = [0.0f32; 4];
            let mut n = 0.0f32;
            for dy in 0..sy {
                let sy_i = (sy0 + dy).min(h - 1);
                for dx in 0..sx {
                    let sx_i = (sx0 + dx).min(w - 1);
                    let si = (sy_i * w + sx_i) * comps;
                    for c in 0..comps {
                        acc[c] += src[si + c];
                    }
                    n += 1.0;
                }
            }
            let di = (y * tw + x) * comps;
            for c in 0..comps {
                out[di + c] = acc[c] / n.max(1.0);
            }
        }
    }
    out
}

fn read_u32(buf: &[u8], off: &mut usize) -> Option<u32> {
    if *off + 4 > buf.len() {
        return None;
    }
    let v = u32::from_be_bytes(buf[*off..*off + 4].try_into().ok()?);
    *off += 4;
    Some(v)
}

fn read_utf16be(buf: &[u8], off: &mut usize) -> Option<String> {
    let nbytes = read_u32(buf, off)? as usize;
    if nbytes % 2 != 0 || *off + nbytes > buf.len() {
        return None;
    }
    let units: Vec<u16> = buf[*off..*off + nbytes]
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    *off += nbytes;
    Some(String::from_utf16_lossy(&units).trim_end_matches('\0').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_german_and_english_painter_labels() {
        let ao = classify_mesh_map(
            "/Umgebungsverdeckungs-Map aus Mesh A3E_bogie?version=a3184115519990782365d6890c05b4dfe46c7664",
        )
        .unwrap();
        assert_eq!(ao.0, PainterMeshMapKind::AmbientOcclusion);
        assert_eq!(ao.1, "A3E_bogie");
        let wsn = classify_mesh_map(
            "/Raum-Normalen A3E_CurrentCollector?version=f8c6539ded14bfe8ed0f68be3ffbc277dd2460a0",
        )
        .unwrap();
        assert_eq!(wsn.0, PainterMeshMapKind::WorldSpaceNormal);
        let nrm = classify_mesh_map(
            "/Normal-Map aus Mesh A3E_bogie?version=afe4fc7428754348e3ad70bb73f12a935d400fae",
        )
        .unwrap();
        assert_eq!(nrm.0, PainterMeshMapKind::Normal);
        let curv = classify_mesh_map(
            "/Curvature A3E_bogie?version=d3aaf18460c9f2cd0a3bec005827c794a86da673",
        )
        .unwrap();
        assert_eq!(curv.0, PainterMeshMapKind::Curvature);
        assert!(classify_mesh_map(
            "/Stärke-Map aus Mesh A3E_bogie?version=98bc327e81743fb6c3f9b6042cdb535ec2a2b0b4",
        )
        .is_none());
    }
}
