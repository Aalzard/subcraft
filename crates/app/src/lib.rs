//! Application layer: the editing [`Session`], undoable [`Command`]s and the shortcut table.
//!
//! No UI toolkit and no GPU here: front ends translate input into commands, actions and stroke
//! calls, and read back the document, history and composites to display them.

pub mod actions;
pub mod asset_apply;
pub mod bake_job;
mod guides;
pub mod brush_preset;
pub mod brush_textures;
pub mod command;
pub mod diagnostics;
pub mod images;
pub mod graphics;
pub mod library;
pub mod launch;
pub mod layer_tree;
pub mod new_project;
pub mod effect_stack;
pub mod parts;
pub mod part_workflow;
pub mod pen;
pub mod recovery;
pub mod session;
pub mod sbsar;
mod sbsar_fill;
pub mod painter_procedural;
pub mod spp;
pub mod spp_job;
pub mod spp_convert;
mod spp_session;
pub mod thumbnails;
pub mod layer_previews;
pub mod updater;

pub use actions::{ACTIONS, Action, ActionInfo, Key, Shortcut, action_for};
pub use bake_job::{BakeJob, BakeRequest, BakePreviewJob};
pub use command::{Command, CommandError, NewLayer};
pub use images::TextureSetImport;
pub use parts::MeshPart;
pub use part_workflow::{PartAction, PartInfo, PartKey, SurfaceTarget};
pub use recovery::{AutosaveEvent, Autosaver, CrashedSession, RecoveryStore};
pub use subcraft_compositor::ChangedTiles;
pub use session::{BrushState, CompositeUpdate, DEFAULT_RESOLUTION, LayerTarget, Pick, ProjectSnapshot, Scene, Selection, Session};
pub use sbsar::{
    DEFAULT_BAKE_SIZE, MAX_BAKE_SIZE, MIN_BAKE_SIZE, PREVIEW_BAKE_SIZE, SbsarEnumValue, SbsarInput,
    SbsarLayerParams, bake_sbsar, bake_sbsar_with_overrides, clamp_bake_size, ensure_preview_bake,
};
pub use spp_convert::{spp_effects_to_mask_effects, spp_node_to_layer};
pub use spp::{SppFormatProbe, SppOpenResult, SppOpenSummary, max_texture_resolution};
pub use spp_job::SppOpenJob;
pub use subcraft_spp::{log_step as spp_log_step, open_log_path as spp_open_log_path};

pub use subcraft_compositor::{Composite, composite_comps};
pub use subcraft_assets as assets;
pub use subcraft_bake::BakeSettings;
pub use subcraft_core::MeshMapKind;
pub use subcraft_export::{ExportSettings, ExportTemplate, Padding};
pub use subcraft_image::FileFormat;
pub use subcraft_project::EXTENSION as PROJECT_EXTENSION;
pub use subcraft_paint::Camera;
