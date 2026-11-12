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

/// Where dragged effects land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectDrop {
    /// Beside an effect of the stack they come from.
    Beside { effect: EffectId, side: DropSide },
    /// On a layer row: the top of that layer's stack of the same kind (content or mask). A mask
    /// is created when mask effects land on a layer that has none.
    Layer(LayerId),
}

/// `ids` (bottom to top) after moving `moving` as one block beside `anchor`, keeping the block's
/// own order. `None` when nothing would change or the anchor is one of the moved effects.
pub fn reordered(ids: &[EffectId], moving: &[EffectId], anchor: EffectId, side: DropSide) -> Option<Vec<EffectId>> {
    if moving.contains(&anchor) {
        return None;
    }
    let block: Vec<EffectId> = ids.iter().copied().filter(|id| moving.contains(id)).collect();
    let mut out: Vec<EffectId> = ids.iter().copied().filter(|id| !moving.contains(id)).collect();
    let at = out.iter().position(|id| *id == anchor)? + usize::from(side == DropSide::Above);
    out.splice(at..at, block);
    (out != ids).then_some(out)
}

/// `ids` (bottom to top) after moving every effect of `moving` one step up or down (Alt+Up /
/// Alt+Down). Effects already at the end stay, and the ones behind them do not overtake them.
/// `None` when nothing moves.
pub fn nudged(ids: &[EffectId], moving: &[EffectId], up: bool) -> Option<Vec<EffectId>> {
    let mut out = ids.to_vec();
    let n = out.len();
    let order: Vec<usize> = if up { (0..n).rev().collect() } else { (0..n).collect() };
    for i in order {
        if !moving.contains(&out[i]) {
            continue;
        }
        let j = if up { i + 1 } else { i.wrapping_sub(1) };
        if j < n && !moving.contains(&out[j]) {
            out.swap(i, j);
        }
    }
    (out != ids).then_some(out)
}

/// Several selected effects of one stack (UI state). The session keeps the primary effect, the
/// one Properties shows; this adds the Ctrl / Shift click extras around it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectSelection {
    stack: Option<(LayerId, LayerTarget)>,
    ids: Vec<EffectId>,
}

impl EffectSelection {
    pub fn clear(&mut self) {
        self.stack = None;
        self.ids.clear();
    }

    /// Apply a click on `clicked`. `rows` is the stack in any consistent order; `primary` is the
    /// session's selected effect of this stack. Returns the effect that should be primary now.
    pub fn click(&mut self, stack: (LayerId, LayerTarget), rows: &[EffectId], primary: Option<EffectId>, clicked: EffectId, toggle: bool, range: bool) -> Option<EffectId> {
        if self.stack != Some(stack) {
            self.ids.clear();
        }
        self.stack = Some(stack);
        self.ids.retain(|id| rows.contains(id));
        if let Some(p) = primary
            && !self.ids.contains(&p)
        {
            self.ids.push(p);
        }
        let pos = |id: EffectId| rows.iter().position(|r| *r == id);
        if range && let (Some(a), Some(b)) = (primary.and_then(pos), pos(clicked)) {
            let span = rows[a.min(b)..=a.max(b)].iter().copied();
            if toggle {
                for id in span {
                    if !self.ids.contains(&id) {
                        self.ids.push(id);
                    }
                }
            } else {
                self.ids = span.collect();
            }
            primary
        } else if toggle {
            if let Some(i) = self.ids.iter().position(|id| *id == clicked) {
                self.ids.remove(i);
                // The primary effect left the selection: another selected one takes over.
                if primary == Some(clicked) { self.ids.last().copied() } else { primary }
            } else {
                self.ids.push(clicked);
                Some(clicked)
            }
        } else {
            self.ids = vec![clicked];
            Some(clicked)
        }
    }

    /// The effects an operation applies to: the multi-selection when it belongs to this stack
    /// and includes `primary`, else just `primary`.
    pub fn targets(&self, stack: (LayerId, LayerTarget), primary: Option<EffectId>) -> Vec<EffectId> {
        match primary {
            Some(p) if self.stack == Some(stack) && self.ids.len() > 1 && self.ids.contains(&p) => self.ids.clone(),
            Some(p) => vec![p],
            None => Vec::new(),
        }
    }

    pub fn contains(&self, stack: (LayerId, LayerTarget), primary: Option<EffectId>, id: EffectId) -> bool {
        self.targets(stack, primary).contains(&id)
    }
}

/// What the stack helpers need from a content or mask effect.
pub(crate) trait StackEffect: Clone {
    fn id(&self) -> EffectId;
    fn set_id(&mut self, id: EffectId);
    fn name_mut(&mut self) -> &mut String;
}

impl StackEffect for subcraft_doc::ContentEffect {
    fn id(&self) -> EffectId { self.id }
    fn set_id(&mut self, id: EffectId) { self.id = id }
    fn name_mut(&mut self) -> &mut String { &mut self.name }
}

impl StackEffect for subcraft_doc::MaskEffect {
    fn id(&self) -> EffectId { self.id }
    fn set_id(&mut self, id: EffectId) { self.id = id }
    fn name_mut(&mut self) -> &mut String { &mut self.name }
}

type Stack<T> = Vec<std::sync::Arc<T>>;

pub(crate) fn stack_ids<T: StackEffect>(stack: &Stack<T>) -> Vec<EffectId> {
    stack.iter().map(|e| e.id()).collect()
}

/// Reorder `stack` to `order` (a permutation of its ids).
pub(crate) fn apply_order<T: StackEffect>(stack: &mut Stack<T>, order: &[EffectId]) {
    stack.sort_by_key(|e| order.iter().position(|id| *id == e.id()).unwrap_or(usize::MAX));
}

/// Take `ids` out of `stack`, in stack order.
pub(crate) fn take<T: StackEffect>(stack: &mut Stack<T>, ids: &[EffectId]) -> Stack<T> {
    let (taken, kept): (Stack<T>, Stack<T>) = std::mem::take(stack).into_iter().partition(|e| ids.contains(&e.id()));
    *stack = kept;
    taken
}

/// Copies of `ids` with fresh ids and " copy" names, in stack order, each paired with the id it
/// was copied from.
pub(crate) fn copies<T: StackEffect>(stack: &Stack<T>, ids: &[EffectId]) -> Vec<(EffectId, std::sync::Arc<T>)> {
    stack
        .iter()
        .filter(|e| ids.contains(&e.id()))
        .map(|e| {
            let mut copy = (**e).clone();
            copy.set_id(EffectId::new());
            let name = copy.name_mut();
            *name = format!("{name} copy");
            (e.id(), std::sync::Arc::new(copy))
        })
        .collect()
}

/// The effects a drop carries: `ids` taken out of `stack`, or copies of them when `copy`.
/// Also returns which of them stands for `grabbed` (itself, or its copy).
pub(crate) fn lift<T: StackEffect>(stack: &mut Stack<T>, ids: &[EffectId], copy: bool, grabbed: EffectId) -> (Stack<T>, EffectId) {
    if !copy {
        return (take(stack, ids), grabbed);
    }
    let copies = copies(stack, ids);
    let selected = copies.iter().find(|(from, _)| *from == grabbed).map_or(grabbed, |(_, c)| c.id());
    (copies.into_iter().map(|(_, c)| c).collect(), selected)
}

/// Copy each effect of `ids` right above its original. Returns the new ids, bottom to top.
pub(crate) fn duplicate_in_place<T: StackEffect>(stack: &mut Stack<T>, ids: &[EffectId]) -> Vec<EffectId> {
    let mut new = Vec::new();
    for (from, copy) in copies(stack, ids) {
        let at = stack.iter().position(|e| e.id() == from).expect("copied from this stack") + 1;
        new.push(copy.id());
        stack.insert(at, copy);
    }
    new
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
    #[test]
    fn a_block_of_effects_moves_beside_an_anchor_keeping_its_own_order() {
        let ids: Vec<EffectId> = (0..6).map(|_| EffectId::new()).collect();
        let pick = |v: &[usize]| v.iter().map(|i| ids[*i]).collect::<Vec<_>>();
        // Given top-first or bottom-first, the block keeps the stack order.
        assert_eq!(reordered(&ids, &pick(&[4, 1]), ids[2], DropSide::Above), Some(pick(&[0, 2, 1, 4, 3, 5])));
        assert_eq!(reordered(&ids, &pick(&[1, 4]), ids[5], DropSide::Above), Some(pick(&[0, 2, 3, 5, 1, 4])));
        assert_eq!(reordered(&ids, &pick(&[1, 4]), ids[0], DropSide::Below), Some(pick(&[1, 4, 0, 2, 3, 5])));
        // Dropping a contiguous block where it already is, or on one of its own rows, changes nothing.
        assert_eq!(reordered(&ids, &pick(&[2, 3]), ids[4], DropSide::Below), None);
        assert_eq!(reordered(&ids, &pick(&[2, 3]), ids[1], DropSide::Above), None);
        assert_eq!(reordered(&ids, &pick(&[2, 3]), ids[3], DropSide::Above), None);
        assert_eq!(reordered(&ids, &pick(&[2]), EffectId::new(), DropSide::Above), None, "stale anchor");
        // One effect: the same answers as the single-effect drop.
        for from in 0..6 {
            for to in 0..6 {
                for side in [DropSide::Above, DropSide::Below] {
                    let mut expected = ids.clone();
                    if from != to {
                        let moved = expected.remove(from);
                        let anchor = expected.iter().position(|v| *v == ids[to]).unwrap();
                        expected.insert(anchor + usize::from(side == DropSide::Above), moved);
                    }
                    let got = reordered(&ids, &[ids[from]], ids[to], side).unwrap_or(ids.clone());
                    assert_eq!(got, expected);
                }
            }
        }
    }

    #[test]
    fn nudging_moves_the_selection_one_step_and_stops_at_the_ends() {
        let ids: Vec<EffectId> = (0..5).map(|_| EffectId::new()).collect();
        let pick = |v: &[usize]| v.iter().map(|i| ids[*i]).collect::<Vec<_>>();
        assert_eq!(nudged(&ids, &pick(&[1]), true), Some(pick(&[0, 2, 1, 3, 4])));
        assert_eq!(nudged(&ids, &pick(&[1]), false), Some(pick(&[1, 0, 2, 3, 4])));
        assert_eq!(nudged(&ids, &pick(&[4]), true), None, "already on top");
        assert_eq!(nudged(&ids, &pick(&[0]), false), None, "already at the bottom");
        // A block moves together; split selections keep their gaps.
        assert_eq!(nudged(&ids, &pick(&[1, 2]), true), Some(pick(&[0, 3, 1, 2, 4])));
        assert_eq!(nudged(&ids, &pick(&[1, 3]), false), Some(pick(&[1, 0, 3, 2, 4])));
        // The top one is stuck: the one right below must not jump over it.
        assert_eq!(nudged(&ids, &pick(&[3, 4]), true), None);
        assert_eq!(nudged(&ids, &pick(&[2, 4]), true), Some(pick(&[0, 1, 3, 2, 4])));
        assert_eq!(nudged(&ids, &[], true), None);
    }

    #[test]
    fn effect_selection_follows_ctrl_and_shift_clicks_within_one_stack() {
        let rows: Vec<EffectId> = (0..5).map(|_| EffectId::new()).collect();
        let stack = (LayerId::new(), LayerTarget::Content);
        let mut sel = EffectSelection::default();
        // A plain click selects one.
        assert_eq!(sel.click(stack, &rows, None, rows[1], false, false), Some(rows[1]));
        assert_eq!(sel.targets(stack, Some(rows[1])), [rows[1]]);
        // Ctrl adds and makes the clicked one primary.
        assert_eq!(sel.click(stack, &rows, Some(rows[1]), rows[3], true, false), Some(rows[3]));
        assert_eq!(sel.targets(stack, Some(rows[3])), [rows[1], rows[3]]);
        // Ctrl on a selected one removes it; removing the primary hands over to another.
        assert_eq!(sel.click(stack, &rows, Some(rows[3]), rows[3], true, false), Some(rows[1]));
        assert_eq!(sel.targets(stack, Some(rows[1])), [rows[1]]);
        // Shift selects the span from the primary, which stays primary.
        assert_eq!(sel.click(stack, &rows, Some(rows[1]), rows[4], false, true), Some(rows[1]));
        assert_eq!(sel.targets(stack, Some(rows[1])), rows[1..=4]);
        assert!(sel.contains(stack, Some(rows[1]), rows[2]) && !sel.contains(stack, Some(rows[1]), rows[0]));
        // The primary selected without a click (a new effect, undo) still joins a Ctrl click.
        let mut fresh = EffectSelection::default();
        fresh.click(stack, &rows, Some(rows[0]), rows[2], true, false);
        assert_eq!(fresh.targets(stack, Some(rows[2])), [rows[0], rows[2]]);
        // Another stack, or a primary outside the selection, falls back to the primary alone.
        let other = (LayerId::new(), LayerTarget::Content);
        assert_eq!(sel.targets(other, Some(rows[1])), [rows[1]]);
        assert_eq!(sel.targets((stack.0, LayerTarget::Mask), Some(rows[1])), [rows[1]]);
        assert_eq!(sel.targets(stack, Some(rows[0])), [rows[0]]);
        assert!(sel.targets(stack, None).is_empty());
        // Removed effects drop out on the next click.
        sel.click(stack, &rows[..3], Some(rows[1]), rows[0], true, false);
        assert_eq!(sel.targets(stack, Some(rows[0])), [rows[1], rows[2], rows[0]]);
        sel.clear();
        assert_eq!(sel.targets(stack, Some(rows[0])), [rows[0]]);
    }
}
