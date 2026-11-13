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
    User4,
    User5,
    User6,
    User7,
    User8,
    User9,
    User10,
    User11,
    User12,
    User13,
    User14,
    User15,
    // The rest of Painter's channel types. Declared after the user channels so the order of
    // the earlier kinds (and with it every sorted map of channels) stays as it was.
    AnisotropyAngle,
    AnisotropyLevel,
    BlendingMask,
    CoatColor,
    CoatNormal,
    CoatOpacity,
    CoatRoughness,
    CoatSpecularLevel,
    Diffuse,
    Displacement,
    Glossiness,
    Ior,
    Reflection,
    Scattering,
    ScatteringColor,
    SheenColor,
    SheenOpacity,
    SheenRoughness,
    SpecularColor,
    SpecularEdgeColor,
    Translucency,
    Transmissive,
}

impl ChannelKind {
    pub const ALL: [ChannelKind; 47] = [
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
        ChannelKind::User4,
        ChannelKind::User5,
        ChannelKind::User6,
        ChannelKind::User7,
        ChannelKind::User8,
        ChannelKind::User9,
        ChannelKind::User10,
        ChannelKind::User11,
        ChannelKind::User12,
        ChannelKind::User13,
        ChannelKind::User14,
        ChannelKind::User15,
        ChannelKind::AnisotropyAngle,
        ChannelKind::AnisotropyLevel,
        ChannelKind::BlendingMask,
        ChannelKind::CoatColor,
        ChannelKind::CoatNormal,
        ChannelKind::CoatOpacity,
        ChannelKind::CoatRoughness,
        ChannelKind::CoatSpecularLevel,
        ChannelKind::Diffuse,
        ChannelKind::Displacement,
        ChannelKind::Glossiness,
        ChannelKind::Ior,
        ChannelKind::Reflection,
        ChannelKind::Scattering,
        ChannelKind::ScatteringColor,
        ChannelKind::SheenColor,
        ChannelKind::SheenOpacity,
        ChannelKind::SheenRoughness,
        ChannelKind::SpecularColor,
        ChannelKind::SpecularEdgeColor,
        ChannelKind::Translucency,
        ChannelKind::Transmissive,
    ];

    /// Painter's sixteen user channels, User0 to User15.
    pub const USER: [ChannelKind; 16] = [
        ChannelKind::User0,
        ChannelKind::User1,
        ChannelKind::User2,
        ChannelKind::User3,
        ChannelKind::User4,
        ChannelKind::User5,
        ChannelKind::User6,
        ChannelKind::User7,
        ChannelKind::User8,
        ChannelKind::User9,
        ChannelKind::User10,
        ChannelKind::User11,
        ChannelKind::User12,
        ChannelKind::User13,
        ChannelKind::User14,
        ChannelKind::User15,
    ];

    /// True for User0 to User15 (data channels the user can rename).
    pub fn is_user(self) -> bool {
        Self::USER.contains(&self)
    }

    /// Painter's other channel types (coat, sheen, anisotropy, scattering, the specular /
    /// glossiness workflow's maps...). They are painted, stored and exported like any channel;
    /// the viewport shader does not read them.
    pub const EXTENDED: [ChannelKind; 22] = [
        ChannelKind::AnisotropyAngle,
        ChannelKind::AnisotropyLevel,
        ChannelKind::BlendingMask,
        ChannelKind::CoatColor,
        ChannelKind::CoatNormal,
        ChannelKind::CoatOpacity,
        ChannelKind::CoatRoughness,
        ChannelKind::CoatSpecularLevel,
        ChannelKind::Diffuse,
        ChannelKind::Displacement,
        ChannelKind::Glossiness,
        ChannelKind::Ior,
        ChannelKind::Reflection,
        ChannelKind::Scattering,
        ChannelKind::ScatteringColor,
        ChannelKind::SheenColor,
        ChannelKind::SheenOpacity,
        ChannelKind::SheenRoughness,
        ChannelKind::SpecularColor,
        ChannelKind::SpecularEdgeColor,
        ChannelKind::Translucency,
        ChannelKind::Transmissive,
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
            ChannelKind::CoatColor | ChannelKind::CoatNormal | ChannelKind::Diffuse | ChannelKind::ScatteringColor | ChannelKind::SheenColor | ChannelKind::SpecularColor | ChannelKind::SpecularEdgeColor | ChannelKind::Transmissive => 3,
            _ => 1,
        }
    }

    /// True for channels holding perceptual color (sRGB-encoded on import/export).
    pub fn is_color(self) -> bool {
        matches!(self, ChannelKind::BaseColor | ChannelKind::Emissive | ChannelKind::CoatColor | ChannelKind::Diffuse | ChannelKind::ScatteringColor | ChannelKind::SheenColor | ChannelKind::SpecularColor | ChannelKind::SpecularEdgeColor | ChannelKind::Transmissive)
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
            ChannelKind::CoatColor | ChannelKind::ScatteringColor | ChannelKind::SheenColor | ChannelKind::SpecularEdgeColor => ChannelValue::rgb(1.0, 1.0, 1.0),
            ChannelKind::CoatNormal => ChannelValue::rgb(0.5, 0.5, 1.0),
            ChannelKind::CoatRoughness => ChannelValue::scalar(0.1),
            ChannelKind::CoatSpecularLevel | ChannelKind::Ior | ChannelKind::Reflection | ChannelKind::SheenRoughness => ChannelValue::scalar(0.5),
            ChannelKind::Diffuse => ChannelValue::rgb(0.5, 0.5, 0.5),
            ChannelKind::Glossiness => ChannelValue::scalar(0.7),
            ChannelKind::SpecularColor => ChannelValue::rgb(0.04, 0.04, 0.04),
            ChannelKind::Transmissive => ChannelValue::rgb(0.0, 0.0, 0.0),
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
            ChannelKind::User4 => "User 4",
            ChannelKind::User5 => "User 5",
            ChannelKind::User6 => "User 6",
            ChannelKind::User7 => "User 7",
            ChannelKind::User8 => "User 8",
            ChannelKind::User9 => "User 9",
            ChannelKind::User10 => "User 10",
            ChannelKind::User11 => "User 11",
            ChannelKind::User12 => "User 12",
            ChannelKind::User13 => "User 13",
            ChannelKind::User14 => "User 14",
            ChannelKind::User15 => "User 15",
            ChannelKind::AnisotropyAngle => "Anisotropy Angle",
            ChannelKind::AnisotropyLevel => "Anisotropy Level",
            ChannelKind::BlendingMask => "Blending Mask",
            ChannelKind::CoatColor => "Coat Color",
            ChannelKind::CoatNormal => "Coat Normal",
            ChannelKind::CoatOpacity => "Coat Opacity",
            ChannelKind::CoatRoughness => "Coat Roughness",
            ChannelKind::CoatSpecularLevel => "Coat Specular Level",
            ChannelKind::Diffuse => "Diffuse",
            ChannelKind::Displacement => "Displacement",
            ChannelKind::Glossiness => "Glossiness",
            ChannelKind::Ior => "IOR",
            ChannelKind::Reflection => "Reflection",
            ChannelKind::Scattering => "Scattering",
            ChannelKind::ScatteringColor => "Scattering Color",
            ChannelKind::SheenColor => "Sheen Color",
            ChannelKind::SheenOpacity => "Sheen Opacity",
            ChannelKind::SheenRoughness => "Sheen Roughness",
            ChannelKind::SpecularColor => "Specular",
            ChannelKind::SpecularEdgeColor => "Specular Edge Color",
            ChannelKind::Translucency => "Translucency",
            ChannelKind::Transmissive => "Transmissive",
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
            ChannelKind::User4 => "u4",
            ChannelKind::User5 => "u5",
            ChannelKind::User6 => "u6",
            ChannelKind::User7 => "u7",
            ChannelKind::User8 => "u8",
            ChannelKind::User9 => "u9",
            ChannelKind::User10 => "u10",
            ChannelKind::User11 => "u11",
            ChannelKind::User12 => "u12",
            ChannelKind::User13 => "u13",
            ChannelKind::User14 => "u14",
            ChannelKind::User15 => "u15",
            ChannelKind::AnisotropyAngle => "aniang",
            ChannelKind::AnisotropyLevel => "anilvl",
            ChannelKind::BlendingMask => "blend",
            ChannelKind::CoatColor => "ccol",
            ChannelKind::CoatNormal => "cnrm",
            ChannelKind::CoatOpacity => "cop",
            ChannelKind::CoatRoughness => "crough",
            ChannelKind::CoatSpecularLevel => "cspec",
            ChannelKind::Diffuse => "diff",
            ChannelKind::Displacement => "disp",
            ChannelKind::Glossiness => "gloss",
            ChannelKind::Ior => "ior",
            ChannelKind::Reflection => "refl",
            ChannelKind::Scattering => "scat",
            ChannelKind::ScatteringColor => "scatc",
            ChannelKind::SheenColor => "shc",
            ChannelKind::SheenOpacity => "shop",
            ChannelKind::SheenRoughness => "shrough",
            ChannelKind::SpecularColor => "specc",
            ChannelKind::SpecularEdgeColor => "spece",
            ChannelKind::Translucency => "transl",
            ChannelKind::Transmissive => "transm",
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
            ChannelKind::User4 => "user4",
            ChannelKind::User5 => "user5",
            ChannelKind::User6 => "user6",
            ChannelKind::User7 => "user7",
            ChannelKind::User8 => "user8",
            ChannelKind::User9 => "user9",
            ChannelKind::User10 => "user10",
            ChannelKind::User11 => "user11",
            ChannelKind::User12 => "user12",
            ChannelKind::User13 => "user13",
            ChannelKind::User14 => "user14",
            ChannelKind::User15 => "user15",
            ChannelKind::AnisotropyAngle => "anisotropyangle",
            ChannelKind::AnisotropyLevel => "anisotropylevel",
            ChannelKind::BlendingMask => "blendingmask",
            ChannelKind::CoatColor => "coatcolor",
            ChannelKind::CoatNormal => "coatnormal",
            ChannelKind::CoatOpacity => "coatopacity",
            ChannelKind::CoatRoughness => "coatroughness",
            ChannelKind::CoatSpecularLevel => "coatspecularlevel",
            ChannelKind::Diffuse => "diffuse",
            ChannelKind::Displacement => "displacement",
            ChannelKind::Glossiness => "glossiness",
            ChannelKind::Ior => "ior",
            ChannelKind::Reflection => "reflection",
            ChannelKind::Scattering => "scattering",
            ChannelKind::ScatteringColor => "scatteringcolor",
            ChannelKind::SheenColor => "sheencolor",
            ChannelKind::SheenOpacity => "sheenopacity",
            ChannelKind::SheenRoughness => "sheenroughness",
            ChannelKind::SpecularColor => "specularcolor",
            ChannelKind::SpecularEdgeColor => "specularedgecolor",
            ChannelKind::Translucency => "translucency",
            ChannelKind::Transmissive => "transmissive",
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
    fn sixteen_user_channels_like_painter() {
        assert_eq!(ChannelKind::USER.len(), 16);
        assert!(ChannelKind::USER.iter().all(|k| k.is_user() && k.components() == 1 && !k.is_color()));
        assert_eq!(ChannelKind::User15.key(), "user15");
        assert_eq!(ChannelKind::User15.display_name(), "User 15");
        assert!(!ChannelKind::Opacity.is_user());
        assert_eq!(ChannelKind::ALL.iter().filter(|k| k.is_user()).count(), 16);
    }

    #[test]
    fn extended_channel_types_have_unique_names_and_painters_colour_management() {
        use std::collections::BTreeSet;
        assert_eq!(ChannelKind::ALL.len(), 9 + 16 + ChannelKind::EXTENDED.len());
        assert_eq!(ChannelKind::EXTENDED.len(), 22);
        for names in [ChannelKind::ALL.map(ChannelKind::key), ChannelKind::ALL.map(ChannelKind::display_name), ChannelKind::ALL.map(ChannelKind::short_name)] {
            assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), ChannelKind::ALL.len(), "{names:?}");
        }
        assert!(ChannelKind::ALL.iter().all(|k| k.key().chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())), "keys go into file names");
        // Painter's colour-managed channel types; every other one is data.
        let colour: Vec<ChannelKind> = ChannelKind::EXTENDED.into_iter().filter(|k| k.is_color()).collect();
        assert_eq!(colour, [ChannelKind::CoatColor, ChannelKind::Diffuse, ChannelKind::ScatteringColor, ChannelKind::SheenColor,
            ChannelKind::SpecularColor, ChannelKind::SpecularEdgeColor, ChannelKind::Transmissive]);
        assert!(colour.iter().all(|k| k.components() == 3));
        // A normal map is three components of data, flat by default.
        assert_eq!((ChannelKind::CoatNormal.components(), ChannelKind::CoatNormal.is_color()), (3, false));
        assert_eq!(ChannelKind::CoatNormal.default_value(), ChannelKind::Normal.default_value());
        assert!(ChannelKind::EXTENDED.iter().all(|k| !k.is_user() && !k.is_signed()));
        assert!(ChannelKind::EXTENDED.iter().all(|k| k.default_value().0.iter().all(|v| (0.0..=1.0).contains(v))));
        // Glossiness is the inverse of the default roughness.
        assert!((ChannelKind::Glossiness.default_value().x() + ChannelKind::Roughness.default_value().x() - 1.0).abs() < 1e-6);
        // The two speculars stay apart: the level is the old kind, the colour the new one.
        assert_eq!((ChannelKind::Specular.display_name(), ChannelKind::SpecularColor.display_name()), ("Specular Level", "Specular"));
        assert_eq!(ChannelKind::from_key("specular"), Some(ChannelKind::Specular));
        assert_eq!(ChannelKind::from_key("sheenroughness"), Some(ChannelKind::SheenRoughness));
        // The earlier kinds sort before the new ones, as they did.
        assert!(ChannelKind::User15 < ChannelKind::AnisotropyAngle && ChannelKind::BaseColor < ChannelKind::User0);
    }

    #[test]
    fn component_counts() {
        assert_eq!(ChannelKind::BaseColor.components(), 3);
        assert_eq!(ChannelKind::Normal.components(), 3);
        assert_eq!(ChannelKind::Roughness.components(), 1);
    }
}
