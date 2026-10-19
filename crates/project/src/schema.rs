//! Version 1 of the logical project schema (`document.json`).
//!
//! The schema mirrors the document *structure* explicitly (texture sets, layer tree, masks,
//! effects, raster references) so in-memory refactors don't silently change the file format.
//! Small value types that already carry stable snake_case serde representations (`ChannelKind`,
//! `BlendMode`, `FillSource`, `Projection`, `ProceduralParams`, `Generator`, `Levels`,
//! `ChannelFormat`) are embedded directly; they are part of the file contract and a golden test
//! pins their JSON. Any incompatible change bumps [`SCHEMA_VERSION`] and adds a migration.

use serde::{Deserialize, Serialize};
use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, ChannelFormat, FillSource, Generator, Levels, ProceduralParams, Projection};
use uuid::Uuid;

/// 1: initial. 2: mask `Image` effect (v1 files load unchanged; v2 files need SubCraft >= 0.1 with
/// image masks).
// 3: object metadata (mesh blob v2) and geometry-mask domains. Legacy projects still load.
// 4: editable pen paths, with anchor UUIDs, UV handles, style and a derived raster cache.
// 5: per-layer content effect stacks (`content_effects`; absent = none, so 1-4 load unchanged).
// 6: texture-set albedo filtering; schemas 1–5 migrate to Bilinear.
// 7: organic generator breakup; absent in schemas 1–6 means legacy projected noise.
// 8: geometry-aware edge spread/variation; absent in schemas 1–7 retains legacy evaluation.
pub const SCHEMA_VERSION: u32 = 8;
pub const FORMAT_NAME: &str = "subcraft-project";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub schema_version: u32,
    /// Application version that wrote the file (informational).
    pub writer: String,
    pub mesh: String,
    pub document: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentV1 {
    pub texture_sets: Vec<TextureSetV1>,
    #[serde(default)]
    pub resources: Vec<ResourceV1>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSetV1 {
    pub id: Uuid,
    pub name: String,
    pub material_slot: u32,
    pub resolution: u32,
    /// Schema 6 migration: older files retain their original bilinear reconstruction.
    #[serde(default)]
    pub albedo_filter: subcraft_image::TextureFilter,
    pub channels: Vec<ChannelDefV1>,
    /// Bottom-to-top.
    pub layers: Vec<LayerV1>,
    /// Baked mesh maps (optional; absent in files written before baking existed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mesh_maps: Vec<MeshMapV1>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshMapV1 {
    pub kind: subcraft_core::MeshMapKind,
    pub raster: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelDefV1 {
    pub kind: ChannelKind,
    pub format: ChannelFormat,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlendV1 {
    pub channel: ChannelKind,
    pub mode: BlendMode,
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerV1 {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub blend: Vec<BlendV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<MaskV1>,
    /// Mesh parts the layer is visible on (absent = everywhere). Older readers ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts: Option<PartsV1>,
    pub content: ContentV1,
    /// Schema 5. Bottom-to-top content effects; absent in older files (no effects).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_effects: Vec<ContentEffectV5>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContentEffectV5 {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub blend: Vec<BlendV1>,
    pub kind: ContentEffectKindV5,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentEffectKindV5 {
    Paint { channels: Vec<PaintChannelV1> },
    Fill { projection: Projection, channels: Vec<FillChannelV1> },
    Levels { channel: ChannelKind, levels: Levels },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartsV1 {
    pub enabled: bool,
    #[serde(default)]
    pub kind: subcraft_core::PartKind,
    pub parts: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaintChannelV1 {
    pub channel: ChannelKind,
    /// Path of the raster blob inside the container.
    pub raster: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FillChannelV1 {
    pub channel: ChannelKind,
    pub enabled: bool,
    pub source: FillSource,
    /// Added after schema 2 shipped; older files have none and mean 1.0. Older readers ignore it.
    #[serde(default = "one")]
    pub strength: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentV1 {
    Paint {
        channels: Vec<PaintChannelV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<PenPathV4>,
    },
    Fill { projection: Projection, channels: Vec<FillChannelV1> },
    Folder { expanded: bool, children: Vec<LayerV1> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PenAnchorV4 {
    pub id: Uuid,
    pub position: [f32; 2],
    pub incoming: [f32; 2],
    pub outgoing: [f32; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PenChannelV4 { pub channel: ChannelKind, pub value: [f32; 3] }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PenPathV4 {
    pub anchors: Vec<PenAnchorV4>,
    pub closed: bool,
    pub radius: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub values: Vec<PenChannelV4>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskV1 {
    pub enabled: bool,
    pub base: String,
    pub effects: Vec<MaskEffectV1>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskEffectV1 {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub blend: BlendMode,
    pub opacity: f32,
    pub kind: MaskEffectKindV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MaskEffectKindV1 {
    Paint { raster: String },
    Fill { value: f32, procedural: Option<ProceduralParams>, projection: Projection },
    Generator { generator: Generator },
    Levels { levels: Levels },
    Invert,
    /// Schema 2.
    Image { resource: Uuid, projection: Projection, invert: bool },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceV1 {
    pub id: Uuid,
    pub name: String,
    pub kind: subcraft_doc::ResourceKind,
    /// Path of the stored file inside the container (original encoded bytes).
    pub file: String,
}
