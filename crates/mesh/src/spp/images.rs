//! Images inside a Painter project: baked mesh maps (`resources/<sha1>`, JPEG 2000 tiles) and the
//! flattened channel textures of the texture cache (`texture-cache/<uid>/<n>/<channel flag>`, LZ4).

use std::collections::BTreeMap;

use rayon::prelude::*;

use super::{jp2, lz4, qt};

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("unsupported image: {0}")]
    Unsupported(String),
    #[error("corrupt image: {0}")]
    Corrupt(&'static str),
    #[error(transparent)]
    Jp2(#[from] jp2::Jp2Error),
    #[error(transparent)]
    Lz4(#[from] lz4::Lz4Error),
}

/// Pixels as 0..1 floats (half-float data keeps its own range), row-major, top row first.
#[derive(Debug, Clone)]
pub struct Plane {
    pub width: u32,
    pub height: u32,
    pub components: usize,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn pixel(&self, x: u32, y: u32) -> &[f32] {
        let i = (y as usize * self.width as usize + x as usize) * self.components;
        &self.data[i..i + self.components]
    }
}

fn corrupt<T>(what: &'static str) -> Result<T, ImageError> {
    Err(ImageError::Corrupt(what))
}

fn half_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) & 1) as u32;
    let exp = ((h >> 10) & 31) as u32;
    let frac = (h & 1023) as u32;
    let bits = match (exp, frac) {
        (0, 0) => sign << 31,
        (0, f) => {
            // Subnormal: renormalise.
            let shift = f.leading_zeros() - 21;
            let f = (f << shift) & 1023;
            (sign << 31) | ((113 - shift) << 23) | (f << 13)
        }
        (31, f) => (sign << 31) | (0xff << 23) | (f << 13),
        (e, f) => (sign << 31) | ((e + 112) << 23) | (f << 13),
    };
    f32::from_bits(bits)
}

/// Largest power-of-two reduction that brings `size` to at most `max`.
fn reduction(size: u32, max: u32) -> u32 {
    let mut s = 1;
    while size / s > max && s < 64 {
        s *= 2;
    }
    s
}

/// Accumulates full-resolution rows or tiles into a box-reduced plane.
struct Reducer {
    out: Plane,
    scale: u32,
    counts: Vec<u32>,
}

impl Reducer {
    fn new(width: u32, height: u32, components: usize, max: u32) -> Self {
        let scale = reduction(width.max(height), max);
        let (w, h) = (width.div_ceil(scale), height.div_ceil(scale));
        Self { out: Plane { width: w, height: h, components, data: vec![0.0; w as usize * h as usize * components] }, scale, counts: vec![0; w as usize * h as usize] }
    }

    fn add(&mut self, x: u32, y: u32, px: &[f32]) {
        let (ox, oy) = ((x / self.scale) as usize, (y / self.scale) as usize);
        let o = oy * self.out.width as usize + ox;
        self.counts[o] += 1;
        let c = self.out.components;
        for (d, v) in self.out.data[o * c..(o + 1) * c].iter_mut().zip(px) {
            *d += v;
        }
    }

    fn finish(mut self) -> Plane {
        let c = self.out.components;
        for (i, n) in self.counts.iter().enumerate() {
            if *n > 1 {
                for d in &mut self.out.data[i * c..(i + 1) * c] {
                    *d /= *n as f32;
                }
            }
        }
        self.out
    }
}

// ----- baked maps (JPEG 2000 tiles) -----------------------------------------------------------------

/// Decode a `resources/<sha1>` image blob, reduced to at most `max_size` on its longer side.
pub fn decode_resource_image(blob: &[u8], max_size: u32) -> Result<Plane, ImageError> {
    let Some((props, mut pos)) = qt::read_properties(blob, 0) else { return corrupt("property header") };
    let get = |k: &str| props.get(k);
    if get("compression").and_then(|v| v.as_str()) != Some("jp2") {
        return Err(ImageError::Unsupported(format!("resource compression {:?}", get("compression"))));
    }
    let (Some(width), Some(height), Some(tiles)) = (get("width").and_then(|v| v.as_u32()), get("height").and_then(|v| v.as_u32()), get("tilesCount").and_then(|v| v.as_u32())) else {
        return corrupt("image size");
    };
    let format = get("format").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let components = match format.as_str() {
        "LUM16" | "LUM8" => 1,
        "RGBA16" | "RGBA8" => 4,
        "RGB16" | "RGB8" => 3,
        other => return Err(ImageError::Unsupported(format!("resource format {other}"))),
    };
    if width == 0 || height == 0 || width > 16384 || height > 16384 || tiles > 4096 {
        return corrupt("image size");
    }
    let le = |b: &[u8], o: usize| -> Option<u32> { b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])) };
    let mut jobs: Vec<([u32; 4], &[u8])> = Vec::with_capacity(tiles as usize);
    for _ in 0..tiles {
        let (Some(x0), Some(y0), Some(x1), Some(y1), Some(len)) = (le(blob, pos), le(blob, pos + 4), le(blob, pos + 8), le(blob, pos + 12), le(blob, pos + 16)) else {
            return corrupt("tile table");
        };
        pos += 24;
        let len = len as usize;
        let data = blob.get(pos..pos + len).ok_or(ImageError::Corrupt("tile data"))?;
        // Each tile's data is padded to a multiple of 8 bytes.
        pos += (len + 7) & !7;
        if x1 <= x0 || y1 <= y0 || x1 > width || y1 > height {
            return corrupt("tile rectangle");
        }
        jobs.push(([x0, y0, x1, y1], data));
    }
    if jobs.is_empty() {
        return corrupt("no tiles");
    }
    // Drop the finest wavelet levels the target size does not need (each halves the size).
    let reduce = reduction(width.max(height), max_size).trailing_zeros() as usize;
    let first = jp2::decode_reduced(jobs[0].1, reduce)?;
    let shift = first.reduced;
    let mut red = Reducer::new(width.div_ceil(1 << shift), height.div_ceil(1 << shift), components, max_size);
    let mut px = vec![0.0f32; components];
    // Tiles are independent codestreams: decode a batch in parallel, then fold it into the result.
    for (b, batch) in jobs.chunks(16).enumerate() {
        let decoded: Vec<Result<jp2::Decoded, jp2::Jp2Error>> = batch.par_iter().map(|(_, data)| jp2::decode_reduced(data, shift)).collect();
        for (k, ((rect, _), tile)) in batch.iter().zip(decoded).enumerate() {
            let tile = if b == 0 && k == 0 { first.clone() } else { tile? };
            let [x0, y0, x1, y1] = *rect;
            if tile.reduced != shift || (x1 - x0).div_ceil(1 << shift) as usize != tile.width || (y1 - y0).div_ceil(1 << shift) as usize != tile.height {
                return corrupt("tile rectangle");
            }
            let max = ((1u32 << tile.precision) - 1) as f32;
            for ty in 0..tile.height {
                for tx in 0..tile.width {
                    let s = &tile.samples[(ty * tile.width + tx) * tile.components..][..tile.components];
                    for (c, p) in px.iter_mut().enumerate() {
                        // Missing components: replicate gray, opaque alpha.
                        *p = match (s.get(c), c) {
                            (Some(v), _) => *v as f32 / max,
                            (None, 3) => 1.0,
                            _ => s[0] as f32 / max,
                        };
                    }
                    red.add((x0 >> shift) + tx as u32, (y0 >> shift) + ty as u32, &px);
                }
            }
        }
    }
    Ok(red.finish())
}

// ----- texture cache (LZ4) --------------------------------------------------------------------------

/// What a texture-cache entry stores, from its OpenGL internal format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheFormat {
    /// `GL_SRGB8_ALPHA8`: color, sRGB encoded, with alpha.
    Srgb8Alpha8,
    /// `GL_RG8`: a value and its alpha.
    Rg8,
    /// `GL_RG16F`.
    Rg16f,
    /// `GL_RGBA16F`.
    Rgba16f,
}

impl CacheFormat {
    fn from_gl(v: u32) -> Option<Self> {
        Some(match v {
            35907 => Self::Srgb8Alpha8,
            33323 => Self::Rg8,
            33327 => Self::Rg16f,
            34842 => Self::Rgba16f,
            _ => return None,
        })
    }
    fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Srgb8Alpha8 => 4,
            Self::Rg8 => 2,
            Self::Rg16f => 4,
            Self::Rgba16f => 8,
        }
    }
    pub fn components(self) -> usize {
        match self {
            Self::Srgb8Alpha8 | Self::Rgba16f => 4,
            Self::Rg8 | Self::Rg16f => 2,
        }
    }
}

/// A decoded texture-cache buffer; `format` tells how to read `plane`.
pub struct CacheImage {
    pub format: CacheFormat,
    pub plane: Plane,
}

/// Decode a `texture-cache/<uid>/<n>/<flag>` blob, reduced to at most `max_size`.
/// Decode an imported image resource (`resources/<sha1>.image`: mesh maps baked elsewhere, bitmap
/// fills, stencils): a property header, then either JPEG 2000 tiles as in the baked maps or one raw
/// LZ4 block of `dataSizeLZ4` bytes holding `height` rows of `pitch` bytes, top row first (unlike
/// the texture cache: text in the stencils of a real project reads upright only this way), then
/// optional ICC data. Formats: BGRA8, RGBA8, RGBA16,
/// RGBA16F, L8, L16. Returned top row first, at most `max_size` on the longer side.
pub fn decode_image_resource(blob: &[u8], max_size: u32) -> Result<Plane, ImageError> {
    let Some((props, pos)) = qt::read_properties(blob, 0) else { return corrupt("property header") };
    match props.get("compression").and_then(|v| v.as_str()) {
        Some("jp2") => return decode_resource_image(blob, max_size),
        Some("lz4") => {}
        other => return Err(ImageError::Unsupported(format!("image compression {other:?}"))),
    }
    let get = |k: &str| props.get(k).and_then(|v| v.as_u32());
    let (Some(width), Some(height), Some(pitch), Some(raw_len), Some(lz4_len)) = (get("width"), get("height"), get("pitch"), get("dataSizeRaw"), get("dataSizeLZ4")) else {
        return corrupt("image header");
    };
    let format = props.get("format").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (components, bytes): (usize, usize) = match format.as_str() {
        "BGRA8" | "RGBA8" => (4, 1),
        "RGBA16" => (4, 2),
        "RGBA16F" => (4, 2),
        "L8" | "LUM8" => (1, 1),
        "L16" | "LUM16" => (1, 2),
        other => return Err(ImageError::Unsupported(format!("image format {other}"))),
    };
    if width == 0 || height == 0 || width > 16384 || height > 16384 || (pitch as usize) < width as usize * components * bytes || raw_len as usize != pitch as usize * height as usize {
        return corrupt("image size");
    }
    let raw = lz4::decompress(blob.get(pos..pos + lz4_len as usize).ok_or(ImageError::Corrupt("image data"))?, raw_len as usize)?;
    let mut red = Reducer::new(width, height, components, max_size);
    let mut px = vec![0.0f32; components];
    for y in 0..height {
        let row = &raw[y as usize * pitch as usize..][..width as usize * components * bytes];
        for x in 0..width as usize {
            let b = &row[x * components * bytes..][..components * bytes];
            for (c, p) in px.iter_mut().enumerate() {
                *p = match (format.as_str(), bytes) {
                    ("BGRA8", _) => b[[2, 1, 0, 3][c]] as f32 / 255.0,
                    (_, 1) => b[c] as f32 / 255.0,
                    ("RGBA16F", _) => half_to_f32(u16::from_le_bytes([b[c * 2], b[c * 2 + 1]])),
                    _ => u16::from_le_bytes([b[c * 2], b[c * 2 + 1]]) as f32 / 65535.0,
                };
            }
            red.add(x as u32, y, &px);
        }
    }
    Ok(red.finish())
}

/// Whether an imported image resource stores 8-bit values (color data in them is sRGB encoded).
pub fn image_resource_is_8bit(blob: &[u8]) -> bool {
    qt::read_properties(blob, 0).and_then(|(p, _)| p.get("format").and_then(|v| v.as_str()).map(|f| f.ends_with('8'))).unwrap_or(false)
}

pub fn decode_cache_image(blob: &[u8], max_size: u32) -> Result<CacheImage, ImageError> {
    let Some((props, pos)) = qt::read_properties(blob, 0) else { return corrupt("property header") };
    if props.get("compression").and_then(|v| v.as_str()) != Some("lz4") {
        return Err(ImageError::Unsupported(format!("cache compression {:?}", props.get("compression"))));
    }
    let get = |k: &str| props.get(k).and_then(|v| v.as_u32());
    let (Some(width), Some(height), Some(buffer), Some(gl)) = (get("width"), get("height"), get("bufferSize"), get("internalFormat")) else {
        return corrupt("cache header");
    };
    let format = CacheFormat::from_gl(gl).ok_or_else(|| ImageError::Unsupported(format!("OpenGL format {gl}")))?;
    if width == 0 || height == 0 || width > 16384 || height > 16384 || buffer as usize != width as usize * height as usize * format.bytes_per_pixel() {
        return corrupt("cache buffer size");
    }
    let mut r = qt::Reader::new(blob, pos);
    let len = r.u32().ok_or(ImageError::Corrupt("cache length"))? as usize;
    let start = r.pos;
    let raw = lz4::decompress(blob.get(start..start + len).ok_or(ImageError::Corrupt("cache data"))?, buffer as usize)?;
    let c = format.components();
    let mut red = Reducer::new(width, height, c, max_size);
    let bpp = format.bytes_per_pixel();
    let mut px = vec![0.0f32; c];
    for y in 0..height {
        for x in 0..width {
            let b = &raw[(y as usize * width as usize + x as usize) * bpp..][..bpp];
            match format {
                CacheFormat::Srgb8Alpha8 | CacheFormat::Rg8 => {
                    for (p, v) in px.iter_mut().zip(b) {
                        *p = *v as f32 / 255.0;
                    }
                }
                CacheFormat::Rg16f | CacheFormat::Rgba16f => {
                    for (p, v) in px.iter_mut().zip(b.chunks_exact(2)) {
                        *p = half_to_f32(u16::from_le_bytes([v[0], v[1]]));
                    }
                }
            }
            red.add(x, y, &px);
        }
    }
    Ok(CacheImage { format, plane: red.finish() })
}

// ----- the resource table of contents ---------------------------------------------------------------

/// `resources.toc`: image resources as `(display name, blob key)`, e.g. `("World Space Normals Chalice",
/// "6f49...")`. The key is the `version=` in the stored name and the dataset name under `resources/`.
pub fn read_toc(toc: &[u8]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut r = qt::Reader::new(toc, 0);
    if r.u32() != Some(1) {
        return out;
    }
    while r.pos < toc.len() {
        let Some(_kind) = r.string() else { break };
        let Some(n) = r.u32() else { break };
        for _ in 0..n {
            let Some(name) = r.string() else { return out };
            if let Some((display, key)) = name.trim_start_matches('/').split_once("?version=") {
                out.insert(display.to_string(), key.to_string());
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BakedKind {
    Normal,
    WorldNormal,
    ColorId,
    AmbientOcclusion,
    Curvature,
    Position,
    Thickness,
}

/// Classify a mesh-map resource name; returns the kind and the texture-set (material) name it ends with.
pub fn classify_baked(display: &str) -> Option<(BakedKind, &str)> {
    const PREFIXES: [(&str, BakedKind); 7] = [
        ("World Space Normals ", BakedKind::WorldNormal),
        ("Normal Map from Mesh ", BakedKind::Normal),
        ("Color Map from Mesh ", BakedKind::ColorId),
        ("Ambient Occlusion Map from Mesh ", BakedKind::AmbientOcclusion),
        ("Curvature Map from Mesh ", BakedKind::Curvature),
        ("Thickness Map from Mesh ", BakedKind::Thickness),
        ("Position ", BakedKind::Position),
    ];
    PREFIXES.iter().find_map(|(p, k)| display.strip_prefix(p).map(|rest| (*k, rest)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats() {
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0xc000), -2.0);
        assert_eq!(half_to_f32(0x3800), 0.5);
        assert_eq!(half_to_f32(0), 0.0);
        assert!((half_to_f32(1) - 5.9604645e-8).abs() < 1e-12);
        assert!(half_to_f32(0x7c00).is_infinite());
    }

    #[test]
    fn baked_names() {
        assert_eq!(classify_baked("World Space Normals Chalice"), Some((BakedKind::WorldNormal, "Chalice")));
        assert_eq!(classify_baked("Position Room Floor"), Some((BakedKind::Position, "Room Floor")));
        assert_eq!(classify_baked("DBAL-A2_normal"), None);
    }

    #[test]
    fn reduction_is_power_of_two() {
        assert_eq!(reduction(8192, 4096), 2);
        assert_eq!(reduction(4096, 4096), 1);
        assert_eq!(reduction(16384, 2048), 8);
    }
}
