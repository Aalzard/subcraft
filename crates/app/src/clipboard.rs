//! In-app clipboard for layers and layer effects.
//!
//! A clip is a self-contained snapshot: the layers (rasters stay shared copy-on-write, so
//! copying is cheap), the image resources they use, the resolution they were made at and a
//! signature of the mesh. It does not belong to a [`Session`](crate::Session), so it survives
//! opening another project: paste into any texture set of any project. Pasting always creates
//! new UUIDs, resamples rasters to the target set's resolution, brings the used resources along
//! and adds the channels the layers need.

use std::sync::Arc;

use subcraft_core::{ChannelKind, EffectId, LayerId};
use subcraft_doc::{ContentEffect, ContentEffectKind, Layer, MaskEffect, MaskEffectKind, Resource, ResourceStore, TextureSet};

/// Cheap identity of a mesh: part masks refer to mesh part ids, so they only survive a paste
/// into the same mesh.
pub type MeshSignature = (usize, usize);

pub fn mesh_signature(mesh: &subcraft_mesh::Mesh) -> MeshSignature {
    (mesh.positions.len(), mesh.indices.len())
}

/// Copied layers (bottom to top, siblings only; folders carry their children).
#[derive(Clone, Debug)]
pub struct LayerClip {
    pub layers: Vec<Layer>,
    pub resources: Vec<Arc<Resource>>,
    pub resolution: u32,
    pub mesh: MeshSignature,
}

impl PartialEq for LayerClip {
    fn eq(&self, o: &Self) -> bool {
        self.layers == o.layers && self.resolution == o.resolution && self.mesh == o.mesh && ids(&self.resources) == ids(&o.resources)
    }
}

fn ids(r: &[Arc<Resource>]) -> Vec<subcraft_core::ResourceId> {
    r.iter().map(|r| r.id).collect()
}

/// Copied effects of one layer: its content stack or its mask stack, never both.
#[derive(Clone, Debug)]
pub struct EffectClip {
    pub content: Vec<ContentEffect>,
    pub mask: Vec<MaskEffect>,
    pub resources: Vec<Arc<Resource>>,
    pub resolution: u32,
}

impl PartialEq for EffectClip {
    fn eq(&self, o: &Self) -> bool {
        self.content == o.content && self.mask == o.mask && self.resolution == o.resolution && ids(&self.resources) == ids(&o.resources)
    }
}

/// `ids` reduced to the layers that are not inside another selected layer, in tree order
/// (bottom to top, depth first). Ids that are not in the set are dropped.
pub fn top_level_in_order(ts: &TextureSet, ids: &[LayerId]) -> Vec<LayerId> {
    let mut out = Vec::new();
    ts.walk(|l, _| {
        if ids.contains(&l.id) && !ids.iter().any(|&a| a != l.id && ts.is_descendant_or_self(l.id, a)) {
            out.push(l.id);
        }
    });
    out
}

fn collect_resources(layers: &[Layer], store: &ResourceStore) -> Vec<Arc<Resource>> {
    let mut wanted = Vec::new();
    for l in layers {
        l.collect_resources(&mut wanted);
    }
    wanted.into_iter().filter_map(|id| store.get(id).cloned()).collect()
}

impl LayerClip {
    /// `ids` must all be in `ts`; descendants of other selected layers are not copied twice.
    pub fn capture(ts: &TextureSet, ids: &[LayerId], store: &ResourceStore, mesh: MeshSignature) -> Option<Self> {
        let layers: Vec<Layer> = top_level_in_order(ts, ids).into_iter().filter_map(|id| ts.find(id).cloned()).collect();
        if layers.is_empty() {
            return None;
        }
        Some(Self { resources: collect_resources(&layers, store), layers, resolution: ts.resolution, mesh })
    }

    /// Channels some copied layer actively blends that `ts` does not have yet.
    pub fn missing_channels(&self, ts: &TextureSet) -> Vec<ChannelKind> {
        fn walk(l: &Layer, ts: &TextureSet, out: &mut Vec<ChannelKind>) {
            for (&k, b) in &l.blend {
                if b.is_active() && !ts.has_channel(k) && !out.contains(&k) {
                    out.push(k);
                }
            }
            for c in l.children() {
                walk(c, ts, out);
            }
        }
        let mut out = Vec::new();
        for l in &self.layers {
            walk(l, ts, &mut out);
        }
        out
    }

    /// The layers as they go into `ts`: fresh UUIDs, rasters at its resolution, part masks only
    /// when the mesh is the one they were made for.
    pub fn instantiate(&self, ts: &TextureSet, target_mesh: MeshSignature) -> Vec<Layer> {
        fn strip_parts(l: &mut Layer) {
            l.parts = None;
            if let Some(c) = l.children_mut() {
                for c in c.iter_mut() {
                    strip_parts(Arc::make_mut(c));
                }
            }
        }
        self.layers
            .iter()
            .map(|l| {
                let mut l = l.duplicate_with_new_ids();
                if self.resolution != ts.resolution {
                    let r = ts.resolution;
                    l.map_rasters(&|img| img.resampled(r, r));
                }
                if self.mesh != target_mesh {
                    strip_parts(&mut l);
                }
                l
            })
            .collect()
    }
}

impl EffectClip {
    pub fn is_empty(&self) -> bool {
        self.content.is_empty() && self.mask.is_empty()
    }

    pub fn capture_content(ts: &TextureSet, layer: LayerId, effects: &[EffectId], store: &ResourceStore) -> Option<Self> {
        let l = ts.find(layer)?;
        // Stack order (bottom to top) is kept whatever order the ids were given in.
        let content: Vec<ContentEffect> = l.content_effects.iter().filter(|e| effects.contains(&e.id)).map(|e| (**e).clone()).collect();
        if content.is_empty() {
            return None;
        }
        let mut wanted = Vec::new();
        for e in &content {
            if let ContentEffectKind::Fill(f) = &e.kind {
                for c in f.channels.values() {
                    if let subcraft_doc::FillSource::Bitmap { resource } = c.source && !wanted.contains(&resource) {
                        wanted.push(resource);
                    }
                }
            }
        }
        let resources = wanted.into_iter().filter_map(|id| store.get(id).cloned()).collect();
        Some(Self { content, mask: Vec::new(), resources, resolution: ts.resolution })
    }

    pub fn capture_mask(ts: &TextureSet, layer: LayerId, effects: &[EffectId], store: &ResourceStore) -> Option<Self> {
        let l = ts.find(layer)?;
        let mask: Vec<MaskEffect> = l.mask.as_ref()?.effects.iter().filter(|e| effects.contains(&e.id)).map(|e| (**e).clone()).collect();
        if mask.is_empty() {
            return None;
        }
        let mut wanted = Vec::new();
        for e in &mask {
            if let MaskEffectKind::Image { resource, .. } = e.kind && !wanted.contains(&resource) {
                wanted.push(resource);
            }
        }
        let resources = wanted.into_iter().filter_map(|id| store.get(id).cloned()).collect();
        Some(Self { content: Vec::new(), mask, resources, resolution: ts.resolution })
    }

    /// Content effects with fresh ids and painted rasters at `resolution`.
    pub fn instantiate_content(&self, resolution: u32) -> Vec<ContentEffect> {
        self.content
            .iter()
            .map(|e| {
                let mut e = e.clone();
                e.id = EffectId::new();
                if let ContentEffectKind::Paint { channels } = &mut e.kind {
                    for img in channels.values_mut() {
                        *img = img.resampled(resolution, resolution);
                    }
                }
                e
            })
            .collect()
    }

    pub fn instantiate_mask(&self, resolution: u32) -> Vec<MaskEffect> {
        self.mask
            .iter()
            .map(|e| {
                let mut e = e.clone();
                e.id = EffectId::new();
                if let MaskEffectKind::Paint { image } = &mut e.kind {
                    *image = image.resampled(resolution, resolution);
                }
                e
            })
            .collect()
    }
}
