//! Blend-mode math and W3C separable compositing.
//!
//! Values are straight (not premultiplied). For a backdrop `(Cb, ab)` and a source `(Cs, as)`:
//!
//! ```text
//! Cs' = (1 - ab)·Cs + ab·B(Cb, Cs)
//! ao  = as + ab·(1 - as)
//! Co  = (as·Cs' + (1 - as)·ab·Cb) / ao
//! ```
//!
//! With an opaque backdrop this is `lerp(Cb, B(Cb, Cs), as)`; with a transparent backdrop (the
//! start of an isolated folder) the source passes through unblended, as in W3C compositing.
//! Normal-map values are encoded 0..1; the normal-map modes decode, combine and re-encode.

use glam::Vec3;
use subcraft_core::color::{hsv_to_rgb, rgb_to_hsv};
use subcraft_doc::BlendMode;
pub use subcraft_core::normal::reoriented;

/// What a blend writes into: decides clamping and whether normal-map modes apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlendTarget {
    /// Values may leave 0..1 (Height): arithmetic modes do not clamp.
    pub signed: bool,
    /// Encoded tangent-space normals: the normal-map modes are meaningful.
    pub normal: bool,
}

impl BlendTarget {
    /// Grayscale masks.
    pub const MASK: BlendTarget = BlendTarget { signed: false, normal: false };

    pub fn for_channel(kind: subcraft_core::ChannelKind) -> Self {
        Self { signed: kind.is_signed(), normal: kind == subcraft_core::ChannelKind::Normal }
    }
}

const SIGNED_LIMIT: f32 = 1.0e4;

#[inline]
fn safe_div(a: f32, b: f32) -> f32 {
    // `b < 0.0`, not `copysign`: the GPU shader treats -0.0 as positive, and Invert on a signed
    // channel produces -0.0, so the two paths must agree on it.
    a / if b.abs() < 1e-6 { if b < 0.0 { -1e-6 } else { 1e-6 } } else { b }
}

#[inline]
fn color_burn(cb: f32, cs: f32) -> f32 {
    if cb >= 1.0 {
        1.0
    } else if cs <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
}

#[inline]
fn color_dodge(cb: f32, cs: f32) -> f32 {
    if cb <= 0.0 {
        0.0
    } else if cs >= 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

#[inline]
fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb * 2.0 * cs
    } else {
        let s = 2.0 * cs - 1.0;
        cb + s - cb * s
    }
}

#[inline]
fn soft_light(cb: f32, cs: f32) -> f32 {
    let cb = cb.max(0.0);
    if cs <= 0.5 {
        cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
    } else {
        let d = if cb <= 0.25 { ((16.0 * cb - 12.0) * cb + 4.0) * cb } else { cb.sqrt() };
        cb + (2.0 * cs - 1.0) * (d - cb)
    }
}

/// Per-component blend function `B(Cb, Cs)` for the separable modes.
#[inline]
fn separable(mode: BlendMode, cb: f32, cs: f32, signed: bool) -> f32 {
    match mode {
        BlendMode::Multiply => cb * cs,
        BlendMode::Divide => safe_div(cb, cs),
        BlendMode::InverseDivide => safe_div(cs, cb),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::LinearDodge => cb + cs,
        BlendMode::Subtract => cb - cs,
        BlendMode::InverseSubtract => cs - cb,
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        // Neutral value adds nothing: 0.5 for 0..1 channels, 0 for signed (height) channels.
        BlendMode::SignedAddition => {
            if signed {
                cb + cs
            } else {
                cb + 2.0 * cs - 1.0
            }
        }
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::LinearBurn => cb + cs - 1.0,
        BlendMode::ColorBurn => color_burn(cb, cs),
        BlendMode::ColorDodge => color_dodge(cb, cs),
        BlendMode::SoftLight => soft_light(cb, cs),
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::VividLight => {
            if cs <= 0.5 {
                color_burn(cb, 2.0 * cs)
            } else {
                color_dodge(cb, 2.0 * cs - 1.0)
            }
        }
        BlendMode::LinearLight => cb + 2.0 * cs - 1.0,
        BlendMode::PinLight => {
            if cs <= 0.5 {
                cb.min(2.0 * cs)
            } else {
                cb.max(2.0 * cs - 1.0)
            }
        }
        // Normal, Replace, Passthrough (on a non-folder) and anything non-separable.
        _ => cs,
    }
}

#[inline]
fn decode_normal(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0] * 2.0 - 1.0, v[1] * 2.0 - 1.0, v[2] * 2.0 - 1.0)
}

#[inline]
fn encode_normal(n: Vec3) -> [f32; 3] {
    let n = n.normalize_or(Vec3::Z);
    [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5]
}

/// Whiteout blend of a detail normal onto a base normal (encoded inputs and output).
pub fn whiteout(base: [f32; 3], detail: [f32; 3]) -> [f32; 3] {
    let (a, b) = (decode_normal(base), decode_normal(detail));
    encode_normal(Vec3::new(a.x + b.x, a.y + b.y, a.z * b.z))
}

/// `B(Cb, Cs)` for any mode (RGB; scalars are passed replicated as gray).
pub fn blend_fn(mode: BlendMode, cb: [f32; 3], cs: [f32; 3], target: BlendTarget) -> [f32; 3] {
    match mode {
        BlendMode::Normal | BlendMode::Replace | BlendMode::Passthrough | BlendMode::Disable => cs,
        BlendMode::Tint => {
            let (b, s) = (rgb_to_hsv(cb), rgb_to_hsv(cs));
            hsv_to_rgb([s[0], b[1], b[2]])
        }
        BlendMode::Saturation => {
            let (b, s) = (rgb_to_hsv(cb), rgb_to_hsv(cs));
            hsv_to_rgb([b[0], s[1], b[2]])
        }
        BlendMode::Color => {
            let (b, s) = (rgb_to_hsv(cb), rgb_to_hsv(cs));
            hsv_to_rgb([s[0], s[1], b[2]])
        }
        BlendMode::Value => {
            let (b, s) = (rgb_to_hsv(cb), rgb_to_hsv(cs));
            hsv_to_rgb([b[0], b[1], s[2]])
        }
        BlendMode::NormalMapCombine | BlendMode::NormalMapDetail | BlendMode::NormalMapInverseDetail if !target.normal => cs,
        BlendMode::NormalMapCombine => whiteout(cb, cs),
        BlendMode::NormalMapDetail => reoriented(cb, cs),
        BlendMode::NormalMapInverseDetail => reoriented(cs, cb),
        _ => {
            let mut out = [0.0; 3];
            for c in 0..3 {
                let v = separable(mode, cb[c], cs[c], target.signed);
                out[c] = if target.signed { v.clamp(-SIGNED_LIMIT, SIGNED_LIMIT) } else { v.clamp(0.0, 1.0) };
            }
            out
        }
    }
}

/// A passthrough folder at strength `f` (opacity x mask): a mix of the backdrop `dst` and the
/// children's result `src`. Texels are straight (not premultiplied) alpha, so the colours are
/// weighted by their alpha: a plain lerp of all four components darkened a passthrough folder
/// inside an isolated one, where the backdrop is transparent. With equal alphas (the usual opaque
/// stack) this is that plain lerp, bit for bit.
#[inline]
pub fn passthrough_mix(dst: &mut [f32; 4], src: &[f32; 4], f: f32) {
    let (wd, ws) = (dst[3] * (1.0 - f), src[3] * f);
    let alpha = wd + ws;
    if dst[3] == src[3] || alpha <= 0.0 {
        for c in 0..4 {
            dst[c] += (src[c] - dst[c]) * f;
        }
        return;
    }
    for c in 0..3 {
        dst[c] = (dst[c] * wd + src[c] * ws) / alpha;
    }
    dst[3] = alpha;
}

/// Composite source `(cs, alpha)` onto `dst = [Cb.r, Cb.g, Cb.b, ab]` in place.
#[inline]
pub fn composite(dst: &mut [f32; 4], cs: [f32; 3], alpha: f32, mode: BlendMode, target: BlendTarget) {
    if mode == BlendMode::Disable {
        return;
    }
    composite_with(dst, cs, alpha, |cb, cs| blend_fn(mode, cb, cs, target));
}

/// [`composite`] for every texel of a run, with the mode's `B(Cb, Cs)` chosen once instead of per
/// texel (same arithmetic, same bits). `alpha(i)` is the source coverage of texel `i`.
#[inline]
pub fn composite_run(dst: &mut [[f32; 4]], src: &[[f32; 4]], mode: BlendMode, target: BlendTarget, alpha: impl Fn(usize, &[f32; 4]) -> f32) {
    #[inline(always)]
    fn run(dst: &mut [[f32; 4]], src: &[[f32; 4]], alpha: impl Fn(usize, &[f32; 4]) -> f32, b: impl Fn([f32; 3], [f32; 3]) -> [f32; 3]) {
        for (i, (d, s)) in dst.iter_mut().zip(src).enumerate() {
            composite_with(d, [s[0], s[1], s[2]], alpha(i, s), &b);
        }
    }
    match mode {
        BlendMode::Disable => {}
        BlendMode::Normal | BlendMode::Replace | BlendMode::Passthrough => run(dst, src, alpha, |_, cs| cs),
        BlendMode::Multiply if !target.signed => run(dst, src, alpha, |cb, cs| std::array::from_fn(|c| (cb[c] * cs[c]).clamp(0.0, 1.0))),
        _ => run(dst, src, alpha, |cb, cs| blend_fn(mode, cb, cs, target)),
    }
}

/// A grayscale mask run: for every texel `m[i] = composite([m; 3, 1], [e[i][0]; 3], e[i][1] *
/// opacity, mode, MASK)[0]`, with the mode chosen once (same arithmetic, same bits).
pub fn composite_mask_run(m: &mut [f32], e: &[[f32; 4]], opacity: f32, mode: BlendMode) {
    #[inline(always)]
    fn run(m: &mut [f32], e: &[[f32; 4]], opacity: f32, b: impl Fn([f32; 3], [f32; 3]) -> [f32; 3]) {
        for (mv, ev) in m.iter_mut().zip(e) {
            let mut dst = [*mv, *mv, *mv, 1.0];
            composite_with(&mut dst, [ev[0]; 3], ev[1] * opacity, &b);
            *mv = dst[0];
        }
    }
    let t = BlendTarget::MASK;
    match mode {
        BlendMode::Disable => {}
        BlendMode::Normal | BlendMode::Replace | BlendMode::Passthrough => run(m, e, opacity, |_, cs| cs),
        BlendMode::Multiply => run(m, e, opacity, |cb, cs| std::array::from_fn(|c| (cb[c] * cs[c]).clamp(0.0, 1.0))),
        _ => run(m, e, opacity, |cb, cs| blend_fn(mode, cb, cs, t)),
    }
}

#[inline(always)]
fn composite_with(dst: &mut [f32; 4], cs: [f32; 3], alpha: f32, blend: impl Fn([f32; 3], [f32; 3]) -> [f32; 3]) {
    if alpha <= 0.0 {
        return;
    }
    let a = alpha.min(1.0);
    let ab = dst[3];
    let cb = [dst[0], dst[1], dst[2]];
    let b = if ab > 0.0 { blend(cb, cs) } else { cs };
    let ao = a + ab * (1.0 - a);
    for c in 0..3 {
        let mixed = (1.0 - ab) * cs[c] + ab * b[c];
        dst[c] = (a * mixed + (1.0 - a) * ab * cb[c]) / ao;
    }
    dst[3] = ao;
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLOR: BlendTarget = BlendTarget { signed: false, normal: false };

    /// Mirrors `composite.wgsl`: -0.0 divides like +0.0, so Divide after Invert on a signed
    /// channel gives the same result on the CPU and the GPU.
    #[test]
    fn safe_div_treats_negative_zero_like_the_shader() {
        assert!(safe_div(1.0, -0.0) > 0.0);
        assert!(safe_div(1.0, 0.0) > 0.0);
        assert!(safe_div(1.0, -1e-9) < 0.0);
        assert_eq!(safe_div(1.0, 2.0), 0.5);
    }

    #[test]
    fn all_modes_finite_and_in_range() {
        let samples = [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0];
        for mode in BlendMode::ALL {
            for &b in &samples {
                for &s in &samples {
                    let o = blend_fn(mode, [b, 1.0 - b, 0.3], [s, 0.2, 1.0 - s], COLOR);
                    assert!(o.iter().all(|v| v.is_finite() && (-1e-5..=1.0 + 1e-5).contains(v)), "{mode:?} {b} {s} -> {o:?}");
                    let mut dst = [b, b, b, 1.0];
                    composite(&mut dst, [s; 3], 0.5, mode, BlendTarget { signed: false, normal: true });
                    assert!(dst.iter().all(|v| v.is_finite()), "{mode:?}");
                }
            }
        }
    }

    #[test]
    fn separable_reference_values() {
        let f = |m| blend_fn(m, [0.6; 3], [0.5; 3], COLOR)[0];
        assert!((f(BlendMode::Multiply) - 0.3).abs() < 1e-6);
        assert!((f(BlendMode::Screen) - 0.8).abs() < 1e-6);
        assert!((f(BlendMode::LinearDodge) - 1.0).abs() < 1e-6, "clamped");
        assert!((f(BlendMode::Subtract) - 0.1).abs() < 1e-6);
        assert!((f(BlendMode::Darken) - 0.5).abs() < 1e-6);
        assert!((f(BlendMode::Difference) - 0.1).abs() < 1e-6);
        assert!((f(BlendMode::Overlay) - 0.6).abs() < 1e-6, "overlay with 0.5 is identity");
        assert!((f(BlendMode::SoftLight) - 0.6).abs() < 1e-6);
        assert!((f(BlendMode::SignedAddition) - 0.6).abs() < 1e-6, "0.5 is neutral");
        assert!((f(BlendMode::LinearLight) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn height_add_subtract_are_signed() {
        let h = BlendTarget { signed: true, normal: false };
        assert_eq!(blend_fn(BlendMode::LinearDodge, [0.8; 3], [0.7; 3], h)[0], 1.5);
        assert!((blend_fn(BlendMode::Subtract, [0.1; 3], [0.4; 3], h)[0] + 0.3).abs() < 1e-6);
        assert_eq!(blend_fn(BlendMode::SignedAddition, [0.2; 3], [-0.5; 3], h)[0], -0.3);
    }

    #[test]
    fn hsv_modes() {
        let red = [1.0, 0.0, 0.0];
        let dark_blue = [0.0, 0.0, 0.4];
        // Value: hue/sat of backdrop, value of source.
        let v = blend_fn(BlendMode::Value, red, dark_blue, COLOR);
        assert!((v[0] - 0.4).abs() < 1e-6 && v[1].abs() < 1e-6);
        // Color: hue/sat of source, value of backdrop.
        let c = blend_fn(BlendMode::Color, red, dark_blue, COLOR);
        assert!((c[2] - 1.0).abs() < 1e-6 && c[0].abs() < 1e-6);
        // Gray scalars: Tint/Saturation/Color keep the backdrop, Value takes the source.
        for m in [BlendMode::Tint, BlendMode::Saturation, BlendMode::Color] {
            assert!((blend_fn(m, [0.3; 3], [0.8; 3], COLOR)[0] - 0.3).abs() < 1e-6, "{m:?}");
        }
        assert!((blend_fn(BlendMode::Value, [0.3; 3], [0.8; 3], COLOR)[0] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn normal_modes_identity_with_flat() {
        let flat = [0.5, 0.5, 1.0];
        let tilted = encode_normal(Vec3::new(0.3, -0.2, 0.9));
        for f in [whiteout, reoriented] {
            let a = f(flat, tilted);
            let b = f(tilted, flat);
            for c in 0..3 {
                assert!((a[c] - tilted[c]).abs() < 1e-5, "{a:?} vs {tilted:?}");
                assert!((b[c] - tilted[c]).abs() < 1e-5, "{b:?} vs {tilted:?}");
            }
        }
        let n = BlendTarget { signed: false, normal: true };
        let other = encode_normal(Vec3::new(-0.4, 0.1, 0.8));
        assert_eq!(blend_fn(BlendMode::NormalMapInverseDetail, tilted, other, n), reoriented(other, tilted));
        // Normal-map modes on a non-normal channel behave like Normal.
        assert_eq!(blend_fn(BlendMode::NormalMapCombine, [0.1; 3], [0.7; 3], COLOR), [0.7; 3]);
    }

    #[test]
    fn composite_run_is_bit_identical_to_composite_per_texel() {
        let vals = [-0.5f32, 0.0, 1e-7, 0.1, 0.3333, 0.5, 0.75, 0.999, 1.0, 1.5, f32::NAN];
        let mut src = Vec::new();
        let mut dst0 = Vec::new();
        for (k, &a) in vals.iter().enumerate() {
            for &b in &vals {
                src.push([a, b, vals[(k + 3) % vals.len()], b]);
                dst0.push([b, a, 0.25, vals[(k + 5) % vals.len()].abs().min(1.0)]);
            }
        }
        for mode in BlendMode::ALL {
            for target in [COLOR, BlendTarget { signed: true, normal: false }, BlendTarget { signed: false, normal: true }] {
                let alpha = |i: usize, s: &[f32; 4]| s[3] * 0.8 * vals[i % vals.len()];
                let mut a = dst0.clone();
                for (i, (d, s)) in a.iter_mut().zip(&src).enumerate() {
                    composite(d, [s[0], s[1], s[2]], alpha(i, s), mode, target);
                }
                let mut b = dst0.clone();
                composite_run(&mut b, &src, mode, target, alpha);
                let bits = |v: &Vec<[f32; 4]>| v.iter().flat_map(|p| p.map(f32::to_bits)).collect::<Vec<_>>();
                assert_eq!(bits(&a), bits(&b), "{mode:?} {target:?}");
            }
        }
    }

    #[test]
    fn composite_mask_run_is_bit_identical_to_composite_per_texel() {
        let vals = [-0.5f32, 0.0, 1e-7, 0.1, 0.3333, 0.5, 0.75, 0.999, 1.0, 1.5, f32::NAN];
        let e: Vec<[f32; 4]> = (0..vals.len() * vals.len()).map(|i| [vals[i % vals.len()], vals[(i / vals.len()) % vals.len()], 0.0, 0.0]).collect();
        let m0: Vec<f32> = (0..e.len()).map(|i| vals[(i * 7 + 3) % vals.len()]).collect();
        for mode in BlendMode::ALL {
            for opacity in [0.0, 0.37, 1.0] {
                let mut a = m0.clone();
                for (mv, ev) in a.iter_mut().zip(&e) {
                    let mut dst = [*mv, *mv, *mv, 1.0];
                    composite(&mut dst, [ev[0]; 3], ev[1] * opacity, mode, BlendTarget::MASK);
                    *mv = dst[0];
                }
                let mut b = m0.clone();
                composite_mask_run(&mut b, &e, opacity, mode);
                assert_eq!(a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), b.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), "{mode:?} {opacity}");
            }
        }
    }

    #[test]
    fn w3c_compositing() {
        // Opaque backdrop: lerp(Cb, B, a).
        let mut d = [0.6, 0.6, 0.6, 1.0];
        composite(&mut d, [0.5; 3], 0.5, BlendMode::Multiply, COLOR);
        assert!((d[0] - (0.6 * 0.5 + 0.3 * 0.5)).abs() < 1e-6 && d[3] == 1.0);
        // Transparent backdrop: the source passes through unblended.
        let mut d = [0.0, 0.0, 0.0, 0.0];
        composite(&mut d, [0.5; 3], 0.8, BlendMode::Multiply, COLOR);
        assert!((d[0] - 0.5).abs() < 1e-6 && (d[3] - 0.8).abs() < 1e-6);
        // Two half-transparent normal layers: alpha union, straight color.
        let mut d = [1.0, 0.0, 0.0, 0.5];
        composite(&mut d, [0.0, 0.0, 1.0], 0.5, BlendMode::Normal, COLOR);
        assert!((d[3] - 0.75).abs() < 1e-6);
        assert!((d[0] - 1.0 / 3.0).abs() < 1e-6 && (d[2] - 2.0 / 3.0).abs() < 1e-6);
    }
}
