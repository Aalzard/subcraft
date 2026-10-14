//! Undoable document commands. The UI builds a [`Command`] and hands it to
//! [`Session::execute`](crate::Session::execute); widgets never edit the document directly.

use subcraft_core::{ChannelKind, EffectId, LayerId, TextureSetId};
use subcraft_doc::{BlendMode, FillChannel, Layer, MaskEffect, MaskEffectKind, PartMask, Projection};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NewLayer {
    Paint,
    Fill,
    Folder,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Undo,
    Redo,
    /// Jump to a history entry (History panel click).
    JumpToHistory(usize),
    /// End a continuous edit (slider drag): the next change starts a new history entry.
    Seal,
    SelectTextureSet(TextureSetId),
    SelectLayer(Option<LayerId>),
    /// Insert above the selected layer (or at the top) of the selected texture set and select it.
    AddLayer(NewLayer),
    /// Insert a ready-made layer (material preset, smart material) like [`Command::AddLayer`].
    InsertLayer { layer: Box<Layer>, label: String },
    /// One part can span texture sets; insert all its layers atomically.
    InsertPartLayers { layers: Vec<(TextureSetId, Box<Layer>)>, label: String },
    /// Atomic combined edit, retaining the node's UUID.
    ReplaceLayer { layer: LayerId, replacement: Box<Layer>, label: String },
    /// Give the layer a black mask (if it has none) and append these effects (smart mask).
    ApplyMaskEffects { layer: LayerId, effects: Vec<MaskEffectKind>, label: String },
    DeleteLayer(LayerId),
    DuplicateLayer(LayerId),
    /// Wrap the layer into a new folder.
    GroupLayer(LayerId),
    MoveLayer { layer: LayerId, parent: Option<LayerId>, index: usize },
    /// Visual layer-tree drop. The controller validates UUIDs, cycles and insertion indices.
    DropLayer { texture_set: TextureSetId, layer: LayerId, target: crate::layer_tree::DropTarget },
    /// One step up / down among siblings (up = towards the top of the stack).
    MoveUp(LayerId),
    MoveDown(LayerId),
    SetVisible { layer: LayerId, visible: bool },
    SetFolderExpanded { layer: LayerId, expanded: bool },
    Rename { layer: LayerId, name: String },
    SetBlendMode { layer: LayerId, channel: ChannelKind, mode: BlendMode },
    /// Coalesces with neighbouring opacity edits of the same layer/channel until [`Command::Seal`].
    SetBlendOpacity { layer: LayerId, channel: ChannelKind, opacity: f32 },
    /// Coalesces like opacity.
    SetFillChannel { layer: LayerId, channel: ChannelKind, fill: FillChannel },
    SetFillProjection { layer: LayerId, projection: Projection },
    AddMask { layer: LayerId, white: bool },
    RemoveMask(LayerId),
    SetMaskEnabled { layer: LayerId, enabled: bool },
    AddMaskEffect { layer: LayerId, kind: MaskEffectKind },
    RemoveMaskEffect { layer: LayerId, effect: EffectId },
    /// Replace an effect (matched by id); coalesces while dragging.
    SetMaskEffect { layer: LayerId, effect: MaskEffect },
    /// Show the layer only on these mesh parts (`None` = everywhere).
    SetParts { layer: LayerId, parts: Option<PartMask> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandError {
    LayerNotFound,
    TextureSetNotFound,
    NotAFillLayer,
    NoMask,
    /// A paint stroke is in progress; finish it first.
    StrokeInProgress,
    Invalid(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::LayerNotFound => f.write_str("layer not found"),
            CommandError::TextureSetNotFound => f.write_str("texture set not found"),
            CommandError::NotAFillLayer => f.write_str("not a fill layer"),
            CommandError::NoMask => f.write_str("layer has no mask"),
            CommandError::StrokeInProgress => f.write_str("a stroke is in progress"),
            CommandError::Invalid(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for CommandError {}
