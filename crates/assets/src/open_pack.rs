//! Open pack: CC0 PBR materials from ambientCG and CC0 HDRI environments from Poly Haven
//! (`assets/open-pack/CREDITS-CC0.txt`). Desktop builds embed the files; images are decoded only
//! when a material is applied or previewed, and panoramas when an environment is first used.
//! The web build leaves the pack out to keep the download small.

#[cfg(not(target_arch = "wasm32"))]
use subcraft_core::{AssetId, ChannelKind, ChannelValue};
#[cfg(not(target_arch = "wasm32"))]
use subcraft_doc::{FillSource, Projection, ProjectionMode};

use crate::environment::Environment;
use crate::presets::MaterialPreset;
#[cfg(not(target_arch = "wasm32"))]
use crate::presets::PresetImage;

/// Gentle bump from 0..1 displacement maps (the procedural presets use similar heights).
#[cfg(not(target_arch = "wasm32"))]
const HEIGHT_STRENGTH: f32 = 0.08;

#[cfg(not(target_arch = "wasm32"))]
macro_rules! map {
    ($id:literal, $map:literal, $channel:expr, $strength:expr) => {
        PresetImage {
            channel: $channel,
            name: concat!($id, "_", $map, ".jpg"),
            strength: $strength,
            bytes: include_bytes!(concat!("../../../assets/open-pack/materials/", $id, "/", $id, "_", $map, ".jpg")),
        }
    };
}

/// Color, roughness, OpenGL normal and height maps of an ambientCG material.
#[cfg(not(target_arch = "wasm32"))]
macro_rules! maps {
    ($id:literal) => {
        vec![
            map!($id, "Color", ChannelKind::BaseColor, 1.0),
            map!($id, "Roughness", ChannelKind::Roughness, 1.0),
            map!($id, "NormalGL", ChannelKind::Normal, 1.0),
            map!($id, "Displacement", ChannelKind::Height, HEIGHT_STRENGTH),
        ]
    };
    ($id:literal, metalness) => {{
        let mut v = maps!($id);
        v.push(map!($id, "Metalness", ChannelKind::Metallic, 1.0));
        v
    }};
}

#[cfg(not(target_arch = "wasm32"))]
/// `tiling` repeats per UV unit (x, y); non-square images repeat more along their short side so
/// texels stay square.
fn material(name: &str, category: &'static str, images: Vec<PresetImage>, tiling: [f32; 2]) -> MaterialPreset {
    let has_metal = images.iter().any(|i| i.channel == ChannelKind::Metallic);
    let channels = if has_metal { Vec::new() } else { vec![(ChannelKind::Metallic, FillSource::uniform(ChannelValue::scalar(0.0)))] };
    MaterialPreset {
        id: AssetId::from_name(&format!("material/open-pack/{name}")),
        name: name.into(),
        category,
        channels,
        images,
        // UV: tangent-space normal maps need the mesh's tangent frame (triplanar would not
        // reorient them).
        projection: Projection { mode: ProjectionMode::Uv, scale: tiling, ..Projection::default() },
    }
}

/// The pack's image-based materials.
#[cfg(not(target_arch = "wasm32"))]
pub fn materials() -> Vec<MaterialPreset> {
    vec![
        material("Scratched Red Paint", "Paint", maps!("PaintedMetal004", metalness), [2.0, 2.0]),
        material("Stained Steel", "Metal", maps!("Metal063", metalness), [2.0, 2.0]),
        material("Metal Plates", "Metal", maps!("MetalPlates006", metalness), [2.0, 2.0]),
        material("Textured Plastic", "Plastic", maps!("Plastic010"), [2.0, 2.0]),
        material("Brown Leather", "Fabric", maps!("Leather037"), [3.0, 3.0]),
        material("Woven Fabric", "Fabric", maps!("Fabric085"), [4.0, 4.0]),
        material("Natural Wood", "Wood", maps!("Wood096"), [2.0, 2.0]),
        material("Bricks", "Stone", maps!("Bricks105"), [2.0, 4.0]),
        material("Smooth Concrete", "Stone", maps!("Concrete034"), [2.0, 4.0]),
        material("Marble", "Stone", maps!("Marble012"), [1.0, 1.0]),
    ]
}

#[cfg(target_arch = "wasm32")]
pub fn materials() -> Vec<MaterialPreset> {
    Vec::new()
}

#[cfg(not(target_arch = "wasm32"))]
fn hdri(name: &str, file: &'static str, bytes: &'static [u8]) -> Environment {
    Environment {
        id: AssetId::from_name(&format!("environment/open-pack/{file}")),
        name: name.into(),
        source: crate::environment::EnvironmentSource::Embedded(std::sync::Arc::new(crate::environment::EmbeddedHdri::new(file, bytes))),
        user_file: None,
    }
}

/// The pack's HDRI panoramas (1K).
#[cfg(not(target_arch = "wasm32"))]
pub fn environments() -> Vec<Environment> {
    macro_rules! env {
        ($name:literal, $file:literal) => {
            hdri($name, $file, include_bytes!(concat!("../../../assets/open-pack/environments/", $file)))
        };
    }
    vec![
        env!("Small Studio (HDRI)", "studio_small_09_1k.exr"),
        env!("Photo Studio (HDRI)", "brown_photostudio_02_1k.exr"),
        env!("Partly Cloudy Sky (HDRI)", "kloofendal_48d_partly_cloudy_puresky_1k.hdr"),
        env!("Sunrise Field (HDRI)", "spruit_sunrise_1k.exr"),
    ]
}

#[cfg(target_arch = "wasm32")]
pub fn environments() -> Vec<Environment> {
    Vec::new()
}
