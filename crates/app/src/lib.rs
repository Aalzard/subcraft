//! Application layer: the editing [`Session`], undoable [`Command`]s and the shortcut table.
//!
//! No UI toolkit and no GPU here: front ends translate input into commands, actions and stroke
//! calls, and read back the document, history and composites to display them.

pub mod actions;
pub mod bake_job;
pub mod brush_preset;
pub mod command;
pub mod images;
pub mod library;
pub mod parts;
pub mod session;
pub mod thumbnails;

pub use actions::{ACTIONS, Action, ActionInfo, Key, Shortcut, action_for};
pub use bake_job::{BakeJob, BakeRequest};
pub use command::{Command, CommandError, NewLayer};
pub use images::TextureSetImport;
pub use parts::MeshPart;
pub use session::{BrushState, CompositeUpdate, DEFAULT_RESOLUTION, Pick, Scene, Selection, Session};

pub use subcraft_compositor::{Composite, composite_comps};
pub use subcraft_assets as assets;
pub use subcraft_bake::BakeSettings;
pub use subcraft_core::MeshMapKind;
pub use subcraft_export::{ExportSettings, ExportTemplate, Padding};
pub use subcraft_image::FileFormat;
pub use subcraft_project::EXTENSION as PROJECT_EXTENSION;
pub use subcraft_paint::Camera;
