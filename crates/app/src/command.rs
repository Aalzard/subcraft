//! Undoable document commands. The UI builds a [`Command`] and hands it to
//! [`Session::execute`](crate::Session::execute); widgets never edit the document directly.

use subcraft_core::{ChannelKind, EffectId, LayerId, TextureSetId};
use subcraft_doc::{BlendMode, ContentEffect, ContentEffectKind, FillChannel, Layer, MaskEffect, MaskEffectKind, PartMask, Projection};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NewLayer {
    Paint,
    Fill,
    Folder,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Tool state only. The active stroke retains the settings it captured at begin.
    SetBrushSettings(subcraft_paint::BrushSettings),
    /// Stroke-path tool state (symmetry/lazy mouse), shared by presets and future strokes.
    SetStrokeOptions(subcraft_paint::StrokeOptions),
    /// Tool state only: affects future strokes, without adding document history.
    SetBrushMaterial(subcraft_paint::BrushMaterial),
    Undo,
    Redo,
    /// Jump to a history entry (History panel click).
    JumpToHistory(usize),
    /// End a continuous edit (slider drag): the next change starts a new history entry.
    Seal,
    SelectTextureSet(TextureSetId),
    /// Change a texture set's working resolution (one of `subcraft_doc::RESOLUTIONS`),
    /// resampling its painted rasters; `None` changes every texture set. One undo step.
    SetResolution { texture_set: Option<TextureSetId>, resolution: u32 },
    SetAlbedoFilter { texture_set: TextureSetId, filter: subcraft_image::TextureFilter },
    SelectLayer(Option<LayerId>),
    SelectLayerTarget { layer: LayerId, target: crate::session::LayerTarget },
    /// Select an effect in the layer's content or mask stack (`None`: the domain itself). Tool
    /// state only: a Paint effect receives strokes; other effects show their Properties.
    SelectEffect { layer: LayerId, target: crate::session::LayerTarget, effect: Option<EffectId> },
    /// Append to the layer's content stack (top) and select it. Not on folders; Levels not on Normal.
    AddContentEffect { layer: LayerId, kind: ContentEffectKind },
    RemoveContentEffect { layer: LayerId, effect: EffectId },
    /// Replace an effect (matched by id); coalesces while dragging.
    SetContentEffect { layer: LayerId, effect: ContentEffect },
    /// Destination is a bottom-to-top index in the layer's content stack.
    MoveContentEffect { layer: LayerId, effect: EffectId, index: usize },
    /// Same-stack visual reorder; resolve UUID anchors against the current document on release.
    DropEffect { source: crate::effect_stack::EffectAddress, target: crate::effect_stack::EffectAddress, side: crate::effect_stack::DropSide },
    DuplicateContentEffect { layer: LayerId, effect: EffectId },
    /// Insert above the selected layer (or at the top) of the selected texture set and select it.
    /// A selected empty folder receives the new layer instead (Painter).
    AddLayer(NewLayer),
    /// Insert a ready-made layer (material preset, smart material) like [`Command::AddLayer`].
    InsertLayer { layer: Box<Layer>, label: String },
    /// Add a material channel (Ambient Occlusion, Emissive, Opacity…) to a texture set and
    /// enable it on the brush. No-op when the set already has it.
    AddChannel { texture_set: TextureSetId, kind: ChannelKind },
    /// Remove a channel from a texture set (layers keep their data for it, so adding the channel
    /// back, or undo, restores it). A set keeps at least one channel.
    RemoveChannel { texture_set: TextureSetId, kind: ChannelKind },
    /// Change a channel's storage format (export bit depth).
    SetChannelFormat { texture_set: TextureSetId, kind: ChannelKind, format: subcraft_doc::ChannelFormat },
    /// [`Command::InsertLayer`] that first adds these channels to the selected texture set when it
    /// lacks them (an imported texture set brings an Ambient Occlusion map). One undo step.
    InsertLayerWithChannels { layer: Box<Layer>, channels: Vec<ChannelKind>, label: String },
    /// One part can span texture sets; insert all its layers atomically.
    InsertPartLayers { layers: Vec<(TextureSetId, Box<Layer>)>, label: String },
    /// Swap a texture set's whole layer list (bottom-to-top) in one history entry; used when an
    /// external editor's changes come back (Edit in PhotoCraft). Selection keeps its layer if it
    /// still exists.
    ReplaceTextureSetLayers { texture_set: TextureSetId, layers: Vec<std::sync::Arc<Layer>>, label: String },
    /// Atomic combined edit, retaining the node's UUID.
    ReplaceLayer { layer: LayerId, replacement: Box<Layer>, label: String },
    /// Give the layer a black mask (if it has none) and append these effects (smart mask).
    ApplyMaskEffects { layer: LayerId, effects: Vec<MaskEffectKind>, label: String },
    DeleteLayer(LayerId),
    DuplicateLayer(LayerId),
    /// Multi-select operations: one undo step each. All layers must be in the same texture set;
    /// a layer inside another selected layer is covered by it.
    DeleteLayers(Vec<LayerId>),
    DuplicateLayers(Vec<LayerId>),
    SetLayersVisible { layers: Vec<LayerId>, visible: bool },
    /// Paste copied layers above the selected layer of the selected texture set (any project).
    PasteLayers(std::sync::Arc<crate::clipboard::LayerClip>),
    /// Paste copied effects onto the end of this layer's matching stack.
    PasteEffects { layer: LayerId, clip: std::sync::Arc<crate::clipboard::EffectClip> },
    RenameTextureSet { texture_set: TextureSetId, name: String },
    /// Show or hide a texture set's part of the model in the viewport. View state only: not saved,
    /// no undo step.
    SetTextureSetHidden { texture_set: TextureSetId, hidden: bool },
    /// Forget one baked mesh map of a texture set (Texture Set Settings > Mesh maps > X). Not an
    /// undo step; bake again to get it back. No-op when the map is not baked.
    ClearMeshMap { texture_set: TextureSetId, kind: subcraft_core::MeshMapKind },
    /// Wrap the layer into a new folder.
    GroupLayer(LayerId),
    /// Show only this texture set's part of the model, or everything again when it is already
    /// the only one shown (Painter's Alt+Q). View state only, like `SetTextureSetHidden`.
    IsolateTextureSet(TextureSetId),
    /// Put several layers of one texture set into one new folder, created where the topmost of
    /// them was; their stacking order is kept (Painter's Ctrl+G). One undo step.
    GroupLayers(Vec<LayerId>),
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
    /// Atomic independent toggle or solo (Alt-click), preserving every source and strength.
    ToggleFillChannel { layer: LayerId, effect: Option<EffectId>, channel: ChannelKind, solo: bool },
    ToggleBrushChannel { channel: ChannelKind, solo: bool },
    /// Turn `channel` on for a fill layer. If the texture set lacks it, the channel is added to the
    /// set (every layer gets its blend entry) in the same undo step.
    AddFillChannel { layer: LayerId, channel: ChannelKind },
    SetFillProjection { layer: LayerId, projection: Projection },
    AddMask { layer: LayerId, white: bool },
    RemoveMask(LayerId),
    CopyMask(LayerId),
    PasteMask(LayerId),
    /// Non-destructive inversion of the evaluated mask, appended as an editable effect.
    InvertMask(LayerId),
    /// Destination is a bottom-to-top index in this mask's own effect stack.
    MoveMaskEffect { layer: LayerId, effect: EffectId, index: usize },
    DuplicateMaskEffect { layer: LayerId, effect: EffectId },
    SetMaskEnabled { layer: LayerId, enabled: bool },
    /// Append to the mask's stack (top) and select it.
    AddMaskEffect { layer: LayerId, kind: MaskEffectKind },
    RemoveMaskEffect { layer: LayerId, effect: EffectId },
    /// Replace an effect (matched by id); coalesces while dragging.
    SetMaskEffect { layer: LayerId, effect: MaskEffect },
    /// Replace the picked colors of an ID Color Mask effect. Never coalesces: every pick or
    /// removal is its own undo step (ADR-0041).
    SetIdColors { layer: LayerId, effect: EffectId, colors: Vec<[f32; 3]> },
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
