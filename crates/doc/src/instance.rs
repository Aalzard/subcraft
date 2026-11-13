//! Layer instances (Painter's "Paste as instance"): a layer that follows a source layer.
//!
//! An instance is an ordinary layer whose `instance_of` names its source. Its content, content
//! effects and mask are a copy of the source's, refreshed by [`sync`] after every document edit;
//! its name, visibility, blending, colour tag and geometry mask are its own. The compositor and
//! the project file therefore see plain layers and need no special case. Rasters stay shared
//! with the source (copy-on-write).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use subcraft_core::{AnchorId, EffectId, LayerId};

use crate::{Document, Layer, LayerContent, TextureSet};

/// What an instance's link looks like right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceState {
    /// Follows its source.
    Linked,
    /// The source is not in the document: the layer keeps what it last showed.
    Broken,
    /// The instance sits inside its own source (directly or through other instances): it is
    /// not refreshed until it is moved out.
    Cycle,
}

/// Which source and instance layers the last [`sync`] left in step, so an edit elsewhere costs
/// nothing. Holding the `Arc`s keeps the pointer comparison sound.
#[derive(Default)]
pub struct InstanceCache(HashMap<LayerId, (Arc<Layer>, Arc<Layer>)>);

fn find_arc(list: &[Arc<Layer>], id: LayerId) -> Option<&Arc<Layer>> {
    for l in list {
        if l.id == id {
            return Some(l);
        }
        if let Some(found) = find_arc(l.children(), id) {
            return Some(found);
        }
    }
    None
}

fn doc_find(doc: &Document, id: LayerId) -> Option<&Arc<Layer>> {
    doc.texture_sets.iter().find_map(|ts| find_arc(&ts.layers, id))
}

/// Whether `target` is in the subtree of `layer`, following instance links.
fn reaches(doc: &Document, layer: &Layer, target: LayerId, seen: &mut HashSet<LayerId>) -> bool {
    if layer.id == target {
        return true;
    }
    if let Some(source) = layer.instance_of
        && seen.insert(source)
        && let Some(next) = doc_find(doc, source)
        && reaches(doc, next, target, seen)
    {
        return true;
    }
    layer.children().iter().any(|c| reaches(doc, c, target, seen))
}

/// State of `layer`'s link; `None` when it is not an instance.
pub fn state(doc: &Document, layer: &Layer) -> Option<InstanceState> {
    let source = layer.instance_of?;
    Some(match doc_find(doc, source) {
        None => InstanceState::Broken,
        Some(s) if reaches(doc, s, layer.id, &mut HashSet::new()) => InstanceState::Cycle,
        Some(_) => InstanceState::Linked,
    })
}

/// The outermost instance that `id` is, or is inside of (its content is read-only).
pub fn root_of(ts: &TextureSet, id: LayerId) -> Option<LayerId> {
    fn rec(list: &[Arc<Layer>], id: LayerId, root: Option<LayerId>) -> Option<Option<LayerId>> {
        for l in list {
            let root = root.or(l.instance_of.map(|_| l.id));
            if l.id == id {
                return Some(root);
            }
            if let Some(found) = rec(l.children(), id, root) {
                return Some(found);
            }
        }
        None
    }
    rec(&ts.layers, id, None).flatten()
}

fn derived_name(instance: LayerId, original: uuid::Uuid) -> String {
    format!("instance {} {}", instance.0, original)
}

/// Copy of `source` with ids derived from the instance's (stable from one refresh to the next).
fn remap(source: &Layer, instance: LayerId, map: &mut HashMap<EffectId, EffectId>, root: bool) -> Layer {
    let mut l = source.clone();
    if !root {
        l.id = LayerId::from_name(&derived_name(instance, source.id.0));
    }
    if let LayerContent::Paint(p) = &mut l.content
        && let Some(path) = &mut p.path
    {
        for a in &mut path.anchors {
            a.id = AnchorId::from_name(&derived_name(instance, a.id.0));
        }
    }
    if let Some(mask) = &mut l.mask {
        for e in mask.effects.iter_mut() {
            let new = EffectId::from_name(&derived_name(instance, e.id.0));
            map.insert(e.id, new);
            Arc::make_mut(e).id = new;
        }
    }
    for e in l.content_effects.iter_mut() {
        let new = EffectId::from_name(&derived_name(instance, e.id.0));
        map.insert(e.id, new);
        Arc::make_mut(e).id = new;
    }
    if let LayerContent::Group(g) = &mut l.content {
        for c in g.children.iter_mut() {
            *c = Arc::new(remap(c, instance, map, false));
        }
    }
    l
}

/// The instance as it should be now: the source's content, content effects and mask under the
/// instance's own id, name, visibility, blending, colour tag and geometry mask.
pub fn derive(source: &Layer, instance: &Layer) -> Layer {
    let mut map = HashMap::new();
    let mut l = remap(source, instance.id, &mut map, true);
    l.id = instance.id;
    l.name = instance.name.clone();
    l.visible = instance.visible;
    l.blend = instance.blend.clone();
    l.color = instance.color;
    l.parts = instance.parts.clone();
    l.instance_of = instance.instance_of;
    if let (LayerContent::Group(new), LayerContent::Group(old)) = (&mut l.content, &instance.content) {
        new.expanded = old.expanded;
    }
    // Anchor references inside the copy read the copied anchors.
    crate::anchor::remap_inputs(&mut l, &map);
    l
}

/// Instances that are not inside another instance (those come with their parent's copy).
fn roots(list: &[Arc<Layer>], out: &mut Vec<(LayerId, LayerId)>) {
    for l in list {
        match l.instance_of {
            Some(source) => out.push((l.id, source)),
            None => roots(l.children(), out),
        }
    }
}

/// Bring every instance in step with its source. An instance whose source was in `previous`
/// and is gone now is removed with it (deleting a source deletes its instances); one whose
/// source was never there stays as it is. Returns whether the document changed.
pub fn sync(doc: &mut Document, previous: Option<&Document>, cache: &mut InstanceCache) -> bool {
    let mut any = false;
    // A source may hold instances of its own: one pass per nesting level.
    for _ in 0..16 {
        let mut list = Vec::new();
        for (i, ts) in doc.texture_sets.iter().enumerate() {
            let mut found = Vec::new();
            roots(&ts.layers, &mut found);
            list.extend(found.into_iter().map(|(instance, source)| (i, instance, source)));
        }
        if list.is_empty() {
            cache.0.clear();
            return any;
        }
        cache.0.retain(|id, _| list.iter().any(|(_, instance, _)| instance == id));
        let mut changed = false;
        for (ts, instance_id, source_id) in list {
            let Some(source) = doc_find(doc, source_id).cloned() else {
                if previous.is_some_and(|p| doc_find(p, source_id).is_some()) && doc.texture_sets[ts].remove(instance_id).is_ok() {
                    cache.0.remove(&instance_id);
                    changed = true;
                }
                continue;
            };
            // Gone with an enclosing instance removed earlier in this pass.
            let Some(instance) = find_arc(&doc.texture_sets[ts].layers, instance_id).cloned() else { continue };
            if reaches(doc, &source, instance_id, &mut HashSet::new()) {
                continue;
            }
            if cache.0.get(&instance_id).is_some_and(|(s, i)| Arc::ptr_eq(s, &source) && Arc::ptr_eq(i, &instance)) {
                continue;
            }
            let new = derive(&source, &instance);
            let set = &mut doc.texture_sets[ts];
            if let Some(slot) = set.find_mut(instance_id) {
                *slot = new;
            }
            if let Some(now) = find_arc(&set.layers, instance_id).cloned() {
                cache.0.insert(instance_id, (source, now));
            }
            changed = true;
        }
        if !changed {
            break;
        }
        any = true;
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MaskEffectKind;
    use subcraft_core::ChannelKind;

    fn doc() -> Document {
        Document::from_material_slots(&["Body".to_string()], 64)
    }

    fn instance_of(source: &Layer) -> Layer {
        let mut l = source.clone();
        l.id = LayerId::new();
        l.name = format!("{} instance", source.name);
        l.instance_of = Some(source.id);
        l
    }

    fn add_instance(doc: &mut Document, source: LayerId) -> LayerId {
        let layer = instance_of(doc.texture_sets[0].find(source).unwrap());
        add(doc, layer)
    }

    fn add(doc: &mut Document, layer: Layer) -> LayerId {
        doc.texture_sets[0].insert_above(None, layer)
    }

    #[test]
    fn an_instance_follows_its_source_and_keeps_its_own_name_blend_and_visibility() {
        let mut d = doc();
        let ts = d.texture_sets[0].clone();
        let source = add(&mut d, Layer::new_fill("Rust", &ts));
        let inst = add_instance(&mut d, source);
        let mut cache = InstanceCache::default();
        sync(&mut d, None, &mut cache);
        // The instance's own settings.
        {
            let l = d.texture_sets[0].find_mut(inst).unwrap();
            l.visible = false;
            l.blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = 0.25;
        }
        // An edit of the source: a mask with an effect.
        {
            let l = d.texture_sets[0].find_mut(source).unwrap();
            let mut mask = crate::Mask::new(64, false);
            mask.effects.push(Arc::new(crate::MaskEffect::new(MaskEffectKind::Invert)));
            l.mask = Some(mask);
        }
        assert!(sync(&mut d, None, &mut cache));
        let (s, i) = (d.texture_sets[0].find(source).unwrap(), d.texture_sets[0].find(inst).unwrap());
        assert_eq!(i.content, s.content);
        let (sm, im) = (s.mask.as_ref().unwrap(), i.mask.as_ref().unwrap());
        assert_eq!(im.effects.len(), 1);
        assert_ne!(im.effects[0].id, sm.effects[0].id, "effects of the copy have their own ids");
        assert_eq!(i.name, "Rust instance");
        assert!(!i.visible);
        assert_eq!(i.blend[&ChannelKind::BaseColor].opacity, 0.25);
        assert_eq!(state(&d, i), Some(InstanceState::Linked));
        // In step: nothing to do, and the ids of the copy are stable.
        let id = im.effects[0].id;
        assert!(!sync(&mut d, None, &mut cache));
        let mut cold = InstanceCache::default();
        sync(&mut d, None, &mut cold);
        assert_eq!(d.texture_sets[0].find(inst).unwrap().mask.as_ref().unwrap().effects[0].id, id);
    }

    #[test]
    fn a_folder_instance_gets_the_layers_added_to_its_source_under_stable_ids() {
        let mut d = doc();
        let ts = d.texture_sets[0].clone();
        let folder = add(&mut d, Layer::new_folder("Group", &ts));
        let inst = add_instance(&mut d, folder);
        let mut cache = InstanceCache::default();
        sync(&mut d, None, &mut cache);
        let child = Layer::new_fill("Inside", &ts);
        let child_id = child.id;
        d.texture_sets[0].find_mut(folder).unwrap().children_mut().unwrap().push(Arc::new(child));
        assert!(sync(&mut d, None, &mut cache));
        let copy = d.texture_sets[0].find(inst).unwrap().children()[0].clone();
        assert_eq!(copy.name, "Inside");
        assert_ne!(copy.id, child_id, "layer ids stay unique in the texture set");
        assert_eq!(root_of(&d.texture_sets[0], copy.id), Some(inst));
        assert_eq!(root_of(&d.texture_sets[0], child_id), None);
        // Another edit of the source keeps the copied child's id.
        d.texture_sets[0].find_mut(child_id).unwrap().visible = false;
        assert!(sync(&mut d, None, &mut cache));
        let again = d.texture_sets[0].find(inst).unwrap().children()[0].clone();
        assert_eq!(again.id, copy.id);
        assert!(!again.visible);
    }

    #[test]
    fn deleting_the_source_deletes_its_instances_and_an_unknown_source_leaves_a_broken_one() {
        let mut d = doc();
        let ts = d.texture_sets[0].clone();
        let source = add(&mut d, Layer::new_fill("Rust", &ts));
        let inst = add_instance(&mut d, source);
        let mut cache = InstanceCache::default();
        sync(&mut d, None, &mut cache);
        let before = d.clone();
        d.texture_sets[0].remove(source).unwrap();
        assert!(sync(&mut d, Some(&before), &mut cache));
        assert!(d.texture_sets[0].find(inst).is_none());
        // A pasted instance whose source was never in this document stays, flagged as broken.
        let mut orphan = Layer::new_fill("Orphan", &ts);
        orphan.instance_of = Some(LayerId::new());
        let orphan = add(&mut d, orphan);
        let before = d.clone();
        assert!(!sync(&mut d, Some(&before), &mut cache));
        assert_eq!(state(&d, d.texture_sets[0].find(orphan).unwrap()), Some(InstanceState::Broken));
    }

    #[test]
    fn an_instance_inside_its_own_source_is_flagged_and_left_alone() {
        let mut d = doc();
        let ts = d.texture_sets[0].clone();
        let folder = add(&mut d, Layer::new_folder("Group", &ts));
        let mut inst = instance_of(d.texture_sets[0].find(folder).unwrap());
        inst.name = "Loop".into();
        let inst_id = inst.id;
        d.texture_sets[0].find_mut(folder).unwrap().children_mut().unwrap().push(Arc::new(inst));
        let mut cache = InstanceCache::default();
        assert!(!sync(&mut d, None, &mut cache), "nothing is copied into itself");
        let l = d.texture_sets[0].find(inst_id).unwrap();
        assert_eq!(state(&d, l), Some(InstanceState::Cycle));
        assert!(l.children().is_empty());
    }
}
