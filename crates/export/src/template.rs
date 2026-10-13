//! Export templates: which files to write and how channels pack into their components.

use serde::{Deserialize, Serialize};
use subcraft_core::ChannelKind;
use subcraft_image::FileFormat;

/// Where one output component comes from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Source {
    /// Component `component` of a channel's composite (`invert` = 1 − v, e.g. smoothness).
    Channel { kind: ChannelKind, component: u8, invert: bool },
    /// Tangent-space normal (normal channel combined with height bumps), encoded 0..1.
    Normal { component: u8, directx: bool },
    /// Height remapped for integer formats: `h · 0.5 + 0.5` (float formats store raw height).
    Height,
    Constant { value: f32 },
}

impl Source {
    pub const fn ch(kind: ChannelKind, component: u8) -> Self {
        Source::Channel { kind, component, invert: false }
    }

    /// Channel this source reads, if any.
    pub fn channel(&self) -> Option<ChannelKind> {
        match self {
            Source::Channel { kind, .. } => Some(*kind),
            Source::Normal { .. } => Some(ChannelKind::Normal),
            Source::Height => Some(ChannelKind::Height),
            Source::Constant { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutputMap {
    /// File-name suffix (`{mesh}_{set}_{suffix}.ext`).
    pub suffix: String,
    pub format: FileFormat,
    /// 1 (gray), 3 (RGB) or 4 (RGBA) sources.
    pub components: Vec<Source>,
    /// Encode color-channel sources to sRGB for integer formats.
    pub srgb: bool,
}

impl OutputMap {
    fn new(suffix: &str, format: FileFormat, components: Vec<Source>, srgb: bool) -> Self {
        Self { suffix: suffix.into(), format, components, srgb }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportTemplate {
    pub name: String,
    pub maps: Vec<OutputMap>,
}

fn rgb(kind: ChannelKind) -> Vec<Source> {
    (0..3).map(|c| Source::ch(kind, c)).collect()
}

fn normal(directx: bool) -> Vec<Source> {
    (0..3).map(|c| Source::Normal { component: c, directx }).collect()
}

impl ExportTemplate {
    /// Separate files per channel (metal/roughness workflow).
    pub fn pbr_metal_rough() -> Self {
        use ChannelKind::*;
        Self {
            name: "PBR Metallic Roughness".into(),
            maps: vec![
                OutputMap::new("BaseColor", FileFormat::Png8, rgb(BaseColor), true),
                OutputMap::new("Roughness", FileFormat::Png8, vec![Source::ch(Roughness, 0)], false),
                OutputMap::new("Metallic", FileFormat::Png8, vec![Source::ch(Metallic, 0)], false),
                OutputMap::new("Normal", FileFormat::Png8, normal(false), false),
                OutputMap::new("Height", FileFormat::Png16, vec![Source::Height], false),
                OutputMap::new("Emissive", FileFormat::Png8, rgb(Emissive), true),
                OutputMap::new("AO", FileFormat::Png8, vec![Source::ch(AmbientOcclusion, 0)], false),
            ],
        }
    }

    /// Unreal Engine: packed Occlusion/Roughness/Metallic, DirectX normals.
    pub fn unreal() -> Self {
        use ChannelKind::*;
        Self {
            name: "Unreal Engine (Packed)".into(),
            maps: vec![
                OutputMap::new("BaseColor", FileFormat::Png8, rgb(BaseColor), true),
                OutputMap::new("Normal", FileFormat::Png8, normal(true), false),
                OutputMap::new(
                    "OcclusionRoughnessMetallic",
                    FileFormat::Png8,
                    vec![Source::ch(AmbientOcclusion, 0), Source::ch(Roughness, 0), Source::ch(Metallic, 0)],
                    false,
                ),
                OutputMap::new("Emissive", FileFormat::Png8, rgb(Emissive), true),
            ],
        }
    }

    /// Unity HDRP: mask map R metallic, G occlusion, B detail mask (unused = 0), A smoothness.
    pub fn unity_hdrp() -> Self {
        use ChannelKind::*;
        Self {
            name: "Unity HDRP (Mask Map)".into(),
            maps: vec![
                OutputMap::new("BaseColor", FileFormat::Png8, rgb(BaseColor), true),
                OutputMap::new("Normal", FileFormat::Png8, normal(false), false),
                OutputMap::new(
                    "MaskMap",
                    FileFormat::Png8,
                    vec![
                        Source::ch(Metallic, 0),
                        Source::ch(AmbientOcclusion, 0),
                        Source::Constant { value: 0.0 },
                        Source::Channel { kind: Roughness, component: 0, invert: true },
                    ],
                    false,
                ),
                OutputMap::new("Emissive", FileFormat::Png8, rgb(Emissive), true),
            ],
        }
    }

    /// glTF 2.0: occlusion (R) + roughness (G) + metallic (B) in one texture, OpenGL normals.
    pub fn gltf() -> Self {
        use ChannelKind::*;
        Self {
            name: "glTF 2.0".into(),
            maps: vec![
                OutputMap::new("BaseColor", FileFormat::Png8, rgb(BaseColor), true),
                OutputMap::new("Normal", FileFormat::Png8, normal(false), false),
                OutputMap::new(
                    "OcclusionRoughnessMetallic",
                    FileFormat::Png8,
                    vec![Source::ch(AmbientOcclusion, 0), Source::ch(Roughness, 0), Source::ch(Metallic, 0)],
                    false,
                ),
                OutputMap::new("Emissive", FileFormat::Png8, rgb(Emissive), true),
            ],
        }
    }

    pub fn builtin() -> Vec<ExportTemplate> {
        vec![Self::pbr_metal_rough(), Self::unreal(), Self::unity_hdrp(), Self::gltf()]
    }
}
