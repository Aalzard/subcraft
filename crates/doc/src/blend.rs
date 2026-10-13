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

    /// Modes meaningful only for the normal channel.
    pub fn is_normal_only(self) -> bool {
        matches!(self, BlendMode::NormalMapCombine | BlendMode::NormalMapDetail | BlendMode::NormalMapInverseDetail)
    }

    /// Modes offered for a given channel / node kind.
    pub fn available(normal_channel: bool, is_folder: bool) -> impl Iterator<Item = BlendMode> {
        Self::ALL.into_iter().filter(move |m| {
            (normal_channel || !m.is_normal_only()) && (is_folder || *m != BlendMode::Passthrough)
        })
    }
}
