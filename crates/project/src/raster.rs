//! Binary codec for `TiledImage`s inside project files.
//!
//! Layout (little endian): magic `SCTILE`, u16 version, u32 width, u32 height, u8 comps,
//! 4 × f32 fill, u32 allocated-tile count, then per tile: u32 index + `TILE_TEXELS × comps` f16.
//! Only allocated tiles are stored (sparse layers stay small); the zip layer compresses them.

use std::sync::Arc;

use half::f16;
use subcraft_image::{TILE_TEXELS, Tile, TiledImage};

const MAGIC: &[u8; 6] = b"SCTILE";
const VERSION: u16 = 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RasterError {
    #[error("not a SubCraft raster blob")]
    BadMagic,
    #[error("unsupported raster blob version {0}")]
    Version(u16),
    #[error("truncated raster blob")]
    Truncated,
    #[error("invalid raster: {0}")]
    Invalid(String),
}

pub fn encode(img: &TiledImage) -> Vec<u8> {
    let comps = img.comps();
    let allocated: Vec<usize> = (0..img.tile_count()).filter(|&i| img.is_allocated(i)).collect();
    let mut out = Vec::with_capacity(40 + allocated.len() * (4 + TILE_TEXELS * comps * 2));
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&img.width().to_le_bytes());
    out.extend_from_slice(&img.height().to_le_bytes());
    out.push(comps as u8);
    for f in img.fill() {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out.extend_from_slice(&(allocated.len() as u32).to_le_bytes());
    for i in allocated {
        out.extend_from_slice(&(i as u32).to_le_bytes());
        let tile = img.tile(i).expect("allocated");
        out.extend_from_slice(bytemuck::cast_slice(&tile.data));
    }
    out
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], RasterError> {
        let s = self.b.get(self.at..self.at + n).ok_or(RasterError::Truncated)?;
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, RasterError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    fn f32(&mut self) -> Result<f32, RasterError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
}

pub fn decode(bytes: &[u8]) -> Result<TiledImage, RasterError> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(6)? != MAGIC {
        return Err(RasterError::BadMagic);
    }
    let version = u16::from_le_bytes(r.take(2)?.try_into().expect("2 bytes"));
    if version != VERSION {
        return Err(RasterError::Version(version));
    }
    let (w, h) = (r.u32()?, r.u32()?);
    let comps = r.take(1)?[0] as usize;
    if !(1..=4).contains(&comps) || w == 0 || h == 0 || w > 16384 || h > 16384 {
        return Err(RasterError::Invalid(format!("{w}x{h}x{comps}")));
    }
    let fill = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
    let mut img = TiledImage::new(w, h, comps, fill);
    let n = r.u32()? as usize;
    let tile_bytes = TILE_TEXELS * comps * 2;
    for _ in 0..n {
        let i = r.u32()? as usize;
        if i >= img.tile_count() {
            return Err(RasterError::Invalid(format!("tile index {i}")));
        }
        let raw = r.take(tile_bytes)?;
        let data: Vec<f16> = raw.chunks_exact(2).map(|c| f16::from_le_bytes([c[0], c[1]])).collect();
        img.set_slot(i, Some(Arc::new(Tile { data: data.into_boxed_slice() })));
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sparse_exact() {
        let mut img = TiledImage::new(200, 130, 2, [0.25, 0.0, 0.0, 0.0]);
        img.set(5, 5, [1.0, 0.5, 0.0, 0.0]);
        img.set(150, 100, [0.125, 1.0, 0.0, 0.0]);
        let bytes = encode(&img);
        let back = decode(&bytes).unwrap();
        assert_eq!(back, img);
        assert_eq!(back.allocated_tiles(), 2);
        assert_eq!(back.fill(), img.fill());
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert_eq!(decode(b"nope").unwrap_err(), RasterError::Truncated);
        assert_eq!(decode(b"XXXXXXXXXXXXXXXXXXXX").unwrap_err(), RasterError::BadMagic);
        let mut img = TiledImage::new(64, 64, 1, [0.0; 4]);
        img.set(1, 1, [1.0; 4]);
        let bytes = encode(&img);
        assert_eq!(decode(&bytes[..bytes.len() - 3]).unwrap_err(), RasterError::Truncated);
    }
}
