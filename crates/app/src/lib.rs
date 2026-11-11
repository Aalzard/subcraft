//! Application layer: the editing [`Session`], undoable [`Command`]s and the shortcut table.
//!
//! No UI toolkit and no GPU here: front ends translate input into commands, actions and stroke
//! calls, and read back the document, history and composites to display them.

pub mod actions;
pub mod asset_apply;
pub mod auto_uv;
pub mod bake_job;
mod guides;
pub mod brush_builtin;
pub mod brush_preset;
pub mod brush_textures;
pub mod command;
pub mod diagnostics;
pub mod id_color;
pub mod images;
pub mod graphics;
pub mod library;
pub mod mesh_reload;
pub mod launch;
pub mod layer_tree;
pub mod new_project;
pub mod effect_stack;
pub mod export_presets;
pub mod filter_shelf;
pub mod parts;
pub mod part_workflow;
pub mod pen;
pub mod photocraft;
pub mod recovery;
pub mod safe_mode;
pub mod startup_probe;
pub mod session;
pub mod shelf_apply;
pub mod spp_cache;
pub mod text;
pub mod spp_fit;
pub mod spp_import;
pub mod spp_job;
pub mod smart_import;
pub mod spp_select;
pub mod stamp_brush;
pub mod spp_stack;
pub mod thumbnails;
pub mod world_blur;
pub mod layer_previews;
pub mod updater;
pub mod unsaved;
pub mod clipboard;
pub mod multi_select;

pub use bake_job::{BakeJob, MapProgress, MapState, BakeLogLevel, BakeLogLine, BakePreviewJob, BakeRequest, log_text as bake_log_text};
pub use actions::{ACTIONS, Action, ActionInfo, Key, KeymapPreset, PAINTER_REFERENCE, PainterEntry, Shortcut, Support, action_for, label_in, shortcuts_in};
pub use spp_job::{ImportControl, SppImportJob, SppPoll};
pub use command::{Command, CommandError, NewLayer};
pub use clipboard::{EffectClip, LayerClip};
pub use images::TextureSetImport;
pub use mesh_reload::{MeshReloadJob, MeshReloadReport, PreparedReload, ReloadMode, ReloadOutput};
pub use auto_uv::AutoUvOutput;
pub use parts::MeshPart;
pub use part_workflow::{PartAction, PartInfo, PartKey, SurfaceTarget};
pub use recovery::{AutosaveEvent, Autosaver, CrashedSession, RecoveryStore};
pub use subcraft_compositor::ChangedTiles;
pub use session::{BrushState, CloneSourceMode, CompositeUpdate, DEFAULT_RESOLUTION, LayerTarget, Pick, ProjectSnapshot, Scene, Selection, Session};

pub use subcraft_compositor::{Composite, composite_comps};
pub use subcraft_assets as assets;
pub use subcraft_bake::BakeSettings;
pub use subcraft_core::{BakeMatch, BakeMatchMode, MeshMapKind};
pub use subcraft_export::{ExportSettings, ExportTemplate, Padding};
pub use subcraft_image::FileFormat;
pub use subcraft_project::EXTENSION as PROJECT_EXTENSION;
pub use subcraft_paint::Camera;
