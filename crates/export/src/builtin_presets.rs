//! Painter's predefined and default output templates (Export window > Output template), as
//! read-only built-ins.
//!
//! Names follow Painter so a Painter user finds them. Map lists come from each engine's or
//! renderer's material inputs (metal / roughness channels, converted maps where the target wants
//! specular / glossiness); file suffixes and formats are SubCraft's reading of those targets and
//! have not been checked file-by-file against Painter's own preset files. Templates that need
//! data SubCraft does not have (Dota 2's mask maps, Unreal SSS's scattering) or alpha packing
//! (the "+ Alpha" / "+ Opacity" templates of #145 cover alpha) are not here.

use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_image::FileFormat;

use crate::template::{Converted, ExportTemplate, OutputMap, Source};

use ChannelKind::*;
use FileFormat::{Exr32, Png16, Png8};

fn rgb(kind: ChannelKind) -> Vec<Source> {
    (0..3).map(|c| Source::ch(kind, c)).collect()
}

fn gray(kind: ChannelKind) -> Vec<Source> {
    vec![Source::ch(kind, 0)]
}

fn conv(map: Converted) -> Vec<Source> {
    (0..map.components() as u8).map(|component| Source::Converted { map, component }).collect()
}

fn normal(directx: bool) -> Vec<Source> {
    (0..3).map(|c| Source::Normal { component: c, directx }).collect()
}

fn m(suffix: &str, format: FileFormat, components: Vec<Source>, srgb: bool) -> OutputMap {
    OutputMap::new(suffix, format, components, srgb)
}

fn t(name: &str, maps: Vec<OutputMap>) -> ExportTemplate {
    ExportTemplate { name: name.into(), maps, ..Default::default() }
}

/// With an explicit UDIM file name (`<mesh>_<set>_<map>.<tile>`), as legacy UDIM workflows read.
fn udim(mut tpl: ExportTemplate) -> ExportTemplate {
    for map in &mut tpl.maps {
        map.pattern = Some("$mesh_$textureSet_$suffix.$udim".into());
    }
    tpl
}

/// Metal / roughness maps most offline renderers take as separate files.
fn metal_rough(name: &str, height: FileFormat) -> ExportTemplate {
    t(name, vec![
        m("BaseColor", Png8, rgb(BaseColor), true),
        m("Metallic", Png8, gray(Metallic), false),
        m("Roughness", Png8, gray(Roughness), false),
        m("Normal", Png8, normal(false), false),
        m("Height", height, vec![Source::Height], false),
        m("Emissive", Png8, rgb(Emissive), true),
    ])
}

/// Specular / glossiness maps converted from the metal / roughness channels.
fn spec_gloss(name: &str, specular: Converted, height: FileFormat) -> ExportTemplate {
    t(name, vec![
        m("Diffuse", Png8, conv(Converted::Diffuse), true),
        m(if specular == Converted::Reflection { "Reflection" } else { "Specular" }, Png8, conv(specular), true),
        m("Glossiness", Png8, conv(Converted::Glossiness), false),
        m("Normal", Png8, normal(false), false),
        m("Height", height, vec![Source::Height], false),
        m("Emissive", Png8, rgb(Emissive), true),
    ])
}

/// Every document channel as its own file, plus the normal (with height) and mixed AO.
fn document_channels() -> ExportTemplate {
    let mut maps = vec![
        m("BaseColor", Png8, rgb(BaseColor), true),
        m("Roughness", Png8, gray(Roughness), false),
        m("Metallic", Png8, gray(Metallic), false),
        m("Height", Png16, vec![Source::Height], false),
        m("Emissive", Png8, rgb(Emissive), true),
        m("Specular", Png8, gray(Specular), false),
    ];
    for (k, suffix) in [(User0, "User0"), (User1, "User1"), (User2, "User2"), (User3, "User3")] {
        maps.push(m(suffix, Png8, gray(k), false));
    }
    maps.push(m("Normal", Png8, normal(false), false));
    maps.push(m("AO", Png8, conv(Converted::MixedAo), false));
    t("Document channels + Normal + AO (No Alpha)", maps)
}

/// Each baked mesh map as its own file (written only where it is baked).
fn mesh_maps() -> ExportTemplate {
    let maps = MeshMapKind::ALL
        .into_iter()
        .map(|kind| {
            let format = if matches!(kind, MeshMapKind::Height | MeshMapKind::Position) { Png16 } else { Png8 };
            let suffix: String = kind.display_name().chars().filter(|c| c.is_alphanumeric()).collect();
            let comps = (0..kind.components() as u8).map(|component| Source::MeshMap { kind, component, invert: false }).collect();
            m(&suffix, format, comps, false)
        })
        .collect();
    t("Mesh Maps", maps)
}

/// Normal map with glossiness in alpha (CryEngine's `_ddna`).
fn ddna() -> Vec<Source> {
    let mut n = normal(false);
    n.push(Source::Converted { map: Converted::Glossiness, component: 0 });
    n
}

/// Painter's templates, alphabetically after the document-channel and glTF ones.
pub fn painter() -> Vec<ExportTemplate> {
    let gloss = Converted::Glossiness;
    let mut gltf_disp = ExportTemplate::gltf();
    gltf_disp.name = "glTF PBR Metal Roughness + Displacement texture (experimental)".into();
    gltf_disp.maps.push(m("Height", Png16, vec![Source::Height], false));
    vec![
        document_channels(),
        gltf_disp,
        t("Amazon Lumberyard", vec![
            m("diff", Png8, conv(Converted::Diffuse), true),
            m("spec", Png8, conv(Converted::Specular), true),
            m("ddna", Png8, ddna(), false),
            m("emissive", Png8, rgb(Emissive), true),
        ]),
        t("Arnold (AiStandard)", vec![
            m("BaseColor", Png8, rgb(BaseColor), true),
            m("Metalness", Png8, gray(Metallic), false),
            m("SpecularRoughness", Png8, gray(Roughness), false),
            m("Normal", Png8, normal(false), false),
            m("Height", Exr32, vec![Source::Height], false),
            m("Emission", Png8, rgb(Emissive), true),
        ]),
        udim(ExportTemplate { name: "Arnold UDIM Legacy (AiStandard)".into(), ..metal_rough("", Exr32) }),
        metal_rough("Blender (Principled BSDF)", Png16),
        t("Corona", vec![
            m("Diffuse", Png8, conv(Converted::Diffuse), true),
            m("Reflection", Png8, conv(Converted::Reflection), true),
            m("ReflectionGlossiness", Png8, conv(gloss), false),
            m("Normal", Png8, normal(false), false),
            m("Height", Png16, vec![Source::Height], false),
            m("Emissive", Png8, rgb(Emissive), true),
        ]),
        t("CryEngine 3", vec![
            m("diff", Png8, conv(Converted::Diffuse), true),
            m("spec", Png8, conv(Converted::Specular), true),
            m("ddna", Png8, ddna(), false),
        ]),
        metal_rough("Keyshot", Png16),
        {
            let mut k = metal_rough("Keyshot 9+", Png16);
            k.maps.push(m("AO", Png8, conv(Converted::MixedAo), false));
            k
        },
        t("Lens Studio", vec![
            m("BaseColor", Png8, rgb(BaseColor), true),
            m("Normal", Png8, normal(false), false),
            m("MaterialParams", Png8, vec![Source::ch(Metallic, 0), Source::ch(Roughness, 0), Source::Converted { map: Converted::MixedAo, component: 0 }], false),
            m("Emissive", Png8, rgb(Emissive), true),
        ]),
        metal_rough("Maxwell (Metallic Roughness)", Png16),
        spec_gloss("Maxwell (Specular Glossiness)", Converted::Specular, Png16),
        mesh_maps(),
        spec_gloss("Non-PBR Specular Glossiness", Converted::Specular, Png8),
        {
            // One sub-folder per texture set ("/" in the file name pattern).
            let mut f = ExportTemplate::pbr_metal_rough();
            f.name = "PBR Metallic Roughness (Folder or PSD Groups)".into();
            for map in &mut f.maps {
                map.pattern = Some("$textureSet/$mesh_$textureSet_$suffix".into());
            }
            f
        },
        {
            let mut s = spec_gloss("PBR Specular Glossiness (converted from Metallic Roughness)", Converted::Specular, Png16);
            s.maps.push(m("AO", Png8, conv(Converted::MixedAo), false));
            s
        },
        metal_rough("Redshift", Exr32),
        t("Renderman (pxrDisney)", vec![
            m("BaseColor", Png8, rgb(BaseColor), true),
            m("Metallic", Png8, gray(Metallic), false),
            m("Roughness", Png8, gray(Roughness), false),
            m("Specular", Png8, gray(Specular), false),
            m("Normal", Png8, normal(false), false),
            m("Height", Exr32, vec![Source::Height], false),
            m("Emissive", Png8, rgb(Emissive), true),
        ]),
        t("Renderman (pxrSurface)", vec![
            m("DiffuseColor", Png8, conv(Converted::Diffuse), true),
            m("SpecularFaceColor", Png8, conv(Converted::Specular), true),
            m("SpecularRoughness", Png8, gray(Roughness), false),
            m("Normal", Png8, normal(false), false),
            m("Height", Exr32, vec![Source::Height], false),
            m("Glow", Png8, rgb(Emissive), true),
        ]),
        t("Roblox (MaterialVariant)", vec![
            m("ColorMap", Png8, rgb(BaseColor), true),
            m("NormalMap", Png8, normal(false), false),
            m("MetalnessMap", Png8, gray(Metallic), false),
            m("RoughnessMap", Png8, gray(Roughness), false),
        ]),
        metal_rough("Shade 3D", Png16),
        t("Spark AR Studio", vec![
            m("BaseColor", Png8, rgb(BaseColor), true),
            m("Normal", Png8, normal(false), false),
            m("ORM", Png8, vec![Source::Converted { map: Converted::MixedAo, component: 0 }, Source::ch(Roughness, 0), Source::ch(Metallic, 0)], false),
            m("Emissive", Png8, rgb(Emissive), true),
        ]),
        t("Unity HD Render Pipeline (Specular)", vec![
            m("BaseColor", Png8, conv(Converted::Diffuse), true),
            m("SpecularColor", Png8, conv(Converted::Specular), true),
            m("MaskMap", Png8, vec![Source::Constant { value: 0.0 }, Source::Converted { map: Converted::MixedAo, component: 0 }, Source::Constant { value: 0.0 }, Source::Converted { map: gloss, component: 0 }], false),
            m("Normal", Png8, normal(false), false),
            m("Emissive", Png8, rgb(Emissive), true),
        ]),
        t("Unity Universal Render Pipeline (Specular)", vec![
            m("Albedo", Png8, conv(Converted::Diffuse), true),
            m("SpecularSmoothness", Png8, {
                let mut s = conv(Converted::Specular);
                s.push(Source::Converted { map: gloss, component: 0 });
                s
            }, true),
            m("Occlusion", Png8, conv(Converted::MixedAo), false),
            m("Normal", Png8, normal(false), false),
            m("Emissive", Png8, rgb(Emissive), true),
            m("Height", Png16, vec![Source::Height], false),
        ]),
        metal_rough("Vray Next (Metallic Roughness)", Exr32),
        spec_gloss("Vray Next (Specular Glossiness)", Converted::Reflection, Exr32),
        udim(ExportTemplate { name: "Vray UDIM Legacy (Metallic Roughness)".into(), ..metal_rough("", Exr32) }),
    ]
}
