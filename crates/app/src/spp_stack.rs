//! Rebuilding a Substance Painter layer stack as SubCraft layers.
//!
//! The Painter document describes every layer (name, visibility, content actions, mask actions,
//! blending) and the procedural graphs it uses by name with their parameter values. Graphs that
//! have a SubCraft counterpart are mapped by name (exact where the data is in the file, approximate
//! where the look is only similar); everything else (brush strokes, decals, procedural materials,
//! unknown graphs, filter layers) stays as a named, hidden placeholder so the structure remains
//! visible. The result is judged against Painter's flattened textures by the caller.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek};
use std::sync::Arc;

use subcraft_core::{ChannelKind, ChannelValue, EffectId, LayerId, ResourceId};
use subcraft_doc::{
    BlendMode, ChannelBlend, FillChannel, FillSource, Filter, Generator, GeneratorKind, Layer, LayerContent, Levels, Mask, MaskEffect, MaskEffectKind, ProceduralKind, ProceduralParams,
    Projection, ProjectionMode, Resource, ResourceKind, ResourceStore, TextureSet,
};
use subcraft_mesh::spp::document::{ActionKind, GraphRef, LayerInfo, LayerKind, LevelsInfo, SourceInfo};
use subcraft_mesh::spp::SppFile;

use crate::spp_import::channel_kind;
use crate::spp_select::{self, Picker, RasterOp};
use subcraft_image::TiledImage;
use subcraft_materials::TextureSetGeometry;

/// What the rebuild did, for the import notes.
#[derive(Default, Debug, Clone)]
pub struct StackStats {
    pub layers: usize,
    pub folders: usize,
    /// Layers that carry no reconstructed content (kept as hidden placeholders).
    pub placeholders: usize,
    pub exact: usize,
    pub approximate: usize,
    /// Reasons something was not reconstructed, with counts (strokes, decals, unmapped graphs...).
    pub skipped: BTreeMap<String, usize>,
    /// Blend mode ids met that have no SubCraft mode yet (treated as the layer's default).
    pub blend_ids: BTreeMap<i32, usize>,
    /// Time spent per rebuild step (strokes, polygon fills, anchors...).
    pub timings: BTreeMap<&'static str, std::time::Duration>,
}

impl StackStats {
    fn skip(&mut self, what: impl Into<String>) {
        *self.skipped.entry(what.into()).or_default() += 1;
    }
}

pub struct Rebuilder<'a, R: Read + Seek> {
    pub spp: &'a mut SppFile<R>,
    pub ts: &'a TextureSet,
    pub resources: &'a mut ResourceStore,
    pub stats: StackStats,
    images: HashMap<String, Option<ResourceId>>,
    /// Allow reading embedded images out of the project's Substance archives.
    pub extract_embedded: bool,
    /// Mesh access for replaying polygon fills; without it they are skipped.
    pub picker: Option<&'a Picker>,
    /// The set's geometry at its resolution (texel to triangle), with `picker`.
    pub geometry: Option<Arc<TextureSetGeometry>>,
    /// Painter blending mode id to SubCraft mode ([`painter_blend`] by default; experiments override it).
    pub blend_map: BTreeMap<i32, BlendMode>,
    /// Mask rasters to paint: (layer, its Paint effect, polygon fills and strokes in order).
    rasters: Vec<(LayerId, EffectId, Vec<RasterOp>)>,
    /// Baked mesh maps, for evaluating anchored masks (references are baked into rasters).
    pub mesh_maps: Option<subcraft_materials::MeshMaps>,
    /// Anchor uid -> (layer, number of its mask effects at the anchor).
    anchors: HashMap<i64, (LayerId, usize)>,
    /// Mask effects standing for an anchor reference: (layer, effect, anchor uid).
    references: Vec<(LayerId, EffectId, i64)>,
}

impl<'a, R: Read + Seek> Rebuilder<'a, R> {
    pub fn new(spp: &'a mut SppFile<R>, ts: &'a TextureSet, resources: &'a mut ResourceStore) -> Self {
        Self { spp, ts, resources, stats: StackStats::default(), images: HashMap::new(), extract_embedded: true, picker: None, geometry: None, blend_map: (0..64).filter_map(|id| Some((id, painter_blend(id)?))).collect(), rasters: Vec::new(), mesh_maps: None, anchors: HashMap::new(), references: Vec::new() }
    }

    /// Layers bottom to top.
    pub fn rebuild(&mut self, infos: &[LayerInfo]) -> Vec<Layer> {
        let mut layers: Vec<Layer> = infos.iter().map(|i| self.layer(i)).collect();
        self.paint_rasters(&mut layers);
        self.resolve_references(&mut layers);
        layers
    }

    /// Paint the polygon fills and strokes collected for the masks, one raster per thread.
    fn paint_rasters(&mut self, layers: &mut [Layer]) {
        use rayon::prelude::*;
        let jobs = std::mem::take(&mut self.rasters);
        let (Some(picker), Some(geometry)) = (self.picker, self.geometry.clone()) else { return };
        let t = std::time::Instant::now();
        let images: Vec<TiledImage> = jobs.par_iter().map(|(_, _, ops)| spp_select::paint_raster(picker, geometry.clone(), ops)).collect();
        for ((layer_id, effect_id, _), image) in jobs.into_iter().zip(images) {
            if let Some(m) = find_layer_mut(layers, layer_id).and_then(|l| l.mask.as_mut()) {
                for e in &mut m.effects {
                    if e.id == effect_id {
                        Arc::make_mut(e).kind = MaskEffectKind::Paint { image: image.clone() };
                    }
                }
            }
        }
        *self.stats.timings.entry("strokes and polygon fills").or_default() += t.elapsed();
    }

    /// Fill each anchor reference with the anchored layer's mask as it is at the anchor (evaluated
    /// once and stored as a raster: the link itself is not live). References inside anchored masks
    /// are resolved first; a reference whose anchor is missing stays empty.
    fn resolve_references(&mut self, layers: &mut [Layer]) {
        let (Some(geometry), Some(maps)) = (self.geometry.clone(), self.mesh_maps.clone()) else {
            for _ in 0..self.references.len() {
                self.stats.skip("anchor references in masks");
            }
            return;
        };
        let t = std::time::Instant::now();
        let mut todo = std::mem::take(&mut self.references);
        for _round in 0..8 {
            let unresolved: Vec<EffectId> = todo.iter().map(|r| r.1).collect();
            let mut left = Vec::new();
            for (layer_id, effect_id, anchor) in todo {
                let Some(&(anchor_layer, count)) = self.anchors.get(&anchor) else {
                    self.stats.skip("anchor references to a missing anchor");
                    continue;
                };
                let Some(mut mask) = find_layer(layers, anchor_layer).and_then(|l| l.mask.clone()) else {
                    self.stats.skip("anchor references to a missing anchor");
                    continue;
                };
                mask.effects.truncate(count);
                if mask.effects.iter().any(|e| unresolved.contains(&e.id) && e.id != effect_id) {
                    left.push((layer_id, effect_id, anchor));
                    continue;
                }
                mask.enabled = true;
                let image = eval_mask_image(&mask, self.ts, &geometry, self.resources, &maps);
                if let Some(l) = find_layer_mut(layers, layer_id) {
                    if let Some(m) = l.mask.as_mut() {
                        for e in &mut m.effects {
                            if e.id == effect_id {
                                Arc::make_mut(e).kind = MaskEffectKind::Paint { image: image.clone() };
                            }
                        }
                    }
                }
                self.stats.exact += 1;
            }
            if left.is_empty() {
                *self.stats.timings.entry("anchor references").or_default() += t.elapsed();
                return;
            }
            todo = left;
        }
        for _ in &todo {
            self.stats.skip("anchor references in a cycle");
        }
    }

    fn layer(&mut self, info: &LayerInfo) -> Layer {
        let ts = self.ts;
        let name = if info.name.is_empty() { "Layer".to_string() } else { info.name.clone() };
        if info.kind == LayerKind::Group {
            self.stats.folders += 1;
            let mut folder = Layer::new_folder(name, ts);
            folder.visible = info.visible;
            let children: Vec<Arc<Layer>> = info.children.iter().map(|c| Arc::new(self.layer(c))).collect();
            if let LayerContent::Group(g) = &mut folder.content {
                g.children = children;
                g.expanded = false;
            }
            self.apply_opacity(&mut folder, info);
            self.attach_mask(&mut folder, info);
            return folder;
        }
        self.stats.layers += 1;
        let mut layer = self.content_layer(&name, info);
        layer.visible = info.visible && layer.visible;
        self.apply_opacity(&mut layer, info);
        self.attach_mask(&mut layer, info);
        layer
    }

    /// Mode and opacity per channel from the stored blending; unknown mode ids are counted and keep
    /// the layer's default mode.
    fn apply_opacity(&mut self, layer: &mut Layer, info: &LayerInfo) {
        for (flags, id, opacity) in &info.blends {
            let mode = self.blend_map.get(id).copied();
            if mode.is_none() {
                *self.stats.blend_ids.entry(*id).or_default() += 1;
            }
            for c in &self.ts.channels {
                if channel_bit(c.kind).is_some_and(|b| flags & b != 0) {
                    if let Some(blend) = layer.blend.get_mut(&c.kind) {
                        if blend.mode != BlendMode::Disable {
                            blend.opacity = *opacity;
                            if let Some(m) = mode.filter(|m| *m != BlendMode::Normal || c.kind != ChannelKind::Normal) {
                                blend.mode = m;
                            }
                        }
                    }
                }
            }
        }
    }

    fn content_layer(&mut self, name: &str, info: &LayerInfo) -> Layer {
        let ts = self.ts;
        let fills: Vec<(&Vec<SourceInfo>, i64)> = info.content.iter().filter_map(|a| if let ActionKind::Fill { sources, projection } = &a.kind { Some((sources, *projection)) } else { None }).collect();
        for a in &info.content {
            match &a.kind {
                ActionKind::Fill { .. } => {}
                ActionKind::Paint { strokes, .. } => self.stats.skip(format!("paint strokes on layer content ({} in one action)", strokes.len())),
                ActionKind::Eraser { .. } => self.stats.skip("eraser strokes on layer content"),
                ActionKind::Filter { graph } => self.stats.skip(format!("filter layer '{}'", graph.name())),
                ActionKind::Levels(_) => self.stats.skip("levels in layer content"),
                ActionKind::Decal { .. } => self.stats.skip("decals"),
                ActionKind::Anchor { .. } => self.stats.skip("anchor points"),
                ActionKind::Other(c) => self.stats.skip(format!("unknown action {c}")),
            }
        }
        if fills.is_empty() {
            return self.placeholder(name);
        }
        let mut layer = Layer::new_fill(name, ts);
        // Painter fills touch only the channels they have sources for.
        for blend in layer.blend.values_mut() {
            blend.mode = BlendMode::Disable;
        }
        let mut covered = 0;
        if let LayerContent::Fill(fill) = &mut layer.content {
            fill.channels.clear();
            let (_, projection) = fills[0];
            if projection == 2 {
                fill.projection = Projection { mode: ProjectionMode::Triplanar, ..Projection::default() };
            }
            for (sources, _) in &fills {
                for source in sources.iter() {
                    for c in &ts.channels {
                        let Some(bit) = channel_bit(c.kind) else { continue };
                        if source.channels() & bit == 0 {
                            continue;
                        }
                        let value = match source {
                            SourceInfo::Uniform { color, .. } => Some(uniform_value(c.kind, *color)),
                            SourceInfo::Bitmap { .. } => {
                                self.stats.skip("bitmap fill sources");
                                None
                            }
                            SourceInfo::Procedural { graph, .. } => {
                                self.stats.skip(format!("procedural material '{}'", graph.name()));
                                None
                            }
                            SourceInfo::Reference { .. } => {
                                self.stats.skip("anchor references in layer content");
                                None
                            }
                        };
                        if let Some(v) = value {
                            fill.channels.insert(c.kind, FillChannel::new(FillSource::uniform(v)));
                            let mode = if c.kind == ChannelKind::Normal { BlendMode::NormalMapCombine } else { BlendMode::Normal };
                            layer.blend.insert(c.kind, ChannelBlend::new(mode));
                            covered += 1;
                        }
                    }
                }
            }
        }
        if covered == 0 {
            let mut p = self.placeholder(name);
            p.id = layer.id;
            return p;
        }
        self.stats.exact += 1;
        layer
    }

    /// An empty hidden layer that keeps a Painter layer's name and place.
    fn placeholder(&mut self, name: &str) -> Layer {
        self.stats.placeholders += 1;
        let mut layer = Layer::new_paint(format!("{name} (not reconstructed)"), self.ts);
        layer.visible = false;
        layer
    }

    fn attach_mask(&mut self, layer: &mut Layer, info: &LayerInfo) {
        if !info.has_mask || info.mask.is_empty() {
            return;
        }
        let res = self.ts.resolution;
        let mut mask = Mask::new(res, info.mask_initial_white);
        mask.enabled = info.mask_enabled;
        let mut mapped = 0;
        // Polygon fills, strokes and erasers of one action group are painted into one raster.
        // They are painted later, all rasters in parallel (see `paint_rasters`).
        let mut pending: Option<(usize, Vec<RasterOp>, (BlendMode, f32))> = None;
        let layer_id = layer.id;
        let flush = |pending: &mut Option<(usize, Vec<RasterOp>, (BlendMode, f32))>, mask: &mut Mask, jobs: &mut Vec<(LayerId, EffectId, Vec<RasterOp>)>| {
            if let Some((_, ops, (mode, opacity))) = pending.take() {
                let mut effect = MaskEffect::new(MaskEffectKind::Paint { image: TiledImage::new(res, res, 2, [0.0; 4]) });
                effect.name = "Painted (Painter strokes and polygon fills)".into();
                effect.blend = mode;
                effect.opacity = opacity;
                jobs.push((layer_id, effect.id, ops));
                mask.effects.push(Arc::new(effect));
            }
        };
        for action in &info.mask {
            if !action.enabled {
                continue;
            }
            let raster_action = matches!(action.kind, ActionKind::Decal { .. } | ActionKind::Paint { .. } | ActionKind::Eraser { .. });
            if !raster_action || pending.as_ref().is_some_and(|p| p.0 != action.group) {
                flush(&mut pending, &mut mask, &mut self.rasters);
            }
            match &action.kind {
                ActionKind::Fill { sources, .. } => {
                    for source in sources {
                        match source {
                            SourceInfo::Uniform { color, .. } => {
                                mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Fill { value: color[0], procedural: None, projection: Projection::default() })));
                                self.stats.exact += 1;
                                mapped += 1;
                            }
                            SourceInfo::Procedural { graph, .. } => {
                                if let Some(effects) = self.map_graph(graph) {
                                    mapped += effects.len();
                                    mask.effects.extend(effects.into_iter().map(Arc::new));
                                } else {
                                    self.stats.skip(format!("mask graph '{}'", graph.name()));
                                }
                            }
                            SourceInfo::Bitmap { .. } => self.stats.skip("bitmap in a mask"),
                            SourceInfo::Reference { anchor, .. } => {
                                let mut effect = MaskEffect::new(MaskEffectKind::Paint { image: TiledImage::new(res, res, 2, [0.0; 4]) });
                                effect.name = "Anchor reference".into();
                                self.references.push((layer.id, effect.id, *anchor));
                                mask.effects.push(Arc::new(effect));
                                mapped += 1;
                            }
                        }
                    }
                }
                ActionKind::Levels(l) => {
                    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(levels_of(l)))));
                    self.stats.exact += 1;
                    mapped += 1;
                }
                ActionKind::Filter { graph } => match map_filter(graph) {
                    Some((filter, exact)) if filter.supports_mask() => {
                        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(filter))));
                        if exact { self.stats.exact += 1 } else { self.stats.approximate += 1 }
                        mapped += 1;
                    }
                    _ => self.stats.skip(format!("mask filter '{}'", graph.name())),
                },
                ActionKind::Paint { strokes, brush: Some(brush), opacity } | ActionKind::Eraser { strokes, brush: Some(brush), opacity } if self.picker.is_some() && self.geometry.is_some() => {
                    let erase = matches!(action.kind, ActionKind::Eraser { .. });
                    let blend = self.group_blend(action);
                    let op = RasterOp::Strokes { strokes: strokes.clone(), brush: brush.clone(), opacity: *opacity, erase };
                    pending.get_or_insert_with(|| (action.group, Vec::new(), blend)).1.push(op);
                    self.stats.approximate += 1;
                    mapped += 1;
                }
                ActionKind::Paint { .. } => self.stats.skip("painted mask strokes"),
                ActionKind::Eraser { .. } => self.stats.skip("erased mask strokes"),
                ActionKind::Decal { hits } => match (self.picker, self.geometry.as_ref()) {
                    (Some(_), Some(_)) => {
                        let blend = self.group_blend(action);
                        pending.get_or_insert_with(|| (action.group, Vec::new(), blend)).1.push(RasterOp::Fill(hits.clone()));
                        self.stats.exact += 1;
                        mapped += 1;
                    }
                    _ => self.stats.skip("mask polygon fills"),
                },
                ActionKind::Anchor { uid } => {
                    self.anchors.insert(*uid, (layer.id, mask.effects.len()));
                }
                ActionKind::Other(c) => self.stats.skip(format!("mask action {c}")),
            }
        }
        flush(&mut pending, &mut mask, &mut self.rasters);
        if mapped == 0 {
            // Nothing could be rebuilt: a white mask would show the layer everywhere, a black one never.
            self.stats.skip("masks with no reconstructible effect (mask left off)");
            return;
        }
        layer.mask = Some(mask);
    }

    /// Blend mode and opacity of the action group around a mask action (Normal when unknown).
    fn group_blend(&self, action: &subcraft_mesh::spp::document::ActionInfo) -> (BlendMode, f32) {
        action.blends.first().map_or((BlendMode::Normal, 1.0), |(_, id, opacity)| (self.blend_map.get(id).copied().unwrap_or(BlendMode::Normal), *opacity))
    }

    /// SubCraft mask effects for a Painter procedural graph, by name; `None` when there is no counterpart.
    fn map_graph(&mut self, g: &GraphRef) -> Option<Vec<MaskEffect>> {
        let name = g.name().to_ascii_lowercase();
        let flag = |id: &str| g.value(id).is_some_and(|v| v >= 0.5);
        let get = |ids: &[&str], default: f32| ids.iter().find_map(|i| g.value(i)).unwrap_or(default);
        let generator = |kind: GeneratorKind, level: f32, softness: f32, invert: bool| {
            let mut gen_ = Generator::new(kind);
            gen_.level = level.clamp(0.0, 1.0);
            gen_.softness = softness.clamp(0.01, 0.5);
            gen_.invert = invert;
            MaskEffect::new(MaskEffectKind::Generator(gen_))
        };
        let effects = if name == "mg_dirt" {
            self.stats.approximate += 1;
            vec![generator(GeneratorKind::Cavity, get(&["dirt_level"], 0.75), 0.4 * (1.0 - get(&["dirt_contrast"], 0.5)) + 0.05, flag("invert"))]
        } else if name.contains("edge_wear") || name == "mg_curvature" {
            self.stats.approximate += 1;
            let level = if name == "mg_curvature" { get(&["global_balance"], 0.5) } else { get(&["wear_level"], 0.3) * 1.5 };
            vec![generator(GeneratorKind::Edges, level, 0.15, flag("invert") || flag("global_invert"))]
        } else if name == "mg_ambient_occlusion" {
            self.stats.approximate += 1;
            vec![generator(GeneratorKind::Cavity, get(&["global_balance"], 0.5), 0.2, flag("global_invert"))]
        } else if name == "mg_position" {
            self.stats.approximate += 1;
            vec![generator(GeneratorKind::PositionGradient, get(&["global_balance"], 0.5), 0.2, flag("global_invert"))]
        } else if let Some(effects) = self.grunge_effects(g, &name) {
            effects
        } else if let Some(kind) = noise_kind(&name) {
            self.stats.approximate += 1;
            let mut p = ProceduralParams::new(kind);
            p.scale = get(&["scale"], 4.0).clamp(0.5, 64.0);
            p.balance = get(&["balance", "histogram_position"], 0.5).clamp(0.0, 1.0);
            p.contrast = (0.5 * (1.0 - get(&["contrast", "historgram_contrast"], 0.0))).clamp(0.02, 0.5);
            p.invert = flag("invert");
            vec![MaskEffect::new(MaskEffectKind::Fill { value: 1.0, procedural: Some(p), projection: Projection::default() })]
        } else {
            return None;
        };
        Some(effects)
    }

    /// Grunge graphs are a bitmap plus balance/contrast/invert: rebuild them from the embedded image.
    fn grunge_effects(&mut self, g: &GraphRef, name: &str) -> Option<Vec<MaskEffect>> {
        if !self.extract_embedded || !(name.starts_with("grunge_") || name.starts_with("mg_") && name.contains("wear")) {
            return None;
        }
        let id = self.embedded_image(&g.url)?;
        let balance = g.value("balance").unwrap_or(0.5);
        let contrast = g.value("contrast").unwrap_or(0.0);
        let invert = g.value("invert").is_some_and(|v| v >= 0.5);
        let width = (1.0 - contrast).max(0.02);
        let in_low = (1.0 - balance) - 0.5 * width;
        let image = MaskEffect::new(MaskEffectKind::Image { resource: id, projection: Projection::default(), invert });
        let levels = MaskEffect::new(MaskEffectKind::Levels(Levels { in_low, in_high: in_low + width, gamma: 1.0, out_low: 0.0, out_high: 1.0 }));
        self.stats.approximate += 1;
        Some(vec![image, levels])
    }

    /// The embedded JPEG of a Substance archive as a resource (decoded once per graph).
    fn embedded_image(&mut self, url: &str) -> Option<ResourceId> {
        if let Some(found) = self.images.get(url) {
            return *found;
        }
        let id = self.spp.embedded_jpegs(url).ok().and_then(|j| j.into_iter().next()).and_then(|bytes| {
            let bitmap = subcraft_image::io::decode_bitmap(&bytes, Some("embedded.jpg")).ok()?;
            let name = url.split('?').next()?.rsplit('/').next()?.to_string();
            Some(self.resources.insert(Resource { id: ResourceId::new(), name, kind: ResourceKind::Texture, bitmap: Arc::new(bitmap), source_path: None, encoded: Some(Arc::new(bytes)) }))
        });
        self.images.insert(url.to_string(), id);
        id
    }
}

/// SubCraft blend mode for a Painter blending mode id; `None` for ids not identified yet.
pub fn painter_blend(id: i32) -> Option<BlendMode> {
    match id {
        // Found by search against Painter's flattened result (DBAL-A2, see docs/spp-research.md).
        4 => Some(BlendMode::Multiply),
        25 => Some(BlendMode::Normal),
        _ => None,
    }
}


fn find_layer(layers: &[Layer], id: LayerId) -> Option<&Layer> {
    layers.iter().find_map(|l| find_in(l, id))
}

fn find_in(l: &Layer, id: LayerId) -> Option<&Layer> {
    if l.id == id {
        return Some(l);
    }
    match &l.content {
        LayerContent::Group(g) => g.children.iter().find_map(|c| find_in(c, id)),
        _ => None,
    }
}

fn find_layer_mut(layers: &mut [Layer], id: LayerId) -> Option<&mut Layer> {
    layers.iter_mut().find_map(|l| find_in_mut(l, id))
}

fn find_in_mut(l: &mut Layer, id: LayerId) -> Option<&mut Layer> {
    if l.id == id {
        return Some(l);
    }
    match &mut l.content {
        LayerContent::Group(g) => g.children.iter_mut().find_map(|c| find_in_mut(Arc::make_mut(c), id)),
        _ => None,
    }
}

/// A mask evaluated on its own as a (value, alpha = 1) raster at the set's resolution.
fn eval_mask_image(mask: &Mask, view: &TextureSet, geometry: &TextureSetGeometry, resources: &ResourceStore, maps: &subcraft_materials::MeshMaps) -> TiledImage {
    let mut set = TextureSet::new("anchor", view.material_slot, view.resolution);
    set.channels.retain(|c| c.kind == ChannelKind::Roughness);
    let tc = set.clone();
    let uniform = |name: &str, v: f32| {
        let mut l = Layer::new_fill(name, &tc);
        if let LayerContent::Fill(f) = &mut l.content {
            f.channels.insert(ChannelKind::Roughness, FillChannel::new(FillSource::uniform(ChannelValue::scalar(v))));
        }
        l
    };
    let low = uniform("0", 0.0);
    let mut high = uniform("1", 1.0);
    high.mask = Some(mask.clone());
    set.layers = vec![Arc::new(low), Arc::new(high)];
    let c = subcraft_compositor::composite_full(&set, &subcraft_compositor::EvalContext { geometry, resources, mesh_maps: maps });
    let r = c.channel(ChannelKind::Roughness).expect("roughness");
    let res = view.resolution;
    let mut out = TiledImage::new(res, res, 2, [0.0, 1.0, 0.0, 0.0]);
    let mut buf = vec![0.0f32; subcraft_image::TILE_TEXELS * r.comps()];
    let mut dst = vec![0.0f32; subcraft_image::TILE_TEXELS * 2];
    for t in 0..r.tile_count() {
        r.read_tile(t, &mut buf);
        for i in 0..subcraft_image::TILE_TEXELS {
            dst[i * 2] = buf[i * r.comps()];
            dst[i * 2 + 1] = 1.0;
        }
        out.write_tile(t, &dst);
    }
    out
}

fn channel_bit(kind: ChannelKind) -> Option<u64> {
    use subcraft_mesh::spp::channel_bits as b;
    Some(match kind {
        ChannelKind::BaseColor => b::BASE_COLOR,
        ChannelKind::Height => b::HEIGHT,
        ChannelKind::Roughness => b::ROUGHNESS,
        ChannelKind::Metallic => b::METALLIC,
        ChannelKind::Normal => b::NORMAL,
        ChannelKind::AmbientOcclusion => b::AMBIENT_OCCLUSION,
        ChannelKind::Emissive => b::EMISSIVE,
        ChannelKind::Opacity => b::OPACITY,
        _ => return None,
    })
}

fn uniform_value(kind: ChannelKind, color: [f32; 3]) -> ChannelValue {
    if kind.components() == 3 { ChannelValue::rgb(color[0], color[1], color[2]) } else { ChannelValue::scalar(color[0]) }
}

fn levels_of(l: &LevelsInfo) -> Levels {
    let avg = |v: [f32; 3]| (v[0] + v[1] + v[2]) / 3.0;
    Levels { in_low: avg(l.in_min), in_high: avg(l.in_max), gamma: avg(l.gamma), out_low: avg(l.out_min), out_high: avg(l.out_max) }
}

/// SubCraft pattern for Painter's noise/pattern graphs (the look is similar, not the same).
fn noise_kind(name: &str) -> Option<ProceduralKind> {
    let n = name;
    Some(if n.contains("dirt") {
        ProceduralKind::Dirt
    } else if n.contains("clouds") || n.contains("fractal") || n.contains("moisture") {
        ProceduralKind::Clouds
    } else if n.contains("crystal") || n.contains("cells") {
        ProceduralKind::Cells
    } else if n.contains("spots") || n.contains("speckle") {
        ProceduralKind::Speckles
    } else if n.contains("anisotropic") || n.contains("fur") || n.contains("brushed") {
        ProceduralKind::Brushed
    } else if n.contains("scratches") {
        ProceduralKind::Scratches
    } else {
        return None;
    })
}

/// SubCraft filter for a Painter procedural filter graph; the flag says whether the mapping is exact.
pub fn map_filter(g: &GraphRef) -> Option<(Filter, bool)> {
    let name = g.name().to_ascii_lowercase();
    match name.as_str() {
        "blur" => Some((Filter::Blur { radius: (g.value("intensity")? * 64.0).clamp(0.0, 32.0) }, false)),
        "sharpen" | "fx_sharpen" => Some((Filter::Sharpen { radius: 2.0, amount: (g.value("intensity")? * 8.0).clamp(0.0, 4.0) }, false)),
        "clamp" => Some((Filter::Clamp { low: g.value("min")?, high: g.value("max")? }, true)),
        _ => None,
    }
}

/// Shorthand used by the importer for a channel kind from Painter's flag.
pub fn kind_of_flag(bits: u64) -> Option<ChannelKind> {
    channel_kind(bits)
}
