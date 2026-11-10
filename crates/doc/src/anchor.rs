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
//!   ([`crate::MaskEffectKind::AnchorRef`]) and as a generator's input image
//!   ([`crate::Generator::input`]).
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
    /// A mask effect: an Anchor Reference, or a generator with an anchor input.
    Mask { layer: LayerId, effect: EffectId },
}

impl RefSite {
    pub fn layer(&self) -> LayerId {
        match *self {
            RefSite::Fill { layer, .. } | RefSite::Mask { layer, .. } => layer,
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

/// The anchor input a mask effect reads, if any.
pub fn mask_effect_input(kind: &MaskEffectKind) -> Option<AnchorInput> {
    match kind {
        MaskEffectKind::AnchorRef { input, .. } => Some(*input),
        MaskEffectKind::Generator(g) => g.input,
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
    mask_has_references(l) || fill_has_references(l)
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

/// [`remap_inputs`] for one mask effect stack.
pub fn remap_mask_inputs(effects: &mut [Arc<MaskEffect>], map: &HashMap<EffectId, EffectId>) {
    for e in effects.iter_mut() {
        let Some(input) = mask_effect_input(&e.kind) else { continue };
        let Some(new) = map.get(&input.anchor) else { continue };
        let input = AnchorInput { anchor: *new, ..input };
        match &mut Arc::make_mut(e).kind {
            MaskEffectKind::AnchorRef { input: i, .. } => *i = input,
            MaskEffectKind::Generator(g) => g.input = Some(input),
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
