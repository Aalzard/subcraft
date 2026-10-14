//! Image file I/O (PNG, TGA, JPEG, TIFF, BMP, HDR, EXR) via the `image` crate.

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
    let img = image::ImageReader::open(path)
        .map_err(image::ImageError::IoError)?
        .with_guessed_format()
        .map_err(image::ImageError::IoError)?
        .decode()?;
    Ok(from_dynamic(img))
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
                2 => v.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
                3 => v.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 1.0]).collect(),
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
pub const IMAGE_EXTENSIONS: [&str; 9] = ["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr", "bmp", "hdr"];

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
}

impl FileFormat {
    pub const ALL: [FileFormat; 5] =
        [FileFormat::Png8, FileFormat::Png16, FileFormat::Tga8, FileFormat::Jpeg, FileFormat::Exr32];

    pub fn extension(self) -> &'static str {
        match self {
            FileFormat::Png8 | FileFormat::Png16 => "png",
            FileFormat::Tga8 => "tga",
            FileFormat::Jpeg => "jpg",
            FileFormat::Exr32 => "exr",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FileFormat::Png8 => "PNG 8-bit",
            FileFormat::Png16 => "PNG 16-bit",
            FileFormat::Tga8 => "TGA 8-bit",
            FileFormat::Jpeg => "JPEG",
            FileFormat::Exr32 => "EXR 32-bit float",
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, FileFormat::Exr32)
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
    assert_eq!(data.len(), (width * height) as usize * comps);
    let q8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let q16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    match format {
        FileFormat::Png8 | FileFormat::Tga8 | FileFormat::Jpeg => {
            let bytes: Vec<u8> = data.iter().map(|&v| q8(v)).collect();
            let comps = if format == FileFormat::Jpeg && comps == 4 {
                // JPEG has no alpha: drop it.
                return save_image(path, width, height, 3, &drop_alpha(data), format);
            } else {
                comps
            };
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
                _ => image::ImageFormat::Jpeg,
            };
            image::save_buffer_with_format(path, &bytes, width, height, color, fmt)?;
        }
        FileFormat::Png16 => {
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
            img.save_with_format(path, image::ImageFormat::Png)?;
        }
        FileFormat::Exr32 => {
            // EXR via the image crate takes RGB/RGBA float.
            let rgba: Vec<f32> = match comps {
                1 => data.iter().flat_map(|&v| [v, v, v, 1.0]).collect(),
                2 => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
                3 => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 1.0]).collect(),
                _ => data.to_vec(),
            };
            let img: image::Rgba32FImage =
                image::ImageBuffer::from_raw(width, height, rgba).expect("size checked");
            image::DynamicImage::ImageRgba32F(img).save_with_format(path, image::ImageFormat::OpenExr)?;
        }
    }
    Ok(())
}

fn drop_alpha(data: &[f32]) -> Vec<f32> {
    data.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
