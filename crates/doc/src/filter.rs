//! Serializable local filter parameters. Kernels live in materials, never in widgets.
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
}

impl Filter {
    pub const ALL: [Self; 4] = [Self::Contrast { gain: 1.0, pivot: 0.5 },
        Self::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 },
        Self::Threshold { cutoff: 0.5 }, Self::Clamp { low: 0.0, high: 1.0 }];

    pub fn label(self) -> &'static str {
        match self { Self::Contrast { .. } => "Contrast", Self::Hsl { .. } => "HSL",
            Self::Threshold { .. } => "Threshold", Self::Clamp { .. } => "Clamp" }
    }

    pub fn supports_channel(self, channel: ChannelKind) -> bool {
        channel != ChannelKind::Normal && (!matches!(self, Self::Hsl { .. }) || channel.is_color())
    }

    pub fn supports_mask(self) -> bool { !matches!(self, Self::Hsl { .. }) }

    pub fn valid(self) -> bool {
        match self {
            Self::Contrast { gain, pivot } => gain.is_finite() && (0.0..=4.0).contains(&gain) && pivot.is_finite(),
            Self::Hsl { hue, saturation, lightness } => hue.is_finite() && (-180.0..=180.0).contains(&hue)
                && saturation.is_finite() && (0.0..=2.0).contains(&saturation)
                && lightness.is_finite() && (-1.0..=1.0).contains(&lightness),
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
        assert!(!Filter::Hsl { hue: 0.0, saturation: 1.0, lightness: 0.0 }.supports_mask());
        assert!(!Filter::Clamp { low: 0.8, high: 0.2 }.valid());
        assert!(!Filter::Threshold { cutoff: f32::NAN }.valid());
        assert!(!Filter::Contrast { gain: f32::INFINITY, pivot: 0.5 }.valid());
    }
}
