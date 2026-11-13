//! Painter's instance list: the source layer and every instance that follows it, for the
//! Properties panel (click an entry to go to that layer).
use subcraft_core::{LayerId, TextureSetId};

use crate::session::Session;

/// One entry of the instance list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceLink {
    pub texture_set: TextureSetId,
    pub texture_set_name: String,
    pub layer: LayerId,
    pub name: String,
    /// The source layer (the first entry); the others are its instances.
    pub source: bool,
}

impl Session {
    /// The source and all instances of the link `layer` belongs to, source first, then the
    /// instances in document order. Empty for a layer that is neither an instance nor a source.
    pub fn instance_links(&self, layer: LayerId) -> Vec<InstanceLink> {
        let doc = &self.doc;
        let Some(l) = doc.texture_sets.iter().find_map(|ts| ts.find(layer)) else { return Vec::new() };
        let source = l.instance_of.unwrap_or(layer);
        let instances = subcraft_doc::instance::instances_of(doc, source);
        if instances.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        // A broken link has no source entry.
        if let Some((ts, s)) = doc.texture_sets.iter().find_map(|ts| ts.find(source).map(|s| (ts, s))) {
            out.push(InstanceLink { texture_set: ts.id, texture_set_name: ts.name.clone(), layer: source, name: s.name.clone(), source: true });
        }
        for (set, id) in instances {
            let ts = &doc.texture_sets[set];
            if let Some(i) = ts.find(id) {
                out.push(InstanceLink { texture_set: ts.id, texture_set_name: ts.name.clone(), layer: id, name: i.name.clone(), source: false });
            }
        }
        out
    }

    /// Every instance in the project that is not inside another instance.
    pub fn all_instances(&self) -> Vec<LayerId> {
        let mut sources: Vec<LayerId> = subcraft_doc::instance::counts(&self.doc).into_keys().collect();
        sources.sort();
        sources.into_iter().flat_map(|s| subcraft_doc::instance::instances_of(&self.doc, s)).map(|(_, id)| id).collect()
    }
}
