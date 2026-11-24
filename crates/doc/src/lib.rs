//! Document model: texture sets, layer trees, masks, fills, blend modes, history, resources.
//!
//! Pure data — no UI toolkit, no GPU, no file format. The project crate maps this model to the
//! versioned on-disk schema; the compositor evaluates it; the UI edits it through commands.

pub mod anchor;
pub mod blend;
pub mod blend_math;
pub mod camera;
pub mod display;
pub mod document;
pub mod effect;
pub mod fill;
pub mod filter;
pub mod graph_material;
pub mod history;
pub mod instance;
pub mod layer;
pub mod material;
pub mod material_expr;
pub mod mask;
pub mod material_filter;
pub mod pen;
pub mod resources;
pub mod settings;
pub mod shader;
pub mod text;
pub mod texture_set;
pub mod warp;

pub use anchor::{AnchorDomain, AnchorIndex, AnchorInfo, AnchorInput, AnchorRef, RefSite, RefStatus};
pub use blend::BlendMode;
pub use camera::{SceneCamera, ViewState};
pub use display::{Anisotropy, ChannelComponent, DisplaySettings, EnvAlignment, FlareCamera, GlareShape, GridAxis, GrainType, ShadowMode, ToneMap};
pub use document::{Document, share_layer_uniform_tiles};
pub use effect::{ContentEffect, ContentEffectKind};
pub use warp::{DEFAULT_WARP_CELLS, MAX_WARP_SIDE, WarpGrid};
pub use fill::{BackfaceCulling, ProjectionTransform, FillSymmetry, FillFiltering, FillSource, ShapeCrop, TRIPLANAR_HARD, UvWrap, ProceduralKind, ProceduralParams, ScratchParams, ScratchPreset, Projection, ProjectionMode, SurfaceScale};
pub use filter::{FillColorMode, BlurMap, ImageSource, MatchSource, SlopeMode, SlopeParts, SlopeSource, MAX_BLUR_SAMPLES, SLOPE_SMOOTHING, BevelCorner, BevelDirection, BevelParts, GrayParts, CurvePreset, OutlineParts, FULL_SUPPORT, Filter, FlipMode, HslMode, MirrorCorner, MirrorMode, RepeatMode, ThresholdMode, Gradient, GradientStop, MAX_GRADIENT_STOPS, MAX_MEDIAN_RADIUS, MAX_KUWAHARA_RADIUS, MAX_KUWAHARA_SHARPNESS, MAX_KUWAHARA_TENSOR, MAX_KUWAHARA_ANISOTROPY, MAX_STYLE_PAINT, MAX_COMIC_BORDER, MAX_COMIC_TONES, MAX_STROKE_SIZE, MAX_STROKE_LENGTH, MAX_WATERCOLOR_SPREAD, DitherPattern, MAX_QUANTIZE_COLORS, MAX_QUANTIZE_SMOOTHING, MAX_MOSAIC_SIZE, MAX_POSTERIZE_LEVELS, MAX_WARP_SCALE, MAX_SLOPE_SAMPLES, GRADIENT_PRESETS, OutlinePosition};
pub use graph_material::{GraphEngine, GraphMaterial, GraphParam, GraphParamKind};
pub use history::{History, HistoryEntry};
pub use material_filter::{FilterSetting, FilterValue, MAX_EDGE_DAMAGE_SPREAD, MAX_MATERIAL_FILTER_SCALE, MaterialFilter};
pub use instance::{InstanceCache, InstanceState};
pub use layer::{ChannelBlend, FillChannel, FillContent, GroupContent, ImageAdjust, Layer, LayerColor, LayerContent, LayerKind, PaintContent, PartMask, changed_paint_tiles, duplicate_layers_with_new_ids, empty_paint_image, same_paint_layout};
pub use material::{Exposable, Binding, BindingProblem, MaterialControls, Param, ParamKind, ParamValue, Prop};
pub use mask::{CompareMask, CompareOp, CompareOperand, LevelsHandle, LevelsRange, LEVELS_GAMMA_MAX, LEVELS_GAMMA_MIN, SelectionBackground, ShapeSettings, light_angles, light_direction, CurvatureDetail, CurvatureDetailMode, CurvatureMode, EditorCurvature, EditorDirection, EditorDirections, EditorOcclusion, EditorTexture, EditorThickness, GrayscaleType, MaskEditor, MaskBuilder, FiberSettings, DripSettings, EdgeSpread, Generator, MicroCurvatureType, MicroDetails, MicroDirection, MicroEffect, PainterControls, GeneratorKind, GradientAxis, IdColorMask, Levels, Mask, MaskEffect, MaskEffectKind};
pub use pen::{PathKind, PathRibbon, PathStamp, PenAnchor, PenPath, RibbonAlign, RibbonMode, RibbonRatio, RibbonTiling};
pub use resources::{Resource, ResourceKind, ResourceStore};
pub use text::TextDecal;
pub use settings::{BakeConfig, ExportConfig, ExportOverrides, GameEngine, LiveLinkTarget, MapFormat, NormalFormat, ProjectSettings};
pub use shader::{MAIN_SHADER, ShaderAssignment, ShaderInstance, ShaderInstanceId, ShaderParams};
pub use texture_set::{AlphaMode, AlphaSettings, AoMixing, ChannelDef, ChannelFormat, HeightToNormal, IdColorEdit, LayerLocation, LayerOpError, MixingSettings, NormalMixing, RESOLUTIONS, TextureSet, UvPadding};
