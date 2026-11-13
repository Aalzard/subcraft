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
// 9: local filter effects and folder adjustments. Schemas 1–8 retain their original stacks.
// 10: neighborhood Blur/Sharpen filters; schemas 1–9 migrate unchanged.
// 11: Directional Blur and Highpass; schemas 1-10 retain their previous stacks.
// 12: Bevel and Mask Outline; schemas 1-11 migrate unchanged (no contour effects to default).
// 13: UDIM tile per texture set and project settings (export choices, normal format); schemas
//     1-12 migrate unchanged (no tile, default settings).
// 14: Gradient Map (stop list), Histogram Scan / Range, Median; schemas 1-13 migrate unchanged.
// 15: ID Color Mask effect (`id_color`); schemas 1-14 migrate unchanged (they cannot contain it).
// 16: editable text decals on paint layers (`text`) and font resources; schemas 1-15 migrate
//     unchanged (no text layers). Font files are stored like images, kind `font`.
// 17: anchor points (ADR-0045): `anchor` content / mask effects, `anchor_ref` mask effects, the
//     `anchor` fill source and the generator `input`. Schemas 1-16 migrate unchanged (they cannot
//     contain anchors); older SubCraft versions refuse schema-17 files instead of dropping them.
// 18: transparent texture sets (`transparent`) and viewport alpha settings (`alpha`, issue #145).
//     Both are omitted when default; schemas 1-17 migrate to opaque sets drawn opaque.
pub const SCHEMA_VERSION: u32 = 18;
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
    /// Schema 13: saved with the project, outside the undo history. Omitted when default.
    #[serde(default, skip_serializing_if = "is_default_settings")]
    pub settings: subcraft_doc::ProjectSettings,
}

fn legacy_mixing() -> subcraft_doc::MixingSettings {
    subcraft_doc::MixingSettings::LEGACY
}

fn is_default_settings(s: &subcraft_doc::ProjectSettings) -> bool {
    *s == subcraft_doc::ProjectSettings::default()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSetV1 {
    pub id: Uuid,
    pub name: String,
    /// Texture Set List description. Additive and optional: absent means empty, older readers ignore it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub material_slot: u32,
    pub resolution: u32,
    /// Schema 6 migration: older files retain their original bilinear reconstruction.
    #[serde(default)]
    pub albedo_filter: subcraft_image::TextureFilter,
    /// Mixing settings. Additive and optional: absent (files written before them) loads as
    /// `MixingSettings::LEGACY`, the behaviour those files were made with.
    #[serde(default = "legacy_mixing")]
    pub mixing: subcraft_doc::MixingSettings,
    /// Schema 13: UDIM tile `[column, row]` this set paints (absent = ordinary set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udim_tile: Option<[i32; 2]>,
    /// Schema 18: the layer stack starts transparent instead of at the channel defaults.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transparent: bool,
    /// Schema 18: viewport alpha mode / threshold / dithering (absent = opaque display).
    #[serde(default, skip_serializing_if = "subcraft_doc::AlphaSettings::is_default")]
    pub alpha: subcraft_doc::AlphaSettings,
    pub channels: Vec<ChannelDefV1>,
    /// Names of renamed user channels. Additive and optional: absent = none renamed.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub user_channel_names: std::collections::BTreeMap<ChannelKind, String>,
    /// Bottom-to-top.
    pub layers: Vec<LayerV1>,
    /// Baked mesh maps (optional; absent in files written before baking existed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mesh_maps: Vec<MeshMapV1>,
    /// ID color editor changes over the stored ID map (optional; absent = none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub id_color_edits: Vec<IdColorEditV1>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdColorEditV1 {
    pub from: [f32; 3],
    pub to: [f32; 3],
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
    /// Colour tag of the row (absent = none). Older readers ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<subcraft_doc::LayerColor>,
    /// The layer this one is an instance of (absent = an ordinary layer). Older readers ignore
    /// it and see the copy the instance last showed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_of: Option<uuid::Uuid>,
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
    /// Filters: further channels the filter processes (absent = only its own). Older readers
    /// ignore it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_channels: Vec<ChannelKind>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentEffectKindV5 {
    Paint { channels: Vec<PaintChannelV1> },
    Fill { projection: Projection, channels: Vec<FillChannelV1> },
    Levels { channel: ChannelKind, levels: Levels },
    Filter { channel: ChannelKind, filter: subcraft_doc::Filter },
    /// Schema 17: anchor point (its name is the effect's name).
    Anchor,
    /// A generator in the content stack.
    Generator { generator: subcraft_doc::Generator },
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
        /// Schema 16.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<TextLayerV16>,
    },
    Fill { projection: Projection, channels: Vec<FillChannelV1> },
    Folder { expanded: bool, children: Vec<LayerV1> },
}

/// Schema 16: an editable text decal. Lengths are fractions of the texture width (em), so the
/// raster cache is always re-derivable at any resolution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayerV16 {
    pub text: String,
    pub font: subcraft_text::FontRef,
    pub letter_spacing: f32,
    pub line_spacing: f32,
    pub align: subcraft_text::Align,
    pub direction: subcraft_text::Direction,
    pub center: [f32; 2],
    pub size: f32,
    pub rotation: f32,
    pub opacity: f32,
    pub values: Vec<PenChannelV4>,
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
    /// Paint along path / Filled path; files from before it read as Paint. Not written for Paint.
    #[serde(default, skip_serializing_if = "is_paint_path")]
    pub kind: subcraft_doc::PathKind,
    /// Brush tip stamped along the path; absent for a continuous stroke.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<subcraft_doc::PathStamp>,
    /// Ribbon settings; absent when never set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ribbon: Option<subcraft_doc::PathRibbon>,
}

fn is_paint_path(kind: &subcraft_doc::PathKind) -> bool {
    *kind == subcraft_doc::PathKind::Paint
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
    Filter { filter: subcraft_doc::Filter },
    Invert,
    /// Schema 2.
    Image { resource: Uuid, projection: Projection, invert: bool },
    /// Schema 15 (ADR-0041).
    IdColor { id_color: subcraft_doc::IdColorMask },
    /// Schema 17 (ADR-0045): anchor point in a mask stack.
    Anchor,
    /// Schema 17 (ADR-0045): grayscale read from an anchor point.
    AnchorRef {
        input: subcraft_doc::AnchorInput,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        levels: Option<Levels>,
    },
    /// Painter's Compare Mask.
    Compare { compare: subcraft_doc::CompareMask },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceV1 {
    pub id: Uuid,
    pub name: String,
    pub kind: subcraft_doc::ResourceKind,
    /// Path of the stored file inside the container (original encoded bytes).
    pub file: String,
}

#[cfg(test)]
mod mixing_tests {
    #[test]
    fn texture_sets_saved_before_mixing_load_with_the_legacy_settings() {
        let json = r#"{"id":"00000000-0000-0000-0000-000000000001","name":"A","material_slot":0,"resolution":256,"channels":[],"layers":[]}"#;
        let ts: super::TextureSetV1 = serde_json::from_str(json).unwrap();
        assert_eq!(ts.mixing, subcraft_doc::MixingSettings::LEGACY);
        assert_eq!(ts.description, "");
        let back: super::TextureSetV1 = serde_json::from_str(&serde_json::to_string(&super::TextureSetV1 { mixing: subcraft_doc::MixingSettings::default(), ..ts }).unwrap()).unwrap();
        assert_eq!(back.mixing, subcraft_doc::MixingSettings::default(), "saved settings load back");
    }
}
