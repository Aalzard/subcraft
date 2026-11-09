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

/// Image reconstruction; independent of color-space decoding and address wrapping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureFilter {
    #[default]
    Bilinear,
    Nearest,
}

impl TextureFilter {
    pub fn label(self) -> &'static str {
        match self { Self::Bilinear => "Bilinear", Self::Nearest => "Nearest" }
    }
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
/// `b as f32 / 255.0` for every byte (a table lookup instead of a division per channel).
static UNORM8_LUT: LazyLock<[f32; 256]> = LazyLock::new(|| std::array::from_fn(|b| b as f32 / 255.0));

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
                    raw[c] = if linearize && !is_alpha { SRGB_LUT[b as usize] } else { UNORM8_LUT[b as usize] };
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
        let x0 = subcraft_core::fastmath::floor(x);
        let y0 = subcraft_core::fastmath::floor(y);
        let fx = x - x0;
        let fy = y - y0;
        // `(fix(i), fix(i + 1))` with `fix` = rem_euclid (Repeat) or clamp to the image: one
        // division (none for power-of-two sizes) instead of two.
        let pair = |i: i64, n: i64| -> (u32, u32) {
            match wrap {
                Wrap::Repeat => {
                    let a = if n & (n - 1) == 0 { i & (n - 1) } else { i.rem_euclid(n) };
                    (a as u32, if a + 1 == n { 0 } else { a as u32 + 1 })
                }
                Wrap::Clamp => (i.clamp(0, n - 1) as u32, (i + 1).clamp(0, n - 1) as u32),
            }
        };
        let (xa, xb) = pair(x0 as i64, w);
        let (ya, yb) = pair(y0 as i64, h);
        if let BitmapData::U8(v) = &self.data {
            // 8-bit images (almost every texture): the four taps decoded by a per-channel-count
            // copy of `texel` (same conversions, so the same values) without its per-call checks.
            let unorm = &*UNORM8_LUT;
            let color = if linearize { &*SRGB_LUT } else { unorm };
            let taps = match self.channels {
                1 => u8_taps::<1>(v, self.width, [xa, xb, ya, yb], color, unorm),
                2 => u8_taps::<2>(v, self.width, [xa, xb, ya, yb], color, unorm),
                3 => u8_taps::<3>(v, self.width, [xa, xb, ya, yb], color, unorm),
                _ => u8_taps::<4>(v, self.width, [xa, xb, ya, yb], color, unorm),
            };
            return bilerp(taps, fx, fy);
        }
        let t00 = self.texel(xa, ya, linearize);
        let t10 = self.texel(xb, ya, linearize);
        let t01 = self.texel(xa, yb, linearize);
        let t11 = self.texel(xb, yb, linearize);
        bilerp([t00, t10, t01, t11], fx, fy)
    }

    pub fn sample_filtered(&self, u: f32, v: f32, wrap: Wrap, linearize: bool, filter: TextureFilter) -> [f32; 4] {
        if filter == TextureFilter::Bilinear { return self.sample(u, v, wrap, linearize); }
        let index = |v: f32, size: u32| {
            let i = (v * size as f32).floor() as i64;
            match wrap { Wrap::Repeat => i.rem_euclid(size as i64) as u32, Wrap::Clamp => i.clamp(0, size as i64 - 1) as u32 }
        };
        self.texel(index(u, self.width), index(v, self.height), linearize)
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

#[inline(always)]
fn bilerp([t00, t10, t01, t11]: [[f32; 4]; 4], fx: f32, fy: f32) -> [f32; 4] {
    let mut out = [0.0; 4];
    for c in 0..4 {
        let top = t00[c] + (t10[c] - t00[c]) * fx;
        let bot = t01[c] + (t11[c] - t01[c]) * fx;
        out[c] = top + (bot - top) * fy;
    }
    out
}

/// What [`Bitmap::texel`] returns for an 8-bit image with `CH` channels at the four taps
/// `(xa, ya), (xb, ya), (xa, yb), (xb, yb)`.
#[inline(always)]
/// `color` decodes color components (sRGB or plain), `unorm` is `b / 255` for alpha.
fn u8_taps<const CH: usize>(v: &[u8], width: u32, [xa, xb, ya, yb]: [u32; 4], color: &[f32; 256], unorm: &[f32; 256]) -> [[f32; 4]; 4] {
    let conv = |c: usize, b: u8| -> f32 {
        let is_alpha = (CH == 2 && c == 1) || c == 3;
        if is_alpha { unorm[b as usize] } else { color[b as usize] }
    };
    let at = |x: u32, y: u32| -> [f32; 4] {
        let o = (y as usize * width as usize + x as usize) * CH;
        let px = &v[o..o + CH];
        match CH {
            1 => {
                let r = conv(0, px[0]);
                [r, r, r, 1.0]
            }
            2 => {
                let r = conv(0, px[0]);
                [r, r, r, conv(1, px[1])]
            }
            3 => [conv(0, px[0]), conv(1, px[1]), conv(2, px[2]), 1.0],
            _ => [conv(0, px[0]), conv(1, px[1]), conv(2, px[2]), conv(3, px[3])],
        }
    };
    [at(xa, ya), at(xb, ya), at(xa, yb), at(xb, yb)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sample` before the fast floor and paired wrap.
    fn sample_reference(b: &Bitmap, u: f32, v: f32, wrap: Wrap, linearize: bool) -> [f32; 4] {
        let (w, h) = (b.width as i64, b.height as i64);
        let x = u * w as f32 - 0.5;
        let y = v * h as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let fix = |i: i64, n: i64| -> u32 {
            match wrap {
                Wrap::Repeat => i.rem_euclid(n) as u32,
                Wrap::Clamp => i.clamp(0, n - 1) as u32,
            }
        };
        let (xa, xb) = (fix(x0 as i64, w), fix(x0 as i64 + 1, w));
        let (ya, yb) = (fix(y0 as i64, h), fix(y0 as i64 + 1, h));
        let (t00, t10, t01, t11) = (b.texel(xa, ya, linearize), b.texel(xb, ya, linearize), b.texel(xa, yb, linearize), b.texel(xb, yb, linearize));
        let mut out = [0.0; 4];
        for c in 0..4 {
            let top = t00[c] + (t10[c] - t00[c]) * fx;
            let bot = t01[c] + (t11[c] - t01[c]) * fx;
            out[c] = top + (bot - top) * fy;
        }
        out
    }

    #[test]
    fn unorm_table_matches_division() {
        for b in 0..=255u8 {
            assert_eq!(UNORM8_LUT[b as usize].to_bits(), (b as f32 / 255.0).to_bits());
        }
    }

    #[test]
    fn sample_is_bit_identical_to_the_reference() {
        for (w, h, ch) in [(1, 1, 4), (2, 2, 4), (8, 8, 4), (7, 5, 4), (64, 48, 4), (3, 16, 4), (8, 8, 1), (7, 5, 2), (6, 9, 3), (16, 16, 3)] {
            let data: Vec<u8> = (0..w * h * ch).map(|i| (i * 37 % 251) as u8).collect();
            let b = Bitmap::new_u8(w, h, ch as u8, data);
            for k in 0..4000 {
                let u = k as f32 * 0.01237 - 13.0;
                let v = (k % 313) as f32 * -0.0291 + 4.5;
                for wrap in [Wrap::Repeat, Wrap::Clamp] {
                    for lin in [false, true] {
                        let a = b.sample(u, v, wrap, lin).map(f32::to_bits);
                        assert_eq!(a, sample_reference(&b, u, v, wrap, lin).map(f32::to_bits), "{w}x{h} {u} {v} {wrap:?}");
                    }
                }
            }
        }
    }

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

    #[test]
    fn nearest_preserves_texels_wraps_and_decodes_color_without_alpha_conversion() {
        let b = Bitmap::new_u8(2, 1, 4, vec![128, 0, 0, 64, 0, 255, 0, 255]);
        let sample = |u, wrap| b.sample_filtered(u, 0.5, wrap, true, TextureFilter::Nearest);
        assert_eq!(sample(0.49, Wrap::Clamp), b.texel(0, 0, true));
        assert_eq!(sample(0.50, Wrap::Clamp), b.texel(1, 0, true));
        assert_eq!(sample(-0.01, Wrap::Repeat), b.texel(1, 0, true));
        assert_eq!(sample(1., Wrap::Repeat), b.texel(0, 0, true));
        assert_eq!(sample(-1., Wrap::Clamp), b.texel(0, 0, true));
        assert_eq!(sample(1., Wrap::Clamp), b.texel(1, 0, true));
        assert!((sample(0.1, Wrap::Clamp)[3] - 64. / 255.).abs() < 1e-6);
        assert_eq!(b.sample_filtered(0.4, 0.5, Wrap::Repeat, true, TextureFilter::Bilinear), b.sample(0.4, 0.5, Wrap::Repeat, true));
        let mut image = crate::TiledImage::new(2, 1, 1, [0.; 4]);
        image.set(1, 0, [1.; 4]);
        assert_eq!(image.resampled_filtered(4, 1, TextureFilter::Nearest).to_dense(), vec![0., 0., 1., 1.]);
        assert_eq!(image.resampled(4, 1).to_dense(), vec![0., 0.25, 0.75, 1.]);
    }
}
