//! Layer-tree drop semantics shared by front ends. Adapted from CalangoTech's Studio fork
//! (bd6c064); indices in the document run bottom-to-top and are measured after removal.

use subcraft_core::LayerId;
use subcraft_doc::{LayerLocation, TextureSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    Above(LayerId),
    Below(LayerId),
    Inside(LayerId),
    RootTop,
    RootBottom,
}

/// `None` for stale identities, cycles, non-folder containers and unchanged positions.
pub fn drop_destination(ts: &TextureSet, source: LayerId, target: DropTarget) -> Option<LayerLocation> {
    let from = ts.location(source)?;
    let (parent, mut index) = match target {
        DropTarget::Inside(id) => {
            let folder = ts.find(id)?;
            if !folder.is_folder() { return None; }
            (Some(id), folder.children().len())
        }
        DropTarget::Above(id) | DropTarget::Below(id) => {
            if source == id { return None; }
            let location = ts.location(id)?;
            (location.parent, location.index + usize::from(matches!(target, DropTarget::Above(_))))
        }
        DropTarget::RootTop => (None, ts.layers.len()),
        DropTarget::RootBottom => (None, 0),
    };
    if parent.is_some_and(|id| ts.is_descendant_or_self(id, source)) { return None; }
    if from.parent == parent && from.index < index { index -= 1; }
    let destination = LayerLocation { parent, index };
    (destination != from).then_some(destination)
}
