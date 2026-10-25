//! Read-only probe of Painter `texture-cache/` (SVT GPU tile blobs in HDF5).

use hdf5_reader::Hdf5File;

#[derive(Debug, Clone, Default)]
pub struct TextureCacheProbe {
    pub present: bool,
    pub header_bytes: usize,
    pub header_version: Option<u32>,
    pub material_caches: usize,
    pub tile_datasets: usize,
    pub total_tile_bytes: u64,
}

/// Decoded composite channel buffers from Painter `texture-cache/{mat_uid}/{stack_id}/`.
#[derive(Debug, Clone)]
pub struct CachedMaterialChannels {
    pub material_name: String,
    pub material_uid: u64,
    pub width: u32,
    pub height: u32,
    /// `GL_SRGB8_ALPHA8` (`0x8C43`), 4 bytes per pixel `[r, g, b, a]`.
    pub base_color_rgba8: Option<Vec<u8>>,
    /// `GL_RG8` (`0x822B`), 2 bytes per pixel `[roughness, alpha]`.
    pub roughness_ga8: Option<Vec<u8>>,
    /// `GL_RG8` (`0x822B`), 2 bytes per pixel `[metallic, alpha]`.
    pub metallic_ga8: Option<Vec<u8>>,
    /// `GL_RG16F` (`0x822F`), 1 `f32` per pixel.
    pub height_f32: Option<Vec<f32>>,
    /// `GL_RGBA16F` (`0x881A`), 3 `f32` per pixel `[nx, ny, nz]` in `[0..1]` OpenGL tangent space.
    pub normal_rgb_f32: Option<Vec<f32>>,
}

/// Layout validated on Painter 11 `Drehgestell.spp`:
/// `texture-cache/header` + `texture-cache/{hash}/{stackId}/{tileKey}` uint8 payloads.
fn sum_all_tile_bytes() -> bool {
    std::env::var("SUBCRAFT_SPP_FULL_CACHE_PROBE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn probe_texture_cache(spp_path: &std::path::Path) -> Result<TextureCacheProbe, String> {
    let sum_all = sum_all_tile_bytes();
    let file = Hdf5File::open(spp_path).map_err(|e| format!("HDF5 open {}: {e}", spp_path.display()))?;
    let root = file.root_group().map_err(|e| format!("root group: {e}"))?;
    let tc = match root.group("texture-cache") {
        Ok(g) => g,
        Err(_) => return Ok(TextureCacheProbe::default()),
    };

    let mut out = TextureCacheProbe {
        present: true,
        ..Default::default()
    };

    if let Ok(header_ds) = tc.dataset("header") {
        let hdr = header_ds
            .read_raw_bytes()
            .map_err(|e| format!("texture-cache/header: {e}"))?;
        out.header_bytes = hdr.len();
        out.header_version = parse_svt_header(&hdr);
    }

    let (groups, datasets) = tc.members().map_err(|e| format!("texture-cache members: {e}"))?;
    for g in groups {
        if g.name() == "header" {
            continue;
        }
        if g.name().chars().all(|c| c.is_ascii_digit()) {
            out.material_caches += 1;
            let path = format!("texture-cache/{}", g.name());
            let (tiles, bytes) = count_tile_datasets(&file, &path, 0, sum_all)?;
            out.tile_datasets += tiles;
            out.total_tile_bytes += bytes;
        }
    }
    for ds in datasets {
        if ds.name() == "header" {
            continue;
        }
        if ds.name().chars().all(|c| c.is_ascii_digit()) {
            out.tile_datasets += 1;
            if sum_all {
                out.total_tile_bytes += ds.raw_byte_len().map_err(|e| e.to_string())? as u64;
            } else if out.total_tile_bytes == 0 {
                out.total_tile_bytes = ds.raw_byte_len().map_err(|e| e.to_string())? as u64;
            }
        }
    }
    if !sum_all && out.tile_datasets > 1 && out.total_tile_bytes > 0 {
        out.total_tile_bytes *= out.tile_datasets as u64;
    }

    Ok(out)
}

pub fn texture_cache_decodable(probe: &TextureCacheProbe) -> bool {
    probe.present
        && probe.header_bytes >= 12
        && probe.header_version.is_some()
        && probe.material_caches > 0
        && probe.tile_datasets > 0
        && probe.total_tile_bytes > 1024
}

fn count_tile_datasets(
    file: &Hdf5File,
    path: &str,
    depth: u32,
    sum_all_tile_bytes: bool,
) -> Result<(usize, u64), String> {
    if depth > 8 {
        return Ok((0, 0));
    }
    let group = file.group(path).map_err(|e| format!("{path}: {e}"))?;
    let (groups, datasets) = group.members().map_err(|e| format!("{path}: {e}"))?;
    let mut count = 0usize;
    let mut bytes = 0u64;
    for ds in &datasets {
        if ds.name().chars().all(|c| c.is_ascii_digit()) {
            count += 1;
            if sum_all_tile_bytes {
                bytes += ds.raw_byte_len().map_err(|e| e.to_string())? as u64;
            } else if bytes == 0 {
                bytes = ds.raw_byte_len().map_err(|e| e.to_string())? as u64;
            }
        }
    }
    for g in groups {
        let child = format!("{}/{}", path, g.name());
        let (c, b) = count_tile_datasets(file, &child, depth + 1, sum_all_tile_bytes)?;
        count += c;
        if sum_all_tile_bytes {
            bytes += b;
        } else if bytes == 0 && b > 0 {
            bytes = b;
        }
    }
    Ok((count, bytes))
}

/// UTF-16LE key/value pairs: `u32 count`, then `(u32 byte_len, utf16 key, u32 value)*`.
fn parse_svt_header(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 12 {
        return None;
    }
    let mut off = 0usize;
    let pair_count = read_u32(bytes, &mut off)? as usize;
    if pair_count == 0 || pair_count > 64 {
        return None;
    }
    let mut version = None;
    for _ in 0..pair_count {
        let byte_len = read_u32(bytes, &mut off)? as usize;
        if byte_len == 0 {
            break;
        }
        if byte_len > 512 || byte_len % 2 != 0 || off + byte_len + 4 > bytes.len() {
            return None;
        }
        let key = String::from_utf16_lossy(
            &bytes[off..off + byte_len]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        );
        off += byte_len;
        let val = read_u32(bytes, &mut off)?;
        if key == "version" {
            version = Some(val);
        }
    }
    version
}

fn read_u32(bytes: &[u8], off: &mut usize) -> Option<u32> {
    if *off + 4 > bytes.len() {
        return None;
    }
    // Painter SVT cache header uses big-endian `u32` fields (verified on Painter 11 .spp).
    let v = u32::from_be_bytes(bytes[*off..*off + 4].try_into().unwrap());
    *off += 4;
    Some(v)
}

/// Decode all material composite channel textures from `texture-cache/` in `spp_path`,
/// downsampled to at most `max_res` × `max_res`.
pub fn decode_texture_cache(
    spp_path: &std::path::Path,
    max_res: u32,
) -> Result<Vec<CachedMaterialChannels>, String> {
    let resolved = super::container::resolve_spp_path(spp_path);
    let file = Hdf5File::open(&resolved)
        .map_err(|e| format!("HDF5 open {}: {e}", resolved.display()))?;
    let root = file.root_group().map_err(|e| format!("root group: {e}"))?;
    let tc = match root.group("texture-cache") {
        Ok(g) => g,
        Err(_) => return Ok(Vec::new()),
    };

    let mat_uids = file
        .dataset("paint/scene.assbin")
        .ok()
        .and_then(|ds| ds.read_raw_bytes().ok())
        .map(|b| extract_assbin_material_uids(&b))
        .unwrap_or_default();

    let (groups, _) = tc.members().map_err(|e| format!("texture-cache members: {e}"))?;
    let mut out = Vec::new();
    for g in &groups {
        let Ok(uid) = g.name().parse::<u64>() else {
            continue;
        };
        let mat_name = mat_uids
            .iter()
            .find(|(_, u)| *u == uid)
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| g.name().to_string());

        let (subgroups, _) = g.members().map_err(|e| format!("tc group {}: {e}", g.name()))?;
        let Some(stack_grp) = subgroups.first() else {
            continue;
        };
        let (_, datasets) = stack_grp
            .members()
            .map_err(|e| format!("tc subgroup {}: {e}", stack_grp.name()))?;

        let target_res = max_res.max(128);
        let mut entry = CachedMaterialChannels {
            material_name: mat_name,
            material_uid: uid,
            width: target_res,
            height: target_res,
            base_color_rgba8: None,
            roughness_ga8: None,
            metallic_ga8: None,
            height_f32: None,
            normal_rgb_f32: None,
        };

        for ds in &datasets {
            match ds.name() {
                "0" => {
                    // Painter 11.x/12.x (`data_version = 91`): channel enum `0` = BaseColor (`GL_RGBA16` / `GL_SRGB8_ALPHA8`).
                    if let Ok(raw) = ds.read_raw_bytes() {
                        if let Ok((w, h, internal_fmt, dec)) = decode_qvariant_lz4_channel(&raw) {
                            let tw = w.min(target_res);
                            let th = h.min(target_res);
                            entry.width = tw;
                            entry.height = th;
                            entry.base_color_rgba8 = Some(downsample_base_color(
                                &dec,
                                w,
                                h,
                                tw,
                                th,
                                internal_fmt,
                            ));
                        }
                    }
                }
                "1" | "16777216" => {
                    // In older Painter (`data_version <= 90`), `"1"` (`1 << 0`) is BaseColor/Diffuse (`0x8C43` or `0x805B`).
                    // In newer Painter (`data_version >= 91`) when `"0"` is present, `"1"` is Height (`0x822F` `GL_RG16F`).
                    if let Ok(raw) = ds.read_raw_bytes() {
                        if let Ok((w, h, internal_fmt, dec)) = decode_qvariant_lz4_channel(&raw) {
                            let tw = w.min(target_res);
                            let th = h.min(target_res);
                            if internal_fmt == 0x822F {
                                if entry.height_f32.is_none() {
                                    entry.height_f32 =
                                        Some(downsample_rg16f_to_f32(&dec, w, h, tw, th));
                                }
                            } else {
                                entry.width = tw;
                                entry.height = th;
                                entry.base_color_rgba8 = Some(downsample_base_color(
                                    &dec,
                                    w,
                                    h,
                                    tw,
                                    th,
                                    internal_fmt,
                                ));
                            }
                        }
                    }
                }
                "7" | "128" => {
                    if let Ok(raw) = ds.read_raw_bytes() {
                        if let Ok((w, h, internal_fmt, dec)) = decode_qvariant_lz4_channel(&raw) {
                            let tw = w.min(target_res);
                            let th = h.min(target_res);
                            entry.roughness_ga8 = Some(downsample_ga(
                                &dec,
                                w,
                                h,
                                tw,
                                th,
                                internal_fmt,
                            ));
                        }
                    }
                }
                "13" | "8192" => {
                    if let Ok(raw) = ds.read_raw_bytes() {
                        if let Ok((w, h, internal_fmt, dec)) = decode_qvariant_lz4_channel(&raw) {
                            let tw = w.min(target_res);
                            let th = h.min(target_res);
                            entry.metallic_ga8 = Some(downsample_ga(
                                &dec,
                                w,
                                h,
                                tw,
                                th,
                                internal_fmt,
                            ));
                        }
                    }
                }
                "2" => {
                    if let Ok(raw) = ds.read_raw_bytes() {
                        if let Ok((w, h, _internal_fmt, dec)) = decode_qvariant_lz4_channel(&raw) {
                            let tw = w.min(target_res);
                            let th = h.min(target_res);
                            entry.height_f32 = Some(downsample_rg16f_to_f32(&dec, w, h, tw, th));
                        }
                    }
                }
                "22" | "4194304" => {
                    if let Ok(raw) = ds.read_raw_bytes() {
                        if let Ok((w, h, _internal_fmt, dec)) = decode_qvariant_lz4_channel(&raw) {
                            let tw = w.min(target_res);
                            let th = h.min(target_res);
                            entry.normal_rgb_f32 =
                                Some(downsample_rgba16f_to_rgb_f32(&dec, w, h, tw, th));
                        }
                    }
                }
                _ => {}
            }
        }
        if entry.base_color_rgba8.is_some()
            || entry.roughness_ga8.is_some()
            || entry.metallic_ga8.is_some()
            || entry.height_f32.is_some()
            || entry.normal_rgb_f32.is_some()
        {
            out.push(entry);
        }
    }

    // Sort in the order materials appear in `scene.assbin` if available.
    if !mat_uids.is_empty() {
        out.sort_by_key(|e| {
            mat_uids
                .iter()
                .position(|(_, u)| *u == e.material_uid)
                .unwrap_or(usize::MAX)
        });
    }
    Ok(out)
}

fn extract_assbin_material_uids(bytes: &[u8]) -> Vec<(String, u64)> {
    const NAME_KEY: &[u8] = b"?mat.name";
    const UID_KEY: &[u8] = b"$mat.uid";
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + NAME_KEY.len() + 24 <= bytes.len() {
        if &bytes[i..i + NAME_KEY.len()] == NAME_KEY {
            let mut cur = i + NAME_KEY.len() + 16;
            if cur + 4 <= bytes.len() {
                let slen = u32::from_le_bytes(bytes[cur..cur + 4].try_into().unwrap()) as usize;
                cur += 4;
                if slen > 0 && slen <= 128 && cur + slen <= bytes.len() {
                    let raw_name = &bytes[cur..cur + slen];
                    let end = raw_name.iter().position(|&b| b == 0).unwrap_or(raw_name.len());
                    if let Ok(s) = std::str::from_utf8(&raw_name[..end]) {
                        let name = s.trim().to_string();
                        cur += slen;
                        // Search for `$mat.uid\0` within the next 256 bytes.
                        let limit = (cur + 256).min(bytes.len());
                        let mut k = cur;
                        while k + UID_KEY.len() + 24 <= limit {
                            if &bytes[k..k + UID_KEY.len()] == UID_KEY {
                                let u_off = k + UID_KEY.len() + 16;
                                if u_off + 8 <= bytes.len() {
                                    let uid =
                                        u64::from_le_bytes(bytes[u_off..u_off + 8].try_into().unwrap());
                                    out.push((name, uid));
                                }
                                break;
                            }
                            k += 1;
                        }
                        i = k;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

fn decode_qvariant_lz4_channel(raw: &[u8]) -> Result<(u32, u32, u32, Vec<u8>), String> {
    let mut off = 0usize;
    let map_count = read_u32(raw, &mut off).ok_or("eof at map_count")?;
    if map_count == 0 || map_count > 32 {
        return Err(format!("unexpected QVariantMap count {map_count}"));
    }
    let mut buffer_size = 0usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut internal_format = 0u32;
    for _ in 0..map_count {
        let key_len = read_u32(raw, &mut off).ok_or("eof at key_len")? as usize;
        if off + key_len > raw.len() || key_len % 2 != 0 {
            return Err("bad key_len".into());
        }
        let key = String::from_utf16_lossy(
            &raw[off..off + key_len]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        );
        off += key_len;
        let qtype = read_u32(raw, &mut off).ok_or("eof at qtype")?;
        if off >= raw.len() {
            return Err("eof at is_null".into());
        }
        off += 1; // is_null byte
        match qtype {
            2 | 3 => {
                let val = read_u32(raw, &mut off).ok_or("eof at int val")?;
                match key.as_str() {
                    "bufferSize" => buffer_size = val as usize,
                    "width" => width = val,
                    "height" => height = val,
                    "internalFormat" => internal_format = val,
                    _ => {}
                }
            }
            10 => {
                let slen = read_u32(raw, &mut off).ok_or("eof at str len")? as usize;
                if off + slen > raw.len() {
                    return Err("eof in QString".into());
                }
                off += slen;
            }
            other => return Err(format!("unsupported QVariant type {other} for key {key}")),
        }
    }
    let comp_len = read_u32(raw, &mut off).ok_or("eof at comp_len")? as usize;
    if off + comp_len > raw.len() || buffer_size == 0 || width == 0 || height == 0 {
        return Err("invalid compressed payload bounds".into());
    }
    let dec = lz4_decompress_block(&raw[off..off + comp_len], buffer_size)?;
    if dec.len() != buffer_size {
        return Err(format!("lz4 size mismatch: {} != {}", dec.len(), buffer_size));
    }
    Ok((width, height, internal_format, dec))
}

fn lz4_decompress_block(src: &[u8], capacity: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(capacity);
    let mut pos = 0usize;
    while pos < src.len() {
        let token = src[pos];
        pos += 1;
        let mut lit_len = (token >> 4) as usize;
        if lit_len == 15 {
            loop {
                if pos >= src.len() {
                    return Err("eof in lit_len".into());
                }
                let b = src[pos] as usize;
                pos += 1;
                lit_len += b;
                if b != 255 {
                    break;
                }
            }
        }
        if pos + lit_len > src.len() {
            return Err("literal overrun".into());
        }
        out.extend_from_slice(&src[pos..pos + lit_len]);
        pos += lit_len;
        if pos >= src.len() {
            break;
        }
        if pos + 2 > src.len() {
            return Err("eof in match offset".into());
        }
        let offset = u16::from_le_bytes([src[pos], src[pos + 1]]) as usize;
        pos += 2;
        if offset == 0 || offset > out.len() {
            return Err(format!("bad match offset {offset}"));
        }
        let mut match_len = ((token & 0x0f) as usize) + 4;
        if (token & 0x0f) == 15 {
            loop {
                if pos >= src.len() {
                    return Err("eof in match_len".into());
                }
                let b = src[pos] as usize;
                pos += 1;
                match_len += b;
                if b != 255 {
                    break;
                }
            }
        }
        let start = out.len() - offset;
        if offset >= match_len {
            out.extend_from_within(start..start + match_len);
        } else {
            for i in 0..match_len {
                let b = out[start + i];
                out.push(b);
            }
        }
    }
    Ok(out)
}

/// Dispatch BaseColor downsampling based on OpenGL `internalFormat` and byte stride:
/// - `0x805B` (`GL_RGBA16`) or 8 bytes/pixel: 16-bit unsigned integer RGBA (`u16` LE per channel)
/// - `0x881A` (`GL_RGBA16F`): 16-bit float RGBA (`f16` LE per channel)
/// - `0x8C43` (`GL_SRGB8_ALPHA8`) or `0x8058` (`GL_RGBA8`) or 4 bytes/pixel: 8-bit RGBA (`u8` per channel)
fn downsample_base_color(
    src: &[u8],
    w: u32,
    h: u32,
    tw: u32,
    th: u32,
    internal_format: u32,
) -> Vec<u8> {
    let bpp = src.len() / ((w as usize).saturating_mul(h as usize).max(1));
    if internal_format == 0x805B || (bpp == 8 && internal_format != 0x881A) {
        downsample_rgba16_to_rgba8(src, w, h, tw, th)
    } else if internal_format == 0x881A {
        downsample_rgba16f_to_rgba8(src, w, h, tw, th)
    } else {
        downsample_rgba8(src, w, h, tw, th)
    }
}

/// Convert linear `[0, 1]` to sRGB `u8` `[0, 255]`.
#[inline]
fn linear_to_srgb_u8(x: f32) -> u8 {
    let x = x.clamp(0.0, 1.0);
    let s = if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5) as u8
}

fn downsample_rgba16_to_rgba8(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw * th * 4) as usize];
    let sx = (w / tw).max(1) as usize;
    let sy = (h / th).max(1) as usize;
    let w = w as usize;
    let h = h as usize;
    let tw = tw as usize;
    let th = th as usize;
    for y in 0..th {
        let sy0 = (th - 1 - y) * sy;
        for x in 0..tw {
            let sx0 = x * sx;
            let mut sum_r = 0u64;
            let mut sum_g = 0u64;
            let mut sum_b = 0u64;
            let mut sum_a = 0u64;
            for dy in 0..sy {
                let sy_i = (sy0 + dy).min(h - 1);
                let row_off = sy_i * w;
                for dx in 0..sx {
                    let sx_i = (sx0 + dx).min(w - 1);
                    let si = (row_off + sx_i) * 8;
                    let r16 = u16::from_le_bytes([src[si], src[si + 1]]) as u64;
                    let g16 = u16::from_le_bytes([src[si + 2], src[si + 3]]) as u64;
                    let b16 = u16::from_le_bytes([src[si + 4], src[si + 5]]) as u64;
                    let a16 = u16::from_le_bytes([src[si + 6], src[si + 7]]) as u64;
                    if a16 > 0 {
                        sum_r += r16 * a16;
                        sum_g += g16 * a16;
                        sum_b += b16 * a16;
                        sum_a += a16;
                    }
                }
            }
            let di = (y * tw + x) * 4;
            if sum_a > 0 {
                let r_lin = (sum_r as f64 / sum_a as f64 / 65535.0) as f32;
                let g_lin = (sum_g as f64 / sum_a as f64 / 65535.0) as f32;
                let b_lin = (sum_b as f64 / sum_a as f64 / 65535.0) as f32;
                out[di] = linear_to_srgb_u8(r_lin);
                out[di + 1] = linear_to_srgb_u8(g_lin);
                out[di + 2] = linear_to_srgb_u8(b_lin);
                out[di + 3] = 255;
            }
        }
    }
    dilate_rgba8(&mut out, tw, th);
    out
}

fn downsample_rgba16f_to_rgba8(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw * th * 4) as usize];
    let sx = (w / tw).max(1) as usize;
    let sy = (h / th).max(1) as usize;
    let w = w as usize;
    let h = h as usize;
    let tw = tw as usize;
    let th = th as usize;
    for y in 0..th {
        let sy0 = (th - 1 - y) * sy;
        for x in 0..tw {
            let sx0 = x * sx;
            let mut sum_r = 0.0f32;
            let mut sum_g = 0.0f32;
            let mut sum_b = 0.0f32;
            let mut sum_a = 0.0f32;
            for dy in 0..sy {
                let sy_i = (sy0 + dy).min(h - 1);
                let row_off = sy_i * w;
                for dx in 0..sx {
                    let sx_i = (sx0 + dx).min(w - 1);
                    let si = (row_off + sx_i) * 8;
                    let r = f16_to_f32(u16::from_le_bytes([src[si], src[si + 1]]));
                    let g = f16_to_f32(u16::from_le_bytes([src[si + 2], src[si + 3]]));
                    let b = f16_to_f32(u16::from_le_bytes([src[si + 4], src[si + 5]]));
                    let a = f16_to_f32(u16::from_le_bytes([src[si + 6], src[si + 7]]));
                    if a.is_finite() && a > 0.0 && r.is_finite() && g.is_finite() && b.is_finite() {
                        sum_r += r.clamp(0.0, 1.0) * a;
                        sum_g += g.clamp(0.0, 1.0) * a;
                        sum_b += b.clamp(0.0, 1.0) * a;
                        sum_a += a;
                    }
                }
            }
            let di = (y * tw + x) * 4;
            if sum_a > 0.0 {
                out[di] = linear_to_srgb_u8(sum_r / sum_a);
                out[di + 1] = linear_to_srgb_u8(sum_g / sum_a);
                out[di + 2] = linear_to_srgb_u8(sum_b / sum_a);
                out[di + 3] = 255;
            }
        }
    }
    dilate_rgba8(&mut out, tw, th);
    out
}

fn downsample_rgba8(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw * th * 4) as usize];
    let sx = (w / tw).max(1) as usize;
    let sy = (h / th).max(1) as usize;
    let w = w as usize;
    let h = h as usize;
    let tw = tw as usize;
    let th = th as usize;
    for y in 0..th {
        // Painter's texture-cache is stored bottom-up (OpenGL texture origin v=0 at row 0),
        // whereas SubCraft bitmaps & mesh UVs use top-down origin (y=0 at v_subcraft=0 = 1-v_assbin).
        let sy0 = (th - 1 - y) * sy;
        for x in 0..tw {
            let sx0 = x * sx;
            let mut sum_r = 0u64;
            let mut sum_g = 0u64;
            let mut sum_b = 0u64;
            let mut sum_a = 0u64;
            for dy in 0..sy {
                let sy_i = (sy0 + dy).min(h - 1);
                let row_off = sy_i * w;
                for dx in 0..sx {
                    let sx_i = (sx0 + dx).min(w - 1);
                    let si = (row_off + sx_i) * 4;
                    let a = src[si + 3] as u64;
                    if a > 0 {
                        sum_r += src[si] as u64 * a;
                        sum_g += src[si + 1] as u64 * a;
                        sum_b += src[si + 2] as u64 * a;
                        sum_a += a;
                    }
                }
            }
            let di = (y * tw + x) * 4;
            if sum_a > 0 {
                out[di] = (sum_r / sum_a) as u8;
                out[di + 1] = (sum_g / sum_a) as u8;
                out[di + 2] = (sum_b / sum_a) as u8;
                out[di + 3] = 255;
            }
        }
    }
    dilate_rgba8(&mut out, tw, th);
    out
}

fn dilate_rgba8(buf: &mut [u8], w: usize, h: usize) {
    let total = w * h;
    let mut queue = std::collections::VecDeque::new();
    let mut has_transparent = false;
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            let idx = row + x;
            if buf[idx * 4 + 3] == 0 {
                has_transparent = true;
                continue;
            }
            if (x > 0 && buf[(idx - 1) * 4 + 3] == 0)
                || (x + 1 < w && buf[(idx + 1) * 4 + 3] == 0)
                || (y > 0 && buf[(idx - w) * 4 + 3] == 0)
                || (y + 1 < h && buf[(idx + w) * 4 + 3] == 0)
            {
                queue.push_back(idx);
            }
        }
    }
    if !has_transparent || queue.is_empty() {
        return;
    }
    while let Some(idx) = queue.pop_front() {
        let x = idx % w;
        let y = idx / w;
        let r = buf[idx * 4];
        let g = buf[idx * 4 + 1];
        let b = buf[idx * 4 + 2];
        let neighbors = [
            (x > 0).then(|| idx - 1),
            (x + 1 < w).then(|| idx + 1),
            (y > 0).then(|| idx - w),
            (y + 1 < h).then(|| idx + w),
        ];
        for n in neighbors.into_iter().flatten() {
            if buf[n * 4 + 3] == 0 {
                buf[n * 4] = r;
                buf[n * 4 + 1] = g;
                buf[n * 4 + 2] = b;
                buf[n * 4 + 3] = 255;
                queue.push_back(n);
            }
        }
    }
    let _ = total;
}

/// Dispatch grayscale+alpha (`GL_RG8` `0x822B` vs `GL_RG16` `0x822C` / `GL_RG16F` `0x822F`).
fn downsample_ga(
    src: &[u8],
    w: u32,
    h: u32,
    tw: u32,
    th: u32,
    internal_format: u32,
) -> Vec<u8> {
    let bpp = src.len() / ((w as usize).saturating_mul(h as usize).max(1));
    if internal_format == 0x822C || (bpp == 4 && internal_format != 0x822F) {
        downsample_ga16_to_ga8(src, w, h, tw, th)
    } else if internal_format == 0x822F {
        let f = downsample_rg16f_to_f32(src, w, h, tw, th);
        let mut out = Vec::with_capacity(f.len() * 2);
        for v in f {
            out.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
            out.push(255);
        }
        out
    } else {
        downsample_ga8(src, w, h, tw, th)
    }
}

fn downsample_ga16_to_ga8(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw * th * 2) as usize];
    let sx = (w / tw).max(1) as usize;
    let sy = (h / th).max(1) as usize;
    let w = w as usize;
    let h = h as usize;
    let tw = tw as usize;
    let th = th as usize;
    for y in 0..th {
        let sy0 = (th - 1 - y) * sy;
        for x in 0..tw {
            let sx0 = x * sx;
            let mut sum_v = 0u64;
            let mut sum_a = 0u64;
            for dy in 0..sy {
                let sy_i = (sy0 + dy).min(h - 1);
                let row_off = sy_i * w;
                for dx in 0..sx {
                    let sx_i = (sx0 + dx).min(w - 1);
                    let si = (row_off + sx_i) * 4;
                    let v16 = u16::from_le_bytes([src[si], src[si + 1]]) as u64;
                    let a16 = u16::from_le_bytes([src[si + 2], src[si + 3]]) as u64;
                    if a16 > 0 {
                        sum_v += v16 * a16;
                        sum_a += a16;
                    }
                }
            }
            let di = (y * tw + x) * 2;
            if sum_a > 0 {
                out[di] = ((sum_v / sum_a + 128) / 257).min(255) as u8;
                out[di + 1] = 255;
            }
        }
    }
    dilate_ga8(&mut out, tw, th);
    out
}

fn downsample_ga8(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let mut out = vec![0u8; (tw * th * 2) as usize];
    let sx = (w / tw).max(1) as usize;
    let sy = (h / th).max(1) as usize;
    let w = w as usize;
    let h = h as usize;
    let tw = tw as usize;
    let th = th as usize;
    for y in 0..th {
        let sy0 = (th - 1 - y) * sy;
        for x in 0..tw {
            let sx0 = x * sx;
            let mut sum_v = 0u64;
            let mut sum_a = 0u64;
            for dy in 0..sy {
                let sy_i = (sy0 + dy).min(h - 1);
                let row_off = sy_i * w;
                for dx in 0..sx {
                    let sx_i = (sx0 + dx).min(w - 1);
                    let si = (row_off + sx_i) * 2;
                    let a = src[si + 1] as u64;
                    if a > 0 {
                        sum_v += src[si] as u64 * a;
                        sum_a += a;
                    }
                }
            }
            let di = (y * tw + x) * 2;
            if sum_a > 0 {
                out[di] = (sum_v / sum_a) as u8;
                out[di + 1] = 255;
            }
        }
    }
    dilate_ga8(&mut out, tw, th);
    out
}

fn dilate_ga8(buf: &mut [u8], w: usize, h: usize) {
    let mut queue = std::collections::VecDeque::new();
    let mut has_transparent = false;
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            let idx = row + x;
            if buf[idx * 2 + 1] == 0 {
                has_transparent = true;
                continue;
            }
            if (x > 0 && buf[(idx - 1) * 2 + 1] == 0)
                || (x + 1 < w && buf[(idx + 1) * 2 + 1] == 0)
                || (y > 0 && buf[(idx - w) * 2 + 1] == 0)
                || (y + 1 < h && buf[(idx + w) * 2 + 1] == 0)
            {
                queue.push_back(idx);
            }
        }
    }
    if !has_transparent || queue.is_empty() {
        return;
    }
    while let Some(idx) = queue.pop_front() {
        let x = idx % w;
        let y = idx / w;
        let v = buf[idx * 2];
        let neighbors = [
            (x > 0).then(|| idx - 1),
            (x + 1 < w).then(|| idx + 1),
            (y > 0).then(|| idx - w),
            (y + 1 < h).then(|| idx + w),
        ];
        for n in neighbors.into_iter().flatten() {
            if buf[n * 2 + 1] == 0 {
                buf[n * 2] = v;
                buf[n * 2 + 1] = 255;
                queue.push_back(n);
            }
        }
    }
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) as u32) << 31;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mant = (bits & 0x03ff) as u32;
    let f_bits = if exp == 0 {
        if mant == 0 {
            sign
        } else {
            let mut m = mant;
            let mut e = 127u32 - 14;
            while (m & 0x0400) == 0 {
                m <<= 1;
                e -= 1;
            }
            m &= 0x03ff;
            sign | (e << 23) | (m << 13)
        }
    } else if exp == 31 {
        sign | 0x7f80_0000 | (mant << 13)
    } else {
        sign | ((exp + 127 - 15) << 23) | (mant << 13)
    };
    f32::from_bits(f_bits)
}

fn downsample_rg16f_to_f32(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<f32> {
    let mut out = vec![0.0f32; (tw * th) as usize];
    let sx = (w / tw).max(1) as usize;
    let sy = (h / th).max(1) as usize;
    let w = w as usize;
    let h = h as usize;
    let tw = tw as usize;
    let th = th as usize;
    for y in 0..th {
        let sy0 = (th - 1 - y) * sy;
        for x in 0..tw {
            let sx0 = x * sx;
            let mut sum_v = 0.0f32;
            let mut sum_a = 0.0f32;
            for dy in 0..sy {
                let sy_i = (sy0 + dy).min(h - 1);
                let row_off = sy_i * w;
                for dx in 0..sx {
                    let sx_i = (sx0 + dx).min(w - 1);
                    let si = (row_off + sx_i) * 4;
                    let val = f16_to_f32(u16::from_le_bytes([src[si], src[si + 1]]));
                    let alpha = f16_to_f32(u16::from_le_bytes([src[si + 2], src[si + 3]]));
                    if alpha.is_finite() && alpha > 0.0 && val.is_finite() {
                        sum_v += val * alpha;
                        sum_a += alpha;
                    }
                }
            }
            out[y * tw + x] = if sum_a > 0.0 { sum_v / sum_a } else { 0.0 };
        }
    }
    out
}

fn downsample_rgba16f_to_rgb_f32(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<f32> {
    let mut out = vec![0.0f32; (tw * th * 3) as usize];
    let sx = (w / tw).max(1) as usize;
    let sy = (h / th).max(1) as usize;
    let w = w as usize;
    let h = h as usize;
    let tw = tw as usize;
    let th = th as usize;
    for y in 0..th {
        let sy0 = (th - 1 - y) * sy;
        for x in 0..tw {
            let sx0 = x * sx;
            let mut sum_r = 0.0f32;
            let mut sum_g = 0.0f32;
            let mut sum_b = 0.0f32;
            let mut sum_a = 0.0f32;
            for dy in 0..sy {
                let sy_i = (sy0 + dy).min(h - 1);
                let row_off = sy_i * w;
                for dx in 0..sx {
                    let sx_i = (sx0 + dx).min(w - 1);
                    let si = (row_off + sx_i) * 8;
                    let r = f16_to_f32(u16::from_le_bytes([src[si], src[si + 1]]));
                    let g = f16_to_f32(u16::from_le_bytes([src[si + 2], src[si + 3]]));
                    let b = f16_to_f32(u16::from_le_bytes([src[si + 4], src[si + 5]]));
                    let a = f16_to_f32(u16::from_le_bytes([src[si + 6], src[si + 7]]));
                    if a.is_finite() && a > 0.0 && r.is_finite() && g.is_finite() && b.is_finite() {
                        sum_r += r.clamp(0.0, 1.0) * a;
                        sum_g += g.clamp(0.0, 1.0) * a;
                        sum_b += b.clamp(0.0, 1.0) * a;
                        sum_a += a;
                    }
                }
            }
            let di = (y * tw + x) * 3;
            if sum_a > 0.0 {
                out[di] = (sum_r / sum_a).clamp(0.0, 1.0);
                out[di + 1] = (sum_g / sum_a).clamp(0.0, 1.0);
                out[di + 2] = (sum_b / sum_a).clamp(0.0, 1.0);
            } else {
                out[di] = 0.5;
                out[di + 1] = 0.5;
                out[di + 2] = 1.0;
            }
        }
    }
    out
}
