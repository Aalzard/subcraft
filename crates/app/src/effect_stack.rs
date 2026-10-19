//! UUID-based visual effect drops. Document indices run bottom-to-top, after removal.
use crate::LayerTarget;
use subcraft_core::{EffectId, LayerId, TextureSetId};
use subcraft_doc::TextureSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectAddress {
    pub texture_set: TextureSetId,
    pub layer: LayerId,
    pub domain: LayerTarget,
    pub effect: EffectId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropSide {
    Above,
    Below,
}

/// Reorder within the same layer/domain only. Stale, incompatible and unchanged drops are None.
pub fn drop_destination(
    ts: &TextureSet,
    source: EffectAddress,
    target: EffectAddress,
    side: DropSide,
) -> Option<usize> {
    if source.texture_set != ts.id
        || target.texture_set != ts.id
        || source.layer != target.layer
        || source.domain != target.domain
        || source.effect == target.effect
    {
        return None;
    }
    let layer = ts.find(source.layer)?;
    let ids: Vec<_> = match source.domain {
        LayerTarget::Content => layer.content_effects.iter().map(|e| e.id).collect(),
        LayerTarget::Mask => layer.mask.as_ref()?.effects.iter().map(|e| e.id).collect(),
    };
    let from = ids.iter().position(|id| *id == source.effect)?;
    let at = ids.iter().position(|id| *id == target.effect)?;
    let mut index = at + usize::from(side == DropSide::Above);
    if from < index {
        index -= 1;
    }
    (index != from).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use subcraft_core::ChannelKind;
    use subcraft_doc::{
        ContentEffect, ContentEffectKind, Layer, Levels, Mask, MaskEffect, MaskEffectKind,
    };
    #[test]
    fn every_visual_anchor_pair_resolves_indices_after_removal_in_both_domains() {
        for count in 1..8 {
            let mut ts = TextureSet::new("T", 0, 128);
            let mut layer = Layer::new_fill("F", &ts);
            layer.mask = Some(Mask::new(128, false));
            for _ in 0..count {
                layer.content_effects.push(Arc::new(ContentEffect::new(
                    ContentEffectKind::Levels {
                        channel: ChannelKind::BaseColor,
                        levels: Levels::default(),
                    },
                    &ts,
                )));
                layer
                    .mask
                    .as_mut()
                    .unwrap()
                    .effects
                    .push(Arc::new(MaskEffect::new(MaskEffectKind::Invert)));
            }
            let id = ts.insert_above(None, layer);
            let layer = ts.find(id).unwrap();
            for domain in [LayerTarget::Content, LayerTarget::Mask] {
                let ids: Vec<_> = if domain == LayerTarget::Content {
                    layer.content_effects.iter().map(|e| e.id).collect()
                } else {
                    layer
                        .mask
                        .as_ref()
                        .unwrap()
                        .effects
                        .iter()
                        .map(|e| e.id)
                        .collect()
                };
                let address = |i| EffectAddress {
                    texture_set: ts.id,
                    layer: id,
                    domain,
                    effect: ids[i],
                };
                for from in 0..count {
                    for to in 0..count {
                        for side in [DropSide::Above, DropSide::Below] {
                            let actual = drop_destination(&ts, address(from), address(to), side);
                            let mut expected = ids.clone();
                            if from != to {
                                let moved = expected.remove(from);
                                let anchor = expected.iter().position(|v| *v == ids[to]).unwrap();
                                expected
                                    .insert(anchor + usize::from(side == DropSide::Above), moved);
                            }
                            let mut reordered = ids.clone();
                            if let Some(at) = actual {
                                let moved = reordered.remove(from);
                                reordered.insert(at, moved);
                            }
                            assert_eq!(reordered, expected);
                            assert_eq!(actual.is_none(), expected == ids);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn stale_or_foreign_addresses_never_resolve() {
        let mut ts = TextureSet::new("T", 0, 128);
        let mut l = Layer::new_fill("F", &ts);
        for _ in 0..2 {
            l.content_effects.push(Arc::new(ContentEffect::new(
                ContentEffectKind::Levels {
                    channel: ChannelKind::BaseColor,
                    levels: Levels::default(),
                },
                &ts,
            )));
        }
        let id = ts.insert_above(None, l);
        let l = ts.find(id).unwrap();
        let source = EffectAddress {
            texture_set: ts.id,
            layer: id,
            domain: LayerTarget::Content,
            effect: l.content_effects[0].id,
        };
        let target = EffectAddress {
            effect: l.content_effects[1].id,
            ..source
        };
        for bad in [
            EffectAddress {
                texture_set: TextureSetId::new(),
                ..source
            },
            EffectAddress {
                layer: LayerId::new(),
                ..source
            },
            EffectAddress {
                domain: LayerTarget::Mask,
                ..source
            },
            EffectAddress {
                effect: EffectId::new(),
                ..source
            },
        ] {
            assert!(drop_destination(&ts, bad, target, DropSide::Above).is_none());
            assert!(drop_destination(&ts, source, bad, DropSide::Above).is_none());
        }
    }
}
