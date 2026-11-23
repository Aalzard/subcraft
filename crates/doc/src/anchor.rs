//! Anchor points (ADR-0045): named captures of a layer's content or mask stack at one point of
//! that stack, which other layers read through an [`AnchorInput`].
//!
//! * An anchor is an effect ([`crate::ContentEffectKind::Anchor`] or
//!   [`crate::MaskEffectKind::Anchor`]); its name is the effect's name, its identity the effect's
//!   UUID. It changes nothing where it sits.
//! * A content anchor holds, per channel, the layer's own content evaluated up to the anchor
//!   (base paint or fill, folder children, content effects below it), as straight
//!   `(value, alpha)`, before the layer's mask, blend and opacity. A mask anchor holds the mask
//!   value evaluated up to it.
//! * References sit in a fill layer's channel ([`crate::FillSource::Anchor`]), in a mask
//!   ([`crate::MaskEffectKind::AnchorRef`]), as a generator's input image
//!   ([`crate::Generator::input`]) and in the image slots of a filter, on content or on a mask
//!   ([`crate::ImageSource::Anchor`]).
//! * Evaluation order is a single walk: for each layer bottom to top, its folder children first,
//!   then its content base (where fill-channel references sit), its content effects, then its mask
//!   effects. A reference resolves only to an anchor strictly earlier in that walk, so the
//!   dependencies always form a DAG; anything else is reported (missing, not below, cycle) and
//!   evaluates as if the reference were not there.
//! * Anchors are captured whether or not their layer is visible or masked out: a reference reads
//!   the layer's content, not its contribution to the stack.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use subcraft_core::{ChannelKind, EffectId, LayerId};

use crate::effect::ContentEffectKind;
use crate::fill::FillSource;
use crate::layer::{Layer, LayerContent};
use crate::mask::{MaskEffect, MaskEffectKind};

/// What a reference reads: an anchor and, for a content anchor, which channel of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnchorInput {
    /// Effect id of the anchor point.
    pub anchor: EffectId,
    /// Channel read from a content anchor. Mask anchors hold one grayscale value and ignore it.
    pub channel: ChannelKind,
}

impl AnchorInput {
    pub fn new(anchor: EffectId, channel: ChannelKind) -> Self {
        Self { anchor, channel }
    }
}

/// Which stack of its layer an anchor sits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnchorDomain {
    Content,
    Mask,
}

/// An anchor point found in a layer tree.
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorInfo {
    pub id: EffectId,
    pub name: String,
    pub layer: LayerId,
    pub layer_name: String,
    pub domain: AnchorDomain,
    /// Index of the anchor effect in its stack.
    pub index: usize,
    ordinal: usize,
}

/// Where a reference sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefSite {
    /// A fill layer's channel whose source is an anchor.
    Fill { layer: LayerId, channel: ChannelKind },
    /// A mask effect: an Anchor Reference, a generator with an anchor input, or a filter with an
    /// anchor in its image slot (mask filters have one slot at most).
    Mask { layer: LayerId, effect: EffectId },
    /// An image slot of a content filter.
    Content { layer: LayerId, effect: EffectId, slot: usize },
}

impl RefSite {
    pub fn layer(&self) -> LayerId {
        match *self {
            RefSite::Fill { layer, .. } | RefSite::Mask { layer, .. } | RefSite::Content { layer, .. } => layer,
        }
    }
}

/// Whether a reference can be evaluated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefStatus {
    Ok,
    /// No anchor with that id in this texture set (deleted, or copied from elsewhere).
    Missing,
    /// The anchor is evaluated after the reference (above it, or later in the same stack).
    NotBelow,
    /// The anchor's own capture depends on this reference.
    Cycle,
}

impl RefStatus {
    pub fn is_ok(self) -> bool {
        self == RefStatus::Ok
    }

    /// Short explanation for the UI.
    pub fn message(self) -> &'static str {
        match self {
            RefStatus::Ok => "",
            RefStatus::Missing => "Anchor not found: it was deleted or belongs to another texture set. This input is ignored.",
            RefStatus::NotBelow => "Anchor is not below this point in the stack: move it lower. This input is ignored.",
            RefStatus::Cycle => "Circular reference: the anchor depends on this input. This input is ignored.",
        }
    }
}

/// A reference found in a layer tree, with its resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorRef {
    pub site: RefSite,
    pub input: AnchorInput,
    pub status: RefStatus,
    ordinal: usize,
}

/// All anchors and references of one layer tree (one texture set), in evaluation order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnchorIndex {
    pub anchors: Vec<AnchorInfo>,
    pub refs: Vec<AnchorRef>,
    /// Evaluation position of every place a reference can sit (for the picker).
    sites: HashMap<RefSite, usize>,
    /// Folder -> every layer below it in the tree.
    descendants: HashMap<LayerId, HashSet<LayerId>>,
    /// Layer -> (name, last evaluation position of the layer, its mask included).
    layers: HashMap<LayerId, (String, usize)>,
}

/// The anchor input a mask effect reads, if any. A generator keeps the input it had before
/// its kind changed to one that reads no input (it comes back if the kind is switched back),
/// but that input is not read, so it is no reference: it used to count as one, and deleting
/// its anchor left an "Anchor not found" problem the Properties panel could not clear.
pub fn mask_effect_input(kind: &MaskEffectKind) -> Option<AnchorInput> {
    match kind {
        MaskEffectKind::AnchorRef { input, .. } => Some(*input),
        MaskEffectKind::Generator(g) => g.input.filter(|_| g.accepts_input()),
        MaskEffectKind::Filter(f) => f.anchor_inputs().first().map(|(_, input)| *input),
        _ => None,
    }
}

/// The anchor input a fill source reads, if any.
pub fn fill_source_input(source: &FillSource) -> Option<AnchorInput> {
    match source {
        FillSource::Anchor { input } => Some(*input),
        _ => None,
    }
}

/// Whether any layer of the tree (hidden ones included) reads an anchor. Cheap: no allocation.
pub fn has_references(layers: &[Arc<Layer>]) -> bool {
    layers.iter().any(|l| layer_has_references(l) || has_references(l.children()))
}

/// Whether this layer itself (not its children) reads an anchor.
pub fn layer_has_references(l: &Layer) -> bool {
    mask_has_references(l) || fill_has_references(l) || content_has_references(l)
}

/// The anchor inputs a content effect reads, by image slot.
pub fn content_effect_inputs(kind: &ContentEffectKind) -> Vec<(usize, AnchorInput)> {
    match kind {
        ContentEffectKind::Filter { filter, .. } => filter.anchor_inputs(),
        _ => Vec::new(),
    }
}

/// Whether one of this layer's content filters reads an anchor.
pub fn content_has_references(l: &Layer) -> bool {
    l.content_effects.iter().any(|e| !content_effect_inputs(&e.kind).is_empty())
}

/// Whether this layer's mask reads an anchor.
pub fn mask_has_references(l: &Layer) -> bool {
    l.mask.as_ref().is_some_and(|m| m.effects.iter().any(|e| mask_effect_input(&e.kind).is_some()))
}

/// Whether one of this fill layer's channels reads an anchor.
pub fn fill_has_references(l: &Layer) -> bool {
    matches!(&l.content, LayerContent::Fill(f) if f.channels.values().any(|c| fill_source_input(&c.source).is_some()))
}

/// Whether the tree holds any anchor point.
pub fn has_anchors(layers: &[Arc<Layer>]) -> bool {
    layers.iter().any(|l| {
        l.content_effects.iter().any(|e| matches!(e.kind, ContentEffectKind::Anchor))
            || l.mask.as_ref().is_some_and(|m| m.effects.iter().any(|e| matches!(e.kind, MaskEffectKind::Anchor)))
            || has_anchors(l.children())
    })
}

impl AnchorIndex {
    pub fn build(layers: &[Arc<Layer>]) -> Self {
        let mut index = Self::default();
        let mut ordinal = 0usize;
        let mut stack = Vec::new();
        for l in layers {
            index.visit(l, &mut ordinal, &mut stack);
        }
        index.resolve_all();
        index
    }

    fn visit(&mut self, layer: &Layer, ordinal: &mut usize, ancestors: &mut Vec<LayerId>) {
        for a in ancestors.iter() {
            self.descendants.entry(*a).or_default().insert(layer.id);
        }
        ancestors.push(layer.id);
        for c in layer.children() {
            self.visit(c, ordinal, ancestors);
        }
        ancestors.pop();
        let mut next = || {
            *ordinal += 1;
            *ordinal
        };
        let base = next();
        if let LayerContent::Fill(f) = &layer.content {
            for (kind, ch) in &f.channels {
                let site = RefSite::Fill { layer: layer.id, channel: *kind };
                self.sites.insert(site, base);
                if let Some(input) = fill_source_input(&ch.source) {
                    self.refs.push(AnchorRef { site, input, status: RefStatus::Ok, ordinal: base });
                }
            }
        }
        for (i, e) in layer.content_effects.iter().enumerate() {
            let o = next();
            if let ContentEffectKind::Filter { filter, .. } = &e.kind {
                for slot in 0..2 {
                    if filter.slot_accepts_anchor(slot) {
                        self.sites.insert(RefSite::Content { layer: layer.id, effect: e.id, slot }, o);
                    }
                }
            }
            for (slot, input) in content_effect_inputs(&e.kind) {
                self.refs.push(AnchorRef { site: RefSite::Content { layer: layer.id, effect: e.id, slot }, input, status: RefStatus::Ok, ordinal: o });
            }
            if matches!(e.kind, ContentEffectKind::Anchor) {
                self.anchors.push(AnchorInfo {
                    id: e.id,
                    name: e.name.clone(),
                    layer: layer.id,
                    layer_name: layer.name.clone(),
                    domain: AnchorDomain::Content,
                    index: i,
                    ordinal: o,
                });
            }
        }
        if let Some(mask) = &layer.mask {
            for (i, e) in mask.effects.iter().enumerate() {
                let o = next();
                let site = RefSite::Mask { layer: layer.id, effect: e.id };
                self.sites.insert(site, o);
                if matches!(e.kind, MaskEffectKind::Anchor) {
                    self.anchors.push(AnchorInfo {
                        id: e.id,
                        name: e.name.clone(),
                        layer: layer.id,
                        layer_name: layer.name.clone(),
                        domain: AnchorDomain::Mask,
                        index: i,
                        ordinal: o,
                    });
                }
                if let Some(input) = mask_effect_input(&e.kind) {
                    self.refs.push(AnchorRef { site, input, status: RefStatus::Ok, ordinal: o });
                }
            }
        }
        self.layers.insert(layer.id, (layer.name.clone(), *ordinal));
    }

    fn resolve_all(&mut self) {
        for i in 0..self.refs.len() {
            let r = &self.refs[i];
            let status = match self.anchor(r.input.anchor) {
                None => RefStatus::Missing,
                Some(a) if a.ordinal < r.ordinal => RefStatus::Ok,
                Some(a) => {
                    if self.depends_on(a, i) { RefStatus::Cycle } else { RefStatus::NotBelow }
                }
            };
            self.refs[i].status = status;
        }
    }

    /// Whether `anchor`'s capture reaches reference `target` (directly or through other anchors).
    fn depends_on(&self, anchor: &AnchorInfo, target: usize) -> bool {
        let mut seen = HashSet::new();
        let mut todo = vec![anchor.id];
        while let Some(id) = todo.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(a) = self.anchor(id) else { continue };
            for (i, r) in self.refs.iter().enumerate() {
                if self.captures(a, r) {
                    if i == target {
                        return true;
                    }
                    todo.push(r.input.anchor);
                }
            }
        }
        false
    }

    /// Whether reference `r` is part of what anchor `a` captures.
    fn captures(&self, a: &AnchorInfo, r: &AnchorRef) -> bool {
        match a.domain {
            AnchorDomain::Content => {
                matches!(r.site, RefSite::Fill { layer, .. } if layer == a.layer)
                    || (matches!(r.site, RefSite::Content { layer, .. } if layer == a.layer) && r.ordinal < a.ordinal)
                    || self.descendants.get(&a.layer).is_some_and(|d| d.contains(&r.site.layer()))
            }
            AnchorDomain::Mask => matches!(r.site, RefSite::Mask { layer, .. } if layer == a.layer) && r.ordinal < a.ordinal,
        }
    }

    pub fn anchor(&self, id: EffectId) -> Option<&AnchorInfo> {
        self.anchors.iter().find(|a| a.id == id)
    }

    pub fn has_refs(&self) -> bool {
        !self.refs.is_empty()
    }

    /// The reference at `site`, if one sits there.
    pub fn reference(&self, site: RefSite) -> Option<&AnchorRef> {
        self.refs.iter().find(|r| r.site == site)
    }

    /// The anchor the reference at `site` reads, when it is valid.
    pub fn resolve(&self, site: RefSite) -> Option<&AnchorInfo> {
        let r = self.reference(site).filter(|r| r.status.is_ok())?;
        self.anchor(r.input.anchor)
    }

    /// Anchors a reference at `site` may read (evaluated before it), in evaluation order.
    pub fn candidates(&self, site: RefSite) -> Vec<&AnchorInfo> {
        let Some(&at) = self.sites.get(&site) else { return Vec::new() };
        self.anchors.iter().filter(|a| a.ordinal < at).collect()
    }

    /// Anchors a new effect added on top of `layer`'s mask may read.
    pub fn candidates_above_mask(&self, layer: LayerId) -> Vec<&AnchorInfo> {
        let Some(&(_, end)) = self.layers.get(&layer) else { return Vec::new() };
        self.anchors.iter().filter(|a| a.ordinal <= end).collect()
    }

    /// Name of a layer of the tree.
    pub fn layer_name(&self, layer: LayerId) -> Option<&str> {
        self.layers.get(&layer).map(|(n, _)| n.as_str())
    }

    /// References reading `anchor`.
    pub fn dependents(&self, anchor: EffectId) -> Vec<&AnchorRef> {
        self.refs.iter().filter(|r| r.input.anchor == anchor).collect()
    }

    /// References that cannot be evaluated.
    pub fn problems(&self) -> impl Iterator<Item = &AnchorRef> {
        self.refs.iter().filter(|r| !r.status.is_ok())
    }
}

/// Point every reference in `layer`'s subtree whose anchor is a key of `map` at the mapped anchor
/// (copies of a subtree keep reading their own copied anchors).
pub fn remap_inputs(layer: &mut Layer, map: &HashMap<EffectId, EffectId>) {
    if map.is_empty() {
        return;
    }
    if let LayerContent::Fill(f) = &mut layer.content {
        for ch in f.channels.values_mut() {
            if let FillSource::Anchor { input } = &mut ch.source
                && let Some(new) = map.get(&input.anchor)
            {
                input.anchor = *new;
            }
        }
    }
    remap_content_inputs(&mut layer.content_effects, map);
    if let Some(mask) = &mut layer.mask {
        remap_mask_inputs(&mut mask.effects, map);
    }
    if let Some(children) = layer.children_mut() {
        for c in children.iter_mut() {
            remap_inputs(Arc::make_mut(c), map);
        }
    }
}

/// Effect id of every effect of `original`'s subtree -> the matching effect of `copy` (a copy
/// with fresh ids and the same structure), added to `map`.
pub fn copied_effect_ids(original: &Layer, copy: &Layer, map: &mut HashMap<EffectId, EffectId>) {
    for (a, b) in original.content_effects.iter().zip(&copy.content_effects) {
        map.insert(a.id, b.id);
    }
    if let (Some(a), Some(b)) = (&original.mask, &copy.mask) {
        for (x, y) in a.effects.iter().zip(&b.effects) {
            map.insert(x.id, y.id);
        }
    }
    for (a, b) in original.children().iter().zip(copy.children()) {
        copied_effect_ids(a, b, map);
    }
}

/// [`remap_inputs`] for one content effect stack (the anchors in its filters' image slots).
pub fn remap_content_inputs(effects: &mut [Arc<crate::effect::ContentEffect>], map: &HashMap<EffectId, EffectId>) {
    for e in effects.iter_mut() {
        if matches!(&e.kind, ContentEffectKind::Filter { filter, .. } if filter.reads_any_anchor(map))
            && let ContentEffectKind::Filter { filter, .. } = &mut Arc::make_mut(e).kind
        {
            filter.remap_anchors(map);
        }
    }
}

/// [`remap_inputs`] for one mask effect stack.
pub fn remap_mask_inputs(effects: &mut [Arc<MaskEffect>], map: &HashMap<EffectId, EffectId>) {
    for e in effects.iter_mut() {
        let Some(input) = mask_effect_input(&e.kind) else { continue };
        let Some(new) = map.get(&input.anchor) else { continue };
        let input = AnchorInput { anchor: *new, ..input };
        match &mut Arc::make_mut(e).kind {
            MaskEffectKind::AnchorRef { input: i, .. } => *i = input,
            MaskEffectKind::Generator(g) => g.input = Some(input),
            MaskEffectKind::Filter(f) => f.remap_anchors(map),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::ContentEffect;
    use crate::layer::FillChannel;
    use crate::mask::{Generator, GeneratorKind, Mask};
    use crate::texture_set::TextureSet;

    fn anchor_layer(ts: &TextureSet, name: &str) -> (Layer, EffectId) {
        let mut l = Layer::new_paint(name, ts);
        let mut e = ContentEffect::new(ContentEffectKind::Anchor, ts);
        e.name = format!("{name} anchor");
        let id = e.id;
        l.content_effects.push(Arc::new(e));
        (l, id)
    }

    fn masked(ts: &TextureSet, input: AnchorInput) -> (Layer, EffectId) {
        let mut l = Layer::new_fill("Masked", ts);
        let mut m = Mask::new(ts.resolution, false);
        let e = MaskEffect::new(MaskEffectKind::AnchorRef { input, levels: None });
        let id = e.id;
        m.effects.push(Arc::new(e));
        l.mask = Some(m);
        (l, id)
    }

    #[test]
    fn filter_image_slots_are_references() {
        use crate::filter::{BlurMap, Filter, ImageSource, MatchSource};
        let ts = TextureSet::new("T", 0, 64);
        let (low, low_anchor) = anchor_layer(&ts, "Low");
        // A layer with [filter, anchor, filter] on its content and a filter on its mask.
        let (mut top, own_anchor) = anchor_layer(&ts, "Top");
        let nub = |anchor| Filter::NonUniformBlur { radius: 4.0, map: BlurMap::Image, scale: 4, seed: 1, samples: 8, image: Some(ImageSource::Anchor(AnchorInput::new(anchor, ChannelKind::Height))) };
        let early = ContentEffect::new(ContentEffectKind::Filter { channel: ChannelKind::Roughness, filter: nub(own_anchor) }, &ts);
        let late = ContentEffect::new(
            ContentEffectKind::Filter {
                channel: ChannelKind::BaseColor,
                filter: Filter::ColorMatch {
                    source_mode: MatchSource::Parameter,
                    source: [0.5; 3],
                    target: [0.5; 3],
                    hue: 1.0,
                    chroma: 1.0,
                    luma: 1.0,
                    // Kept, but not read outside the Input mode: no reference.
                    source_image: Some(ImageSource::Anchor(AnchorInput::new(EffectId::new(), ChannelKind::BaseColor))),
                    target_image: Some(ImageSource::Anchor(AnchorInput::new(low_anchor, ChannelKind::BaseColor))),
                },
            },
            &ts,
        );
        let (early_id, late_id) = (early.id, late.id);
        top.content_effects.insert(0, Arc::new(early));
        top.content_effects.push(Arc::new(late));
        let mut m = Mask::new(64, false);
        let on_mask = MaskEffect::new(MaskEffectKind::Filter(nub(own_anchor)));
        let mask_id = on_mask.id;
        m.effects.push(Arc::new(on_mask));
        top.mask = Some(m);
        let top_id = top.id;
        let layers = vec![Arc::new(low), Arc::new(top)];
        assert!(has_references(&layers) && content_has_references(&layers[1]) && !content_has_references(&layers[0]));
        let index = AnchorIndex::build(&layers);
        let status = |site| index.reference(site).map(|r| r.status);
        // The filter under its own layer's anchor cannot read it (the anchor captures that very
        // filter: a cycle); the mask filter can.
        assert_eq!(status(RefSite::Content { layer: top_id, effect: early_id, slot: 0 }), Some(RefStatus::Cycle));
        assert_eq!(status(RefSite::Content { layer: top_id, effect: late_id, slot: 1 }), Some(RefStatus::Ok));
        assert_eq!(status(RefSite::Content { layer: top_id, effect: late_id, slot: 0 }), None);
        assert_eq!(status(RefSite::Mask { layer: top_id, effect: mask_id }), Some(RefStatus::Ok));
        assert_eq!(index.dependents(low_anchor).len(), 1);
        assert_eq!(index.dependents(own_anchor).len(), 2);
        // The picker of each slot offers the anchors below that effect.
        let names = |site| index.candidates(site).iter().map(|a| a.id).collect::<Vec<_>>();
        assert_eq!(names(RefSite::Content { layer: top_id, effect: early_id, slot: 0 }), vec![low_anchor]);
        assert_eq!(names(RefSite::Content { layer: top_id, effect: late_id, slot: 1 }), vec![low_anchor, own_anchor]);
        // A copy of the tree reads its own copied anchors, in shown and in kept slots alike.
        let mut copy = (*layers[1]).clone();
        let new_anchor = EffectId::new();
        remap_inputs(&mut copy, &HashMap::from([(own_anchor, new_anchor)]));
        let ContentEffectKind::Filter { filter, .. } = &copy.content_effects[0].kind else { unreachable!() };
        assert_eq!(filter.anchor_inputs(), vec![(0, AnchorInput::new(new_anchor, ChannelKind::Height))]);
        assert_eq!(mask_effect_input(&copy.mask.as_ref().unwrap().effects[0].kind), Some(AnchorInput::new(new_anchor, ChannelKind::Height)));
        let ContentEffectKind::Filter { filter, .. } = &copy.content_effects[2].kind else { unreachable!() };
        assert_eq!(filter.anchor_inputs(), vec![(1, AnchorInput::new(low_anchor, ChannelKind::BaseColor))]);
        // Files: an anchor slot round-trips, next to an image slot.
        let f = nub(own_anchor);
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains(r#""image":{"anchor":"#), "{json}");
        assert_eq!(serde_json::from_str::<Filter>(&json).unwrap(), f);
        let mut g = f;
        let image = subcraft_core::ResourceId::new();
        assert!(g.set_image(0, Some(image)) && g.anchor_inputs().is_empty() && g.resources() == vec![image]);
        assert_eq!(serde_json::from_str::<Filter>(&serde_json::to_string(&g).unwrap()).unwrap(), g);
        assert!(g.set_anchor(0, AnchorInput::new(own_anchor, ChannelKind::Height)) && g.resources().is_empty() && g == f);
        assert!(!Filter::gradient_dynamic().set_anchor(0, AnchorInput::new(own_anchor, ChannelKind::Height)));
    }

    /// QA 2026-11: an input kept by a generator switched to a kind that reads none still
    /// counted as a reference (a stale problem once its anchor was deleted, and a dependent).
    #[test]
    fn an_input_the_generator_kind_does_not_read_is_no_reference() {
        let ts = TextureSet::new("T", 0, 64);
        let (low, a) = anchor_layer(&ts, "Low");
        let mut top = Layer::new_fill("Top", &ts);
        let mut g = Generator::new(GeneratorKind::Edges);
        g.input = Some(AnchorInput::new(a, ChannelKind::Height));
        let mut m = Mask::new(ts.resolution, false);
        m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
        top.mask = Some(m);
        let layers = |top: &Layer| -> Vec<Arc<Layer>> { vec![Arc::new(low.clone()), Arc::new(top.clone())] };
        assert_eq!(AnchorIndex::build(&layers(&top)).dependents(a).len(), 1, "an input the kind reads is a reference");
        // Switch the generator to a kind without input: the stored input stays, unread.
        let e = &mut top.mask.as_mut().unwrap().effects[0];
        if let MaskEffectKind::Generator(g) = &mut Arc::make_mut(e).kind {
            g.set_kind(GeneratorKind::MaskEditor);
            assert!(g.input.is_some(), "kept for when the kind is switched back");
        }
        assert!(AnchorIndex::build(&layers(&top)).dependents(a).is_empty());
        // The anchor deleted: no problem left behind.
        let alone: Vec<Arc<Layer>> = vec![Arc::new(top.clone())];
        assert_eq!(AnchorIndex::build(&alone).problems().count(), 0);
        assert!(!has_references(&alone));
    }

    #[test]
    fn references_resolve_only_to_anchors_below() {
        let ts = TextureSet::new("T", 0, 64);
        let (low, a) = anchor_layer(&ts, "Low");
        let (top, r) = masked(&ts, AnchorInput::new(a, ChannelKind::Height));
        let (high, b) = anchor_layer(&ts, "High");
        let (mid, r2) = masked(&ts, AnchorInput::new(b, ChannelKind::Height));
        let missing = EffectId::new();
        let (bad, r3) = masked(&ts, AnchorInput::new(missing, ChannelKind::Height));
        let layers: Vec<Arc<Layer>> = [low, top, mid, high, bad].into_iter().map(Arc::new).collect();
        let index = AnchorIndex::build(&layers);
        let status = |layer: &Arc<Layer>, effect| index.reference(RefSite::Mask { layer: layer.id, effect }).unwrap().status;
        assert_eq!(status(&layers[1], r), RefStatus::Ok);
        assert_eq!(status(&layers[2], r2), RefStatus::NotBelow, "the anchor sits above the reference");
        assert_eq!(status(&layers[4], r3), RefStatus::Missing);
        assert_eq!(index.resolve(RefSite::Mask { layer: layers[1].id, effect: r }).unwrap().id, a);
        assert!(index.resolve(RefSite::Mask { layer: layers[2].id, effect: r2 }).is_none());
        let names: Vec<_> = index.candidates(RefSite::Mask { layer: layers[2].id, effect: r2 }).iter().map(|a| a.name.clone()).collect();
        assert_eq!(names, ["Low anchor"]);
        assert_eq!(index.dependents(a).len(), 1);
        assert_eq!(index.problems().count(), 2);
        assert!(has_references(&layers) && has_anchors(&layers));
    }

    #[test]
    fn same_stack_order_and_cycles() {
        let ts = TextureSet::new("T", 0, 64);
        // Mask: [anchor, ref -> anchor] is fine; [ref -> anchor, anchor] is a cycle (the anchor
        // captures the stack including the reference).
        let mut l = Layer::new_fill("L", &ts);
        let mut m = Mask::new(64, true);
        let anchor = MaskEffect::new(MaskEffectKind::Anchor);
        let id = anchor.id;
        let good = MaskEffect::new(MaskEffectKind::AnchorRef { input: AnchorInput::new(id, ChannelKind::Height), levels: None });
        let mut g = Generator::new(GeneratorKind::MeshMap(subcraft_core::MeshMapKind::Curvature));
        let later = MaskEffect::new(MaskEffectKind::Anchor);
        g.input = Some(AnchorInput::new(later.id, ChannelKind::Height));
        let cyc = MaskEffect::new(MaskEffectKind::Generator(g));
        let (good_id, cyc_id) = (good.id, cyc.id);
        m.effects = vec![Arc::new(anchor), Arc::new(good), Arc::new(cyc), Arc::new(later)];
        l.mask = Some(m);
        // A folder whose content anchor captures a child reading it: also a cycle.
        let mut folder = Layer::new_folder("F", &ts);
        let fa = ContentEffect::new(ContentEffectKind::Anchor, &ts);
        let fa_id = fa.id;
        folder.content_effects.push(Arc::new(fa));
        let mut child = Layer::new_fill("Child", &ts);
        if let LayerContent::Fill(f) = &mut child.content {
            f.channels.insert(ChannelKind::Roughness, FillChannel::new(FillSource::Anchor { input: AnchorInput::new(fa_id, ChannelKind::Roughness) }));
        }
        let child_id = child.id;
        folder.children_mut().unwrap().push(Arc::new(child));
        let layers = vec![Arc::new(l.clone()), Arc::new(folder)];
        let index = AnchorIndex::build(&layers);
        assert_eq!(index.reference(RefSite::Mask { layer: l.id, effect: good_id }).unwrap().status, RefStatus::Ok);
        assert_eq!(index.reference(RefSite::Mask { layer: l.id, effect: cyc_id }).unwrap().status, RefStatus::Cycle);
        assert_eq!(index.reference(RefSite::Fill { layer: child_id, channel: ChannelKind::Roughness }).unwrap().status, RefStatus::Cycle);
    }

    #[test]
    fn duplicates_read_their_own_copied_anchors() {
        let ts = TextureSet::new("T", 0, 64);
        let (low, a) = anchor_layer(&ts, "Low");
        let (top, _) = masked(&ts, AnchorInput::new(a, ChannelKind::Height));
        let mut folder = Layer::new_folder("F", &ts);
        folder.children_mut().unwrap().extend([Arc::new(low), Arc::new(top)]);
        let copy = folder.duplicate_with_new_ids();
        let new_anchor = copy.children()[0].content_effects[0].id;
        assert_ne!(new_anchor, a);
        let input = mask_effect_input(&copy.children()[1].mask.as_ref().unwrap().effects[0].kind).unwrap();
        assert_eq!(input.anchor, new_anchor, "the copy reads the copied anchor");
        // A copy of the reading layer alone keeps reading the original anchor.
        let lone = folder.children()[1].duplicate_with_new_ids();
        assert_eq!(mask_effect_input(&lone.mask.as_ref().unwrap().effects[0].kind).unwrap().anchor, a);
    }
}
