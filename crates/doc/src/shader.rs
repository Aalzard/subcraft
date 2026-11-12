//! Shader instances (Painter's Shader Settings): named sets of shader parameters that texture
//! sets point to. Several texture sets can share one instance; a project without any has the
//! implicit "Main shader" with the default parameters. Plain data saved with the project settings.

use serde::{Deserialize, Serialize};
use subcraft_core::TextureSetId;

/// Identity of a shader instance.
pub type ShaderInstanceId = uuid::Uuid;

/// Name of the instance every texture set starts with.
pub const MAIN_SHADER: &str = "Main shader";

/// The common parameters of Painter's PBR shaders that SubCraft's viewport shader implements.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShaderParams {
    /// How much the ambient occlusion map darkens the environment light, 0..1.
    pub ao_intensity: f32,
    /// Multiplier of the Emissive channel, 0..100.
    pub emissive_intensity: f32,
    /// Fades reflections that a normal map tilts below the surface's horizon, 0 (off) .. 2.
    pub horizon_fading: f32,
}

impl Default for ShaderParams {
    fn default() -> Self {
        Self { ao_intensity: 0.75, emissive_intensity: 1.0, horizon_fading: 1.3 }
    }
}

impl ShaderParams {
    pub const AO_RANGE: std::ops::RangeInclusive<f32> = 0.0..=1.0;
    pub const EMISSIVE_RANGE: std::ops::RangeInclusive<f32> = 0.0..=100.0;
    pub const HORIZON_RANGE: std::ops::RangeInclusive<f32> = 0.0..=2.0;

    /// The parameters in range; a non-finite value (a damaged file) takes the default.
    pub fn clamped(self) -> Self {
        let d = Self::default();
        let fit = |v: f32, r: &std::ops::RangeInclusive<f32>, def: f32| if v.is_finite() { v.clamp(*r.start(), *r.end()) } else { def };
        Self {
            ao_intensity: fit(self.ao_intensity, &Self::AO_RANGE, d.ao_intensity),
            emissive_intensity: fit(self.emissive_intensity, &Self::EMISSIVE_RANGE, d.emissive_intensity),
            horizon_fading: fit(self.horizon_fading, &Self::HORIZON_RANGE, d.horizon_fading),
        }
    }
}

/// A named shader instance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShaderInstance {
    pub id: uuid::Uuid,
    pub name: String,
    pub params: ShaderParams,
}

impl Default for ShaderInstance {
    fn default() -> Self {
        Self::main()
    }
}

impl ShaderInstance {
    /// The implicit instance of a project that never made one (stable id, so it needs no saving).
    pub fn main() -> Self {
        Self { id: uuid::Uuid::nil(), name: MAIN_SHADER.into(), params: ShaderParams::default() }
    }
}

/// Which instance a texture set uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShaderAssignment {
    pub texture_set: TextureSetId,
    pub instance: uuid::Uuid,
}

/// `wanted` (trimmed), or with " (2)", " (3)"... appended so it differs from every name in
/// `taken` (Painter keeps instance names unique). An empty name becomes "Shader".
pub fn unique_name<'a>(wanted: &str, taken: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let base = match wanted.trim() {
        "" => "Shader",
        w => w,
    };
    let free = |name: &str| !taken.clone().into_iter().any(|t| t == name);
    if free(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} ({n})")).find(|n| free(n)).expect("a free name exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_painter_and_bad_values_are_clamped() {
        let d = ShaderParams::default();
        assert_eq!((d.ao_intensity, d.emissive_intensity, d.horizon_fading), (0.75, 1.0, 1.3));
        let wild = ShaderParams { ao_intensity: 3.0, emissive_intensity: f32::NAN, horizon_fading: -1.0 }.clamped();
        assert_eq!((wild.ao_intensity, wild.emissive_intensity, wild.horizon_fading), (1.0, 1.0, 0.0));
        let read: ShaderInstance = serde_json::from_str(r#"{"name":"Glass","params":{"emissive_intensity":5}}"#).unwrap();
        assert_eq!((read.name.as_str(), read.params.emissive_intensity, read.params.ao_intensity), ("Glass", 5.0, 0.75));
        assert_eq!(ShaderInstance::main().id, uuid::Uuid::nil());
    }

    #[test]
    fn instance_names_stay_unique() {
        let taken = ["Main shader", "Main shader (2)", "Skin"];
        assert_eq!(unique_name("Skin", taken), "Skin (2)");
        assert_eq!(unique_name("Main shader", taken), "Main shader (3)");
        assert_eq!(unique_name("  Metal ", taken), "Metal");
        assert_eq!(unique_name("", taken), "Shader");
    }
}
