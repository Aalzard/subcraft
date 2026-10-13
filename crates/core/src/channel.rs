//! Texture channel vocabulary.
//!
//! A *channel* is a semantic material property painted by layers (base color, roughness, ...).
//! Color channels are stored and blended in **linear** space; data channels are never
//! color-transformed (see ADR-0004 color spaces).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    BaseColor,
    Height,
    Roughness,
    Metallic,
    Normal,
    AmbientOcclusion,
    Emissive,
    Opacity,
    Specular,
    User0,
    User1,
    User2,
    User3,
}

impl ChannelKind {
    pub const ALL: [ChannelKind; 13] = [
        ChannelKind::BaseColor,
        ChannelKind::Height,
        ChannelKind::Roughness,
        ChannelKind::Metallic,
        ChannelKind::Normal,
        ChannelKind::AmbientOcclusion,
        ChannelKind::Emissive,
        ChannelKind::Opacity,
        ChannelKind::Specular,
        ChannelKind::User0,
        ChannelKind::User1,
        ChannelKind::User2,
        ChannelKind::User3,
    ];

    /// Channels of the default "PBR - Metallic Roughness" project template.
    pub const METAL_ROUGH: [ChannelKind; 5] = [
        ChannelKind::BaseColor,
        ChannelKind::Height,
        ChannelKind::Roughness,
        ChannelKind::Metallic,
        ChannelKind::Normal,
    ];

    /// Number of value components (alpha/coverage is stored separately).
    pub fn components(self) -> usize {
        match self {
            ChannelKind::BaseColor | ChannelKind::Normal | ChannelKind::Emissive => 3,
            _ => 1,
        }
    }

    /// True for channels holding perceptual color (sRGB-encoded on import/export).
    pub fn is_color(self) -> bool {
        matches!(self, ChannelKind::BaseColor | ChannelKind::Emissive)
    }

    /// True if values may legitimately leave the 0..1 range.
    pub fn is_signed(self) -> bool {
        matches!(self, ChannelKind::Height)
    }

    /// Value of an untouched texel.
    pub fn default_value(self) -> ChannelValue {
        match self {
            ChannelKind::BaseColor => ChannelValue::rgb(0.5, 0.5, 0.5),
            ChannelKind::Normal => ChannelValue::rgb(0.5, 0.5, 1.0),
            ChannelKind::Emissive => ChannelValue::rgb(0.0, 0.0, 0.0),
            ChannelKind::Height => ChannelValue::scalar(0.0),
            ChannelKind::Roughness => ChannelValue::scalar(0.3),
            ChannelKind::Metallic => ChannelValue::scalar(0.0),
            ChannelKind::AmbientOcclusion | ChannelKind::Opacity => ChannelValue::scalar(1.0),
            ChannelKind::Specular => ChannelValue::scalar(0.5),
            _ => ChannelValue::scalar(0.0),
        }
    }

    /// Default value used by a freshly created paint brush / fill layer for this channel.
    pub fn default_paint_value(self) -> ChannelValue {
        match self {
            ChannelKind::BaseColor => ChannelValue::rgb(0.8, 0.8, 0.8),
            ChannelKind::Roughness => ChannelValue::scalar(0.3),
            ChannelKind::Metallic => ChannelValue::scalar(0.0),
            ChannelKind::Height => ChannelValue::scalar(0.0),
            _ => self.default_value(),
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ChannelKind::BaseColor => "Base Color",
            ChannelKind::Height => "Height",
            ChannelKind::Roughness => "Roughness",
            ChannelKind::Metallic => "Metallic",
            ChannelKind::Normal => "Normal",
            ChannelKind::AmbientOcclusion => "Ambient Occlusion",
            ChannelKind::Emissive => "Emissive",
            ChannelKind::Opacity => "Opacity",
            ChannelKind::Specular => "Specular Level",
            ChannelKind::User0 => "User 0",
            ChannelKind::User1 => "User 1",
            ChannelKind::User2 => "User 2",
            ChannelKind::User3 => "User 3",
        }
    }

    /// Compact label used on channel toggle buttons.
    pub fn short_name(self) -> &'static str {
        match self {
            ChannelKind::BaseColor => "color",
            ChannelKind::Height => "height",
            ChannelKind::Roughness => "rough",
            ChannelKind::Metallic => "metal",
            ChannelKind::Normal => "nrm",
            ChannelKind::AmbientOcclusion => "ao",
            ChannelKind::Emissive => "emiss",
            ChannelKind::Opacity => "op",
            ChannelKind::Specular => "spec",
            ChannelKind::User0 => "u0",
            ChannelKind::User1 => "u1",
            ChannelKind::User2 => "u2",
            ChannelKind::User3 => "u3",
        }
    }

    /// Stable identifier used in file names and export templates.
    pub fn key(self) -> &'static str {
        match self {
            ChannelKind::BaseColor => "basecolor",
            ChannelKind::Height => "height",
            ChannelKind::Roughness => "roughness",
            ChannelKind::Metallic => "metallic",
            ChannelKind::Normal => "normal",
            ChannelKind::AmbientOcclusion => "ao",
            ChannelKind::Emissive => "emissive",
            ChannelKind::Opacity => "opacity",
            ChannelKind::Specular => "specular",
            ChannelKind::User0 => "user0",
            ChannelKind::User1 => "user1",
            ChannelKind::User2 => "user2",
            ChannelKind::User3 => "user3",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }
}

/// A channel value: up to three components (linear space for color channels).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelValue(pub [f32; 3]);

impl ChannelValue {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self([r, g, b])
    }
    pub const fn scalar(v: f32) -> Self {
        Self([v, v, v])
    }
    pub fn x(self) -> f32 {
        self.0[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_roundtrip() {
        for c in ChannelKind::ALL {
            assert_eq!(ChannelKind::from_key(c.key()), Some(c));
        }
    }

    #[test]
    fn component_counts() {
        assert_eq!(ChannelKind::BaseColor.components(), 3);
        assert_eq!(ChannelKind::Normal.components(), 3);
        assert_eq!(ChannelKind::Roughness.components(), 1);
    }
}
