//! The lens and color stage at the end of the post-effect stack (lateral aberration, vignette,
//! sharpen, film grain, color correction, gamma) and the tone-mapping functions, as plain math.
//!
//! The viewport runs the same formulas on the GPU (`post.wgsl` `fs_grade`, `tonemap.wgsl`); this
//! module is the reference they are tested against, pixel for pixel.

use subcraft_doc::display::ToneMap;

const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

fn luma(c: [f32; 3]) -> f32 {
    c[0] * LUMA[0] + c[1] * LUMA[1] + c[2] * LUMA[2]
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Value of the shaders' tone-map switch.
pub fn tone_map_id(t: ToneMap) -> f32 {
    match t.resolved() {
        ToneMap::Aces => 0.0,
        ToneMap::Filmic => 1.0,
        ToneMap::Reinhard => 2.0,
        ToneMap::Linear => 3.0,
        ToneMap::LinearSat => 5.0,
        ToneMap::Sensitometric | ToneMap::Auto => 6.0,
        ToneMap::ReinhardLum => 7.0,
        ToneMap::Log => 8.0,
        ToneMap::LogLum => 9.0,
    }
}

/// Map linear HDR light `c` to the display range 0..1 with function `t`. `factor` is the
/// Mapping Factor: it scales the input of the Reinhard and Log curves and is ignored by the rest.
pub fn tone_map(t: ToneMap, c: [f32; 3], factor: f32) -> [f32; 3] {
    let f = factor.max(1e-3);
    let sat = |v: f32| v.clamp(0.0, 1.0);
    let scaled = |m: f32, l: f32| c.map(|v| sat(v * (m / l.max(1e-6))));
    match t.resolved() {
        ToneMap::Aces => c.map(|x| sat((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14))),
        ToneMap::Filmic => {
            let hable = |x: f32| ((x * (0.15 * x + 0.10 * 0.50) + 0.20 * 0.02) / (x * (0.15 * x + 0.50) + 0.20 * 0.30)) - 0.02 / 0.30;
            let white = hable(11.2);
            c.map(|x| sat(hable(x * 2.0) / white))
        }
        ToneMap::Reinhard => c.map(|x| sat(x * f / (x * f + 1.0))),
        ToneMap::Linear | ToneMap::LinearSat => c.map(sat),
        ToneMap::Sensitometric | ToneMap::Auto => c.map(|x| sat(1.0 - (-x.max(0.0)).exp())),
        ToneMap::ReinhardLum => {
            let l = luma(c);
            scaled(l * f / (l * f + 1.0), l)
        }
        ToneMap::Log => c.map(|x| sat((1.0 + x.max(0.0) * f).ln() / (1.0 + f).ln())),
        ToneMap::LogLum => {
            let l = luma(c);
            scaled((1.0 + l.max(0.0) * f).ln() / (1.0 + f).ln(), l)
        }
    }
}

/// Color of a black body at `kelvin` (1000..40000), each channel 0..1 (Tanner Helland's fit).
fn black_body(kelvin: f32) -> [f32; 3] {
    let t = kelvin.clamp(1000.0, 40000.0) / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698_73 * (t - 60.0).powf(-0.133_204_76) };
    let g = if t <= 66.0 { 99.470_8 * t.ln() - 161.119_57 } else { 288.122_17 * (t - 60.0).powf(-0.075_514_85) };
    let b = if t >= 66.0 { 255.0 } else if t <= 19.0 { 0.0 } else { 138.517_73 * (t - 10.0).ln() - 305.044_8 };
    [r, g, b].map(|v| v.clamp(0.0, 255.0) / 255.0)
}

/// Channel multipliers of a White Balance Temperature: light of that temperature becomes
/// neutral, so a low setting cools the picture and a high one warms it. 6500 K is exactly 1.
pub fn white_balance(kelvin: f32) -> [f32; 3] {
    if (kelvin - 6500.0).abs() < 0.5 {
        return [1.0; 3];
    }
    let (neutral, light) = (black_body(6500.0), black_body(kelvin));
    let m = [0, 1, 2].map(|i| neutral[i] / light[i].max(1e-3));
    // Keep the brightness: normalise so the luminance of white stays 1.
    let l = luma(m).max(1e-3);
    m.map(|v| v / l)
}

/// Settings of the lens and color stage. The default changes nothing ([`Grade::active`] is false).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    /// Lateral aberration strength (0 = off): at 1 the red and blue images are shifted apart
    /// by 4% of the distance from the centre.
    pub aberration: f32,
    /// Distance from the centre, in half diagonals, where the fringes reach half strength.
    pub aberration_radius: f32,
    /// Half width of the transition around the radius.
    pub aberration_softness: f32,
    /// Horizontal stretch of the unaffected zone (1 = circular).
    pub aberration_ratio: f32,
    /// Centre of the effect, 0..1 across the view.
    pub aberration_center: [f32; 2],
    /// Blur along the shift instead of splitting red and blue.
    pub aberration_achromatic: bool,
    /// Darkening of the corners, 0 (off) .. 1 (black corners).
    pub vignette: f32,
    /// Unsharp-mask strength (0 = off) and the distance, in pixels, between the compared samples.
    pub sharpen: f32,
    pub sharpen_diameter: f32,
    /// Film grain strength (0 = off), grain size in pixels, one noise per channel or shared.
    pub grain: f32,
    pub grain_radius: f32,
    pub grain_colored: bool,
    /// Color correction on: saturation, contrast, brightness, bias, sepia and white balance apply.
    pub color_correction: bool,
    pub saturation: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub bias: f32,
    pub sepia: f32,
    /// White balance temperature in kelvin (6500 = neutral).
    pub temperature: f32,
    /// Display gamma after everything else (1 = none; 2 brightens the mid tones).
    pub gamma: f32,
}

impl Default for Grade {
    fn default() -> Self {
        Self {
            aberration: 0.0,
            aberration_radius: 0.5,
            aberration_softness: 0.3,
            aberration_ratio: 1.0,
            aberration_center: [0.5, 0.5],
            aberration_achromatic: false,
            vignette: 0.0,
            sharpen: 0.0,
            sharpen_diameter: 2.0,
            grain: 0.0,
            grain_radius: 1.0,
            grain_colored: false,
            color_correction: false,
            saturation: 1.0,
            contrast: 1.0,
            brightness: 1.0,
            bias: 0.0,
            sepia: 0.0,
            temperature: 6500.0,
            gamma: 1.0,
        }
    }
}

/// Largest shift of the red / blue image as a share of the distance from the centre.
pub const ABERRATION_SHIFT: f32 = 0.04;

impl Grade {
    /// Whether the stage changes the picture at all.
    pub fn active(&self) -> bool {
        self.aberration > 0.0 || self.vignette > 0.0 || self.sharpen > 0.0 || self.grain > 0.0 || self.color_correction || (self.gamma - 1.0).abs() > 1e-6
    }

    /// Offset (in view units, 0..1) of the red image at `uv`; blue moves the opposite way.
    pub fn aberration_shift(&self, uv: [f32; 2]) -> [f32; 2] {
        if self.aberration <= 0.0 {
            return [0.0; 2];
        }
        let d = [uv[0] - self.aberration_center[0], uv[1] - self.aberration_center[1]];
        let e = [d[0] / self.aberration_ratio.max(1e-3) * std::f32::consts::SQRT_2, d[1] * std::f32::consts::SQRT_2];
        let dist = (e[0] * e[0] + e[1] * e[1]).sqrt();
        let soft = self.aberration_softness.max(1e-4);
        let k = self.aberration * ABERRATION_SHIFT * smoothstep(self.aberration_radius - soft, self.aberration_radius + soft, dist);
        [d[0] * k, d[1] * k]
    }

    /// Brightness multiplier of the vignette at `uv` (1 in the middle of the view).
    pub fn vignette_factor(&self, uv: [f32; 2]) -> f32 {
        let r = ((uv[0] - 0.5).powi(2) + (uv[1] - 0.5).powi(2)).sqrt() * std::f32::consts::SQRT_2;
        1.0 - self.vignette.clamp(0.0, 1.0) * smoothstep(0.4, 1.0, r)
    }

    /// Color correction of one display-linear color: white balance, contrast around mid gray,
    /// brightness and bias, saturation, sepia. Identity when color correction is off.
    pub fn correct(&self, c: [f32; 3]) -> [f32; 3] {
        if !self.color_correction {
            return c;
        }
        let wb = white_balance(self.temperature);
        let mut c = [0, 1, 2].map(|i| ((c[i] * wb[i] - 0.5) * self.contrast + 0.5) * self.brightness + self.bias);
        let l = luma(c);
        c = c.map(|v| l + (v - l) * self.saturation);
        let sepia = [
            0.393 * c[0] + 0.769 * c[1] + 0.189 * c[2],
            0.349 * c[0] + 0.686 * c[1] + 0.168 * c[2],
            0.272 * c[0] + 0.534 * c[1] + 0.131 * c[2],
        ];
        let s = self.sepia.clamp(0.0, 1.0);
        [0, 1, 2].map(|i| (c[i] + (sepia[i] - c[i]) * s).max(0.0))
    }

    /// The final gamma: `c^(1/gamma)`.
    pub fn apply_gamma(&self, c: [f32; 3]) -> [f32; 3] {
        let g = 1.0 / self.gamma.clamp(0.1, 10.0);
        c.map(|v| v.max(0.0).powf(g))
    }

    /// The whole stage on an sRGB RGBA8 image of `size` (what the viewport pass does), without the
    /// film grain (its noise is not reproducible across devices). Used as the numeric reference.
    pub fn apply(&self, image: &[u8], size: [u32; 2]) -> Vec<u8> {
        let (w, h) = (size[0] as usize, size[1] as usize);
        let linear: Vec<[f32; 3]> = image.chunks_exact(4).map(|p| [srgb_to_linear(p[0]), srgb_to_linear(p[1]), srgb_to_linear(p[2])]).collect();
        // Bilinear, clamp-to-edge lookup at view coordinates (the pass's sampler).
        let sample = |uv: [f32; 2]| -> [f32; 3] {
            let x = (uv[0] * w as f32 - 0.5).clamp(0.0, w as f32 - 1.0);
            let y = (uv[1] * h as f32 - 0.5).clamp(0.0, h as f32 - 1.0);
            let (x0, y0) = (x.floor() as usize, y.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            let (fx, fy) = (x - x0 as f32, y - y0 as f32);
            [0, 1, 2].map(|c| {
                let top = linear[y0 * w + x0][c] * (1.0 - fx) + linear[y0 * w + x1][c] * fx;
                let bottom = linear[y1 * w + x0][c] * (1.0 - fx) + linear[y1 * w + x1][c] * fx;
                top * (1.0 - fy) + bottom * fy
            })
        };
        let fetch = |uv: [f32; 2]| -> [f32; 3] {
            let s = self.aberration_shift(uv);
            let c = if self.aberration > 0.0 {
                let (plus, mid, minus) = (sample([uv[0] + s[0], uv[1] + s[1]]), sample(uv), sample([uv[0] - s[0], uv[1] - s[1]]));
                if self.aberration_achromatic { [0, 1, 2].map(|i| (plus[i] + mid[i] + minus[i]) / 3.0) } else { [plus[0], mid[1], minus[2]] }
            } else {
                sample(uv)
            };
            let v = self.vignette_factor(uv);
            c.map(|x| x * v)
        };
        let mut out = Vec::with_capacity(image.len());
        for y in 0..h {
            for x in 0..w {
                let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
                let mut c = fetch(uv);
                if self.sharpen > 0.0 {
                    let o = [0.5 * self.sharpen_diameter / w as f32, 0.5 * self.sharpen_diameter / h as f32];
                    let taps = [[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]].map(|k: [f32; 2]| fetch([uv[0] + k[0] * o[0], uv[1] + k[1] * o[1]]));
                    c = [0, 1, 2].map(|i| (c[i] + self.sharpen * (c[i] - 0.25 * (taps[0][i] + taps[1][i] + taps[2][i] + taps[3][i]))).max(0.0));
                }
                let c = self.apply_gamma(self.correct(c));
                out.extend([linear_to_srgb(c[0]), linear_to_srgb(c[1]), linear_to_srgb(c[2]), image[(y * w + x) * 4 + 3]]);
            }
        }
        out
    }
}

pub fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(v: f32) -> u8 {
    let c = v.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5)
    }

    #[test]
    fn tone_functions_match_their_definitions() {
        let c = [0.5, 2.0, 0.1];
        assert!(close(tone_map(ToneMap::Linear, c, 1.0), [0.5, 1.0, 0.1]));
        assert_eq!(tone_map(ToneMap::LinearSat, c, 1.0), tone_map(ToneMap::Linear, c, 7.0), "no mapping factor on the linear functions");
        assert!(close(tone_map(ToneMap::Reinhard, c, 1.0), [0.5 / 1.5, 2.0 / 3.0, 0.1 / 1.1]));
        assert!(close(tone_map(ToneMap::Reinhard, c, 2.0), [0.5, 0.8, 0.2 / 1.2]), "the factor scales the input");
        assert!(close(tone_map(ToneMap::Sensitometric, c, 1.0), [1.0 - (-0.5f32).exp(), 1.0 - (-2.0f32).exp(), 1.0 - (-0.1f32).exp()]));
        assert_eq!(tone_map(ToneMap::Auto, c, 1.0), tone_map(ToneMap::Sensitometric, c, 3.0), "Auto picks Sensitometric, which ignores the factor");
        assert!(close(tone_map(ToneMap::Log, [1.0, 0.0, 3.0], 1.0), [1.0, 0.0, 1.0]), "log2(1 + x), clamped");
        assert!(close(tone_map(ToneMap::Log, [0.5; 3], 3.0), [(2.5f32).ln() / 4f32.ln(); 3]));
        // The luminance variants keep the channel ratios and bring light above 1 back into range.
        // (Log lifts dim values: log2(1 + l) lies above l between 0 and 1, so the compression
        // check uses a color brighter than 1.)
        for t in [ToneMap::ReinhardLum, ToneMap::LogLum] {
            let out = tone_map(t, [0.2, 0.4, 0.1], 1.0);
            assert!((out[1] / out[0] - 2.0).abs() < 1e-4 && (out[0] / out[2] - 2.0).abs() < 1e-4, "{t:?} keeps the hue: {out:?}");
            let bright = [0.8, 1.6, 0.4];
            assert!(luma(bright) > 1.0 && luma(tone_map(t, bright, 1.0)) < luma(bright), "{t:?} compresses");
        }
        let l = luma([0.2, 0.4, 0.1]);
        assert!((luma(tone_map(ToneMap::ReinhardLum, [0.2, 0.4, 0.1], 1.0)) - l / (1.0 + l)).abs() < 1e-5);
        // Every function is monotonic on gray and never leaves 0..1.
        for t in ToneMap::ALL {
            let mut last = -1.0;
            for i in 0..=80 {
                let v = tone_map(t, [i as f32 * 0.1; 3], 1.0)[0];
                assert!((0.0..=1.0).contains(&v) && v >= last - 1e-6, "{t:?} at {}: {v} after {last}", i as f32 * 0.1);
                last = v;
            }
        }
        // Each function has its own shader id, except the two aliases.
        let ids: std::collections::HashSet<u32> = ToneMap::ALL.iter().map(|t| tone_map_id(*t) as u32).collect();
        assert_eq!(ids.len(), ToneMap::ALL.len() - 1);
        assert_eq!(tone_map_id(ToneMap::Auto), tone_map_id(ToneMap::Sensitometric));
    }

    #[test]
    fn white_balance_is_neutral_at_6500_and_shifts_the_right_way() {
        assert_eq!(white_balance(6500.0), [1.0; 3]);
        let cool = white_balance(3000.0);
        assert!(cool[2] > cool[1] && cool[1] > cool[0], "a low temperature setting cools the picture: {cool:?}");
        let warm = white_balance(12000.0);
        assert!(warm[0] > warm[1] && warm[1] > warm[2], "a high temperature setting warms it: {warm:?}");
        for k in [2000.0, 3000.0, 5000.0, 9000.0, 20000.0] {
            assert!((luma(white_balance(k)) - 1.0).abs() < 1e-4, "white keeps its luminance at {k} K");
        }
    }

    #[test]
    fn color_correction_terms_do_what_their_names_say() {
        let on = Grade { color_correction: true, ..Default::default() };
        let c = [0.2, 0.5, 0.8];
        assert!(!Grade::default().active() && close(Grade::default().correct(c), c));
        assert!(close(on.correct(c), c), "neutral settings change nothing");
        let gray = Grade { saturation: 0.0, ..on }.correct(c);
        assert!(close(gray, [luma(c); 3]), "saturation 0 is grayscale");
        assert!(close(Grade { contrast: 2.0, ..on }.correct(c), [0.0, 0.5, 1.1]), "contrast pivots on mid gray (and never goes below black)");
        assert!(close(Grade { brightness: 0.5, bias: 0.1, ..on }.correct(c), [0.2, 0.35, 0.5]), "brightness multiplies, bias adds");
        let sepia = Grade { sepia: 1.0, ..on }.correct([0.5; 3]);
        assert!(sepia[0] > sepia[1] && sepia[1] > sepia[2], "sepia is a warm brown: {sepia:?}");
        assert!(close(Grade { gamma: 2.0, ..Default::default() }.apply_gamma([0.25, 1.0, 0.0]), [0.5, 1.0, 0.0]));
    }

    #[test]
    fn vignette_and_aberration_grow_from_the_centre() {
        let g = Grade { vignette: 0.8, aberration: 1.0, aberration_radius: 0.5, aberration_softness: 0.1, ..Default::default() };
        assert_eq!(g.vignette_factor([0.5, 0.5]), 1.0);
        assert!((g.vignette_factor([0.0, 0.0]) - 0.2).abs() < 1e-5, "corners lose the full amount");
        assert!(g.vignette_factor([0.5, 0.1]) > g.vignette_factor([0.1, 0.1]));
        assert_eq!(g.aberration_shift([0.5, 0.5]), [0.0, 0.0]);
        assert_eq!(g.aberration_shift([0.6, 0.5]), [0.0, 0.0], "inside the radius: no fringe");
        let corner = g.aberration_shift([1.0, 1.0]);
        assert!((corner[0] - 0.5 * ABERRATION_SHIFT).abs() < 1e-6 && corner[0] == corner[1], "full strength at the corner: {corner:?}");
        // A wide ratio stretches the clean zone horizontally.
        let wide = Grade { aberration_ratio: 2.0, ..g };
        assert_eq!(wide.aberration_shift([0.95, 0.5]), [0.0, 0.0]);
        assert!(g.aberration_shift([0.95, 0.5])[0] > 0.0);
    }

    #[test]
    fn the_reference_pass_is_the_identity_when_nothing_is_on() {
        let image: Vec<u8> = (0..4 * 4 * 4).map(|i| (i * 37 % 256) as u8).collect();
        assert_eq!(Grade::default().apply(&image, [4, 4]), image);
        // Sharpening a flat picture changes nothing; a vignette darkens only away from the centre.
        let flat: Vec<u8> = [120u8, 90, 200, 255].repeat(64);
        assert_eq!(Grade { sharpen: 2.0, ..Default::default() }.apply(&flat, [8, 8]), flat);
        let v = Grade { vignette: 1.0, ..Default::default() }.apply(&flat, [8, 8]);
        assert!(v[0] < 60 && v[(3 * 8 + 3) * 4] == 120, "corner {} centre {}", v[0], v[(3 * 8 + 3) * 4]);
    }
}
