//! Deterministic point filters. Read only the previous effect stage at the same texel (halo 0).
use subcraft_core::{ChannelKind, color::{linear3_to_srgb, srgb3_to_linear}};
use subcraft_doc::Filter;

pub fn apply(filter: Filter, input: [f32; 3], channel: ChannelKind) -> [f32; 3] {
    if !filter.valid() || !filter.supports_channel(channel) { return input; }
    match filter {
        Filter::Contrast { gain: 1.0, .. } => input,
        Filter::Contrast { gain, pivot } => input.map(|v| {
            let v = pivot + (v - pivot) * gain;
            if channel.is_signed() { v } else { v.clamp(0.0, 1.0) }
        }),
        Filter::Blur { .. } | Filter::Sharpen { .. } => input, // Neighborhood evaluation is staged by compositor.
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
