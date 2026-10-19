//! Bounded UV previews from immutable snapshots. No GPU readbacks or UI toolkit dependency.
use crate::Session;
use std::collections::VecDeque;
use std::sync::{Arc, mpsc};
use subcraft_compositor::{TileContext, mask::eval_mask, tile::preview_content};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{Layer, LayerContent, ResourceStore};
use subcraft_materials::{GeometryCache, MeshMaps, SampleContext};

pub const CACHE_LIMIT: usize = 96;
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct PreviewKey {
    pub layer: LayerId,
    pub channel: ChannelKind,
    pub mask: bool,
    pub size: u32,
}

#[derive(Debug)]
pub struct LayerPreview {
    pub size: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone)]
struct Source {
    layer: Arc<Layer>,
    resources: ResourceStore,
    maps: MeshMaps,
    maps_generation: u64,
    resolution: u32,
    albedo_filter: subcraft_image::TextureFilter,
    slot: u32,
    geometry: Arc<GeometryCache>,
}
impl Source {
    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.layer, &other.layer)
            && Arc::ptr_eq(&self.geometry, &other.geometry)
            && self.maps_generation == other.maps_generation
            && self.resolution == other.resolution
            && self.albedo_filter == other.albedo_filter
    }
}
struct Entry {
    key: PreviewKey,
    source: Source,
    image: Option<Arc<LayerPreview>>,
    used: u64,
}
type ResultMessage = (PreviewKey, Source, Arc<LayerPreview>);

#[derive(Default)]
pub struct LayerPreviews {
    entries: Vec<Entry>,
    queue: VecDeque<PreviewKey>,
    running: Option<mpsc::Receiver<ResultMessage>>,
    geometry: Option<(Arc<subcraft_mesh::Mesh>, Arc<GeometryCache>)>,
    clock: u64,
}

fn find(layers: &[Arc<Layer>], id: LayerId) -> Option<Arc<Layer>> {
    for layer in layers {
        if layer.id == id {
            return Some(layer.clone());
        }
        if let LayerContent::Group(group) = &layer.content
            && let Some(found) = find(&group.children, id)
        {
            return Some(found);
        }
    }
    None
}

impl LayerPreviews {
    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Only call for visible rows. Returns the last completed image while an update is pending.
    /// Thumbnails are 64²; isolated inspection is bounded at 256², regardless of document size.
    pub fn request(&mut self, session: &Session, mut key: PreviewKey) -> Option<Arc<LayerPreview>> {
        key.size = if key.size <= 64 { 64 } else { 256 };
        let mesh = session.scene().mesh.clone();
        if self
            .geometry
            .as_ref()
            .is_none_or(|(old, _)| !Arc::ptr_eq(old, &mesh))
        {
            *self = Self::default();
            self.geometry = Some((mesh.clone(), Arc::new(GeometryCache::new(mesh))));
        }
        self.poll();
        let (ts, layer) = session
            .document()
            .texture_sets
            .iter()
            .find_map(|ts| find(&ts.layers, key.layer).map(|layer| (ts, layer)))?;
        if key.mask && layer.mask.is_none() {
            return None;
        }
        self.clock += 1;
        let index = if let Some(index) = self.entries.iter().position(|e| e.key == key) {
            index
        } else {
            if self.entries.len() >= CACHE_LIMIT {
                let index = self
                    .entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.used)
                    .unwrap()
                    .0;
                let removed = self.entries.remove(index).key;
                self.queue.retain(|k| *k != removed);
            }
            self.entries.push(Entry {
                key,
                source: Source {
                    layer: layer.clone(),
                    resources: session.resources().clone(),
                    maps: session.mesh_maps(ts.id).cloned().unwrap_or_default(),
                    maps_generation: session.maps_generation(),
                    resolution: ts.resolution,
                    albedo_filter: ts.albedo_filter,
                    slot: ts.material_slot,
                    geometry: self.geometry.as_ref().unwrap().1.clone(),
                },
                image: None,
                used: self.clock,
            });
            self.entries.len() - 1
        };
        let entry = &mut self.entries[index];
        let changed = !Arc::ptr_eq(&entry.source.layer, &layer)
            || entry.source.albedo_filter != ts.albedo_filter
            || entry.source.resolution != ts.resolution
            || entry.source.maps_generation != session.maps_generation();
        if changed {
            entry.source.albedo_filter = ts.albedo_filter;
            entry.source.layer = layer;
            entry.source.resources = session.resources().clone();
            entry.source.maps = session.mesh_maps(ts.id).cloned().unwrap_or_default();
            entry.source.maps_generation = session.maps_generation();
            entry.source.resolution = ts.resolution;
        }
        entry.used = self.clock;
        let image = entry.image.clone();
        if (changed || image.is_none()) && !self.queue.contains(&key) {
            self.queue.push_back(key);
        }
        self.start_next();
        image
    }

    pub fn poll(&mut self) {
        if let Some(receiver) = &self.running {
            match receiver.try_recv() {
                Ok((key, source, image)) => {
                    if let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|e| e.key == key && e.source.same(&source))
                    {
                        entry.image = Some(image);
                        self.queue.retain(|queued| *queued != key);
                    }
                    self.running = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.running = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        self.start_next();
    }

    fn start_next(&mut self) {
        if self.running.is_some() {
            return;
        }
        while let Some(key) = self.queue.pop_front() {
            let Some(entry) = self.entries.iter().find(|entry| entry.key == key) else {
                continue;
            };
            let source = entry.source.clone();
            let (tx, rx) = mpsc::channel();
            self.running = Some(rx);
            subcraft_core::task::spawn(move || {
                let image = Arc::new(evaluate(key, &source));
                let _ = tx.send((key, source, image));
            });
            break;
        }
    }
}

fn evaluate(key: PreviewKey, source: &Source) -> LayerPreview {
    let geometry = source.geometry.get(source.slot, key.size);
    let mut rgba = vec![0; (key.size * key.size * 4) as usize];
    // Disabled masks still display their authored result, with disabled state drawn by UI.
    let mask = source.layer.mask.as_ref().map(|mask| {
        let mut m = mask.clone();
        m.enabled = true;
        m
    });
    let tiles = key.size.div_ceil(64);
    for tile in 0..tiles * tiles {
        let geo = geometry.tile(tile as usize);
        let tc = TileContext {
            albedo_filter: source.albedo_filter,
            geo: &geo,
            set_geometry: &geometry,
            sample: SampleContext {
                resources: &source.resources,
                mesh_maps: &source.maps,
            },
        };
        let pixels = if key.mask {
            eval_mask(mask.as_ref(), &tc)
                .unwrap_or_else(|| vec![0.0; 4096])
                .into_iter()
                .map(|v| [v, v, v, 1.0])
                .collect()
        } else {
            preview_content(&source.layer, key.channel, &tc)
        };
        for (i, pixel) in pixels.iter().enumerate() {
            let (x, y) = geo.texel_xy(i);
            if x >= key.size || y >= key.size {
                continue;
            }
            let at = ((y * key.size + x) * 4) as usize;
            for c in 0..3 {
                let v = if !key.mask && key.channel.is_color() {
                    subcraft_core::color::linear_to_srgb(pixel[c])
                } else {
                    pixel[c]
                };
                rgba[at + c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            rgba[at + 3] = (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    LayerPreview {
        size: key.size,
        rgba,
    }
}

#[cfg(test)]
mod tests;
