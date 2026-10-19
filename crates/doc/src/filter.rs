//! Serializable filter parameters. Kernels live in materials, never in widgets.
use serde::{Deserialize, Serialize};
use subcraft_core::ChannelKind;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Filter {
    Contrast { gain: f32, pivot: f32 },
    /// Hue in degrees, saturation multiplier, additive HSL lightness. Color channels only.
    Hsl { hue: f32, saturation: f32, lightness: f32 },
    /// Color inputs use linear Rec.709 luminance; scalar inputs use their raw value.
    Threshold { cutoff: f32 },
    Clamp { low: f32, high: f32 },
    /// Gaussian support radius in working texture pixels.
    Blur { radius: f32 },
    Sharpen { radius: f32, amount: f32 },
    /// Symmetric Gaussian line along a UV axis. Positive angle follows image +v (down).
    DirectionalBlur { radius: f32, angle_deg: f32 },
    /// Source minus Gaussian low frequencies; neutral 0.5, or 0 for signed Height.
    Highpass { radius: f32, amount: f32 },
}

impl Filter {
    pub const ALL: [Self; 8] = [Self::Contrast { gain: 1.0, pivot: 0.5 },
        Self::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 },
        Self::Threshold { cutoff: 0.5 }, Self::Clamp { low: 0.0, high: 1.0 }, Self::Blur { radius: 4.0 }, Self::Sharpen { radius: 2.0, amount: 1.0 }, Self::DirectionalBlur { radius: 8.0, angle_deg: 0.0 }, Self::Highpass { radius: 4.0, amount: 1.0 }];

    pub fn label(self) -> &'static str {
        match self { Self::Contrast { .. } => "Contrast", Self::Hsl { .. } => "HSL",
            Self::Threshold { .. } => "Threshold", Self::Clamp { .. } => "Clamp", Self::Blur { .. } => "Blur", Self::Sharpen { .. } => "Sharpen", Self::DirectionalBlur { .. } => "Directional Blur", Self::Highpass { .. } => "Highpass" }
    }

    pub fn supports_channel(self, channel: ChannelKind) -> bool {
        channel != ChannelKind::Normal && (!matches!(self, Self::Hsl { .. }) || channel.is_color())
    }

    pub fn supports_mask(self) -> bool { !matches!(self, Self::Hsl { .. }) }

    pub fn radius(self) -> Option<f32> {
        match self { Self::Blur { radius } | Self::Sharpen { radius, .. } | Self::DirectionalBlur { radius, .. } | Self::Highpass { radius, .. } => Some(radius), _ => None }
    }

    pub fn softens_coverage(self) -> bool { matches!(self, Self::Blur { .. } | Self::DirectionalBlur { .. }) }

    /// Highpass is deliberately excluded: radius/amount zero gives its neutral value.
    pub fn spatial_identity(self) -> bool {
        matches!(self, Self::Blur {radius:0.0} | Self::DirectionalBlur {radius:0.0,..}
            | Self::Sharpen {radius:0.0,..} | Self::Sharpen {amount:0.0,..})
    }

    pub fn valid(self) -> bool {
        match self {
            Self::Contrast { gain, pivot } => gain.is_finite() && (0.0..=4.0).contains(&gain) && pivot.is_finite(),
            Self::Hsl { hue, saturation, lightness } => hue.is_finite() && (-180.0..=180.0).contains(&hue)
                && saturation.is_finite() && (0.0..=2.0).contains(&saturation)
                && lightness.is_finite() && (-1.0..=1.0).contains(&lightness),
            Self::Blur { radius } => radius.is_finite() && (0.0..=32.0).contains(&radius),
            Self::DirectionalBlur { radius, angle_deg } => Self::Blur {radius}.valid() && angle_deg.is_finite() && (-180.0..=180.0).contains(&angle_deg),
            Self::Sharpen { radius, amount } | Self::Highpass { radius, amount } => Self::Blur { radius }.valid() && amount.is_finite() && (0.0..=4.0).contains(&amount),
            Self::Threshold { cutoff } => cutoff.is_finite(),
            Self::Clamp { low, high } => low.is_finite() && high.is_finite() && low <= high,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_parameters_and_domains_are_checked() {
        for f in Filter::ALL { assert!(f.valid()); assert!(!f.supports_channel(ChannelKind::Normal)); }
        assert!(!Filter::DirectionalBlur {radius:2.0,angle_deg:181.0}.valid());
        assert!(!Filter::Highpass {radius:2.0,amount:f32::INFINITY}.valid());
        assert!(!Filter::Highpass {radius:0.0,amount:0.0}.spatial_identity());
        assert!(Filter::DirectionalBlur {radius:0.0,angle_deg:45.0}.spatial_identity());
        assert!(!Filter::Blur {radius:32.01}.valid());
        assert!(!Filter::Sharpen {radius:2.0,amount:f32::NAN}.valid());
        assert!(!Filter::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 }.supports_mask());
        assert!(!Filter::Clamp { low: 0.8, high: 0.2 }.valid());
        assert!(!Filter::Threshold { cutoff: f32::NAN }.valid());
        assert!(!Filter::Contrast { gain: f32::INFINITY, pivot: 0.5 }.valid());
    }
}
