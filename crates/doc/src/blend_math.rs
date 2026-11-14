//! Blend-mode math (W3C separable modes, HSV modes, normal-map modes), shared by the
//! compositor and the paint tools (a Polygon Fill blends into the layer with these modes).
//! Values are straight (not premultiplied); normal-map values are encoded 0..1.

use glam::Vec3;
use subcraft_core::color::{hsv_to_rgb, rgb_to_hsv};
use crate::BlendMode;
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

#[doc(hidden)]
pub const SIGNED_LIMIT: f32 = 1.0e4;

#[inline]
#[doc(hidden)]
pub fn safe_div(a: f32, b: f32) -> f32 {
    // `b < 0.0`, not `copysign`: the GPU shader treats -0.0 as positive, and Invert on a signed
    // channel produces -0.0, so the two paths must agree on it.
    a / if b.abs() < 1e-6 { if b < 0.0 { -1e-6 } else { 1e-6 } } else { b }
}

#[inline]
#[doc(hidden)]
pub fn color_burn(cb: f32, cs: f32) -> f32 {
    if cb >= 1.0 {
        1.0
    } else if cs <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - cb) / cs).min(1.0)
    }
}

#[inline]
#[doc(hidden)]
pub fn color_dodge(cb: f32, cs: f32) -> f32 {
    if cb <= 0.0 {
        0.0
    } else if cs >= 1.0 {
        1.0
    } else {
        (cb / (1.0 - cs)).min(1.0)
    }
}

#[inline]
#[doc(hidden)]
pub fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        cb * 2.0 * cs
    } else {
        let s = 2.0 * cs - 1.0;
        cb + s - cb * s
    }
}

#[inline]
#[doc(hidden)]
pub fn soft_light(cb: f32, cs: f32) -> f32 {
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
#[doc(hidden)]
pub fn separable(mode: BlendMode, cb: f32, cs: f32, signed: bool) -> f32 {
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
#[doc(hidden)]
pub fn decode_normal(v: [f32; 3]) -> Vec3 {
    Vec3::new(v[0] * 2.0 - 1.0, v[1] * 2.0 - 1.0, v[2] * 2.0 - 1.0)
}

#[inline]
#[doc(hidden)]
pub fn encode_normal(n: Vec3) -> [f32; 3] {
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
