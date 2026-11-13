//! Image file I/O (PNG, TGA, JPEG, TIFF, BMP, HDR, WebP, EXR) via the `image` crate; half-float
//! EXR through the `exr` crate.

use std::path::Path;

use crate::bitmap::{Bitmap, BitmapData};

#[derive(Debug, thiserror::Error)]
pub enum ImageIoError {
    #[error("image decode/encode failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("unsupported image layout: {0}")]
    Unsupported(String),
}

/// Load any supported image file as an encoded-value bitmap.
pub fn load_bitmap(path: &Path) -> Result<Bitmap, ImageIoError> {
    let bytes = subcraft_core::vfs::read(path).map_err(image::ImageError::IoError)?;
    decode_bitmap(&bytes, path.file_name().and_then(|n| n.to_str()))
}

/// Decode an in-memory image file. The format is detected from the content; `name` (a file
/// name such as `wood.tga`) is the fallback for formats without a signature, like TGA.
pub fn decode_bitmap(bytes: &[u8], name: Option<&str>) -> Result<Bitmap, ImageIoError> {
    let format = match image::guess_format(bytes) {
        Ok(f) => f,
        Err(e) => name
            .and_then(|n| image::ImageFormat::from_path(n).ok())
            .ok_or(e)?,
    };
    Ok(from_dynamic(image::load_from_memory_with_format(bytes, format)?))
}

fn from_dynamic(img: image::DynamicImage) -> Bitmap {
    use image::DynamicImage as D;
    let (w, h) = (img.width(), img.height());
    match img {
        D::ImageLuma8(b) => Bitmap { width: w, height: h, channels: 1, data: BitmapData::U8(b.into_raw()) },
        D::ImageLumaA8(b) => Bitmap { width: w, height: h, channels: 2, data: BitmapData::U8(b.into_raw()) },
        D::ImageRgb8(b) => Bitmap { width: w, height: h, channels: 3, data: BitmapData::U8(b.into_raw()) },
        D::ImageRgba8(b) => Bitmap { width: w, height: h, channels: 4, data: BitmapData::U8(b.into_raw()) },
        D::ImageLuma16(b) => Bitmap { width: w, height: h, channels: 1, data: BitmapData::U16(b.into_raw()) },
        D::ImageLumaA16(b) => Bitmap { width: w, height: h, channels: 2, data: BitmapData::U16(b.into_raw()) },
        D::ImageRgb16(b) => Bitmap { width: w, height: h, channels: 3, data: BitmapData::U16(b.into_raw()) },
        D::ImageRgba16(b) => Bitmap { width: w, height: h, channels: 4, data: BitmapData::U16(b.into_raw()) },
        D::ImageRgb32F(b) => Bitmap { width: w, height: h, channels: 3, data: BitmapData::F32(b.into_raw()) },
        D::ImageRgba32F(b) => Bitmap { width: w, height: h, channels: 4, data: BitmapData::F32(b.into_raw()) },
        other => {
            let b = other.to_rgba32f();
            Bitmap { width: w, height: h, channels: 4, data: BitmapData::F32(b.into_raw()) }
        }
    }
}

/// Encode a bitmap losslessly in memory: PNG (8 / 16-bit) or EXR for float data.
/// Returns the bytes and the file extension.
pub fn encode_bitmap(b: &Bitmap) -> Result<(Vec<u8>, &'static str), ImageIoError> {
    use image::DynamicImage as D;
    let (w, h) = (b.width, b.height);
    let bad = || ImageIoError::Unsupported(format!("{} channels", b.channels));
    let (img, fmt, ext) = match &b.data {
        BitmapData::U8(v) => {
            let v = v.clone();
            let img = match b.channels {
                1 => D::ImageLuma8(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
                2 => D::ImageLumaA8(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
                3 => D::ImageRgb8(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
                _ => D::ImageRgba8(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
            };
            (img, image::ImageFormat::Png, "png")
        }
        BitmapData::U16(v) => {
            let v = v.clone();
            let img = match b.channels {
                1 => D::ImageLuma16(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
                2 => D::ImageLumaA16(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
                3 => D::ImageRgb16(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
                _ => D::ImageRgba16(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
            };
            (img, image::ImageFormat::Png, "png")
        }
        BitmapData::F32(v) => {
            let rgba: Vec<f32> = match b.channels {
                1 => v.iter().flat_map(|&x| [x, x, x, 1.0]).collect(),
                2 => v.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
                3 => v.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 1.0]).collect(),
                _ => v.clone(),
            };
            let img = D::ImageRgba32F(image::ImageBuffer::from_raw(w, h, rgba).ok_or_else(bad)?);
            (img, image::ImageFormat::OpenExr, "exr")
        }
    };
    let mut cur = std::io::Cursor::new(Vec::new());
    img.write_to(&mut cur, fmt)?;
    Ok((cur.into_inner(), ext))
}

/// File extensions accepted as texture images (file dialogs, texture-set import).
pub const IMAGE_EXTENSIONS: [&str; 10] = ["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr", "bmp", "hdr", "webp"];

/// Extension for encoded image bytes: detected from the content, else taken from `name` if it is
/// a known image extension (TGA has no signature), else "bin".
pub fn guess_extension(bytes: &[u8], name: Option<&str>) -> String {
    if let Some(ext) = image::guess_format(bytes).ok().and_then(|f| f.extensions_str().first().copied()) {
        return ext.to_string();
    }
    name.and_then(|n| std::path::Path::new(n).extension())
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or_else(|| "bin".to_string())
}

/// Output file encodings supported by the exporter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    Png8,
    Png16,
    Tga8,
    Jpeg,
    Exr32,
    Tiff8,
    Tiff16,
    /// 32-bit float TIFF (RGB/RGBA; one-component data is written as gray RGB).
    Tiff32,
    /// 16-bit (half) float EXR.
    Exr16,
    Bmp8,
    /// Radiance HDR (RGBE, RGB only: alpha is dropped, gray is written as gray RGB).
    Hdr32,
    /// Lossless WebP.
    Webp8,
}

/// File container of a [`FileFormat`] (what the preset editor's "Format" menu picks).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Container {
    Png,
    Tga,
    Exr,
    Tiff,
    Jpeg,
    Bmp,
    Hdr,
    Webp,
}

/// Bits per component (32 = float).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BitDepth {
    B8,
    B16,
    /// 16-bit (half) float.
    F16,
    F32,
}

impl Container {
    /// Alphabetical by extension, like Painter's File type menu.
    pub const ALL: [Container; 8] = [
        Container::Bmp,
        Container::Exr,
        Container::Hdr,
        Container::Jpeg,
        Container::Png,
        Container::Tga,
        Container::Tiff,
        Container::Webp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Container::Png => "PNG",
            Container::Tga => "TGA",
            Container::Exr => "EXR",
            Container::Tiff => "TIFF",
            Container::Jpeg => "JPEG",
            Container::Bmp => "BMP",
            Container::Hdr => "HDR",
            Container::Webp => "WebP",
        }
    }

    /// Bit depths this container can store, smallest first.
    pub fn depths(self) -> &'static [BitDepth] {
        match self {
            Container::Png => &[BitDepth::B8, BitDepth::B16],
            Container::Tga | Container::Jpeg | Container::Bmp | Container::Webp => &[BitDepth::B8],
            Container::Exr => &[BitDepth::F16, BitDepth::F32],
            Container::Hdr => &[BitDepth::F32],
            Container::Tiff => &[BitDepth::B8, BitDepth::B16, BitDepth::F32],
        }
    }
}

impl BitDepth {
    pub fn label(self) -> &'static str {
        match self {
            BitDepth::B8 => "8-bit",
            BitDepth::B16 => "16-bit",
            BitDepth::F16 => "16-bit float",
            BitDepth::F32 => "32-bit float",
        }
    }
}

impl FileFormat {
    pub const ALL: [FileFormat; 12] = [
        FileFormat::Png8,
        FileFormat::Png16,
        FileFormat::Tga8,
        FileFormat::Jpeg,
        FileFormat::Exr32,
        FileFormat::Tiff8,
        FileFormat::Tiff16,
        FileFormat::Tiff32,
        FileFormat::Exr16,
        FileFormat::Bmp8,
        FileFormat::Hdr32,
        FileFormat::Webp8,
    ];

    pub fn container(self) -> Container {
        match self {
            FileFormat::Png8 | FileFormat::Png16 => Container::Png,
            FileFormat::Tga8 => Container::Tga,
            FileFormat::Jpeg => Container::Jpeg,
            FileFormat::Exr32 | FileFormat::Exr16 => Container::Exr,
            FileFormat::Tiff8 | FileFormat::Tiff16 | FileFormat::Tiff32 => Container::Tiff,
            FileFormat::Bmp8 => Container::Bmp,
            FileFormat::Hdr32 => Container::Hdr,
            FileFormat::Webp8 => Container::Webp,
        }
    }

    pub fn depth(self) -> BitDepth {
        match self {
            FileFormat::Png8 | FileFormat::Tga8 | FileFormat::Jpeg | FileFormat::Tiff8 | FileFormat::Bmp8 | FileFormat::Webp8 => BitDepth::B8,
            FileFormat::Png16 | FileFormat::Tiff16 => BitDepth::B16,
            FileFormat::Exr16 => BitDepth::F16,
            FileFormat::Exr32 | FileFormat::Tiff32 | FileFormat::Hdr32 => BitDepth::F32,
        }
    }

    /// The format storing `container` at `depth`; when the container can't, the deepest depth it
    /// has below `depth` (TGA 16 -> TGA 8, PNG 32f -> PNG 16), else its smallest (EXR 8 -> 16f).
    pub fn from_parts(container: Container, depth: BitDepth) -> FileFormat {
        let depths = container.depths();
        // A float depth the container lacks goes to its other float depth before any integer one:
        // TIFF 16f is TIFF 32f, not 16-bit integer (which clamps to 0..1).
        let float = |d: BitDepth| matches!(d, BitDepth::F16 | BitDepth::F32);
        let depth = if float(depth) && !depths.contains(&depth) && let Some(other) = depths.iter().copied().find(|d| float(*d)) {
            other
        } else {
            depths.iter().rev().copied().find(|d| *d <= depth).unwrap_or(depths[0])
        };
        match (container, depth) {
            (Container::Png, BitDepth::B8) => FileFormat::Png8,
            (Container::Png, _) => FileFormat::Png16,
            (Container::Tga, _) => FileFormat::Tga8,
            (Container::Jpeg, _) => FileFormat::Jpeg,
            (Container::Exr, BitDepth::F16) => FileFormat::Exr16,
            (Container::Exr, _) => FileFormat::Exr32,
            (Container::Bmp, _) => FileFormat::Bmp8,
            (Container::Hdr, _) => FileFormat::Hdr32,
            (Container::Webp, _) => FileFormat::Webp8,
            (Container::Tiff, BitDepth::B8) => FileFormat::Tiff8,
            (Container::Tiff, BitDepth::B16) => FileFormat::Tiff16,
            (Container::Tiff, _) => FileFormat::Tiff32,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            FileFormat::Png8 | FileFormat::Png16 => "png",
            FileFormat::Tga8 => "tga",
            FileFormat::Jpeg => "jpg",
            FileFormat::Exr32 | FileFormat::Exr16 => "exr",
            FileFormat::Tiff8 | FileFormat::Tiff16 | FileFormat::Tiff32 => "tif",
            FileFormat::Bmp8 => "bmp",
            FileFormat::Hdr32 => "hdr",
            FileFormat::Webp8 => "webp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FileFormat::Png8 => "PNG 8-bit",
            FileFormat::Png16 => "PNG 16-bit",
            FileFormat::Tga8 => "TGA 8-bit",
            FileFormat::Jpeg => "JPEG",
            FileFormat::Exr32 => "EXR 32-bit float",
            FileFormat::Tiff8 => "TIFF 8-bit",
            FileFormat::Tiff16 => "TIFF 16-bit",
            FileFormat::Tiff32 => "TIFF 32-bit float",
            FileFormat::Exr16 => "EXR 16-bit float",
            FileFormat::Bmp8 => "BMP 8-bit",
            FileFormat::Hdr32 => "HDR 32-bit float",
            FileFormat::Webp8 => "WebP 8-bit",
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, FileFormat::Exr32 | FileFormat::Exr16 | FileFormat::Tiff32 | FileFormat::Hdr32)
    }

    /// 8 bits per component: the formats "8-bit + dithering" applies to.
    pub fn is_8bit(self) -> bool {
        self.depth() == BitDepth::B8
    }
}

/// Write `data` (row-major, `comps` interleaved, already in the file's encoding) to `path`.
/// For integer formats values are clamped to 0..1 and quantized with rounding.
pub fn save_image(
    path: &Path,
    width: u32,
    height: u32,
    comps: usize,
    data: &[f32],
    format: FileFormat,
) -> Result<(), ImageIoError> {
    let bytes = encode_image(width, height, comps, data, format)?;
    subcraft_core::vfs::write(path, &bytes).map_err(|e| ImageIoError::Image(image::ImageError::IoError(e)))
}

/// Encode `data` like [`save_image`] into an in-memory file.
pub fn encode_image(width: u32, height: u32, comps: usize, data: &[f32], format: FileFormat) -> Result<Vec<u8>, ImageIoError> {
    encode_image_dithered(width, height, comps, data, format, false)
}

/// Triangular noise in -1..1 for value `i`: deterministic, so a dithered export writes the same
/// bytes every time.
fn tpdf(i: usize) -> f32 {
    let h = |mut x: u64| {
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        x ^= x >> 33;
        (x >> 40) as f32 / (1u64 << 24) as f32
    };
    let i = i as u64;
    h(i.wrapping_mul(2)) + h(i.wrapping_mul(2) + 1) - 1.0
}

/// Quantize to 8 bits: rounded, or with triangular dither (Painter's "8 bits + dithering"),
/// which turns banding in smooth gradients into fine noise. The average level is kept; 0 and 1
/// stay exact.
pub fn quantize8(data: &[f32], dither: bool) -> Vec<u8> {
    if !dither {
        return data.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();
    }
    data.iter()
        .enumerate()
        .map(|(i, &v)| {
            let v = v.clamp(0.0, 1.0) * 255.0;
            // Pure black and white stay exact (masks, flat channels).
            if v <= 0.0 || v >= 255.0 {
                return v as u8;
            }
            (v + tpdf(i) + 0.5).floor().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// [`encode_image`], dithering 8-bit formats when `dither` is set (ignored for deeper formats).
pub fn encode_image_dithered(width: u32, height: u32, comps: usize, data: &[f32], format: FileFormat, dither: bool) -> Result<Vec<u8>, ImageIoError> {
    // usize arithmetic: `width * height` in u32 overflows past 65536 x 65536.
    assert_eq!(data.len(), width as usize * height as usize * comps);
    let mut out = std::io::Cursor::new(Vec::new());
    let q16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    match format {
        // JPEG has no alpha: drop it (gray + alpha keeps the gray).
        FileFormat::Jpeg if comps == 4 => return encode_image_dithered(width, height, 3, &drop_alpha(data), format, dither),
        FileFormat::Jpeg if comps == 2 => return encode_image_dithered(width, height, 1, &gray_only(data), format, dither),
        // The TIFF encoder has no gray + alpha layout: write it as RGBA with the gray in R, G, B.
        FileFormat::Tiff8 | FileFormat::Tiff16 if comps == 2 => return encode_image_dithered(width, height, 4, &gray_alpha_to_rgba(data), format, dither),
        FileFormat::Png8 | FileFormat::Tga8 | FileFormat::Jpeg | FileFormat::Tiff8 | FileFormat::Bmp8 | FileFormat::Webp8 => {
            let bytes = quantize8(data, dither);
            let color = match comps {
                1 => image::ExtendedColorType::L8,
                2 => image::ExtendedColorType::La8,
                3 => image::ExtendedColorType::Rgb8,
                4 => image::ExtendedColorType::Rgba8,
                n => return Err(ImageIoError::Unsupported(format!("{n} components"))),
            };
            let fmt = match format {
                FileFormat::Png8 => image::ImageFormat::Png,
                FileFormat::Tga8 => image::ImageFormat::Tga,
                FileFormat::Tiff8 => image::ImageFormat::Tiff,
                FileFormat::Bmp8 => image::ImageFormat::Bmp,
                FileFormat::Webp8 => image::ImageFormat::WebP,
                _ => image::ImageFormat::Jpeg,
            };
            image::write_buffer_with_format(&mut out, &bytes, width, height, color, fmt)?;
        }
        FileFormat::Png16 | FileFormat::Tiff16 => {
            let words: Vec<u16> = data.iter().map(|&v| q16(v)).collect();
            let img = match comps {
                1 => image::DynamicImage::ImageLuma16(
                    image::ImageBuffer::from_raw(width, height, words).expect("size checked"),
                ),
                2 => image::DynamicImage::ImageLumaA16(
                    image::ImageBuffer::from_raw(width, height, words).expect("size checked"),
                ),
                3 => image::DynamicImage::ImageRgb16(
                    image::ImageBuffer::from_raw(width, height, words).expect("size checked"),
                ),
                4 => image::DynamicImage::ImageRgba16(
                    image::ImageBuffer::from_raw(width, height, words).expect("size checked"),
                ),
                n => return Err(ImageIoError::Unsupported(format!("{n} components"))),
            };
            img.write_to(&mut out, if format == FileFormat::Tiff16 { image::ImageFormat::Tiff } else { image::ImageFormat::Png })?;
        }
        FileFormat::Tiff32 => {
            // The TIFF encoder takes RGB/RGBA float; gray data is written as gray RGB.
            let (color, floats): (_, Vec<f32>) = match comps {
                1 => (image::ExtendedColorType::Rgb32F, data.iter().flat_map(|&v| [v, v, v]).collect()),
                2 => (image::ExtendedColorType::Rgba32F, data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()),
                3 => (image::ExtendedColorType::Rgb32F, data.to_vec()),
                4 => (image::ExtendedColorType::Rgba32F, data.to_vec()),
                n => return Err(ImageIoError::Unsupported(format!("{n} components"))),
            };
            let bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_ne_bytes()).collect();
            image::write_buffer_with_format(&mut out, &bytes, width, height, color, image::ImageFormat::Tiff)?;
        }
        FileFormat::Exr32 => {
            // EXR via the image crate takes RGB/RGBA float.
            let rgba: Vec<f32> = match comps {
                1 => data.iter().flat_map(|&v| [v, v, v, 1.0]).collect(),
                2 => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
                3 => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 1.0]).collect(),
                4 => data.to_vec(),
                // More components used to reach `from_raw` with the wrong length and panic.
                n => return Err(ImageIoError::Unsupported(format!("{n} components"))),
            };
            let img: image::Rgba32FImage =
                image::ImageBuffer::from_raw(width, height, rgba).expect("size checked");
            image::DynamicImage::ImageRgba32F(img).write_to(&mut out, image::ImageFormat::OpenExr)?;
        }
        FileFormat::Exr16 => {
            use exr::prelude::{f16, Image, SpecificChannels, Vec2, WritableImage};
            let w = width as usize;
            let size = (w, height as usize);
            // Clamped to the half-float range: a larger value became infinity in the file.
            let h = |v: f32| f16::from_f32(v.clamp(-65504.0, 65504.0));
            let at = |p: Vec2<usize>, c: usize| data[(p.1 * w + p.0) * comps + c];
            let written = match comps {
                1 => Image::from_channels(size, SpecificChannels::rgb(|p: Vec2<usize>| (h(at(p, 0)), h(at(p, 0)), h(at(p, 0))))).write().to_buffered(&mut out),
                2 => Image::from_channels(size, SpecificChannels::rgba(|p: Vec2<usize>| (h(at(p, 0)), h(at(p, 0)), h(at(p, 0)), h(at(p, 1))))).write().to_buffered(&mut out),
                3 => Image::from_channels(size, SpecificChannels::rgb(|p: Vec2<usize>| (h(at(p, 0)), h(at(p, 1)), h(at(p, 2))))).write().to_buffered(&mut out),
                4 => Image::from_channels(size, SpecificChannels::rgba(|p: Vec2<usize>| (h(at(p, 0)), h(at(p, 1)), h(at(p, 2)), h(at(p, 3))))).write().to_buffered(&mut out),
                n => return Err(ImageIoError::Unsupported(format!("{n} components"))),
            };
            written.map_err(|e| ImageIoError::Unsupported(format!("EXR: {e}")))?;
        }
        FileFormat::Hdr32 => {
            // Radiance HDR stores RGB only, and no negative values.
            let rgb: Vec<f32> = match comps {
                1 => data.iter().flat_map(|&v| [v, v, v]).collect(),
                2 => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0]]).collect(),
                3 => data.to_vec(),
                4 => drop_alpha(data),
                n => return Err(ImageIoError::Unsupported(format!("{n} components"))),
            };
            let bytes: Vec<u8> = rgb.iter().flat_map(|v| v.max(0.0).to_ne_bytes()).collect();
            image::write_buffer_with_format(&mut out, &bytes, width, height, image::ExtendedColorType::Rgb32F, image::ImageFormat::Hdr)?;
        }
    }
    Ok(out.into_inner())
}

fn drop_alpha(data: &[f32]) -> Vec<f32> {
    data.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
}

fn gray_only(data: &[f32]) -> Vec<f32> {
    data.chunks_exact(2).map(|p| p[0]).collect()
}

fn gray_alpha_to_rgba(data: &[f32]) -> Vec<f32> {
    data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA 2026-11: a gray + alpha map (an export preset with L and A destinations) failed in
    /// JPEG and TIFF ("unsupported color type"), and more than four components panicked EXR 32.
    #[test]
    fn gray_alpha_maps_encode_in_every_format_and_extra_components_are_an_error() {
        let (w, h) = (4u32, 2u32);
        let data: Vec<f32> = (0..w * h).flat_map(|i| [i as f32 / 8.0, 1.0 - i as f32 / 8.0]).collect();
        for format in [FileFormat::Png8, FileFormat::Tga8, FileFormat::Jpeg, FileFormat::Tiff8, FileFormat::Tiff16, FileFormat::Tiff32, FileFormat::Png16, FileFormat::Exr16, FileFormat::Exr32] {
            let bytes = encode_image_dithered(w, h, 2, &data, format, false).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            let back = decode_bitmap(&bytes, Some(&format!("t.{}", format.extension()))).unwrap_or_else(|e| panic!("{format:?} does not read back: {e}"));
            assert_eq!((back.width, back.height), (w, h), "{format:?}");
            // Gray and alpha survive (8-bit formats quantize); JPEG is lossy and has no alpha.
            if format != FileFormat::Jpeg {
                let [g, _, _, a] = back.texel(3, 1, false);
                assert!((g - 7.0 / 8.0).abs() < 1.0 / 200.0 && (a - 1.0 / 8.0).abs() < 1.0 / 200.0, "{format:?}: gray {g}, alpha {a}");
            }
        }
        let five = vec![0.5f32; (w * h * 5) as usize];
        for format in [FileFormat::Exr32, FileFormat::Exr16, FileFormat::Png8, FileFormat::Png16, FileFormat::Tiff32, FileFormat::Hdr32] {
            assert!(encode_image_dithered(w, h, 5, &five, format, false).is_err(), "{format:?} rejects 5 components");
        }
    }

    #[test]
    fn png8_roundtrip_exact_quantization() {
        let dir = std::env::temp_dir().join(format!("subcraft-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.png");
        let data: Vec<f32> = (0..16 * 8 * 3).map(|i| (i % 256) as f32 / 255.0).collect();
        save_image(&path, 16, 8, 3, &data, FileFormat::Png8).unwrap();
        let b = load_bitmap(&path).unwrap();
        assert_eq!((b.width, b.height, b.channels), (16, 8, 3));
        let BitmapData::U8(bytes) = &b.data else { panic!("expected u8") };
        for (i, &v) in bytes.iter().enumerate() {
            assert_eq!(v as usize, i % 256);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn guesses_extension_from_content() {
        let dir = std::env::temp_dir().join(format!("subcraft-guess-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, fmt, ext) in [("a.png", FileFormat::Png8, "png"), ("b.tga", FileFormat::Tga8, "tga"), ("c.exr", FileFormat::Exr32, "exr")] {
            let path = dir.join(name);
            save_image(&path, 2, 2, 3, &[0.5; 12], fmt).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(guess_extension(&bytes, Some(name)), ext, "{name}");
            // Decoding from memory works for every format, TGA via the name hint.
            let b = decode_bitmap(&bytes, Some(name)).unwrap();
            assert_eq!((b.width, b.height), (2, 2), "{name}");
        }
        assert_eq!(guess_extension(b"<png bytes>", Some("x.png")), "png", "name fallback");
        assert_eq!(guess_extension(b"not an image", Some("notes.txt")), "bin");
        assert_eq!(guess_extension(b"not an image", None), "bin");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn encode_bitmap_roundtrips_every_depth() {
        let cases = [
            Bitmap::new_u8(2, 1, 3, vec![1, 2, 3, 250, 251, 252]),
            Bitmap { width: 1, height: 2, channels: 1, data: BitmapData::U16(vec![7, 65000]) },
            Bitmap::new_f32(1, 1, 3, vec![0.25, 2.5, -1.0]),
        ];
        for b in cases {
            let (bytes, ext) = encode_bitmap(&b).unwrap();
            let back = decode_bitmap(&bytes, None).unwrap();
            assert_eq!((back.width, back.height), (b.width, b.height));
            for y in 0..b.height {
                for x in 0..b.width {
                    let (a, c) = (b.texel(x, y, false), back.texel(x, y, false));
                    for k in 0..3 {
                        assert!((a[k] - c[k]).abs() < 1e-6, "{ext}: {a:?} vs {c:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn new_formats_roundtrip() {
        // 4x2 RGB gradient, values on the 8-bit grid so 8-bit formats are exact.
        let data: Vec<f32> = (0..4 * 2 * 3).map(|i| (i * 10) as f32 / 255.0).collect();
        for format in [FileFormat::Bmp8, FileFormat::Webp8] {
            let bytes = encode_image(4, 2, 3, &data, format).unwrap();
            let b = decode_bitmap(&bytes, Some(&format!("t.{}", format.extension()))).unwrap();
            assert_eq!((b.width, b.height), (4, 2), "{format:?}");
            for (i, px) in data.chunks_exact(3).enumerate() {
                let t = b.texel(i as u32 % 4, i as u32 / 4, false);
                for c in 0..3 {
                    assert!((t[c] - px[c]).abs() < 1e-6, "{format:?} lossless: {t:?} vs {px:?}");
                }
            }
            assert_eq!(guess_extension(&bytes, None), format.extension());
        }
        // Half float keeps HDR values to half precision; gray is written as gray RGB.
        let hdr = vec![0.0, 0.5, 2.5, -1.0];
        let b = decode_bitmap(&encode_image(2, 2, 1, &hdr, FileFormat::Exr16).unwrap(), Some("t.exr")).unwrap();
        assert!(b.is_hdr());
        for (i, &v) in hdr.iter().enumerate() {
            let t = b.texel(i as u32 % 2, i as u32 / 2, false);
            assert!((t[0] - v).abs() < 1e-3 && (t[2] - v).abs() < 1e-3, "{t:?} vs {v}");
        }
        // Half float is half the size of 32-bit float for the same image.
        let big: Vec<f32> = (0..64 * 64 * 3).map(|i| (i % 97) as f32 * 0.37).collect();
        let (h, f) = (encode_image(64, 64, 3, &big, FileFormat::Exr16).unwrap(), encode_image(64, 64, 3, &big, FileFormat::Exr32).unwrap());
        assert!(h.len() < f.len(), "half {} vs full {}", h.len(), f.len());
        // Radiance HDR: RGBE (shared exponent) keeps about 1% precision; negatives clamp to 0.
        let b = decode_bitmap(&encode_image(2, 2, 1, &hdr, FileFormat::Hdr32).unwrap(), Some("t.hdr")).unwrap();
        assert!(b.is_hdr());
        assert!((b.texel(0, 1, false)[0] - 2.5).abs() < 0.03 && b.texel(1, 1, false)[0] == 0.0);
    }

    #[test]
    fn dithering_keeps_the_mean_and_breaks_up_bands() {
        // A level between two 8-bit steps: plain rounding writes one flat value, dithering mixes
        // the neighbours so the average stays at the true level.
        let level = 100.3 / 255.0;
        let data = vec![level; 4096];
        let plain = quantize8(&data, false);
        assert!(plain.iter().all(|&v| v == 100));
        let dithered = quantize8(&data, true);
        let mean = dithered.iter().map(|&v| v as f32).sum::<f32>() / dithered.len() as f32;
        assert!((mean - 100.3).abs() < 0.05, "mean {mean}");
        assert!(dithered.iter().all(|&v| (99..=102).contains(&v)), "noise stays within a step or two");
        assert_eq!(dithered, quantize8(&data, true), "deterministic");
        // Black and white stay exact.
        assert!(quantize8(&[0.0; 64], true).iter().all(|&v| v == 0));
        assert!(quantize8(&[1.0; 64], true).iter().all(|&v| v == 255));
        // Deeper formats ignore the flag.
        let g: Vec<f32> = (0..16).map(|i| i as f32 / 15.0).collect();
        assert_eq!(encode_image_dithered(4, 4, 1, &g, FileFormat::Png16, true).unwrap(), encode_image(4, 4, 1, &g, FileFormat::Png16).unwrap());
    }

    #[test]
    fn containers_list_their_depths() {
        assert_eq!(FileFormat::from_parts(Container::Exr, BitDepth::F16), FileFormat::Exr16);
        assert_eq!(FileFormat::from_parts(Container::Exr, BitDepth::B8), FileFormat::Exr16, "smallest the container has");
        assert_eq!(FileFormat::from_parts(Container::Hdr, BitDepth::B16), FileFormat::Hdr32);
        assert_eq!(FileFormat::from_parts(Container::Png, BitDepth::F16), FileFormat::Png16);
        for f in FileFormat::ALL {
            assert_eq!(FileFormat::from_parts(f.container(), f.depth()), f, "{f:?}");
            assert!(f.container().depths().contains(&f.depth()));
        }
    }

    #[test]
    fn exr_roundtrip_float() {
        let dir = std::env::temp_dir().join(format!("subcraft-exr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.exr");
        let data = vec![0.0, 0.5, 2.5, -1.0];
        save_image(&path, 2, 2, 1, &data, FileFormat::Exr32).unwrap();
        let b = load_bitmap(&path).unwrap();
        assert!(b.is_hdr());
        assert!((b.texel(1, 1, false)[0] + 1.0).abs() < 1e-6);
        assert!((b.texel(0, 1, false)[0] - 2.5).abs() < 1e-6);
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod depth_fallback_tests {
    use super::*;

    /// QA 2026-11: TIFF at "16f" fell back to 16-bit integer (values clamp to 0..1) instead of the
    /// float depth TIFF does have; half-float EXR wrote infinity for values above 65504.
    #[test]
    fn float_depths_stay_float_and_half_floats_stay_finite() {
        assert_eq!(FileFormat::from_parts(Container::Tiff, BitDepth::F16), FileFormat::Tiff32);
        assert_eq!(FileFormat::from_parts(Container::Tiff, BitDepth::B16), FileFormat::Tiff16);
        assert_eq!(FileFormat::from_parts(Container::Png, BitDepth::F32), FileFormat::Png16, "no float depth: the deepest integer one");
        assert_eq!(FileFormat::from_parts(Container::Exr, BitDepth::B8), FileFormat::Exr16);
        let bytes = encode_image(1, 1, 1, &[1.0e6], FileFormat::Exr16).unwrap();
        let back = decode_bitmap(&bytes, Some("x.exr")).unwrap();
        let v = back.texel(0, 0, false)[0];
        assert!(v.is_finite() && v > 60000.0, "{v}");
    }
}
