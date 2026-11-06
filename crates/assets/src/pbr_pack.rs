//! PBR pack: 18 image-based materials made for SubCraft (`assets/pbr-pack/CREDITS.txt`), each
//! with Base Color, Roughness, Metallic, Normal (OpenGL), Height and Ambient Occlusion maps at
//! 1024². Desktop builds embed them; images are decoded only when a material is applied or
//! previewed. They appear on the Shelf as smart materials (and as plain materials). The web build
//! leaves the pack out to keep the download small.

use crate::presets::MaterialPreset;

#[cfg(not(target_arch = "wasm32"))]
use subcraft_core::{AssetId, ChannelKind};
#[cfg(not(target_arch = "wasm32"))]
use subcraft_doc::{Projection, ProjectionMode};
#[cfg(not(target_arch = "wasm32"))]
use crate::presets::PresetImage;

/// Height maps cover the full 0..1 range: a gentle bump like the other image materials.
#[cfg(not(target_arch = "wasm32"))]
const HEIGHT_STRENGTH: f32 = 0.08;

#[cfg(not(target_arch = "wasm32"))]
macro_rules! map {
    ($id:literal, $map:literal, $channel:expr, $strength:expr) => {
        PresetImage {
            channel: $channel,
            name: concat!($id, "_", $map, ".jpg"),
            strength: $strength,
            bytes: include_bytes!(concat!("../../../assets/pbr-pack/materials/", $id, "/", $id, "_", $map, ".jpg")),
        }
    };
}

/// All six maps of a pack material.
#[cfg(not(target_arch = "wasm32"))]
macro_rules! maps {
    ($id:literal) => {
        vec![
            map!($id, "Color", ChannelKind::BaseColor, 1.0),
            map!($id, "Roughness", ChannelKind::Roughness, 1.0),
            map!($id, "Metalness", ChannelKind::Metallic, 1.0),
            map!($id, "NormalGL", ChannelKind::Normal, 1.0),
            map!($id, "Height", ChannelKind::Height, HEIGHT_STRENGTH),
            map!($id, "AO", ChannelKind::AmbientOcclusion, 1.0),
        ]
    };
}

#[cfg(not(target_arch = "wasm32"))]
fn material(name: &str, category: &'static str, images: Vec<PresetImage>, tiling: f32) -> MaterialPreset {
    MaterialPreset {
        id: AssetId::from_name(&format!("material/pbr-pack/{name}")),
        name: name.into(),
        category,
        channels: Vec::new(),
        images,
        // UV: tangent-space normal maps need the mesh's tangent frame.
        projection: Projection { mode: ProjectionMode::Uv, scale: [tiling, tiling], ..Projection::default() },
    }
}

/// The pack's 18 materials, in the pack's order.
#[cfg(not(target_arch = "wasm32"))]
pub fn materials() -> Vec<MaterialPreset> {
    vec![
        material("Black Basalt", "Stone", maps!("Black_Basalt"), 2.0),
        material("Travertine", "Stone", maps!("Travertine"), 2.0),
        material("Terrazzo", "Stone", maps!("Terrazzo"), 2.0),
        material("Cobblestone", "Stone", maps!("Cobblestone"), 2.0),
        material("Cracked Asphalt", "Stone", maps!("Cracked_Asphalt"), 2.0),
        material("Dry Mud", "Nature", maps!("Dry_Mud"), 2.0),
        material("Desert Sand", "Nature", maps!("Desert_Sand"), 2.0),
        material("Mossy Stone", "Nature", maps!("Mossy_Stone"), 2.0),
        material("Tree Bark", "Wood", maps!("Tree_Bark"), 2.0),
        material("Cork", "Wood", maps!("Cork"), 2.0),
        material("Bamboo Flooring", "Wood", maps!("Bamboo_Flooring"), 2.0),
        material("Woven Linen", "Fabric", maps!("Woven_Linen"), 3.0),
        material("Worn Leather", "Fabric", maps!("Worn_Leather"), 3.0),
        material("Industrial Rubber", "Plastic", maps!("Industrial_Rubber"), 2.0),
        material("Brushed Aluminum", "Metal", maps!("Brushed_Aluminum"), 2.0),
        material("Oxidized Copper", "Metal", maps!("Oxidized_Copper"), 2.0),
        material("Galvanized Steel", "Metal", maps!("Galvanized_Steel"), 2.0),
        material("Hex Armor", "Metal", maps!("Hex_Armor"), 2.0),
    ]
}

#[cfg(target_arch = "wasm32")]
pub fn materials() -> Vec<MaterialPreset> {
    Vec::new()
}
