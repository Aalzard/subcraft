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
pub const SCHEMA_VERSION: u32 = 2;
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
    pub content: ContentV1,
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
    Paint { channels: Vec<PaintChannelV1> },
    Fill { projection: Projection, channels: Vec<FillChannelV1> },
    Folder { expanded: bool, children: Vec<LayerV1> },
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
