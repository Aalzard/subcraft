//! Deterministic point filters. Read only the previous effect stage at the same texel (halo 0).
use subcraft_core::{ChannelKind, color::{linear3_to_srgb, srgb3_to_linear}};
use subcraft_doc::Filter;
pub mod contour;
pub mod median;
pub mod warp;
pub mod pixelwise;

pub fn apply(filter: Filter, input: [f32; 3], channel: ChannelKind) -> [f32; 3] {
    if !filter.valid() || !filter.supports_channel(channel) { return input; }
    match filter {
        Filter::Contrast { gain: 1.0, .. } => input,
        Filter::Contrast { gain, pivot } => input.map(|v| {
            let v = pivot + (v - pivot) * gain;
            if channel.is_signed() { v } else { v.clamp(0.0, 1.0) }
        }),
        Filter::BlurWrap { .. } | Filter::Transform { .. } | Filter::Mirror { .. } => input, // Staged by the compositor too.
        Filter::GradientDynamic { .. } => input, // Reads an image: evaluated by the compositor.
        Filter::Smoothstep { min, max } => input.map(|v| smoothstep(v, min, max)),
        Filter::ThresholdCompare { cutoff, mode } => {
            let value = if channel.is_color() { luminance(input) } else { input[0] };
            [if mode.test(value, cutoff) { 1.0 } else { 0.0 }; 3]
        }
        Filter::HistogramShiftWrap { shift } => input.map(|v| shift_wrapped(v, shift, channel)),
        Filter::HslPerceptive { hue: 0.0, saturation: 1.0, lightness: 0.0 } => input,
        Filter::HslPerceptive { hue, saturation, lightness } => hsl_perceptive(input, hue, saturation, lightness),
        Filter::ContrastLuminosity { contrast: 0.0, luminosity: 0.0 } => input,
        Filter::ContrastLuminosity { contrast, luminosity } => input.map(|v| contrast_luminosity(v, contrast, luminosity, channel)),
        Filter::ColorCorrect { shadows, midtones, highlights } => color_correct(input, shadows, midtones, highlights),
        Filter::Blur { .. } | Filter::Sharpen { .. } | Filter::DirectionalBlur { .. } | Filter::Highpass { .. } | Filter::Bevel {..} | Filter::MaskOutline {..} | Filter::Median { .. } | Filter::Warp { .. }
            | Filter::EdgeDetect { .. } | Filter::Glow { .. } | Filter::DirectionalWarp { .. } | Filter::Mosaic { .. } => input, // Neighborhood evaluation is staged by compositor.
        Filter::Grayscale => [luminance(input); 3],
        Filter::Invert => input.map(|v| if channel.is_signed() { -v } else { 1.0 - v }),
        Filter::Posterize { levels } => input.map(|v| posterize(v, levels, channel)),
        Filter::HistogramShift { shift } => input.map(|v| {
            let v = v + shift;
            if channel.is_signed() { v.clamp(-1.0, 1.0) } else { v.clamp(0.0, 1.0) }
        }),
        Filter::ColorBalance { shadows, midtones, highlights } => color_balance(input, shadows, midtones, highlights),
        Filter::ReplaceColor { from, to, tolerance, softness } => replace_color(input, from, to, tolerance, softness),
        Filter::NormalInvert { flip_x, flip_y, flip_z } => {
            let mut out = input;
            for (i, flip) in [flip_x, flip_y, flip_z].into_iter().enumerate() { if flip { out[i] = 1.0 - out[i]; } }
            out
        }
        Filter::GradientMap { gradient, to_gray } => gradient_map(&gradient, to_gray, input, channel),
        Filter::HistogramScan { position, contrast, invert } => input.map(|v| histogram_scan(v, position, contrast, invert)),
        Filter::HistogramRange { position, range, softness, invert } => input.map(|v| histogram_range(v, position, range, softness, invert)),
        Filter::Clamp { low, high } => input.map(|v| v.clamp(low, high)),
        Filter::Threshold { cutoff } => {
            let value = if channel.is_color() { input[0] * 0.2126 + input[1] * 0.7152 + input[2] * 0.0722 } else { input[0] };
            [if value >= cutoff { 1.0 } else { 0.0 }; 3]
        }
        Filter::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 } => input,
        Filter::Hsl { hue, saturation, lightness } => {
            // Perceptual HSL is evaluated in encoded sRGB, then returned to linear storage.
            // Data channels and masks never enter this conversion (validated by app/project).
            let rgb = linear3_to_srgb(input).map(|v| v.clamp(0.0, 1.0));
            let max = rgb[0].max(rgb[1]).max(rgb[2]); let min = rgb[0].min(rgb[1]).min(rgb[2]);
            let delta = max - min; let l = (max + min) * 0.5;
            let (h, s) = if delta <= 1e-8 { (0.0, 0.0) } else {
                let h = if max == rgb[0] { ((rgb[1] - rgb[2]) / delta).rem_euclid(6.0) }
                    else if max == rgb[1] { (rgb[2] - rgb[0]) / delta + 2.0 } else { (rgb[0] - rgb[1]) / delta + 4.0 };
                (h / 6.0, delta / (1.0 - (2.0 * l - 1.0).abs()).max(1e-8))
            };
            let h = (h + hue / 360.0).rem_euclid(1.0);
            let s = (s * saturation).clamp(0.0, 1.0); let l = (l + lightness).clamp(0.0, 1.0);
            let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
            let sector = h * 6.0; let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
            let rgb = match sector as u32 { 0 => [chroma, x, 0.0], 1 => [x, chroma, 0.0], 2 => [0.0, chroma, x],
                3 => [0.0, x, chroma], 4 => [x, 0.0, chroma], _ => [chroma, 0.0, x] };
            srgb3_to_linear(rgb.map(|v| v + l - chroma * 0.5))
        }
    }
}

/// Hermite smoothstep between `min` and `max` (inverted when `min > max`, a step when equal).
pub fn smoothstep(v: f32, min: f32, max: f32) -> f32 {
    if min == max { return if v >= min { 1.0 } else { 0.0 }; }
    let t = ((v - min) / (max - min)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `v + shift` wrapped around the channel range (0..1, or -1..1 for signed Height): what leaves
/// one end re-enters at the other. A zero shift is the identity and the range's top stays the top.
pub fn shift_wrapped(v: f32, shift: f32, channel: ChannelKind) -> f32 {
    if shift == 0.0 { return v; }
    let (lo, span) = if channel.is_signed() { (-1.0, 2.0) } else { (0.0, 1.0) };
    let x = (v - lo + shift) / span;
    let f = x - x.floor();
    // An exact multiple of the range above its start is the range's end, not its start.
    lo + span * if f == 0.0 && x > 0.0 { 1.0 } else { f }
}

/// Contrast then luminosity, both -1..=1: contrast >= 0 stretches around the pivot by
/// `1 / (1 - c)` (c = 1: a hard step), contrast < 0 compresses by `1 + c`; luminosity is added.
/// The pivot is 0.5 (0 for signed Height); unsigned results clamp to 0..=1.
pub fn contrast_luminosity(v: f32, contrast: f32, luminosity: f32, channel: ChannelKind) -> f32 {
    let pivot = if channel.is_signed() { 0.0 } else { 0.5 };
    let d = v - pivot;
    let stretched = if contrast >= 1.0 {
        // Hard step: the half-range on either side of the pivot.
        let half = if channel.is_signed() { 1.0 } else { 0.5 };
        if d > 0.0 { half } else if d < 0.0 { -half } else { 0.0 }
    } else if contrast >= 0.0 { d / (1.0 - contrast) } else { d * (1.0 + contrast) };
    let out = pivot + stretched + luminosity;
    if channel.is_signed() { out.clamp(-1.0, 1.0) } else { out.clamp(0.0, 1.0) }
}

/// Color Correct: the three ranges' `[contrast, luminosity, saturation]` blended by
/// [`tonal_weights`] of the linear luminance, applied to the sRGB-encoded color (contrast and
/// luminosity as [`contrast_luminosity`], saturation as a mix with the encoded gray by
/// `1 + saturation`), then decoded. All zeros is exactly the identity.
pub fn color_correct(c: [f32; 3], shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3]) -> [f32; 3] {
    if shadows == [0.0; 3] && midtones == [0.0; 3] && highlights == [0.0; 3] { return c; }
    let w = tonal_weights(luminance(c));
    let p: [f32; 3] = std::array::from_fn(|i| w[0] * shadows[i] + w[1] * midtones[i] + w[2] * highlights[i]);
    let enc = linear3_to_srgb(c.map(|v| v.clamp(0.0, 1.0)));
    let enc = enc.map(|v| contrast_luminosity(v, p[0], p[1], ChannelKind::BaseColor));
    let gray = luminance(enc);
    srgb3_to_linear(enc.map(|v| (gray + (v - gray) * (1.0 + p[2])).clamp(0.0, 1.0)))
}

/// Linear sRGB to OKLab (Bjorn Ottosson, 2020).
pub fn linear_to_oklab(c: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_47 * c[0] + 0.536_332_55 * c[1] + 0.051_445_995 * c[2]).cbrt();
    let m = (0.211_903_5 * c[0] + 0.680_699_5 * c[1] + 0.107_396_96 * c[2]).cbrt();
    let s = (0.088_302_46 * c[0] + 0.281_718_85 * c[1] + 0.629_978_7 * c[2]).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
     1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
     0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

/// OKLab to linear sRGB (may leave the gamut).
pub fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let l = (lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2]).powi(3);
    [4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
     -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
     -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s]
}

/// Perceptive HSL: hue rotation (degrees), chroma multiplier and additive lightness in OKLCH,
/// so a hue shift keeps the perceived brightness. Results are clipped to non-negative RGB.
pub fn hsl_perceptive(c: [f32; 3], hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let lab = linear_to_oklab(c.map(|v| v.max(0.0)));
    let (sin, cos) = hue.to_radians().sin_cos();
    let (a, b) = ((lab[1] * cos - lab[2] * sin) * saturation, (lab[1] * sin + lab[2] * cos) * saturation);
    oklab_to_linear([(lab[0] + lightness).max(0.0), a, b]).map(|v| v.max(0.0))
}

/// Linear ramp from `position - w/2` to `position + w/2` with `w = 1 - contrast`; hard step at 1.
pub fn histogram_scan(v: f32, position: f32, contrast: f32, invert: bool) -> f32 {
    let width = 1.0 - contrast;
    let out = if width <= 1e-6 { if v >= position { 1.0 } else { 0.0 } } else { ((v - (position - width * 0.5)) / width).clamp(0.0, 1.0) };
    if invert { 1.0 - out } else { out }
}

/// 1 inside `position +- range / 2` (inclusive), 0 outside; each edge ramps over
/// `softness * range / 2` toward the inside.
pub fn histogram_range(v: f32, position: f32, range: f32, softness: f32, invert: bool) -> f32 {
    let (lo, hi) = (position - range * 0.5, position + range * 0.5);
    let ramp = softness * range * 0.5;
    let out = if v < lo || v > hi { 0.0 } else if ramp <= 1e-6 { 1.0 } else { ((v - lo) / ramp).min((hi - v) / ramp).clamp(0.0, 1.0) };
    if invert { 1.0 - out } else { out }
}

/// Rounds `v` to `levels` evenly spaced steps over the channel range (0..1, or -1..1 for signed).
pub fn posterize(v: f32, levels: u32, channel: ChannelKind) -> f32 {
    let steps = (levels.max(2) - 1) as f32;
    let (lo, hi) = if channel.is_signed() { (-1.0, 1.0) } else { (0.0, 1.0) };
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    lo + (t * steps).round() / steps * (hi - lo)
}

/// Tonal weights of a linear luminance: shadows fall from 1 at 0 to 0 at 0.5, highlights rise from
/// 0 at 0.5 to 1 at 1, midtones take the rest, so the three always sum to 1.
pub fn tonal_weights(lum: f32) -> [f32; 3] {
    let l = lum.clamp(0.0, 1.0);
    let sh = (1.0 - 2.0 * l).max(0.0);
    let hi = (2.0 * l - 1.0).max(0.0);
    [sh, 1.0 - sh - hi, hi]
}

pub fn color_balance(c: [f32; 3], shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3]) -> [f32; 3] {
    let w = tonal_weights(luminance(c));
    std::array::from_fn(|i| (c[i] + w[0] * shadows[i] + w[1] * midtones[i] + w[2] * highlights[i]).clamp(0.0, 1.0))
}

/// Weight 1 within `tolerance` of `from`, falling linearly to 0 over `softness` more distance.
pub fn replace_color(c: [f32; 3], from: [f32; 3], to: [f32; 3], tolerance: f32, softness: f32) -> [f32; 3] {
    let d = ((c[0] - from[0]).powi(2) + (c[1] - from[1]).powi(2) + (c[2] - from[2]).powi(2)).sqrt();
    let w = if d <= tolerance { 1.0 } else if softness <= 0.0 { 0.0 } else { (1.0 - (d - tolerance) / softness).clamp(0.0, 1.0) };
    std::array::from_fn(|i| (c[i] + w * (to[i] - from[i])).max(0.0))
}

fn luminance(c: [f32; 3]) -> f32 { c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722 }

/// Color inputs are looked up by linear luminance, scalar inputs by their raw value (signed
/// Height is remapped from -1..1 to 0..1 and the result mapped back). Scalar channels receive the
/// gradient's luminance; color channels receive its RGB unless `to_gray`.
fn gradient_map(g: &subcraft_doc::Gradient, to_gray: bool, input: [f32; 3], channel: ChannelKind) -> [f32; 3] {
    let signed = channel.is_signed();
    let v = if channel.is_color() { luminance(input) } else { input[0] };
    let t = if signed { v * 0.5 + 0.5 } else { v }.clamp(0.0, 1.0);
    let c = g.eval(t);
    if channel.is_color() && !to_gray { return c; }
    let l = luminance(c);
    [if signed { l * 2.0 - 1.0 } else { l }; 3]
}

pub fn apply_mask(filter: Filter, value: f32) -> f32 {
    if filter.supports_mask() { apply(filter, [value; 3], ChannelKind::Roughness)[0] } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_core::color::srgb_to_linear;

    #[test]
    fn scalar_filters_have_numeric_boundaries_and_preserve_signed_height() {
        let contrast = Filter::Contrast { gain: 2.0, pivot: 0.5 };
        assert_eq!(apply(contrast, [0.2, 0.5, 0.9], ChannelKind::BaseColor), [0.0, 0.5, 1.0]);
        assert!((apply(contrast, [-0.2; 3], ChannelKind::Height)[0] + 0.9).abs() < 1e-6);
        assert_eq!(apply_mask(Filter::Clamp { low: 0.2, high: 0.7 }, 0.9), 0.7);
        assert_eq!(apply_mask(Filter::Threshold { cutoff: 0.4 }, 0.4), 1.0);
        assert_eq!(apply_mask(Filter::Threshold { cutoff: 0.4 }, 0.399), 0.0);
        // Red's linear luminance is below 0.3; green's is above. Output is grayscale.
        assert_eq!(apply(Filter::Threshold { cutoff: 0.3 }, [1.0, 0.0, 0.0], ChannelKind::BaseColor), [0.0; 3]);
        assert_eq!(apply(Filter::Threshold { cutoff: 0.3 }, [0.0, 1.0, 0.0], ChannelKind::BaseColor), [1.0; 3]);
    }

    #[test]
    fn gradient_map_interpolates_in_linear_space_and_picks_channel_output() {
        use subcraft_doc::{Gradient, GradientStop};
        let st = |position, color| GradientStop { position, color };
        let g = Gradient::new(&[st(0.0, [0.0; 3]), st(0.5, [1.0, 0.0, 0.0]), st(1.0, [1.0; 3])]).unwrap();
        let f = |to_gray| Filter::GradientMap { gradient: g, to_gray };
        let close = |a: [f32; 3], b: [f32; 3]| assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6), "{a:?} vs {b:?}");
        // Color input: luminance 0.25 sits halfway between stop 0 and stop 1 (linear lerp).
        let gray = |v: f32| [v; 3];
        close(apply(f(false), gray(0.25), ChannelKind::BaseColor), [0.5, 0.0, 0.0]);
        close(apply(f(false), gray(0.0), ChannelKind::BaseColor), [0.0; 3]);
        close(apply(f(false), gray(5.0), ChannelKind::BaseColor), [1.0; 3]);
        close(apply(f(false), gray(0.75), ChannelKind::BaseColor), [1.0, 0.5, 0.5]);
        // to_gray: luminance of the looked-up color.
        close(apply(f(true), gray(0.5), ChannelKind::BaseColor), [0.2126; 3]);
        // Scalar channel and mask: gray luminance, raw value is the lookup.
        close(apply(f(false), gray(0.5), ChannelKind::Roughness), [0.2126; 3]);
        assert!((apply_mask(f(false), 1.0) - 1.0).abs() < 1e-6);
        assert!(apply_mask(f(false), 0.0).abs() < 1e-6);
        // Signed Height: -1..1 maps to the gradient, output returns to -1..1.
        close(apply(f(false), gray(0.0), ChannelKind::Height), [0.2126 * 2.0 - 1.0; 3]);
        close(apply(f(false), gray(-1.0), ChannelKind::Height), [-1.0; 3]);
        // Hard step on equal positions, and a single-stop gradient is constant.
        let step = Gradient::new(&[st(0.0, [0.0; 3]), st(0.5, [0.0; 3]), st(0.5, [1.0; 3]), st(1.0, [1.0; 3])]).unwrap();
        let s = Filter::GradientMap { gradient: step, to_gray: true };
        assert_eq!(apply_mask(s, 0.499), 0.0); assert_eq!(apply_mask(s, 0.5), 1.0);
        let one = Gradient::new(&[st(0.3, [0.2, 0.4, 0.6])]).unwrap();
        close(apply(Filter::GradientMap { gradient: one, to_gray: false }, gray(0.9), ChannelKind::BaseColor), [0.2, 0.4, 0.6]);
        // Invalid gradients and Normal maps pass through.
        let bad = Gradient::new(&[st(0.8, [0.0; 3]), st(0.2, [1.0; 3])]).unwrap();
        close(apply(Filter::GradientMap { gradient: bad, to_gray: false }, gray(0.5), ChannelKind::BaseColor), gray(0.5));
        close(apply(f(false), gray(0.5), ChannelKind::Normal), gray(0.5));
    }

    #[test]
    fn histogram_scan_and_range_match_hand_computed_values() {
        let scan = |position, contrast, invert| Filter::HistogramScan { position, contrast, invert };
        let range = |position, range, softness, invert| Filter::HistogramRange { position, range, softness, invert };
        let m = apply_mask;
        // Hard threshold at contrast 1 (inclusive at the position).
        assert_eq!(m(scan(0.4, 1.0, false), 0.399), 0.0); assert_eq!(m(scan(0.4, 1.0, false), 0.4), 1.0);
        // Contrast 0.5 -> ramp 0.25..0.75 around 0.5.
        let s = scan(0.5, 0.5, false);
        assert_eq!(m(s, 0.25), 0.0); assert_eq!(m(s, 0.75), 1.0); assert!((m(s, 0.5) - 0.5).abs() < 1e-6); assert!((m(s, 0.375) - 0.25).abs() < 1e-6);
        // Contrast 0 -> ramp over the full 0..1 range when centred on 0.5 (identity).
        for v in [0.0, 0.1, 0.5, 0.9, 1.0] { assert!((m(scan(0.5, 0.0, false), v) - v).abs() < 1e-6); }
        // Invert is exactly one minus.
        assert!((m(scan(0.5, 0.5, true), 0.375) - 0.75).abs() < 1e-6);
        // Range: hard band 0.3..0.5 (inclusive both ends), outside is 0.
        let r = range(0.4, 0.2, 0.0, false);
        assert_eq!(m(r, 0.299), 0.0); assert_eq!(m(r, 0.3), 1.0); assert_eq!(m(r, 0.5), 1.0); assert_eq!(m(r, 0.501), 0.0);
        // Softness 1: triangle peaking at the position, ramp = half-range = 0.1.
        let t = range(0.4, 0.2, 1.0, false);
        assert!((m(t, 0.35) - 0.5).abs() < 1e-5); assert!((m(t, 0.4) - 1.0).abs() < 1e-6); assert!((m(t, 0.45) - 0.5).abs() < 1e-5);
        assert_eq!(m(range(0.4, 0.2, 0.0, true), 0.4), 0.0); assert_eq!(m(range(0.4, 0.2, 0.0, true), 0.9), 1.0);
        // Color channels are processed per channel, signed Height keeps raw values in -1..1.
        assert_eq!(apply(scan(0.5, 1.0, false), [0.2, 0.6, 0.5], ChannelKind::BaseColor), [0.0, 1.0, 1.0]);
        assert_eq!(apply(range(-0.5, 0.2, 0.0, false), [-0.55, -0.7, 0.0], ChannelKind::Height), [1.0, 0.0, 0.0]);
        // Invalid parameters and Normal maps pass through.
        assert_eq!(apply(scan(0.5, 2.0, false), [0.3; 3], ChannelKind::Roughness), [0.3; 3]);
        assert_eq!(apply(scan(0.5, 1.0, false), [0.3; 3], ChannelKind::Normal), [0.3; 3]);
    }

    #[test]
    fn hsl_uses_perceptual_lightness_and_roundtrips_to_linear_storage() {
        let hsl = |hue, saturation, lightness| Filter::Hsl { hue, saturation, lightness };
        let close = |a: [f32; 3], b: [f32; 3]| assert!(a.iter().zip(b).all(|(x, y)| (*x-y).abs() < 1e-5), "{a:?} vs {b:?}");
        close(apply(hsl(120.0, 1.0, 0.0), [1.0, 0.0, 0.0], ChannelKind::BaseColor), [0.0, 1.0, 0.0]);
        close(apply(hsl(0.0, 0.0, 0.0), [1.0, 0.0, 0.0], ChannelKind::BaseColor), [srgb_to_linear(0.5); 3]);
        close(apply(hsl(0.0, 1.0, 0.25), [1.0, 0.0, 0.0], ChannelKind::BaseColor), [1.0, srgb_to_linear(0.5), srgb_to_linear(0.5)]);
        let hdr = [2.0, 0.035, 0.1234];
        assert_eq!(apply(hsl(0.0, 1.0, 0.0), hdr, ChannelKind::Emissive), hdr);
        assert_eq!(apply(hsl(50.0, 1.0, 0.0), [0.3; 3], ChannelKind::Roughness), [0.3; 3]);
        assert_eq!(apply_mask(hsl(50.0, 1.0, 0.0), 0.3), 0.3);
    }
}

/// Normalized Gaussian with finite support ceil(radius), sigma = radius / 3.
/// A zero radius is exactly the identity; tiny preview radii converge to it.
pub fn gaussian_weights(radius: f32) -> Vec<f32> {
    if !radius.is_finite() || radius <= 0.0 { return vec![1.0]; }
    let radius = radius.min(128.0);
    let support = radius.ceil() as i32;
    let sigma = (radius / 3.0).max(1e-5);
    let mut weights: Vec<_> = (-support..=support).map(|x| (-0.5 * (x as f32 / sigma).powi(2)).exp()).collect();
    let sum: f32 = weights.iter().sum();
    for w in &mut weights { *w /= sum; }
    weights
}

#[cfg(test)]
mod gaussian_tests {
    use super::*;
    #[test]
    fn finite_gaussian_is_normalized_symmetric_and_has_exact_identity() {
        assert_eq!(gaussian_weights(0.0),vec![1.0]);
        for r in [0.001,0.5,1.0,3.0,32.0] {
            let w=gaussian_weights(r);assert_eq!(w.len(),2*r.ceil() as usize+1);
            assert!((w.iter().sum::<f32>()-1.0).abs()<2e-6);
            for (a,b) in w.iter().zip(w.iter().rev()) {assert_eq!(a,b);}
        }
        let w=gaussian_weights(3.0);assert!((w[3]-0.3990503).abs()<1e-6);
    }
}

/// Gaussian line taps in top-left image coordinates; 180-degree periodic, cardinal axes exact.
pub fn directional_taps(radius:f32,angle_deg:f32)->Vec<[f32;3]> {
    if !angle_deg.is_finite() {return vec![[0.0,0.0,1.0]];}
    let angle=angle_deg.rem_euclid(180.0);
    let axis=if angle==0.0 {[1.0,0.0]} else if angle==90.0 {[0.0,1.0]} else {
        let (sin,cos)=angle.to_radians().sin_cos();[cos,sin]
    };
    let weights=gaussian_weights(radius);let r=(weights.len()/2) as f32;
    weights.into_iter().enumerate().filter(|(_,w)|*w>0.0).map(|(i,w)| {
        let t=i as f32-r;[axis[0]*t,axis[1]*t,w]
    }).collect()
}

pub fn highpass(source:[f32;3],blurred:[f32;3],amount:f32,channel:ChannelKind)->[f32;3] {
    if channel==ChannelKind::Normal || !amount.is_finite() || !(0.0..=4.0).contains(&amount) {return source;}
    let neutral=if channel.is_signed(){0.0}else{0.5};
    std::array::from_fn(|c| {let v=neutral+amount*(source[c]-blurred[c]);if channel.is_signed(){v}else{v.clamp(0.0,1.0)}})
}

#[cfg(test)]
mod directional_highpass_tests {
    use super::*;
    #[test]
    fn line_taps_preserve_cardinal_axes_periodicity_and_weight() {
        for a in [-180.0,0.0,180.0] {for t in directional_taps(4.0,a) {assert_eq!(t[1],0.0);}}
        assert_eq!(directional_taps(4.0,-90.0),directional_taps(4.0,90.0));
        assert_eq!(directional_taps(4.0,-135.0),directional_taps(4.0,45.0));
        let t=directional_taps(3.0,45.0);assert!((t.iter().map(|t|t[2]).sum::<f32>()-1.0).abs()<1e-6);
        assert!((t[0][0]-t[0][1]).abs()<1e-6);assert_eq!(directional_taps(0.0,0.0),vec![[0.0,0.0,1.0]]);
    }
    #[test]
    fn highpass_has_explicit_neutral_signed_and_clamped_values() {
        let k=ChannelKind::BaseColor;
        assert_eq!(highpass([0.2;3],[0.2;3],4.0,k),[0.5;3]);
        assert_eq!(highpass([0.9,0.1,0.4],[0.1,0.9,0.4],1.0,k),[1.0,0.0,0.5]);
        assert_eq!(highpass([-0.5;3],[0.25;3],2.0,ChannelKind::Height),[-1.5;3]);
        assert_eq!(highpass([0.9;3],[0.1;3],0.0,k),[0.5;3]);
        assert_eq!(highpass([0.9;3],[0.1;3],0.0,ChannelKind::Height),[0.0;3]);
    }
}

#[cfg(test)]
mod standard_filter_tests {
    use super::*;
    use ChannelKind as K;
    use subcraft_doc::{HslMode, ThresholdMode};
    fn close(a: [f32; 3], b: [f32; 3], eps: f32) { assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < eps), "{a:?} vs {b:?}"); }

    #[test]
    fn smoothstep_is_the_hermite_ramp_between_min_and_max() {
        let f = |min, max| Filter::Smoothstep { min, max };
        // Defaults 0..1: t^2 (3 - 2t).
        for (v, e) in [(0.0, 0.0), (0.25, 0.15625), (0.5, 0.5), (0.75, 0.84375), (1.0, 1.0)] {
            assert!((apply_mask(f(0.0, 1.0), v) - e).abs() < 1e-6, "{v}");
        }
        // 0.2..0.6: flat outside, t = 0.25 at 0.3.
        assert_eq!(apply_mask(f(0.2, 0.6), 0.1), 0.0);
        assert_eq!(apply_mask(f(0.2, 0.6), 0.7), 1.0);
        assert!((apply_mask(f(0.2, 0.6), 0.3) - 0.15625).abs() < 1e-6);
        // Inverted limits invert the ramp; equal limits are a step.
        assert!((apply_mask(f(0.6, 0.2), 0.3) - 0.84375).abs() < 1e-6);
        assert_eq!((apply_mask(f(0.4, 0.4), 0.399), apply_mask(f(0.4, 0.4), 0.4)), (0.0, 1.0));
        // Per component on color, raw values on signed Height.
        close(apply(f(0.0, 1.0), [0.25, 0.5, 2.0], K::BaseColor), [0.15625, 0.5, 1.0], 1e-6);
        close(apply(f(-1.0, 1.0), [0.0, -1.0, -0.5], K::Height), [0.5, 0.0, 0.15625], 1e-6);
    }

    #[test]
    fn threshold_modes_differ_only_at_and_across_the_threshold() {
        let t = |mode| Filter::threshold(0.4, mode);
        let row = |mode| [0.399, 0.4, 0.401].map(|v| apply_mask(t(mode), v));
        assert_eq!(row(ThresholdMode::Greater), [0.0, 0.0, 1.0]);
        assert_eq!(row(ThresholdMode::GreaterOrEqual), [0.0, 1.0, 1.0]);
        assert_eq!(row(ThresholdMode::Lower), [1.0, 0.0, 0.0]);
        assert_eq!(row(ThresholdMode::LowerOrEqual), [1.0, 1.0, 0.0]);
        // Color compares the linear luminance: red 0.2126 is lower than 0.3, green 0.7152 is not.
        assert_eq!(apply(Filter::threshold(0.3, ThresholdMode::Lower), [1.0, 0.0, 0.0], K::BaseColor), [1.0; 3]);
        assert_eq!(apply(Filter::threshold(0.3, ThresholdMode::Lower), [0.0, 1.0, 0.0], K::BaseColor), [0.0; 3]);
    }

    #[test]
    fn histogram_shift_wraps_around_the_range_instead_of_clamping() {
        let w = |shift| Filter::histogram_shift(shift, true);
        close(apply(w(0.3), [0.5, 0.9, 0.0], K::Roughness), [0.8, 0.2, 0.3], 1e-6);
        close(apply(w(-0.3), [0.5, 0.1, 1.0], K::Roughness), [0.2, 0.8, 0.7], 1e-6);
        // The clamping variant is unchanged.
        close(apply(Filter::histogram_shift(0.3, false), [0.5, 0.9, 0.0], K::Roughness), [0.8, 1.0, 0.3], 1e-6);
        // Zero is the identity (white stays white), a full turn too.
        assert_eq!(apply(w(0.0), [1.0, 0.0, 0.37], K::Roughness), [1.0, 0.0, 0.37]);
        close(apply(w(1.0), [1.0, 0.25, 0.5], K::Roughness), [1.0, 0.25, 0.5], 1e-6);
        // Signed Height wraps over -1..1 (a range of 2).
        close(apply(w(0.5), [0.75, -0.75, 0.0], K::Height), [-0.75, -0.25, 0.5], 1e-6);
        for v in [-1.0f32, -0.3, 0.0, 0.6, 1.0] {
            for shift in [-1.0f32, -0.45, 0.2, 0.9] {
                let (a, b) = (shift_wrapped(v, shift, K::Height), shift_wrapped(v.abs(), shift, K::Roughness));
                assert!((-1.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b), "{v} {shift}: {a} {b}");
            }
        }
    }

    #[test]
    fn contrast_luminosity_stretches_around_mid_gray_then_offsets() {
        let f = |contrast, luminosity| Filter::ContrastLuminosity { contrast, luminosity };
        assert_eq!(apply(f(0.0, 0.0), [0.1, 0.5, 0.9], K::BaseColor), [0.1, 0.5, 0.9]);
        // Contrast 0.5 doubles the distance to 0.5; -0.5 halves it.
        close(apply(f(0.5, 0.0), [0.4, 0.5, 0.9], K::Roughness), [0.3, 0.5, 1.0], 1e-6);
        close(apply(f(-0.5, 0.0), [0.1, 0.5, 0.9], K::Roughness), [0.3, 0.5, 0.7], 1e-6);
        // The extremes: a hard step, and flat mid gray.
        assert_eq!(apply(f(1.0, 0.0), [0.49, 0.5, 0.51], K::Roughness), [0.0, 0.5, 1.0]);
        close(apply(f(-1.0, 0.0), [0.0, 0.3, 1.0], K::Roughness), [0.5; 3], 1e-6);
        // Luminosity is a plain offset after the contrast.
        close(apply(f(0.0, 0.2), [0.1, 0.5, 0.9], K::Roughness), [0.3, 0.7, 1.0], 1e-6);
        close(apply(f(0.5, -0.1), [0.4, 0.5, 0.6], K::Roughness), [0.2, 0.4, 0.6], 1e-6);
        // Signed Height pivots on 0 and keeps its sign range.
        close(apply(f(0.5, 0.0), [-0.2, 0.0, 0.3], K::Height), [-0.4, 0.0, 0.6], 1e-6);
        close(apply(f(0.0, -0.5), [-0.8, 0.0, 0.3], K::Height), [-1.0, -0.5, -0.2], 1e-6);
        assert!((apply_mask(f(0.5, 0.0), 0.4) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn color_correct_acts_on_the_tonal_range_it_is_set_for() {
        let cc = |shadows, midtones, highlights| Filter::ColorCorrect { shadows, midtones, highlights };
        let z = [0.0; 3];
        let (black, mid, white) = ([0.0; 3], [0.5; 3], [1.0; 3]);
        assert_eq!(apply(cc(z, z, z), [0.3, 0.6, 0.1], K::BaseColor), [0.3, 0.6, 0.1]);
        // Luminosity on one range moves only that range (weights 1 / 0 at luminance 0, 0.5, 1).
        let lift = cc([0.0, 0.2, 0.0], z, z);
        close(apply(lift, black, K::BaseColor), [subcraft_core::color::srgb_to_linear(0.2); 3], 1e-5);
        close(apply(lift, mid, K::BaseColor), mid, 1e-5);
        close(apply(lift, white, K::BaseColor), white, 1e-5);
        let dim = cc(z, z, [0.0, -0.25, 0.0]);
        close(apply(dim, white, K::BaseColor), [subcraft_core::color::srgb_to_linear(0.75); 3], 1e-5);
        close(apply(dim, black, K::BaseColor), black, 1e-5);
        // Saturation -1 on the midtones: a mid-luminance color becomes its encoded gray.
        let c = [0.8, 0.5, 0.2];
        let lum = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        let wm = tonal_weights(lum)[1];
        assert!(wm > 0.8, "{wm}");
        let enc = linear3_to_srgb(c);
        let gray = enc[0] * 0.2126 + enc[1] * 0.7152 + enc[2] * 0.0722;
        let expect = srgb3_to_linear(enc.map(|v| gray + (v - gray) * (1.0 - wm)));
        close(apply(cc(z, [0.0, 0.0, -1.0], z), c, K::BaseColor), expect, 1e-5);
        // Saturation +1 doubles the distance to gray (clamped), contrast spreads the components.
        let more = apply(cc([0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]), c, K::BaseColor);
        assert!(more[0] > c[0] && more[2] < c[2], "{more:?}");
        let contrast = apply(cc([0.5, 0.0, 0.0], [0.5, 0.0, 0.0], [0.5, 0.0, 0.0]), c, K::BaseColor);
        let e = linear3_to_srgb(c).map(|v| (0.5 + (v - 0.5) * 2.0).clamp(0.0, 1.0));
        close(contrast, srgb3_to_linear(e), 1e-5);
        // Data channels and masks pass through.
        assert_eq!(apply(lift, [0.3; 3], K::Roughness), [0.3; 3]);
    }

    #[test]
    fn perceptive_hsl_works_in_oklch_and_keeps_perceived_lightness() {
        let p = |hue, saturation, lightness| Filter::hsl(HslMode::Perceptive, hue, saturation, lightness);
        // OKLab reference values: white is (1, 0, 0); sRGB red is (0.62796, 0.22486, 0.12585).
        close(linear_to_oklab([1.0; 3]), [1.0, 0.0, 0.0], 2e-4);
        close(linear_to_oklab([1.0, 0.0, 0.0]), [0.627_96, 0.224_86, 0.125_85], 2e-4);
        for c in [[0.2, 0.5, 0.9], [1.0, 0.0, 0.0], [0.05, 0.05, 0.05]] { close(oklab_to_linear(linear_to_oklab(c)), c, 2e-4); }
        // Neutral settings are exactly the identity, HDR included.
        assert_eq!(apply(p(0.0, 1.0, 0.0), [2.0, 0.035, 0.1234], K::Emissive), [2.0, 0.035, 0.1234]);
        // A hue shift keeps OKLab lightness and chroma; regular HSL does not keep the lightness.
        let c = [0.6, 0.2, 0.2];
        let shifted = apply(p(120.0, 1.0, 0.0), c, K::BaseColor);
        let (a, b) = (linear_to_oklab(c), linear_to_oklab(shifted));
        let chroma = |l: [f32; 3]| (l[1] * l[1] + l[2] * l[2]).sqrt();
        assert!(shifted[1] > shifted[0] && shifted[1] > shifted[2], "red turned green: {shifted:?}");
        // (The shifted color is clipped to the gamut, so allow a little drift.)
        assert!((a[0] - b[0]).abs() < 0.03 && (chroma(a) - chroma(b)).abs() < 0.03, "{a:?} {b:?}");
        let regular = apply(Filter::hsl(HslMode::Regular, 120.0, 1.0, 0.0), c, K::BaseColor);
        assert!((linear_to_oklab(regular)[0] - a[0]).abs() > 0.05, "regular HSL changes the perceived lightness");
        // Saturation 0: the gray of the same OKLab lightness (r = g = b = L^3).
        let gray = apply(p(0.0, 0.0, 0.0), c, K::BaseColor);
        close(gray, [a[0].powi(3); 3], 2e-4);
        // Grays stay gray under any hue; lightness adds to L.
        close(apply(p(77.0, 1.5, 0.0), [0.18; 3], K::BaseColor), [0.18; 3], 2e-4);
        let l = 0.18f32.cbrt();
        close(apply(p(0.0, 1.0, 0.1), [0.18; 3], K::BaseColor), [(l + 0.1).powi(3); 3], 2e-4);
        close(apply(p(0.0, 1.0, -1.0), c, K::BaseColor), [0.0; 3], 1e-3);
        // Color channels only.
        assert_eq!(apply(p(50.0, 1.0, 0.0), [0.3; 3], K::Roughness), [0.3; 3]);
        assert_eq!(apply_mask(p(50.0, 1.0, 0.0), 0.3), 0.3);
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    use ChannelKind as K;
    fn close(a: [f32; 3], b: [f32; 3]) { assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5), "{a:?} vs {b:?}"); }

    #[test]
    fn grayscale_invert_shift_posterize_goldens() {
        close(apply(Filter::Grayscale, [1.0, 0.0, 0.0], K::BaseColor), [0.2126; 3]);
        assert_eq!(apply(Filter::Grayscale, [1.0, 0.0, 0.0], K::Roughness), [1.0, 0.0, 0.0], "scalar channels are untouched");
        close(apply(Filter::Invert, [0.25, 0.5, 1.0], K::BaseColor), [0.75, 0.5, 0.0]);
        close(apply(Filter::Invert, [0.25; 3], K::Height), [-0.25; 3]);
        close(apply(Filter::HistogramShift { shift: 0.3 }, [0.5, 0.9, 0.0], K::Roughness), [0.8, 1.0, 0.3]);
        close(apply(Filter::HistogramShift { shift: -0.3 }, [0.5; 3], K::Height), [0.2; 3]);
        close(apply(Filter::HistogramShift { shift: 0.9 }, [0.5; 3], K::Height), [1.0; 3]);
        // 3 levels: 0, 0.5, 1.
        for (v, e) in [(0.2, 0.0), (0.3, 0.5), (0.74, 0.5), (0.76, 1.0), (1.0, 1.0)] {
            assert_eq!(posterize(v, 3, K::Roughness), e, "{v}");
        }
        assert_eq!(posterize(-0.3, 3, K::Height), 0.0, "signed 3 levels: -1, 0, 1");
        assert_eq!(posterize(0.6, 3, K::Height), 1.0);
        assert_eq!(posterize(0.4, 2, K::Roughness), 0.0);
    }

    #[test]
    fn normal_invert_flips_selected_axes() {
        let f = Filter::NormalInvert { flip_x: false, flip_y: true, flip_z: false };
        close(apply(f, [0.7, 0.2, 0.9], K::Normal), [0.7, 0.8, 0.9]);
        // Twice is the identity, and a flat normal (0.5, 0.5, 1) only changes in the flipped axes.
        close(apply(f, apply(f, [0.7, 0.2, 0.9], K::Normal), K::Normal), [0.7, 0.2, 0.9]);
        close(apply(f, [0.5, 0.5, 1.0], K::Normal), [0.5, 0.5, 1.0]);
        assert_eq!(apply(f, [0.7, 0.2, 0.9], K::BaseColor), [0.7, 0.2, 0.9], "wrong channel is a no-op");
    }

    #[test]
    fn color_balance_weights_and_goldens() {
        for l in [0.0, 0.1, 0.25, 0.5, 0.8, 1.0] { assert!((tonal_weights(l).iter().sum::<f32>() - 1.0).abs() < 1e-6); }
        assert_eq!(tonal_weights(0.0), [1.0, 0.0, 0.0]);
        assert_eq!(tonal_weights(0.5), [0.0, 1.0, 0.0]);
        assert_eq!(tonal_weights(1.0), [0.0, 0.0, 1.0]);
        let (s, m, h) = ([0.2, 0.0, 0.0], [0.0, 0.1, 0.0], [0.0, 0.0, -0.4]);
        close(color_balance([0.0; 3], s, m, h), [0.2, 0.0, 0.0]);
        close(color_balance([0.5; 3], s, m, h), [0.5, 0.6, 0.5]);
        close(color_balance([1.0; 3], s, m, h), [1.0, 1.0, 0.6]);
        close(color_balance([0.4; 3], [0.0; 3], [0.0; 3], [0.0; 3]), [0.4; 3]);
    }

    #[test]
    fn replace_color_tolerance_and_softness() {
        let (from, to) = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        close(replace_color(from, from, to, 0.1, 0.2), to);
        close(replace_color([0.95, 0.0, 0.0], from, to, 0.1, 0.2), [0.95 - 1.0, 0.0, 1.0].map(|v: f32| v.max(0.0)));
        // Distance 0.2 with tolerance 0.1 and softness 0.2: weight 0.5.
        close(replace_color([0.8, 0.0, 0.0], from, to, 0.1, 0.2), [0.3, 0.0, 0.5]);
        close(replace_color([0.0, 1.0, 0.0], from, to, 0.1, 0.2), [0.0, 1.0, 0.0]);
        close(replace_color([0.8, 0.0, 0.0], from, to, 0.1, 0.0), [0.8, 0.0, 0.0]);
    }
}
