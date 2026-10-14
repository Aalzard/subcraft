//! Dense source bitmaps (imported textures, brush alphas, environment maps).
//!
//! Values are kept in their *encoded* form; callers decide per use whether to linearize
//! (a bitmap plugged into Base Color is sRGB, the same bitmap in Roughness is raw data).

use std::sync::LazyLock;

use subcraft_core::color::{srgb8_to_linear_lut, srgb_to_linear};

#[derive(Clone, Debug, PartialEq)]
pub enum BitmapData {
    U8(Vec<u8>),
    U16(Vec<u16>),
    F32(Vec<f32>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Wrap {
    #[default]
    Repeat,
    Clamp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// 1 = gray, 2 = gray+alpha, 3 = rgb, 4 = rgba.
    pub channels: u8,
    pub data: BitmapData,
}

static SRGB_LUT: LazyLock<[f32; 256]> = LazyLock::new(srgb8_to_linear_lut);

impl Bitmap {
    pub fn new_f32(width: u32, height: u32, channels: u8, data: Vec<f32>) -> Self {
        assert_eq!(data.len(), (width * height) as usize * channels as usize);
        Self { width, height, channels, data: BitmapData::F32(data) }
    }

    pub fn new_u8(width: u32, height: u32, channels: u8, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), (width * height) as usize * channels as usize);
        Self { width, height, channels, data: BitmapData::U8(data) }
    }

    pub fn is_hdr(&self) -> bool {
        matches!(self.data, BitmapData::F32(_))
    }

    pub fn byte_size(&self) -> usize {
        match &self.data {
            BitmapData::U8(v) => v.len(),
            BitmapData::U16(v) => v.len() * 2,
            BitmapData::F32(v) => v.len() * 4,
        }
    }

    /// Normalized texel as RGBA. Gray is replicated; missing alpha is 1.
    /// When `linearize` is set, RGB is decoded from sRGB (no-op for float data).
    #[inline]
    pub fn texel(&self, x: u32, y: u32, linearize: bool) -> [f32; 4] {
        let ch = self.channels as usize;
        let o = (y as usize * self.width as usize + x as usize) * ch;
        let mut raw = [0.0f32; 4];
        match &self.data {
            BitmapData::U8(v) => {
                for c in 0..ch {
                    let b = v[o + c];
                    let is_alpha = (ch == 2 && c == 1) || c == 3;
                    raw[c] = if linearize && !is_alpha { SRGB_LUT[b as usize] } else { b as f32 / 255.0 };
                }
            }
            BitmapData::U16(v) => {
                for c in 0..ch {
                    let f = v[o + c] as f32 / 65535.0;
                    let is_alpha = (ch == 2 && c == 1) || c == 3;
                    raw[c] = if linearize && !is_alpha { srgb_to_linear(f) } else { f };
                }
            }
            BitmapData::F32(v) => raw[..ch].copy_from_slice(&v[o..o + ch]),
        }
        match ch {
            1 => [raw[0], raw[0], raw[0], 1.0],
            2 => [raw[0], raw[0], raw[0], raw[1]],
            3 => [raw[0], raw[1], raw[2], 1.0],
            _ => raw,
        }
    }

    /// Bilinear sample at normalized coordinates (texel centers at (i+0.5)/w).
    pub fn sample(&self, u: f32, v: f32, wrap: Wrap, linearize: bool) -> [f32; 4] {
        let (w, h) = (self.width as i64, self.height as i64);
        let x = u * w as f32 - 0.5;
        let y = v * h as f32 - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let fix = |i: i64, n: i64| -> u32 {
            match wrap {
                Wrap::Repeat => i.rem_euclid(n) as u32,
                Wrap::Clamp => i.clamp(0, n - 1) as u32,
            }
        };
        let (xa, xb) = (fix(x0 as i64, w), fix(x0 as i64 + 1, w));
        let (ya, yb) = (fix(y0 as i64, h), fix(y0 as i64 + 1, h));
        let t00 = self.texel(xa, ya, linearize);
        let t10 = self.texel(xb, ya, linearize);
        let t01 = self.texel(xa, yb, linearize);
        let t11 = self.texel(xb, yb, linearize);
        let mut out = [0.0; 4];
        for c in 0..4 {
            let top = t00[c] + (t10[c] - t00[c]) * fx;
            let bot = t01[c] + (t11[c] - t01[c]) * fx;
            out[c] = top + (bot - top) * fy;
        }
        out
    }

    /// Copy with `f(component_index, value)` applied to the color components (never alpha).
    /// Values are normalized 0..1 for integer data and kept as-is for float data.
    pub fn map_color(&self, f: impl Fn(usize, f32) -> f32) -> Bitmap {
        let ch = self.channels as usize;
        let color_comps = if ch == 2 || ch == 4 { ch - 1 } else { ch };
        let mut out = self.clone();
        match &mut out.data {
            BitmapData::U8(v) => {
                for (i, b) in v.iter_mut().enumerate() {
                    let c = i % ch;
                    if c < color_comps {
                        *b = (f(c, *b as f32 / 255.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                    }
                }
            }
            BitmapData::U16(v) => {
                for (i, w) in v.iter_mut().enumerate() {
                    let c = i % ch;
                    if c < color_comps {
                        *w = (f(c, *w as f32 / 65535.0).clamp(0.0, 1.0) * 65535.0).round() as u16;
                    }
                }
            }
            BitmapData::F32(v) => {
                for (i, x) in v.iter_mut().enumerate() {
                    let c = i % ch;
                    if c < color_comps {
                        *x = f(c, *x);
                    }
                }
            }
        }
        out
    }

    /// Generate a grayscale bitmap from a function of normalized coordinates.
    pub fn from_fn_gray(width: u32, height: u32, f: impl Fn(f32, f32) -> f32 + Sync) -> Self {
        let mut data = vec![0.0f32; (width * height) as usize];
        use rayon::prelude::*;
        data.par_chunks_mut(width as usize).enumerate().for_each(|(y, row)| {
            for (x, v) in row.iter_mut().enumerate() {
                *v = f((x as f32 + 0.5) / width as f32, (y as f32 + 0.5) / height as f32);
            }
        });
        Self::new_f32(width, height, 1, data)
    }

    /// Downscaled RGBA8 preview (sRGB-encoded for display), nearest-filtered.
    pub fn preview_rgba8(&self, max_size: u32) -> (u32, u32, Vec<u8>) {
        let scale = (self.width.max(self.height) as f32 / max_size as f32).max(1.0);
        let pw = ((self.width as f32 / scale) as u32).max(1);
        let ph = ((self.height as f32 / scale) as u32).max(1);
        let mut out = Vec::with_capacity((pw * ph * 4) as usize);
        for y in 0..ph {
            for x in 0..pw {
                let sx = ((x as f32 + 0.5) * scale) as u32;
                let sy = ((y as f32 + 0.5) * scale) as u32;
                let t = self.texel(sx.min(self.width - 1), sy.min(self.height - 1), false);
                let enc = |v: f32| {
                    let v = if self.is_hdr() { subcraft_core::color::linear_to_srgb(v / (1.0 + v)) } else { v };
                    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                };
                out.extend_from_slice(&[enc(t[0]), enc(t[1]), enc(t[2]), (t[3].clamp(0.0, 1.0) * 255.0) as u8]);
            }
        }
        (pw, ph, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_replicates_and_alpha_defaults() {
        let b = Bitmap::new_u8(1, 1, 1, vec![255]);
        assert_eq!(b.texel(0, 0, false), [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn linearize_affects_color_not_alpha() {
        let b = Bitmap::new_u8(1, 1, 4, vec![128, 128, 128, 128]);
        let t = b.texel(0, 0, true);
        assert!((t[0] - srgb_to_linear(128.0 / 255.0)).abs() < 1e-6);
        assert!((t[3] - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn map_color_skips_alpha_and_keeps_depth() {
        let b = Bitmap::new_u8(1, 1, 4, vec![0, 100, 255, 7]);
        let inv = b.map_color(|_, v| 1.0 - v);
        assert_eq!(inv.data, BitmapData::U8(vec![255, 155, 0, 7]));
        let g = Bitmap { width: 1, height: 1, channels: 3, data: BitmapData::U16(vec![0, 65535, 0]) };
        let flipped = g.map_color(|c, v| if c == 1 { 1.0 - v } else { v });
        assert_eq!(flipped.data, BitmapData::U16(vec![0, 0, 0]));
    }

    #[test]
    fn bilinear_center_of_two_texels() {
        let b = Bitmap::new_f32(2, 1, 1, vec![0.0, 1.0]);
        let s = b.sample(0.5, 0.5, Wrap::Clamp, false);
        assert!((s[0] - 0.5).abs() < 1e-6);
        let s = b.sample(0.25, 0.5, Wrap::Clamp, false);
        assert!(s[0].abs() < 1e-6);
    }
}
