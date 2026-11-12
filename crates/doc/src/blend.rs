//! Blend mode vocabulary (math lives in the compositor).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    Normal,
    /// Folders only: children composite directly onto the stack below.
    Passthrough,
    /// Channel ignored for this layer.
    Disable,
    /// Overwrites below, ignoring content transparency (mask/opacity still apply).
    Replace,
    Multiply,
    Divide,
    InverseDivide,
    Darken,
    Lighten,
    LinearDodge,
    Subtract,
    InverseSubtract,
    Difference,
    Exclusion,
    SignedAddition,
    Overlay,
    Screen,
    LinearBurn,
    ColorBurn,
    ColorDodge,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    Tint,
    Saturation,
    Color,
    Value,
    NormalMapCombine,
    NormalMapDetail,
    NormalMapInverseDetail,
}

impl BlendMode {
    pub const ALL: [BlendMode; 32] = [
        BlendMode::Normal,
        BlendMode::Passthrough,
        BlendMode::Disable,
        BlendMode::Replace,
        BlendMode::Multiply,
        BlendMode::Divide,
        BlendMode::InverseDivide,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::LinearDodge,
        BlendMode::Subtract,
        BlendMode::InverseSubtract,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::SignedAddition,
        BlendMode::Overlay,
        BlendMode::Screen,
        BlendMode::LinearBurn,
        BlendMode::ColorBurn,
        BlendMode::ColorDodge,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::Tint,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Value,
        BlendMode::NormalMapCombine,
        BlendMode::NormalMapDetail,
        BlendMode::NormalMapInverseDetail,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Passthrough => "Passthrough",
            BlendMode::Disable => "Disable",
            BlendMode::Replace => "Replace",
            BlendMode::Multiply => "Multiply",
            BlendMode::Divide => "Divide",
            BlendMode::InverseDivide => "Inverse Divide",
            BlendMode::Darken => "Darken (Min)",
            BlendMode::Lighten => "Lighten (Max)",
            BlendMode::LinearDodge => "Linear Dodge (Add)",
            BlendMode::Subtract => "Subtract",
            BlendMode::InverseSubtract => "Inverse Subtract",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::SignedAddition => "Signed Addition (AddSub)",
            BlendMode::Overlay => "Overlay",
            BlendMode::Screen => "Screen",
            BlendMode::LinearBurn => "Linear Burn",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::HardLight => "Hard Light",
            BlendMode::VividLight => "Vivid Light",
            BlendMode::LinearLight => "Linear Light",
            BlendMode::PinLight => "Pin Light",
            BlendMode::Tint => "Tint",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Value => "Value",
            BlendMode::NormalMapCombine => "Normal Map Combine",
            BlendMode::NormalMapDetail => "Normal Map Detail",
            BlendMode::NormalMapInverseDetail => "Normal Map Inverse Detail",
        }
    }

    /// Compact label for the layer-stack row.
    /// Group of the mode in the blending dropdown; a separator is drawn where it changes.
    /// Painter's order: basic, multiply, add / subtract, overlay, light, HSV, normal map.
    pub fn group(self) -> u8 {
        use BlendMode::*;
        match self {
            Normal | Passthrough | Disable | Replace => 0,
            Multiply | Divide | InverseDivide | Darken | Lighten => 1,
            LinearDodge | Subtract | InverseSubtract | Difference | Exclusion | SignedAddition => 2,
            Overlay | Screen | LinearBurn | ColorBurn | ColorDodge => 3,
            SoftLight | HardLight | VividLight | LinearLight | PinLight => 4,
            Tint | Saturation | Color | Value => 5,
            NormalMapCombine | NormalMapDetail | NormalMapInverseDetail => 6,
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Norm",
            BlendMode::Passthrough => "Pass",
            BlendMode::Disable => "Dis",
            BlendMode::Replace => "Repl",
            BlendMode::Multiply => "Mul",
            BlendMode::Divide => "Div",
            BlendMode::InverseDivide => "IDiv",
            BlendMode::Darken => "Min",
            BlendMode::Lighten => "Max",
            BlendMode::LinearDodge => "Add",
            BlendMode::Subtract => "Sub",
            BlendMode::InverseSubtract => "ISub",
            BlendMode::Difference => "Diff",
            BlendMode::Exclusion => "Excl",
            BlendMode::SignedAddition => "AddSub",
            BlendMode::Overlay => "Ovr",
            BlendMode::Screen => "Scr",
            BlendMode::LinearBurn => "LBrn",
            BlendMode::ColorBurn => "CBrn",
            BlendMode::ColorDodge => "CDdg",
            BlendMode::SoftLight => "Soft",
            BlendMode::HardLight => "Hard",
            BlendMode::VividLight => "Vivid",
            BlendMode::LinearLight => "LLgt",
            BlendMode::PinLight => "Pin",
            BlendMode::Tint => "Tint",
            BlendMode::Saturation => "Sat",
            BlendMode::Color => "Col",
            BlendMode::Value => "Val",
            BlendMode::NormalMapCombine => "NMC",
            BlendMode::NormalMapDetail => "NMD",
            BlendMode::NormalMapInverseDetail => "NMID",
        }
    }

    /// Blend mode a new layer gets for `kind`, as in Painter: folders pass through, Height adds
    /// (Linear Dodge), Normal uses Normal Map Detail, Ambient Occlusion multiplies, the rest is Normal.
    pub fn default_for(kind: subcraft_core::ChannelKind, folder: bool) -> Self {
        use subcraft_core::ChannelKind as K;
        match kind {
            _ if folder => BlendMode::Passthrough,
            K::Height => BlendMode::LinearDodge,
            K::Normal => BlendMode::NormalMapDetail,
            K::AmbientOcclusion => BlendMode::Multiply,
            _ => BlendMode::Normal,
        }
    }

    /// Modes meaningful only for the normal channel.
    pub fn is_normal_only(self) -> bool {
        matches!(self, BlendMode::NormalMapCombine | BlendMode::NormalMapDetail | BlendMode::NormalMapInverseDetail)
    }

    /// Modes offered for a layer's channel. Passthrough is offered on every layer (Painter: on a
    /// paint or fill layer its effects then process the stack below); `is_folder` is kept for
    /// callers that list modes for something else, such as effects.
    pub fn available(normal_channel: bool, is_folder: bool) -> impl Iterator<Item = BlendMode> {
        let _ = is_folder;
        Self::ALL.into_iter().filter(move |m| normal_channel || !m.is_normal_only())
    }

    /// Modes offered for an effect row (no Passthrough: an effect already reads what is below).
    pub fn available_for_effect(normal_channel: bool) -> impl Iterator<Item = BlendMode> {
        Self::available(normal_channel, false).filter(|m| *m != BlendMode::Passthrough)
    }
}

#[cfg(test)]
mod tests {
    use super::BlendMode;

    #[test]
    fn the_mode_list_is_ordered_in_seven_groups() {
        let groups: Vec<u8> = BlendMode::ALL.iter().map(|m| m.group()).collect();
        assert!(groups.windows(2).all(|w| w[1] == w[0] || w[1] == w[0] + 1), "groups are contiguous and in order: {groups:?}");
        assert_eq!((groups[0], *groups.last().unwrap()), (0, 6));
        assert_eq!(BlendMode::Replace.group(), BlendMode::Normal.group());
        assert_ne!(BlendMode::Multiply.group(), BlendMode::Replace.group());
        assert_eq!(BlendMode::LinearDodge.group(), BlendMode::Subtract.group());
        // Every normal-map mode, and nothing else, is in the last group.
        assert!(BlendMode::ALL.iter().all(|m| (m.group() == 6) == m.is_normal_only()));
    }
}
