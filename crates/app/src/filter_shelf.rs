//! Filters from the Shelf: clicking a filter swaps the type of the selected filter effect (its
//! channel, blend and visibility stay), or adds the filter to the selected layer: to its mask
//! when the mask is being edited, otherwise to its content on the first channel it supports.

use subcraft_core::ChannelKind;
use subcraft_doc::{ContentEffectKind, Filter, MaskEffectKind};

use crate::session::LayerTarget;
use crate::{Command, Session};

impl Session {
    /// The command a Shelf click on `filter` runs, or `None` when no layer is selected or the
    /// filter fits nowhere (e.g. a color-only filter on a set without color channels).
    pub fn shelf_filter_command(&self, filter: Filter) -> Option<Command> {
        let layer = self.selected_layer()?;
        let effect = self.selected_effect();
        match self.layer_target() {
            LayerTarget::Mask => {
                let mask = layer.mask.as_ref()?;
                if !filter.supports_mask() {
                    return None;
                }
                if let Some(e) = effect.and_then(|id| mask.effects.iter().find(|e| e.id == id))
                    && let MaskEffectKind::Filter(old) = &e.kind
                {
                    let mut e = subcraft_doc::MaskEffect::clone(e);
                    if e.name == old.label() {
                        e.name = filter.label().into();
                    }
                    e.kind = MaskEffectKind::Filter(filter);
                    return Some(Command::SetMaskEffect { layer: layer.id, effect: e });
                }
                Some(Command::AddMaskEffect { layer: layer.id, kind: MaskEffectKind::Filter(filter) })
            }
            LayerTarget::Content => {
                if let Some(e) = effect.and_then(|id| layer.content_effects.iter().find(|e| e.id == id))
                    && let ContentEffectKind::Filter { channel, filter: old } = &e.kind
                    && filter.supports_channel(*channel)
                {
                    let mut e = subcraft_doc::ContentEffect::clone(e);
                    let channel = *channel;
                    if e.name == old.label() || e.name == e.kind.label() {
                        e.name = ContentEffectKind::Filter { channel, filter }.label();
                    }
                    e.kind = ContentEffectKind::Filter { channel, filter };
                    return Some(Command::SetContentEffect { layer: layer.id, effect: e });
                }
                let channels: Vec<ChannelKind> = self.selected_texture_set().channels.iter().map(|c| c.kind).collect();
                let channel = std::iter::once(ChannelKind::BaseColor)
                    .filter(|k| channels.contains(k))
                    .chain(channels.iter().copied())
                    .find(|k| filter.supports_channel(*k))?;
                Some(Command::AddContentEffect { layer: layer.id, kind: ContentEffectKind::Filter { channel, filter } })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter_of(s: &Session) -> Vec<(ChannelKind, Filter)> {
        s.selected_layer().unwrap().content_effects.iter().filter_map(|e| match &e.kind {
            ContentEffectKind::Filter { channel, filter } => Some((*channel, filter.clone())),
            _ => None,
        }).collect()
    }

    #[test]
    fn shelf_filters_add_then_swap_the_selected_one() {
        let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 32);
        let blur = Filter::ALL.iter().find(|f| matches!(f, Filter::Blur { .. })).unwrap().clone();
        let invert = Filter::Invert;
        let cmd = s.shelf_filter_command(blur.clone()).expect("a layer is selected");
        s.execute(cmd).unwrap();
        assert_eq!(filter_of(&s), vec![(ChannelKind::BaseColor, blur)]);
        // Select the new filter, then a second Shelf click swaps its type in place.
        let layer = s.selected_layer().unwrap();
        let (lid, eid) = (layer.id, layer.content_effects.last().unwrap().id);
        s.execute(Command::SelectEffect { layer: lid, target: LayerTarget::Content, effect: Some(eid) }).unwrap();
        let cmd = s.shelf_filter_command(invert.clone()).unwrap();
        assert!(matches!(cmd, Command::SetContentEffect { .. }), "swaps rather than adds");
        s.execute(cmd).unwrap();
        assert_eq!(filter_of(&s), vec![(ChannelKind::BaseColor, invert)]);
        assert_eq!(s.selected_layer().unwrap().content_effects.last().unwrap().id, eid, "same effect");
        s.execute(Command::Undo).unwrap();
        assert!(matches!(filter_of(&s)[0].1, Filter::Blur { .. }), "one undo step");
    }
}
