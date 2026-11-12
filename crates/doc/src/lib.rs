//! Document model: texture sets, layer trees, masks, fills, blend modes, history, resources.
//!
//! Pure data — no UI toolkit, no GPU, no file format. The project crate maps this model to the
//! versioned on-disk schema; the compositor evaluates it; the UI edits it through commands.

pub mod anchor;
pub mod blend;
pub mod display;
pub mod document;
pub mod effect;
pub mod fill;
pub mod filter;
pub mod history;
pub mod layer;
pub mod mask;
pub mod pen;
pub mod resources;
pub mod settings;
pub mod text;
pub mod texture_set;

pub use anchor::{AnchorDomain, AnchorIndex, AnchorInfo, AnchorInput, AnchorRef, RefSite, RefStatus};
pub use blend::BlendMode;
pub use display::{Anisotropy, ChannelComponent, DisplaySettings, EnvAlignment, FlareCamera, GlareShape, GridAxis, GrainType, ToneMap};
pub use document::Document;
pub use effect::{ContentEffect, ContentEffectKind};
pub use fill::{FillFiltering, FillSource, ShapeCrop, TRIPLANAR_HARD, UvWrap, ProceduralKind, ProceduralParams, ScratchParams, ScratchPreset, Projection, ProjectionMode};
pub use filter::{FULL_SUPPORT, Filter, FlipMode, HslMode, MirrorCorner, MirrorMode, RepeatMode, ThresholdMode, Gradient, GradientStop, MAX_GRADIENT_STOPS, MAX_MEDIAN_RADIUS, MAX_MOSAIC_SIZE, MAX_POSTERIZE_LEVELS, MAX_WARP_SCALE, GRADIENT_PRESETS, OutlinePosition};
pub use history::{History, HistoryEntry};
pub use layer::{ChannelBlend, FillChannel, FillContent, GroupContent, Layer, LayerColor, LayerContent, LayerKind, PaintContent, PartMask, changed_paint_tiles, duplicate_layers_with_new_ids, empty_paint_image, same_paint_layout};
pub use mask::{ShapeSettings, light_angles, light_direction, CurvatureMode, EditorCurvature, EditorDirection, EditorDirections, EditorOcclusion, EditorTexture, EditorThickness, GrayscaleType, MaskEditor, EdgeSpread, Generator, MicroDirection, MicroEffect, PainterControls, GeneratorKind, GradientAxis, IdColorMask, Levels, Mask, MaskEffect, MaskEffectKind};
pub use pen::{PenAnchor, PenPath};
pub use resources::{Resource, ResourceKind, ResourceStore};
pub use text::TextDecal;
pub use settings::{NormalFormat, ProjectSettings};
pub use texture_set::{AlphaMode, AlphaSettings, ChannelDef, ChannelFormat, LayerLocation, LayerOpError, RESOLUTIONS, TextureSet};
