//! Export templates (presets): which files to write and how channels pack into their components.
//!
//! The built-in templates are read-only; user presets are the same struct, saved as JSON by
//! [`crate::preset`]. Every option added for the preset editor is optional and absent from the
//! built-ins, so they keep writing exactly the files they wrote before.

use serde::{Deserialize, Serialize};
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_image::FileFormat;

use crate::Padding;

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
    /// Component of whatever the 2D view shows (`ExportSettings::view_2d`): single-component
    /// sources read as gray, values clamped to 0..1, color channels sRGB-encoded like the display.
    View2d { component: u8 },
    /// Grayscale conversion of a channel: Rec. 709 luminance of its linear RGB (a one-component
    /// channel reads as itself).
    Gray { kind: ChannelKind, invert: bool },
    /// Component of a baked mesh map (Ambient Occlusion, Curvature, Thickness...). Maps not baked
    /// read their default value.
    MeshMap { kind: MeshMapKind, component: u8, invert: bool },
    /// Effective opacity (issue #145): the Opacity channel (1 where the set has none) times the
    /// Base Color stack's coverage (below 1 only in a transparent texture set). What the
    /// "+ Opacity" presets put in Base Color's alpha and in their separate Opacity map.
    Alpha,
    /// Component of one of Painter's converted maps (Diffuse, Specular, Glossiness...), computed
    /// from the metal / roughness channels.
    Converted { map: Converted, component: u8 },
}

/// Painter's "converted maps": values derived from the metallic / roughness channels for
/// renderers and engines that want another workflow. All values are linear; the colour ones
/// (Diffuse, Specular, Reflection) are sRGB-encoded in 8/16-bit sRGB maps like Base Color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Converted {
    /// Base color × (1 − metallic): the diffuse albedo of a specular / glossiness material.
    Diffuse,
    /// Reflectance at normal incidence as a colour: f0 for dielectrics, base color for metals,
    /// blended by metallic.
    Specular,
    /// Reflection colour for renderers whose reflection uses Fresnel from an IOR (V-Ray, Corona):
    /// white for dielectrics, base color for metals, blended by metallic.
    Reflection,
    /// 1 − roughness.
    Glossiness,
    /// (1 − roughness)².
    GlossinessSquared,
    /// Ambient Occlusion channel × baked ambient occlusion (either alone when the other is absent).
    MixedAo,
    /// Dielectric reflectance at normal incidence from the Specular level channel: 0.08 × level
    /// (level 0.5 = 0.04, an IOR of 1.5).
    F0,
    /// 1 / IOR for that reflectance: (1 − √f0) / (1 + √f0) (level 0.5 = 0.667).
    InverseIor,
}

impl Converted {
    pub const ALL: [Converted; 8] = [
        Converted::Diffuse,
        Converted::Specular,
        Converted::Reflection,
        Converted::Glossiness,
        Converted::GlossinessSquared,
        Converted::MixedAo,
        Converted::F0,
        Converted::InverseIor,
    ];

    /// Painter's name for the map.
    pub fn label(self) -> &'static str {
        match self {
            Converted::Diffuse => "Diffuse",
            Converted::Specular => "Specular",
            Converted::Reflection => "Reflection",
            Converted::Glossiness => "Glossiness",
            Converted::GlossinessSquared => "Glossiness squared",
            Converted::MixedAo => "Mixed AO",
            Converted::F0 => "f0",
            Converted::InverseIor => "1/ior",
        }
    }

    pub fn components(self) -> usize {
        if self.is_color() { 3 } else { 1 }
    }

    pub fn is_color(self) -> bool {
        matches!(self, Converted::Diffuse | Converted::Specular | Converted::Reflection)
    }

    /// Channels the map is computed from; it is written for a texture set that has any of them.
    pub fn inputs(self) -> &'static [ChannelKind] {
        use ChannelKind::*;
        match self {
            Converted::Diffuse | Converted::Reflection => &[BaseColor, Metallic],
            Converted::Specular => &[BaseColor, Metallic, Specular],
            Converted::Glossiness | Converted::GlossinessSquared => &[Roughness],
            Converted::MixedAo => &[AmbientOcclusion],
            Converted::F0 | Converted::InverseIor => &[Specular, BaseColor, Metallic, Roughness],
        }
    }
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
            Source::Gray { kind, .. } => Some(*kind),
            Source::Alpha => Some(ChannelKind::Opacity),
            Source::Constant { .. } | Source::View2d { .. } | Source::MeshMap { .. } | Source::Converted { .. } => None,
        }
    }

    /// Short label for lists ("Roughness", "1 - Roughness", "Base Color.R", "Gray(Base Color)").
    pub fn label(&self) -> String {
        const C: [&str; 4] = ["R", "G", "B", "A"];
        let comp = |c: u8| C[(c as usize).min(3)];
        let inv = |inv: bool, s: String| if inv { format!("1 - {s}") } else { s };
        match *self {
            Source::Channel { kind, component, invert } => inv(invert, if kind.components() > 1 { format!("{}.{}", kind.display_name(), comp(component)) } else { kind.display_name().to_string() }),
            Source::Normal { component, directx } => format!("Normal {}.{}", if directx { "DX" } else { "GL" }, comp(component)),
            Source::Height => "Height".into(),
            Source::Constant { value } => format!("{value:.2}"),
            Source::View2d { component } => format!("2D view.{}", comp(component)),
            Source::Gray { kind, invert } => inv(invert, format!("Gray({})", kind.display_name())),
            Source::Alpha => "Alpha (Opacity)".into(),
            Source::MeshMap { kind, component, invert } => inv(invert, if kind.components() > 1 { format!("Baked {}.{}", kind.display_name(), comp(component)) } else { format!("Baked {}", kind.display_name()) }),
            Source::Converted { map, component } => if map.components() > 1 { format!("{}.{}", map.label(), comp(component)) } else { map.label().to_string() },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutputMap {
    /// File-name suffix (`{mesh}_{set}_{suffix}.ext`) and the map's identity in the export
    /// checklist: unique within a template.
    pub suffix: String,
    pub format: FileFormat,
    /// 1 (gray), 2 (gray + alpha), 3 (RGB) or 4 (RGBA) sources.
    pub components: Vec<Source>,
    /// Encode color-channel sources to sRGB for integer formats.
    pub srgb: bool,
    /// File name pattern without extension (`$mesh_$textureSet_$suffix`; see
    /// [`crate::expand_pattern`]). `None` = the classic `{mesh}_{set}_{suffix}` naming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Padding for this map, overriding the template's and the export window's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<Padding>,
    /// Output size in texels (square), overriding the template's; `None` = texture set size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    /// DirectX (`true`) / OpenGL (`false`) normals for this map, overriding the template's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_directx: Option<bool>,
    /// Dither when this map is written at 8 bits ("8 bits + dithering").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dither: Option<bool>,
    /// The first three components shown as one RGB slot (`Some(true)`, Painter's RGB / RGB+A
    /// maps) or as three gray slots (`Some(false)`, R+G+B). `None` = one slot when they read
    /// the R, G and B of the same source. Only the editor uses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rgb_slot: Option<bool>,
}

impl OutputMap {
    pub fn new(suffix: &str, format: FileFormat, components: Vec<Source>, srgb: bool) -> Self {
        Self { suffix: suffix.into(), format, components, srgb, pattern: None, padding: None, size: None, normal_directx: None, dither: None, rgb_slot: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ExportTemplate {
    pub name: String,
    pub maps: Vec<OutputMap>,
    /// Persistent identity of a user preset (UUID); `None` for the built-in templates, which
    /// are identified by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Padding for every map without its own, overriding the export window's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub padding: Option<Padding>,
    /// Output size for every map without its own; `None` = texture set size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    /// Normal map convention for every map without its own; `None` = each source's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_directx: Option<bool>,
}

fn rgb(kind: ChannelKind) -> Vec<Source> {
    (0..3).map(|c| Source::ch(kind, c)).collect()
}

/// Base Color RGB with the effective opacity in alpha.
fn rgba(kind: ChannelKind) -> Vec<Source> {
    let mut c = rgb(kind);
    c.push(Source::Alpha);
    c
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
            ..Default::default()
        }
    }

    /// Unreal Engine: packed Occlusion/Roughness/Metallic, DirectX normals.
    pub fn unreal() -> Self {
        use ChannelKind::*;
        Self {
            name: "Unreal Engine (ORM packed)".into(),
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        }
    }

    /// Unity URP / Built-in Lit: Metallic map (R metallic, A smoothness), a separate Occlusion
    /// map (green channel, shown as gray), OpenGL normals.
    pub fn unity_urp() -> Self {
        use ChannelKind::*;
        Self {
            name: "Unity URP (Metallic + Occlusion)".into(),
            maps: vec![
                OutputMap::new("BaseColor", FileFormat::Png8, rgb(BaseColor), true),
                OutputMap::new("Normal", FileFormat::Png8, normal(false), false),
                OutputMap::new(
                    "MetallicSmoothness",
                    FileFormat::Png8,
                    vec![
                        Source::ch(Metallic, 0),
                        Source::ch(Metallic, 0),
                        Source::ch(Metallic, 0),
                        Source::Channel { kind: Roughness, component: 0, invert: true },
                    ],
                    false,
                ),
                OutputMap::new("Occlusion", FileFormat::Png8, vec![Source::ch(AmbientOcclusion, 0); 3], false),
                OutputMap::new("Emissive", FileFormat::Png8, rgb(Emissive), true),
                OutputMap::new("Height", FileFormat::Png16, vec![Source::Height], false),
            ],
            ..Default::default()
        }
    }

    /// Godot 4 StandardMaterial3D: one ORM texture (R occlusion, G roughness, B metallic),
    /// OpenGL normals, plus Emission and Height.
    pub fn godot() -> Self {
        use ChannelKind::*;
        Self {
            name: "Godot 4 (ORM)".into(),
            maps: vec![
                OutputMap::new("BaseColor", FileFormat::Png8, rgb(BaseColor), true),
                OutputMap::new("Normal", FileFormat::Png8, normal(false), false),
                OutputMap::new(
                    "ORM",
                    FileFormat::Png8,
                    vec![Source::ch(AmbientOcclusion, 0), Source::ch(Roughness, 0), Source::ch(Metallic, 0)],
                    false,
                ),
                OutputMap::new("Emissive", FileFormat::Png8, rgb(Emissive), true),
                OutputMap::new("Height", FileFormat::Png16, vec![Source::Height], false),
            ],
            ..Default::default()
        }
    }

    /// The current 2D view as one RGB texture per texture set (Painter's "2D View" export).
    /// One raw baked mesh map, read through the 2D-view source (set `ExportSettings::view_2d` to
    /// `View2d::MeshMap(kind)`). The suffix `baked_<key>` keeps raw maps apart from material
    /// textures and from each other; scalar maps are one grey channel, height and position 16-bit.
    pub fn baked_map(kind: subcraft_core::MeshMapKind) -> Self {
        use subcraft_core::MeshMapKind as M;
        let format = if matches!(kind, M::Height | M::Position) { FileFormat::Png16 } else { FileFormat::Png8 };
        let components = (0..kind.components() as u8).map(|component| Source::View2d { component }).collect();
        Self { name: format!("Baked {}", kind.display_name()), maps: vec![OutputMap::new(&Self::baked_suffix(kind), format, components, false)], ..Default::default() }
    }

    /// File suffix of a raw baked map (`baked_ambient_occlusion`, `baked_normal`, ...).
    pub fn baked_suffix(kind: subcraft_core::MeshMapKind) -> String {
        let key = kind.key();
        format!("baked_{}", key.strip_suffix("_base").unwrap_or(key))
    }

    pub fn view_2d() -> Self {
        Self { name: "2D View".into(), maps: vec![OutputMap::new("View2D", FileFormat::Png8, (0..3).map(|component| Source::View2d { component }).collect(), true)], ..Default::default() }
    }

    /// Roblox SurfaceAppearance: ColorMap, NormalMap, MetalnessMap, RoughnessMap, EmissiveMask,
    /// all separate 8-bit files (Roblox has no packed map). Switch the normal convention in the
    /// export settings if your Roblox version expects the other one.
    pub fn roblox() -> Self {
        use ChannelKind::*;
        Self {
            name: "Roblox (SurfaceAppearance)".into(),
            maps: vec![
                OutputMap::new("ColorMap", FileFormat::Png8, rgb(BaseColor), true),
                OutputMap::new("NormalMap", FileFormat::Png8, normal(false), false),
                OutputMap::new("MetalnessMap", FileFormat::Png8, vec![Source::ch(Metallic, 0)], false),
                OutputMap::new("RoughnessMap", FileFormat::Png8, vec![Source::ch(Roughness, 0)], false),
                OutputMap::new("EmissiveMask", FileFormat::Png8, rgb(Emissive), false),
            ],
            ..Default::default()
        }
    }

    /// Sketchfab / web viewers: separate base color, normal, roughness, metalness, AO and emission.
    pub fn sketchfab() -> Self {
        use ChannelKind::*;
        Self {
            name: "Sketchfab (separate maps)".into(),
            maps: vec![
                OutputMap::new("BaseColor", FileFormat::Png8, rgb(BaseColor), true),
                OutputMap::new("Normal", FileFormat::Png8, normal(false), false),
                OutputMap::new("Roughness", FileFormat::Png8, vec![Source::ch(Roughness, 0)], false),
                OutputMap::new("Metalness", FileFormat::Png8, vec![Source::ch(Metallic, 0)], false),
                OutputMap::new("AO", FileFormat::Png8, vec![Source::ch(AmbientOcclusion, 0)], false),
                OutputMap::new("Emission", FileFormat::Png8, rgb(Emissive), true),
            ],
            ..Default::default()
        }
    }

    /// A copy of `self` with alpha (issue #145): Base Color gains the effective opacity as its
    /// fourth component (JPEG maps switch to PNG) and, when `opacity_map`, a separate grey
    /// "Opacity" map follows it. Painter's "... + Opacity" / "with alpha" presets.
    fn with_alpha(mut self, name: &str, opacity_map: bool) -> Self {
        self.name = name.into();
        let color = self.maps.iter().position(|m| m.components.len() == 3 && m.components.iter().all(|s| matches!(s, Source::Channel { kind: ChannelKind::BaseColor, .. })));
        if let Some(i) = color {
            let m = &mut self.maps[i];
            m.components = rgba(ChannelKind::BaseColor);
            if m.format == FileFormat::Jpeg {
                m.format = FileFormat::Png8;
            }
            if opacity_map {
                self.maps.insert(i + 1, OutputMap::new("Opacity", FileFormat::Png8, vec![Source::Alpha], false));
            }
        }
        self
    }

    /// Painter's "PBR Metallic Roughness" with an alpha channel in Base Color and a separate
    /// Opacity map.
    pub fn pbr_metal_rough_opacity() -> Self {
        Self::pbr_metal_rough().with_alpha("PBR Metallic Roughness + Opacity", true)
    }

    /// Unreal Engine with opacity in Base Color's alpha (Translucent / Masked materials) and a
    /// separate Opacity map for the Opacity / Opacity Mask pin.
    pub fn unreal_opacity() -> Self {
        Self::unreal().with_alpha("Unreal Engine (ORM packed) + Opacity", true)
    }

    /// Unity HDRP / URP read transparency from Base Map alpha (Surface Type Transparent or Alpha
    /// Clipping).
    pub fn unity_hdrp_alpha() -> Self {
        Self::unity_hdrp().with_alpha("Unity HDRP (Mask Map) + Alpha", false)
    }

    pub fn unity_urp_alpha() -> Self {
        Self::unity_urp().with_alpha("Unity URP (Metallic + Occlusion) + Alpha", false)
    }

    /// Godot 4: Albedo alpha drives Transparency (Alpha / Alpha Scissor / Alpha Hash).
    pub fn godot_alpha() -> Self {
        Self::godot().with_alpha("Godot 4 (ORM) + Alpha", false)
    }

    /// glTF 2.0: `baseColorTexture` alpha with `alphaMode` BLEND or MASK.
    pub fn gltf_alpha() -> Self {
        Self::gltf().with_alpha("glTF 2.0 + Alpha", false)
    }

    /// Whether this is one of the read-only built-in templates.
    pub fn is_builtin(&self) -> bool {
        self.id.is_none()
    }

    /// Stable key stored in project settings: the UUID of a user preset, the name of a built-in.
    pub fn key(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.name)
    }

    /// Problems that stop this template from exporting (empty = fine).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.name.trim().is_empty() {
            out.push("the preset needs a name".to_string());
        }
        if self.size.is_some_and(|s| !(1..=16384).contains(&s)) {
            out.push("the preset size must be 1..16384".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for m in &self.maps {
            if m.suffix.is_empty() || crate::sanitize(&m.suffix) != m.suffix {
                out.push(format!("map suffix \"{}\" may only use letters, digits, - and _", m.suffix));
            }
            if !seen.insert(m.suffix.as_str()) {
                out.push(format!("two maps share the suffix \"{}\"", m.suffix));
            }
            if !(1..=4).contains(&m.components.len()) {
                out.push(format!("{}: a map has 1 to 4 components", m.suffix));
            }
            if m.size.is_some_and(|s| !(1..=16384).contains(&s)) {
                out.push(format!("{}: size must be 1..16384", m.suffix));
            }
            if let Some(p) = &m.pattern
                && crate::expand_pattern(p, "m", "s", None, &m.suffix).is_empty()
            {
                out.push(format!("{}: the file name pattern is empty", m.suffix));
            }
        }
        out
    }

    /// The built-in templates: SubCraft's first nine (their order is kept: `--export-template`
    /// indexes it), then Painter's predefined and default templates
    /// ([`crate::builtin_presets`]).
    pub fn builtin() -> Vec<ExportTemplate> {
        let mut all = vec![
            Self::pbr_metal_rough(),
            Self::unreal(),
            Self::unity_hdrp(),
            Self::gltf(),
            Self::unity_urp(),
            Self::godot(),
            Self::roblox(),
            Self::sketchfab(),
            Self::view_2d(),
            Self::pbr_metal_rough_opacity(),
            Self::unreal_opacity(),
            Self::unity_hdrp_alpha(),
            Self::unity_urp_alpha(),
            Self::godot_alpha(),
            Self::gltf_alpha(),
        ];
        all.extend(crate::builtin_presets::painter());
        // File > Export to Blender's default (last, so earlier indexes stay put).
        all.push(crate::blender::template());
        all
    }
}

#[cfg(test)]
mod source_label_tests {
    use super::*;

    /// QA 2026-11: Coat Normal has three components like Normal, but its label dropped the
    /// component, so the three sources of a coat normal map all read "Coat Normal".
    #[test]
    fn every_multi_component_channel_names_its_component() {
        assert_eq!(Source::ch(ChannelKind::CoatNormal, 1).label(), "Coat Normal.G");
        assert_eq!(Source::ch(ChannelKind::Normal, 2).label(), "Normal.B");
        assert_eq!(Source::ch(ChannelKind::BaseColor, 0).label(), "Base Color.R");
        assert_eq!(Source::ch(ChannelKind::Roughness, 0).label(), "Roughness");
    }
}
