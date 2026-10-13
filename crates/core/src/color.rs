//! Color-space math.
//!
//! Working space is linear Rec.709/sRGB primaries. Only color channels pass through these
//! conversions; data channels (roughness, normals, masks, height) never do.
//! OpenColorIO integration will plug in behind this module (ADR-0004).

/// sRGB electro-optical transfer: encoded (0..1) -> linear.
#[inline]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Inverse sRGB transfer: linear -> encoded (0..1).
#[inline]
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb3_to_linear(c: [f32; 3]) -> [f32; 3] {
    [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2])]
}

pub fn linear3_to_srgb(c: [f32; 3]) -> [f32; 3] {
    [linear_to_srgb(c[0]), linear_to_srgb(c[1]), linear_to_srgb(c[2])]
}

/// Lookup table for fast 8-bit sRGB decode.
pub fn srgb8_to_linear_lut() -> [f32; 256] {
    let mut lut = [0.0; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        *v = srgb_to_linear(i as f32 / 255.0);
    }
    lut
}

/// RGB (0..1) -> HSV with h in 0..1.
pub fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let s = if max > 0.0 { d / max } else { 0.0 };
    let h = if d <= 1e-8 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, s, v]
}

/// HSV (h in 0..1) -> RGB.
pub fn hsv_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let [h, s, v] = c;
    let h6 = h.rem_euclid(1.0) * 6.0;
    let i = h6.floor();
    let f = h6 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_roundtrip() {
        for i in 0..=255 {
            let e = i as f32 / 255.0;
            let back = linear_to_srgb(srgb_to_linear(e));
            assert!((back - e).abs() < 1e-5, "{e} -> {back}");
        }
        assert!((srgb_to_linear(0.5) - 0.214_041).abs() < 1e-5);
    }

    #[test]
    fn hsv_roundtrip() {
        for c in [[1.0, 0.0, 0.0], [0.2, 0.7, 0.3], [0.1, 0.1, 0.9], [0.5, 0.5, 0.5]] {
            let back = hsv_to_rgb(rgb_to_hsv(c));
            for k in 0..3 {
                assert!((back[k] - c[k]).abs() < 1e-5, "{c:?} -> {back:?}");
            }
        }
    }
}
