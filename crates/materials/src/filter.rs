//! Deterministic point filters. Read only the previous effect stage at the same texel (halo 0).
use subcraft_core::{ChannelKind, color::{linear3_to_srgb, srgb3_to_linear}};
use subcraft_doc::Filter;
pub mod contour;

pub fn apply(filter: Filter, input: [f32; 3], channel: ChannelKind) -> [f32; 3] {
    if !filter.valid() || !filter.supports_channel(channel) { return input; }
    match filter {
        Filter::Contrast { gain: 1.0, .. } => input,
        Filter::Contrast { gain, pivot } => input.map(|v| {
            let v = pivot + (v - pivot) * gain;
            if channel.is_signed() { v } else { v.clamp(0.0, 1.0) }
        }),
        Filter::Blur { .. } | Filter::Sharpen { .. } | Filter::DirectionalBlur { .. } | Filter::Highpass { .. } | Filter::Bevel {..} | Filter::MaskOutline {..} => input, // Neighborhood evaluation is staged by compositor.
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
