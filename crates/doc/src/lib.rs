//! Document model: texture sets, layer trees, masks, fills, blend modes, history, resources.
//!
//! Pure data — no UI toolkit, no GPU, no file format. The project crate maps this model to the
//! versioned on-disk schema; the compositor evaluates it; the UI edits it through commands.

pub mod blend;
pub mod document;
pub mod effect;
pub mod fill;
pub mod history;
pub mod layer;
pub mod mask;
pub mod pen;
pub mod resources;
pub mod texture_set;

pub use blend::BlendMode;
pub use document::Document;
pub use effect::{ContentEffect, ContentEffectKind};
pub use fill::{FillSource, ProceduralKind, ProceduralParams, Projection, ProjectionMode};
pub use history::{History, HistoryEntry};
pub use layer::{ChannelBlend, FillChannel, FillContent, GroupContent, Layer, LayerContent, LayerKind, PaintContent, PartMask, changed_paint_tiles, empty_paint_image, same_paint_layout};
pub use mask::{Generator, GeneratorKind, Levels, Mask, MaskEffect, MaskEffectKind};
pub use pen::{PenAnchor, PenPath};
pub use resources::{Resource, ResourceKind, ResourceStore};
pub use texture_set::{ChannelDef, ChannelFormat, LayerLocation, LayerOpError, RESOLUTIONS, TextureSet};
