//! Application layer: the editing [`Session`], undoable [`Command`]s and the shortcut table.
//!
//! No UI toolkit and no GPU here: front ends translate input into commands, actions and stroke
//! calls, and read back the document, history and composites to display them.

pub mod actions;
pub mod command;
pub mod session;

pub use actions::{ACTIONS, Action, ActionInfo, Key, Shortcut, action_for};
pub use command::{Command, CommandError, NewLayer};
pub use session::{BrushState, CompositeUpdate, DEFAULT_RESOLUTION, Pick, Scene, Selection, Session};

pub use subcraft_compositor::{Composite, composite_comps};
pub use subcraft_paint::Camera;
