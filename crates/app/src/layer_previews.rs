//! Bounded UV previews from immutable snapshots. No GPU readbacks or UI toolkit dependency.
use crate::Session;
use std::collections::VecDeque;
use std::sync::{Arc, mpsc};
use subcraft_compositor::{TileContext, mask::eval_mask_scaled, tile::preview_content_context_scaled};
use subcraft_core::{ChannelKind, EffectId, LayerId};
use subcraft_doc::{ContentEffectKind, Layer, LayerContent, ResourceStore};
use subcraft_materials::{GeometryCache, MeshMaps, SampleContext};

pub const CACHE_LIMIT: usize = 96;
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct PreviewKey {
    pub layer: LayerId,
    pub channel: ChannelKind,
    pub mask: bool,
    /// Inclusive effect stage; None means the complete content/mask stack.
    pub effect: Option<EffectId>,
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
    /// Folder previews need their actual lower accumulation, including ancestor isolation.
    stack: Vec<Arc<Layer>>,
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
            && self.stack.len() == other.stack.len()
            && self.stack.iter().zip(&other.stack).all(|(a,b)|Arc::ptr_eq(a,b))
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
    /// The last input histogram (Levels): its key, the layer it was computed from and the result.
    histogram: Option<(PreviewKey, Arc<Layer>, u64, Arc<Histogram>)>,
}

/// Distribution of an effect's input values over the UV square (Painter's Levels histogram).
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    /// Per component (R, G, B, luminance): texel counts in [`Histogram::BINS`] equal bins over 0..1
    /// (values outside count in the end bins).
    pub bins: [Vec<u32>; 4],
    /// Per component: the smallest and largest value (texels the input covers).
    pub min: [f32; 4],
    pub max: [f32; 4],
    /// Texels counted.
    pub count: u32,
}

impl Histogram {
    pub const BINS: usize = 128;
    /// Edge of the square the histogram is sampled on (bounded, independent of the document).
    pub const SIZE: u32 = 128;

    /// Histogram of linear texels; texels with zero alpha (nothing there) are skipped.
    pub fn of(pixels: &[[f32; 4]], gray: bool) -> Self {
        let mut bins: [Vec<u32>; 4] = std::array::from_fn(|_| vec![0; Self::BINS]);
        let (mut min, mut max) = ([f32::INFINITY; 4], [f32::NEG_INFINITY; 4]);
        let mut count = 0;
        for p in pixels.iter().filter(|p| p[3] > 0.0) {
            let luma = if gray { p[0] } else { 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2] };
            let v = if gray { [p[0], p[0], p[0], luma] } else { [p[0], p[1], p[2], luma] };
            for c in 0..4 {
                if !v[c].is_finite() {
                    continue;
                }
                let b = ((v[c].clamp(0.0, 1.0) * Self::BINS as f32) as usize).min(Self::BINS - 1);
                bins[c][b] += 1;
                min[c] = min[c].min(v[c]);
                max[c] = max[c].max(v[c]);
            }
            count += 1;
        }
        Self { bins, min, max, count }
    }
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
        if let Some(effect) = key.effect {
            let exists = if key.mask {layer.mask.as_ref().is_some_and(|m|m.effects.iter().any(|e|e.id==effect))}
                else {layer.content_effects.iter().any(|e|e.id==effect)};
            if !exists {
                self.entries.retain(|e|e.key!=key); self.queue.retain(|k|*k!=key);
                return None;
            }
        }
        let stack = if !key.mask && layer.is_folder() {ts.layers.clone()} else {Vec::new()};
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
                    stack: stack.clone(),
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
            || entry.source.stack.len()!=stack.len()
            || !entry.source.stack.iter().zip(&stack).all(|(a,b)|Arc::ptr_eq(a,b))
            || entry.source.albedo_filter != ts.albedo_filter
            || entry.source.resolution != ts.resolution
            || entry.source.maps_generation != session.maps_generation();
        if changed {
            entry.source.albedo_filter = ts.albedo_filter;
            entry.source.layer = layer;
            entry.source.stack = stack;
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

    /// Histogram of what the effect `key.effect` receives: the content or mask stack right below
    /// it, on a [`Histogram::SIZE`]² UV square. Computed synchronously (small) and kept until the
    /// layer or the mesh maps change.
    pub fn input_histogram(&mut self, session: &Session, mut key: PreviewKey) -> Option<Arc<Histogram>> {
        key.size = Histogram::SIZE;
        let mesh = session.scene().mesh.clone();
        if self.geometry.as_ref().is_none_or(|(old, _)| !Arc::ptr_eq(old, &mesh)) {
            *self = Self::default();
            self.geometry = Some((mesh.clone(), Arc::new(GeometryCache::new(mesh))));
        }
        let (ts, layer) = session.document().texture_sets.iter().find_map(|ts| find(&ts.layers, key.layer).map(|layer| (ts, layer)))?;
        let generation = session.maps_generation();
        if let Some((k, l, g, h)) = &self.histogram
            && *k == key
            && Arc::ptr_eq(l, &layer)
            && *g == generation
        {
            return Some(h.clone());
        }
        let id = key.effect?;
        // The stage right below the effect: everything before it, or the bare base for the first.
        let mut truncated = layer.as_ref().clone();
        let channel = if key.mask {
            let mask = truncated.mask.as_mut()?;
            let index = mask.effects.iter().position(|e| e.id == id)?;
            mask.effects.truncate(index);
            key.channel
        } else {
            let index = truncated.content_effects.iter().position(|e| e.id == id)?;
            let channel = match truncated.content_effects[index].kind {
                ContentEffectKind::Filter { channel, .. } | ContentEffectKind::Levels { channel, .. } => channel,
                _ => key.channel,
            };
            truncated.content_effects.truncate(index);
            channel
        };
        let source = Source {
            layer: Arc::new(truncated),
            stack: if !key.mask && layer.is_folder() { ts.layers.clone() } else { Vec::new() },
            resources: session.resources().clone(),
            maps: session.mesh_maps(ts.id).cloned().unwrap_or_default(),
            maps_generation: generation,
            resolution: ts.resolution,
            albedo_filter: ts.albedo_filter,
            slot: ts.material_slot,
            geometry: self.geometry.as_ref()?.1.clone(),
        };
        let pixels = stage_pixels(&source.layer, channel, key.mask, key.size, &source);
        let gray = key.mask || channel.components() == 1;
        let h = Arc::new(Histogram::of(&pixels, gray));
        self.histogram = Some((key, layer, generation, h.clone()));
        Some(h)
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
    // Truncate only a worker-owned COW snapshot, never document data. Later effects cannot
    // influence this stage. Bypassed effects stay bypassed, including in their own preview.
    let mut layer = source.layer.as_ref().clone();
    let mut channel = key.channel;
    if let Some(id) = key.effect {
        if key.mask {
            if let Some(mask)=&mut layer.mask {
                let index=mask.effects.iter().position(|e|e.id==id).expect("validated effect");
                mask.effects.truncate(index+1);
            }
        } else {
            let index=layer.content_effects.iter().position(|e|e.id==id).expect("validated effect");
            if let ContentEffectKind::Filter {channel:target,..} | ContentEffectKind::Levels {channel:target,..}=layer.content_effects[index].kind {channel=target;}
            layer.content_effects.truncate(index+1);
        }
    }
    let pixels = stage_pixels(&layer, channel, key.mask, key.size, source);
    let mut rgba = vec![0; (key.size * key.size * 4) as usize];
    for (at, pixel) in pixels.iter().enumerate() {
        let at = at * 4;
        for c in 0..3 {
            let v = if !key.mask && channel.is_color() { subcraft_core::color::linear_to_srgb(pixel[c]) } else { pixel[c] };
            rgba[at + c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        rgba[at + 3] = (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    LayerPreview {
        size: key.size,
        rgba,
    }
}

/// The layer's content (or mask) in linear values on a `size`² UV square, row by row.
fn stage_pixels(layer: &Layer, channel: ChannelKind, mask: bool, size: u32, source: &Source) -> Vec<[f32; 4]> {
    let geometry = source.geometry.get(source.slot, size);
    let mut out = vec![[0.0f32; 4]; (size * size) as usize];
    let radius_scale = size as f32 / source.resolution as f32;
    // Disabled masks still display their authored result, with disabled state drawn by UI.
    let mask_stack = layer.mask.as_ref().map(|mask| {
        let mut m = mask.clone();
        m.enabled = true;
        m
    });
    let tiles = size.div_ceil(64);
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
        let pixels: Vec<[f32; 4]> = if mask {
            eval_mask_scaled(mask_stack.as_ref(), &tc, radius_scale)
                .unwrap_or_else(|| vec![0.0; 4096])
                .into_iter()
                .map(|v| [v, v, v, 1.0])
                .collect()
        } else {
            preview_content_context_scaled(layer, channel, &tc, radius_scale, &source.stack)
        };
        for (i, pixel) in pixels.iter().enumerate() {
            let (x, y) = geo.texel_xy(i);
            if x < size && y < size {
                out[(y * size + x) as usize] = *pixel;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod effect_tests;
