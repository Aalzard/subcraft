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
    BlendMode, ChannelBlend, ContentEffect, ContentEffectKind, FillChannel, FillSource, Filter, Generator, GeneratorKind, Layer, LayerContent, Levels, Mask, MaskEffect, MaskEffectKind, ProceduralKind, ProceduralParams,
    Projection, ProjectionMode, Resource, ResourceKind, ResourceStore, TextureSet,
};
use subcraft_mesh::spp::document::{ActionKind, BrushInfo, GraphRef, LayerInfo, LayerKind, LevelsInfo, SourceInfo};
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

/// How Painter's mesh-map generators map onto SubCraft's threshold generators: for each family the
/// generator level is `level[0] + level[1] * p` (p = the graph's level/balance parameter) and its
/// softness `soft[0] + soft[1] * (1 - c)` (c = its contrast). Calibrated against Painter's
/// flattened result (docs/spp-research.md); the lab example searches them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeneratorMap {
    pub level: [f32; 2],
    pub soft: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// `mg_dirt` as Cavity (1 - ambient occlusion), from `dirt_level` / `dirt_contrast`.
    pub dirt: GeneratorMap,
    /// `mg_*edge_wear` as Edges (curvature), from `wear_level` / `wear_contrast`.
    pub edge_wear: GeneratorMap,
    /// `mg_curvature` as Edges, from `global_balance` / `global_contrast`.
    pub curvature: GeneratorMap,
    /// `mg_ambient_occlusion` as Cavity, from `global_balance` / `global_contrast`.
    pub ambient_occlusion: GeneratorMap,
    /// `mg_position` as a height gradient, from `global_balance` / `global_contrast`.
    pub position: GeneratorMap,
    /// Noise graphs as SubCraft patterns: pattern balance `level[0] + level[1] * balance` and
    /// contrast `soft[0] + soft[1] * contrast` (Substance's balance brightens upwards, SubCraft's
    /// pattern balance downwards).
    pub noise: GeneratorMap,
}

impl Tuning {
    /// The defaults, with `SUBCRAFT_SPP_TUNE="noise=l0,l1,s0,s1;dirt=..."` overriding families
    /// (developer aid for cross-project checks).
    pub fn from_env() -> Self {
        let mut t = Self::default();
        if let Ok(v) = std::env::var("SUBCRAFT_SPP_TUNE") {
            for item in v.split(';') {
                let Some((name, nums)) = item.split_once('=') else { continue };
                let n: Vec<f32> = nums.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if n.len() != 4 {
                    continue;
                }
                let m = GeneratorMap { level: [n[0], n[1]], soft: [n[2], n[3]] };
                match name.trim() {
                    "dirt" => t.dirt = m,
                    "edge_wear" => t.edge_wear = m,
                    "curvature" => t.curvature = m,
                    "ambient_occlusion" => t.ambient_occlusion = m,
                    "position" => t.position = m,
                    "noise" => t.noise = m,
                    _ => {}
                }
            }
        }
        t
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            // Calibrated on DBAL-A2 at 256 texels (coordinate descent, fills refitted per trial;
            // docs/spp-research.md). Dirt: more dirt with a higher Dirt Level, as in Painter.
            dirt: GeneratorMap { level: [0.75, -0.5], soft: [0.5, 0.6] },
            edge_wear: GeneratorMap { level: [-0.25, 1.0], soft: [0.5, 1.0] },
            // Curvature calibrates to an (almost) constant mask: SubCraft's curvature threshold does
            // not line up with Painter's, so a constant is closer.
            curvature: GeneratorMap { level: [-0.5, -1.5], soft: [0.15, 0.0] },
            // Ambient occlusion: no measurable effect, kept. Position: 0.0001 on DBAL-A2 but
            // CarInteriro's Base Color 0.030 -> 0.010 with it.
            ambient_occlusion: GeneratorMap { level: [0.0, 1.0], soft: [0.2, 0.0] },
            position: GeneratorMap { level: [1.0, 1.0], soft: [0.2, 0.0] },
            // Noise: Substance's balance mirrored, contrast as is. The DBAL-A2 calibration (noise
            // almost off) cost Street_cyberpunk 0.014 Roughness, so the plain mapping stays.
            noise: GeneratorMap { level: [1.0, -1.0], soft: [0.0, 1.0] },
        }
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
    references: Vec<(LayerId, EffectId, i64, Option<LevelsInfo>)>,
    /// Fill channels whose source (a procedural material or pattern, a bitmap) has no SubCraft
    /// counterpart: rebuilt as uniform values to be fitted against Painter's result
    /// ([`crate::spp_fit`]), in rebuild order (the same for any resolution).
    pub fitted: Vec<crate::spp_fit::Fitted>,
    /// Rasters painted by an earlier rebuild of the same stack at the same resolution, in job
    /// order (they do not depend on blending); filled on first use. For repeated experiments.
    pub raster_cache: Option<Arc<std::sync::Mutex<Vec<TiledImage>>>>,
    pub tuning: Tuning,
    /// Progress and cancellation of a background import.
    pub control: Option<Arc<crate::spp_job::ImportControl>>,
}

impl<'a, R: Read + Seek> Rebuilder<'a, R> {
    pub fn new(spp: &'a mut SppFile<R>, ts: &'a TextureSet, resources: &'a mut ResourceStore) -> Self {
        Self { spp, ts, resources, stats: StackStats::default(), images: HashMap::new(), extract_embedded: true, picker: None, geometry: None, blend_map: blend_table(), rasters: Vec::new(), mesh_maps: None, anchors: HashMap::new(), references: Vec::new(), fitted: Vec::new(), raster_cache: None, tuning: Tuning::from_env(), control: None }
    }

    /// Layers bottom to top.
    pub fn rebuild(&mut self, infos: &[LayerInfo]) -> Vec<Layer> {
        let t = std::time::Instant::now();
        let mut layers: Vec<Layer> = infos.iter().map(|i| self.layer(i)).collect();
        *self.stats.timings.entry("layer conversion").or_default() += t.elapsed();
        self.paint_rasters(&mut layers);
        self.resolve_references(&mut layers);
        layers
    }

    /// Paint the polygon fills and strokes collected for the masks, one raster per thread.
    fn paint_rasters(&mut self, layers: &mut [Layer]) {
        use rayon::prelude::*;
        let jobs = std::mem::take(&mut self.rasters);
        if self.cancelled() {
            return;
        }
        let (Some(picker), Some(full)) = (self.picker, self.geometry.clone()) else { return };
        let t = std::time::Instant::now();
        // Strokes cost grows with the texels they cover: rasters are painted at no more than
        // `raster_max_resolution()` and resampled to the set's resolution.
        let res = full.resolution;
        let cap = raster_max_resolution().min(res);
        let geometry = if cap < res { Arc::new(TextureSetGeometry::new(full.mesh().clone(), full.material_slot, cap)) } else { full.clone() };
        let gutter = GutterFill::new(&geometry);
        let control = self.control.clone();
        let cached = self.raster_cache.as_ref().map(|c| c.lock().unwrap_or_else(|p| p.into_inner()).clone()).filter(|c| c.len() == jobs.len());
        let images: Vec<TiledImage> = match cached {
            Some(c) => c,
            None => {
                let shared = std::sync::Mutex::new(HashMap::new());
                jobs.par_iter().map(|(_, _, ops)| {
                    if control.as_ref().is_some_and(|c| c.is_cancelled()) {
                        return TiledImage::new(res, res, 2, [0.0; 4]);
                    }
                    // The padding outside the UV islands takes the nearest island texel's value, as
                    // Painter's own textures do: left empty, mipmapped views blend it into every
                    // part's border (bright outlines on 4K sets).
                    let image = gutter.fill(&spp_select::paint_raster(picker, geometry.clone(), ops));
                    let mut image = if cap < res { image.resampled(res, res) } else { image };
                    share_constant_tiles(&mut image, &shared);
                    image
                }).collect()
            }
        };
        if let Some(c) = &self.raster_cache {
            *c.lock().unwrap_or_else(|p| p.into_inner()) = images.clone();
        }
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
        let shared = std::sync::Mutex::new(HashMap::new());
        // Anchored masks are evaluated at no more than `raster_max_resolution() / 2` (each is a
        // composite of the mask; at 4096 they took 4 minutes for one project) and resampled.
        let res = self.ts.resolution;
        let cap = (raster_max_resolution() / 2).min(res).max(64);
        let (eval_view, eval_geometry, eval_maps) = if cap < res {
            let mut view = self.ts.clone();
            view.resolution = cap;
            let g = Arc::new(TextureSetGeometry::new(geometry.mesh().clone(), geometry.material_slot, cap));
            let m = subcraft_materials::MeshMaps::from_map(maps.iter().map(|(k, v)| (k, v.resampled(cap, cap))).collect());
            (view, g, m)
        } else {
            (self.ts.clone(), geometry.clone(), maps.clone())
        };
        let mut todo = std::mem::take(&mut self.references);
        for _round in 0..8 {
            let unresolved: Vec<EffectId> = todo.iter().map(|r| r.1).collect();
            let mut left = Vec::new();
            for (layer_id, effect_id, anchor, levels) in todo {
                if self.cancelled() {
                    return;
                }
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
                    left.push((layer_id, effect_id, anchor, levels));
                    continue;
                }
                mask.enabled = true;
                if let Some(l) = &levels {
                    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(levels_of(l)))));
                }
                let image = if cap < res {
                    eval_mask_image(&scaled_mask(&mask, cap as f32 / res as f32, cap), &eval_view, &eval_geometry, self.resources, &eval_maps).resampled(res, res)
                } else {
                    eval_mask_image(&mask, &eval_view, &eval_geometry, self.resources, &eval_maps)
                };
                let mut image = image;
                share_constant_tiles(&mut image, &shared);
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

    fn cancelled(&self) -> bool {
        self.control.as_ref().is_some_and(|c| c.is_cancelled())
    }

    /// Painter's geometry mask (the layer applies to some mesh objects only) as a part mask by
    /// object, matched by name.
    fn geometry_mask(&mut self, layer: &mut Layer, info: &LayerInfo) {
        let Some((default_on, listed)) = &info.mesh_filter else { return };
        let Some(geometry) = &self.geometry else { return };
        let objects = &geometry.mesh().objects;
        let ids = objects.iter().enumerate().filter(|(_, o)| *default_on != listed.contains(&o.name)).map(|(i, _)| i as u32);
        layer.parts = Some(subcraft_doc::PartMask::for_kind(subcraft_core::PartKind::Object, ids));
        if std::env::var_os("SUBCRAFT_SPP_DEBUG").is_some() {
            eprintln!("geometry mask of '{}': {:?} -> {:?} of {} objects", info.name, info.mesh_filter, layer.parts.as_ref().map(|p| &p.parts), objects.len());
        }
        if listed.iter().any(|n| !objects.iter().any(|o| &o.name == n)) {
            self.stats.skip("geometry mask naming a mesh the model does not have");
        } else {
            self.stats.exact += 1;
        }
    }

    fn layer(&mut self, info: &LayerInfo) -> Layer {
        let ts = self.ts;
        if let Some(c) = &self.control {
            c.layer_done();
            if c.is_cancelled() {
                return Layer::new_folder("cancelled", ts);
            }
        }
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
            self.geometry_mask(&mut folder, info);
            return folder;
        }
        self.stats.layers += 1;
        let mut layer = self.content_layer(&name, info);
        layer.visible = info.visible && layer.visible;
        let filter_layer = matches!(layer.content, LayerContent::Group(_));
        self.apply_opacity(&mut layer, info);
        if filter_layer {
            for b in layer.blend.values_mut() {
                if b.mode == BlendMode::Normal {
                    b.mode = BlendMode::Passthrough;
                }
            }
        }
        self.attach_mask(&mut layer, info);
        self.geometry_mask(&mut layer, info);
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
                            if let Some(m) = mode {
                                // Id 2 is measured on color channels only; Normal keeps its default.
                                let unmeasured = *id == 2 && c.kind == ChannelKind::Normal;
                                blend.mode = if m == BlendMode::Normal || unmeasured { default_mode(c.kind) } else { m };
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
        // Filters and Levels in the content act on what is below them: the layer's own fill, or
        // (in a layer that has no fill, Painter's filter layer) the stack below the layer.
        let mut effects: Vec<ContentEffect> = Vec::new();
        for a in &info.content {
            match &a.kind {
                ActionKind::Fill { .. } => {}
                ActionKind::Paint { strokes, .. } => self.stats.skip(format!("paint strokes on layer content ({} in one action)", strokes.len())),
                ActionKind::Eraser { .. } => self.stats.skip("eraser strokes on layer content"),
                ActionKind::Filter { graph } if a.enabled => match map_filter(graph, ts.resolution) {
                    Some((filter, exact)) => {
                        let before = effects.len();
                        for c in &ts.channels {
                            if channel_bit(c.kind).is_some_and(|b| a.channels & b != 0) && filter.supports_channel(c.kind) {
                                effects.push(ContentEffect::new(ContentEffectKind::Filter { channel: c.kind, filter }, ts));
                            }
                        }
                        if effects.len() > before {
                            if exact { self.stats.exact += 1 } else { self.stats.approximate += 1 }
                        }
                    }
                    None => self.stats.skip(format!("filter layer '{}'", graph.name())),
                },
                ActionKind::Filter { .. } => {}
                ActionKind::Levels(l) if a.enabled => {
                    for c in &ts.channels {
                        if channel_bit(c.kind).is_some_and(|b| a.channels & b != 0) && c.kind != ChannelKind::Normal {
                            effects.push(ContentEffect::new(ContentEffectKind::Levels { channel: c.kind, levels: levels_of(l) }, ts));
                        }
                    }
                    self.stats.exact += 1;
                }
                ActionKind::Levels(_) => {}
                ActionKind::Decal { .. } => self.stats.skip("decals"),
                ActionKind::Anchor { .. } => self.stats.skip("anchor points"),
                ActionKind::Other(c) => self.stats.skip(format!("unknown action {c}")),
            }
        }
        if fills.is_empty() {
            if !effects.is_empty() && !skip("filter_layers") && info.content.iter().all(|a| matches!(a.kind, ActionKind::Filter { .. } | ActionKind::Levels(_))) {
                // An empty pass-through folder filters the stack below it, mixed in by its mask.
                // A filter that reads neighbouring texels (sharpen, blur) of everything below makes
                // each composite evaluate the whole stack around every tile, again for every such
                // layer above: DBAL-A2 at 512 took 350 s instead of 9 s. They change none of the
                // measured differences, so they are rebuilt switched off.
                let neighbourhood = effects.iter().any(|e| matches!(e.kind, ContentEffectKind::Filter { filter, .. } if filter.radius().is_some()));
                let mut folder = Layer::new_folder(if neighbourhood { format!("{name} (off: filters the whole stack below, slow)") } else { name.to_string() }, ts);
                folder.content_effects = effects.into_iter().map(Arc::new).collect();
                if neighbourhood {
                    folder.visible = false;
                    self.stats.skip("filter layers that sharpen or blur the stack below (rebuilt, switched off)");
                }
                return folder;
            }
            return self.placeholder(name);
        }
        let mut layer = Layer::new_fill(name, ts);
        // Painter fills touch only the channels they have sources for.
        for blend in layer.blend.values_mut() {
            blend.mode = BlendMode::Disable;
        }
        let mut covered = 0;
        let mut fitted: Vec<ChannelKind> = Vec::new();
        // A material's normal, solved from Painter's result at import (many layers put a material
        // into Normal only: Rubber, Plastic, Iron on DBAL-A2).
        let mut solved_normal = false;
        if let LayerContent::Fill(fill) = &mut layer.content {
            fill.channels.clear();
            let (_, projection) = fills[0];
            let uv = info.content.iter().find_map(|a| if matches!(a.kind, ActionKind::Fill { .. }) { a.uv } else { None });
            fill.projection = projection_of(projection, uv.as_ref());
            for (sources, _) in &fills {
                for source in sources.iter() {
                    for c in &ts.channels {
                        let Some(bit) = channel_bit(c.kind) else { continue };
                        if source.channels() & bit == 0 {
                            continue;
                        }
                        let value = match source {
                            SourceInfo::Uniform { color, .. } => Some(uniform_value(c.kind, *color)),
                            SourceInfo::Bitmap { url, .. } if c.kind != ChannelKind::Normal && !skip("content_bitmaps") && self.bitmap(url, true).is_some() => {
                                let resource = self.bitmap(url, true).unwrap();
                                fill.channels.insert(c.kind, FillChannel::new(FillSource::Bitmap { resource }));
                                layer.blend.insert(c.kind, ChannelBlend::new(BlendMode::Normal));
                                covered += 1;
                                None
                            }
                            // Fitted uniform values stand in for these. The normal starts flat (a
                            // no-op under normal-map combine) and is solved from Painter's result
                            // with the other channels (`spp_import::fit_solve_composite`).
                            SourceInfo::Bitmap { .. } | SourceInfo::Procedural { .. } if c.kind == ChannelKind::Normal => {
                                fill.channels.insert(c.kind, FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.5, 0.5, 1.0))));
                                layer.blend.insert(c.kind, ChannelBlend::new(default_mode(c.kind)));
                                covered += 1;
                                solved_normal = true;
                                None
                            }
                            // Painter's fractal sums are noise SubCraft can draw: kept as a pattern
                            // (fitting them flattened their grain to one value).
                            SourceInfo::Procedural { graph, .. } if fractal_sum(graph).is_some() && !skip("fractal") => {
                                let params = fractal_sum(graph).unwrap();
                                // Range calibrated on DBAL-A2's fine color variation (detail under 5
                                // texels at 2K): the full 0..1 range gave twice Painter's.
                                let k = std::env::var("SUBCRAFT_SPP_FRACTAL_RANGE").ok().and_then(|v| v.parse().ok()).unwrap_or(FRACTAL_RANGE);
                                let (lo, hi) = (0.5 - 0.5 * k, 0.5 + 0.5 * k);
                                let (low, high) = if c.kind.components() == 3 { (ChannelValue::rgb(lo, lo, lo), ChannelValue::rgb(hi, hi, hi)) } else { (ChannelValue::scalar(lo), ChannelValue::scalar(hi)) };
                                fill.channels.insert(c.kind, FillChannel::new(FillSource::Procedural { params, low, high }));
                                layer.blend.insert(c.kind, ChannelBlend::new(default_mode(c.kind)));
                                covered += 1;
                                self.stats.approximate += 1;
                                None
                            }
                            SourceInfo::Bitmap { .. } | SourceInfo::Procedural { .. } => {
                                let what = match source {
                                    SourceInfo::Procedural { graph, .. } => format!("procedural fill '{}' (fitted uniform values)", graph.name()),
                                    _ => "bitmap fill (fitted uniform values)".to_string(),
                                };
                                self.stats.skip(what);
                                fitted.push(c.kind);
                                Some(c.kind.default_paint_value())
                            }
                            SourceInfo::Reference { .. } => {
                                self.stats.skip("anchor references in layer content");
                                None
                            }
                        };
                        if let Some(v) = value {
                            fill.channels.insert(c.kind, FillChannel::new(FillSource::uniform(v)));
                            let mode = default_mode(c.kind);
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
        if fitted.is_empty() && !solved_normal {
            self.stats.exact += 1;
        } else {
            fitted.sort();
            fitted.dedup();
            self.fitted.push((layer.id, fitted));
            self.stats.approximate += 1;
        }
        layer.content_effects = effects.into_iter().map(Arc::new).collect();
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
        if !info.has_mask {
            return;
        }
        let res = self.ts.resolution;
        if info.mask.iter().all(|a| !a.enabled) {
            // An empty mask is its initial value: a black one hides the layer.
            let mut mask = Mask::new(res, info.mask_initial_white);
            mask.enabled = info.mask_enabled;
            layer.mask = Some(mask);
            self.stats.exact += 1;
            return;
        }
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
                ActionKind::Fill { sources, projection: proj_id } => {
                    let (mode, opacity) = self.action_blend(action);
                    let projection = projection_of(*proj_id, action.uv.as_ref());
                    for source in sources {
                        let mut effects: Vec<MaskEffect> = match source {
                            SourceInfo::Uniform { color, .. } => {
                                self.stats.exact += 1;
                                vec![MaskEffect::new(MaskEffectKind::Fill { value: color[0], procedural: None, projection: Projection::default() })]
                            }
                            SourceInfo::Procedural { graph, .. } => match self.map_graph(graph, projection) {
                                Some(effects) => effects,
                                None => {
                                    self.stats.skip(format!("mask graph '{}'", graph.name()));
                                    Vec::new()
                                }
                            },
                            // Placed once (not tiled): baked into a raster with Painter's placement.
                            SourceInfo::Bitmap { url, .. } if action.uv.is_some_and(|t| !t.tiling) && self.geometry.is_some() && self.bitmap(url, false).is_some() => {
                                let id = self.bitmap(url, false).unwrap();
                                let bitmap = self.resources.get(id).unwrap().bitmap.clone();
                                let t = std::time::Instant::now();
                                let image = spp_select::placed_bitmap(self.geometry.as_ref().unwrap(), &bitmap, action.uv.as_ref().unwrap());
                                *self.stats.timings.entry("placed bitmaps").or_default() += t.elapsed();
                                self.stats.exact += 1;
                                let mut e = MaskEffect::new(MaskEffectKind::Paint { image });
                                e.name = "Placed image".into();
                                vec![e]
                            }
                            SourceInfo::Bitmap { url, .. } => match self.bitmap(url, false).filter(|_| !skip("mask_bitmaps")) {
                                Some(resource) => {
                                    self.stats.exact += 1;
                                    vec![MaskEffect::new(MaskEffectKind::Image { resource, projection, invert: false })]
                                }
                                None => {
                                    self.stats.skip("bitmap in a mask");
                                    Vec::new()
                                }
                            },
                            SourceInfo::Reference { anchor, levels, .. } => {
                                let mut effect = MaskEffect::new(MaskEffectKind::Paint { image: TiledImage::new(res, res, 2, [0.0; 4]) });
                                effect.name = "Anchor reference".into();
                                self.references.push((layer.id, effect.id, *anchor, levels.clone()));
                                vec![effect]
                            }
                        };
                        // The fill's blending applies to its (single) result.
                        if let Some(e) = effects.first_mut() {
                            e.blend = mode;
                            e.opacity = opacity;
                        }
                        mapped += effects.len();
                        mask.effects.extend(effects.into_iter().map(Arc::new));
                    }
                }
                ActionKind::Levels(l) => {
                    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(levels_of(l)))));
                    self.stats.exact += 1;
                    mapped += 1;
                }
                ActionKind::Filter { graph } => match map_filter(graph, res) {
                    Some((filter, exact)) if filter.supports_mask() => {
                        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(filter))));
                        if exact { self.stats.exact += 1 } else { self.stats.approximate += 1 }
                        mapped += 1;
                    }
                    _ => self.stats.skip(format!("mask filter '{}'", graph.name())),
                },
                // A text tip stamps glyphs of a font; SubCraft would stamp a disc of the whole tip
                // size instead (on DBAL-A2 one "2" eraser stamp wiped most of a label), so these
                // strokes are left out and named.
                ActionKind::Paint { brush: Some(BrushInfo { tip_text: Some(text), tip, .. }), .. } | ActionKind::Eraser { brush: Some(BrushInfo { tip_text: Some(text), tip, .. }), .. } => {
                    self.stats.skip(format!("text brush stamps (\"{text}\" in {tip})"));
                }
                ActionKind::Paint { strokes, brush: Some(brush), opacity } | ActionKind::Eraser { strokes, brush: Some(brush), opacity } if self.picker.is_some() && self.geometry.is_some() => {
                    let erase = matches!(action.kind, ActionKind::Eraser { .. });
                    let blend = self.group_blend(action);
                    let stencil_image = brush.stencil.as_ref().and_then(|(url, m)| {
                        // Kept with its alpha: the stencil lets paint through by brightness x alpha.
                        // A Substance stencil (`.sbsar`) is its embedded image when it has one.
                        let id = if url.contains(".sbsar") { self.embedded_image(url)? } else { self.bitmap(url, true)? };
                        Some((self.resources.get(id)?.bitmap.clone(), *m))
                    });
                    // Without its stencil a stroke would paint its whole brush path (on DBAL-A2,
                    // solid white blobs where Painter has splatter dots or stencilled marks): left out.
                    if let (Some((url, _)), None) = (&brush.stencil, &stencil_image) {
                        let name = url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or(url);
                        self.stats.skip(format!("stencilled strokes whose stencil could not be read ({name})"));
                        continue;
                    }
                    let stencil = stencil_image.as_ref().map(|(bitmap, m)| Arc::new(spp_select::stencil_of(bitmap.clone(), m)));
                    let uv_stencil = stencil_image.map(|(bitmap, matrix)| spp_select::UvStencil { bitmap, matrix });
                    let op = RasterOp::Strokes { strokes: strokes.clone(), brush: brush.clone(), opacity: *opacity, erase, stencil, uv_stencil };
                    pending.get_or_insert_with(|| (action.group, Vec::new(), blend)).1.push(op);
                    self.stats.approximate += 1;
                    mapped += 1;
                }
                ActionKind::Paint { .. } => self.stats.skip("painted mask strokes"),
                ActionKind::Eraser { .. } => self.stats.skip("erased mask strokes"),
                ActionKind::Decal { hits } => match (self.picker, self.geometry.as_ref()) {
                    (Some(picker), Some(_)) => {
                        if std::env::var_os("SUBCRAFT_SPP_DEBUG").is_some() {
                            for h in hits {
                                let mut objects: Vec<u32> = picker.select(h, h.through).iter().filter_map(|&t| picker.mesh.tri_object.get(t as usize).copied().flatten()).collect();
                                objects.sort_unstable();
                                objects.dedup();
                                eprintln!("polygon fill in '{}': granularity {} value {} points {:?} -> objects {:?}", info.name, h.granularity, h.value, h.points, objects.iter().map(|&o| picker.mesh.objects[o as usize].name.as_str()).collect::<Vec<_>>());
                            }
                        }
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

    /// A mask effect's own blending, else its group's (Normal when unknown).
    fn action_blend(&mut self, action: &subcraft_mesh::spp::document::ActionInfo) -> (BlendMode, f32) {
        match action.own_blends.first() {
            Some((_, id, opacity)) => {
                if !self.blend_map.contains_key(id) {
                    *self.stats.blend_ids.entry(*id).or_default() += 1;
                }
                (self.blend_map.get(id).copied().unwrap_or(BlendMode::Normal), *opacity)
            }
            None => self.group_blend(action),
        }
    }

    /// SubCraft mask effects for a Painter procedural graph, by name; `None` when there is no counterpart.
    fn map_graph(&mut self, g: &GraphRef, projection: Projection) -> Option<Vec<MaskEffect>> {
        let name = g.name().to_ascii_lowercase();
        let flag = |id: &str| g.value(id).is_some_and(|v| v >= 0.5);
        let get = |ids: &[&str], default: f32| ids.iter().find_map(|i| g.value(i)).unwrap_or(default);
        // Painter's own breakup ("Grunge Amount") instead of SubCraft's new-generator default (0.3).
        // The geometric edge band stays: without it (curvature-map edges with the current
        // mapping) DBAL-A2's Base Color went 0.0140 -> 0.0162 (SUBCRAFT_SPP_GEN=painter).
        let breakup = get(&["grunge_amount"], 0.0);
        let mode = std::env::var("SUBCRAFT_SPP_GEN").unwrap_or_else(|_| "spread".into());
        let generator = |kind: GeneratorKind, level: f32, softness: f32, invert: bool| {
            let mut gen_ = Generator::new(kind);
            gen_.level = level.clamp(0.0, 1.0);
            gen_.softness = softness.clamp(0.01, 1.0);
            gen_.invert = invert;
            if mode != "subcraft" {
                gen_.noise_amount = breakup.clamp(0.0, 1.0);
                if mode != "spread" {
                    gen_.edge_spread = None;
                }
            }
            MaskEffect::new(MaskEffectKind::Generator(gen_))
        };
        let t = self.tuning;
        let mapped = |m: GeneratorMap, kind: GeneratorKind, level: f32, contrast: f32, invert: bool| generator(kind, m.level[0] + m.level[1] * level, m.soft[0] + m.soft[1] * (1.0 - contrast), invert);
        let effects = if name == "mg_dirt" {
            self.stats.approximate += 1;
            vec![mapped(t.dirt, GeneratorKind::Cavity, get(&["dirt_level"], 0.75), get(&["dirt_contrast"], 0.5), flag("invert"))]
        } else if name.contains("edge_wear") {
            self.stats.approximate += 1;
            vec![mapped(t.edge_wear, GeneratorKind::Edges, get(&["wear_level"], 0.3), get(&["wear_contrast"], 0.5), flag("invert"))]
        } else if name == "mg_curvature" {
            self.stats.approximate += 1;
            vec![mapped(t.curvature, GeneratorKind::Edges, get(&["global_balance"], 0.5), get(&["global_contrast"], 0.0), flag("global_invert"))]
        } else if name == "mg_ambient_occlusion" {
            self.stats.approximate += 1;
            vec![mapped(t.ambient_occlusion, GeneratorKind::Cavity, get(&["global_balance"], 0.5), get(&["global_contrast"], 0.0), flag("global_invert"))]
        } else if name == "mg_position" {
            self.stats.approximate += 1;
            vec![mapped(t.position, GeneratorKind::PositionGradient, get(&["global_balance"], 0.5), get(&["global_contrast"], 0.0), flag("global_invert"))]
        } else if let Some(effects) = self.grunge_effects(g, &name, projection) {
            effects
        } else if let Some(kind) = noise_kind(&name).filter(|_| !skip("noise")) {
            self.stats.approximate += 1;
            let mut p = ProceduralParams::new(kind);
            p.scale = get(&["scale"], 4.0).clamp(0.5, 64.0);
            p.balance = (t.noise.level[0] + t.noise.level[1] * get(&["balance", "histogram_position"], 0.5)).clamp(0.0, 1.0);
            p.contrast = (t.noise.soft[0] + t.noise.soft[1] * get(&["contrast", "historgram_contrast"], 0.0)).clamp(0.0, 0.98);
            p.invert = flag("invert");
            vec![MaskEffect::new(MaskEffectKind::Fill { value: 1.0, procedural: Some(p), projection })]
        } else {
            return None;
        };
        Some(effects)
    }

    /// Grunge graphs are a bitmap plus balance/contrast/invert: rebuild them from the embedded image.
    /// The balance and contrast are baked into a copy of the image, so the mask effect blends
    /// the adjusted grunge (not a Levels of the whole mask built so far).
    fn grunge_effects(&mut self, g: &GraphRef, name: &str, projection: Projection) -> Option<Vec<MaskEffect>> {
        if !self.extract_embedded || !(name.starts_with("grunge_") || name.starts_with("mg_") && name.contains("wear")) {
            return None;
        }
        let balance = g.value("balance").unwrap_or(0.5);
        let contrast = g.value("contrast").unwrap_or(0.0);
        let invert = g.value("invert").is_some_and(|v| v >= 0.5);
        // Grunge balance/contrast as Levels on the image: the window centre moves by
        // `shift + slope * (0.5 - balance)` from 0.5, its width is `1 - contrast`.
        let (shift, slope, gamma_k) = grunge_mapping();
        let width = (1.0 - contrast).max(0.02);
        let in_low = 0.5 + shift + slope * (0.5 - balance) - 0.5 * width;
        let gamma = 2f32.powf(gamma_k * (balance - 0.5));
        let levels = Levels { in_low, in_high: in_low + width, gamma, out_low: 0.0, out_high: 1.0 };
        let id = self.embedded_image_leveled(&g.url, levels)?;
        self.stats.approximate += 1;
        Some(vec![MaskEffect::new(MaskEffectKind::Image { resource: id, projection, invert })])
    }

    /// An imported image of the project (`.image` resource) as a texture resource, decoded once.
    /// `color` keeps it as stored (8-bit data is sRGB encoded, sampled as such for color
    /// channels); otherwise it becomes a grayscale copy without alpha, as a mask reads it.
    pub fn bitmap(&mut self, url: &str, color: bool) -> Option<ResourceId> {
        let key = format!("{url}#{}", if color { "color" } else { "gray" });
        if let Some(found) = self.images.get(&key) {
            return *found;
        }
        let t = std::time::Instant::now();
        let max = self.ts.resolution.max(256);
        let decoded = self.spp.image_resource_blob(url).ok().flatten().and_then(|blob| {
            let eight = subcraft_mesh::spp::images::image_resource_is_8bit(&blob);
            let plane = subcraft_mesh::spp::images::decode_image_resource(&blob, max).ok()?;
            Some((plane, eight))
        });
        let id = decoded.map(|(p, eight)| {
            let n = (p.width * p.height) as usize;
            let bitmap = if color {
                if eight {
                    subcraft_image::Bitmap::new_u8(p.width, p.height, p.components as u8, p.data.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect())
                } else {
                    subcraft_image::Bitmap::new_f32(p.width, p.height, p.components as u8, p.data.clone())
                }
            } else {
                let gray: Vec<f32> = (0..n).map(|i| p.data[i * p.components]).collect();
                subcraft_image::Bitmap::new_f32(p.width, p.height, 1, gray)
            };
            let name = url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or("image").to_string();
            self.resources.insert(Resource { id: ResourceId::new(), name, kind: ResourceKind::Texture, bitmap: Arc::new(bitmap), source_path: None, encoded: None })
        });
        self.images.insert(key, id);
        *self.stats.timings.entry("bitmap decoding").or_default() += t.elapsed();
        id
    }

    /// [`Self::embedded_image`] with Levels applied to its values (one resource per graph and levels).
    fn embedded_image_leveled(&mut self, url: &str, levels: Levels) -> Option<ResourceId> {
        let key = format!("{url}#{:?}", levels);
        if let Some(found) = self.images.get(&key) {
            return *found;
        }
        let base = self.embedded_image(url)?;
        let source = self.resources.get(base)?.clone();
        let bitmap = source.bitmap.map_color(|_, v| levels.apply(v));
        let id = Some(self.resources.insert(Resource { id: ResourceId::new(), name: format!("{} (levels)", source.name), kind: ResourceKind::Texture, bitmap: Arc::new(bitmap), source_path: None, encoded: None }));
        self.images.insert(key, id);
        id
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

/// The id table: [`painter_blend`], with `SUBCRAFT_SPP_BLENDS="29=Subtract,10=LinearDodge"`
/// overriding entries (developer aid for experiments).
pub fn blend_table() -> BTreeMap<i32, BlendMode> {
    let mut map: BTreeMap<i32, BlendMode> = (0..64).filter_map(|id| Some((id, painter_blend(id)?))).collect();
    if let Ok(v) = std::env::var("SUBCRAFT_SPP_BLENDS") {
        for item in v.split(',') {
            let Some((id, name)) = item.split_once('=') else { continue };
            let (Ok(id), Some(mode)) = (id.trim().parse::<i32>(), BlendMode::ALL.into_iter().find(|m| format!("{m:?}") == name.trim())) else { continue };
            map.insert(id, mode);
        }
    }
    map
}

/// SubCraft blend mode for a Painter blending mode id; `None` for ids not identified yet.
pub fn painter_blend(id: i32) -> Option<BlendMode> {
    match id {
        // Found by search against Painter's flattened result (DBAL-A2, see docs/spp-research.md):
        // each id tried with 18 modes, everything else fixed, fitted fills refitted every time.
        // 4 ties with Darken / Linear burn on near-binary masks. 10 was Multiply too until the
        // stencils, placed images and brush sizes were fixed; searched again then, Linear burn
        // wins (DBAL-A2 at 1024: Metallic 0.0542 -> 0.0528, CarInteriro unchanged). 15 ties with
        // Overlay, Hard light, Color and Divide (all far better than Normal); 2 is Hard light by
        // a small margin and is not applied to the Normal channel (see `apply_opacity`).
        4 => Some(BlendMode::Multiply),
        10 => Some(BlendMode::LinearBurn),
        2 => Some(BlendMode::HardLight),
        9 => Some(BlendMode::Color),
        11 | 15 => Some(BlendMode::SoftLight),
        25 => Some(BlendMode::Normal),
        29 => Some(BlendMode::Subtract),
        _ => None,
    }
}


/// SubCraft projection for a Painter fill projection id (0 UV, 2 tri-planar; the mesh-map driven
/// generators' 6 has no tiling) and UV transformation.
fn projection_of(id: i64, uv: Option<&subcraft_mesh::spp::document::UvTransform>) -> Projection {
    let mut p = Projection::default();
    if id == 2 {
        p.mode = ProjectionMode::Triplanar;
    }
    if let Some(t) = uv {
        p.scale = t.scale;
        if p.mode == ProjectionMode::Uv && !skip("uv_phase") {
            // Painter's transform is affine in UV: the same map as SubCraft's scale, rotation
            // (mirrored by the opposite V axes) and offset (where UV 0,0 lands).
            p.rotation_deg = -(t.rotation * spp_select::rotation_sign()).to_degrees();
            p.offset = spp_select::painter_image_uv(t, glam::Vec2::ZERO).into();
        } else {
            p.rotation_deg = t.rotation.to_degrees();
            p.offset = t.offset;
        }
    }
    p
}

/// How Painter's default blending ("Normal", id 25) composites a channel. Height adds up: on
/// Street_cyberpunk the painted lines reach the sum of the stacked heights (1.2 = 0.65 + 0.55),
/// and adding cuts its Height difference 0.088 -> 0.065 (no change on the other projects).
fn default_mode(kind: ChannelKind) -> BlendMode {
    match kind {
        ChannelKind::Normal => BlendMode::NormalMapCombine,
        ChannelKind::Height => BlendMode::LinearDodge,
        _ => BlendMode::Normal,
    }
}

/// For every texel outside the set's UV triangles, the nearest texel inside one.
pub struct GutterFill {
    res: u32,
    inside: Vec<bool>,
    nearest: Vec<u32>,
}

impl GutterFill {
    pub fn new(geometry: &TextureSetGeometry) -> Self {
        let res = geometry.resolution;
        let mut inside = vec![false; (res * res) as usize];
        for t in 0..geometry.tile_count() {
            let tile = geometry.tile(t);
            if tile.is_empty() {
                continue;
            }
            for i in 0..subcraft_image::TILE_TEXELS {
                let (x, y) = tile.texel_xy(i);
                if x < res && y < res && tile.triangle(i).is_some() {
                    inside[(y * res + x) as usize] = true;
                }
            }
        }
        let nearest = subcraft_image::dilate::nearest_covered(res as usize, res as usize, &inside);
        Self { res, inside, nearest }
    }

    /// `image` (at the geometry's resolution) with its outside texels copied from the nearest
    /// inside texel; sparse images stay sparse where nothing is painted nearby.
    pub fn fill(&self, image: &TiledImage) -> TiledImage {
        if image.allocated_tiles() == 0 || image.width() != self.res {
            return image.clone();
        }
        let comps = image.comps();
        let res = self.res as usize;
        let mut data = vec![0.0f32; res * res * comps];
        let mut buf = vec![0.0f32; subcraft_image::TILE_TEXELS * comps];
        for t in 0..image.tile_count() {
            image.read_tile(t, &mut buf);
            let r = image.tile_rect(t);
            for y in r.y0..r.y1 {
                let row = (y - r.y0) as usize * subcraft_image::TILE_SIZE as usize;
                let src = &buf[row * comps..(row + (r.x1 - r.x0) as usize) * comps];
                let o = (y as usize * res + r.x0 as usize) * comps;
                data[o..o + src.len()].copy_from_slice(src);
            }
        }
        subcraft_image::dilate::apply_nearest(comps, &mut data, &self.inside, &self.nearest);
        TiledImage::from_dense(image.width(), image.height(), comps, image.fill(), &data)
    }
}

/// Replace tiles whose texels are all equal by one shared tile per value (polygon-fill masks are
/// mostly constant inside the selected parts: hundreds of 4K masks would otherwise hold gigabytes).
pub fn share_constant_tiles(image: &mut TiledImage, shared: &std::sync::Mutex<HashMap<(usize, Vec<u16>), Arc<subcraft_image::Tile>>>) {
    let comps = image.comps();
    for i in 0..image.tile_count() {
        let Some(tile) = image.tile(i) else { continue };
        let first: Vec<u16> = tile.data[..comps].iter().map(|v| v.to_bits()).collect();
        if !tile.data.chunks_exact(comps).all(|px| px.iter().zip(&first).all(|(a, b)| a.to_bits() == *b)) {
            continue;
        }
        let key = (comps, first);
        let t = {
            let mut map = shared.lock().unwrap_or_else(|p| p.into_inner());
            map.entry(key).or_insert_with(|| tile.clone()).clone()
        };
        if !Arc::ptr_eq(&t, tile) {
            image.set_slot(i, Some(t));
        }
    }
}

/// Largest resolution mask strokes and polygon fills are painted at (`SUBCRAFT_SPP_RASTER_MAX`
/// overrides it).
pub fn raster_max_resolution() -> u32 {
    std::env::var("SUBCRAFT_SPP_RASTER_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(2048)
}

/// Grunge balance mapping (`shift`, `slope`, `gamma_k`), see `grunge_effects`;
/// `SUBCRAFT_SPP_GRUNGE="shift,slope,gamma_k"` overrides it for experiments. Measured on DBAL-A2:
/// moving the levels window with the balance (0, 1, 0: the first mapping) made masks that feed
/// thresholding Levels far too full (Metallic 0.067); balance as a midtone gamma with the window
/// fixed (0, 0, 2) gives Metallic 0.062 and the lowest total of the variants tried.
fn grunge_mapping() -> (f32, f32, f32) {
    std::env::var("SUBCRAFT_SPP_GRUNGE").ok().and_then(|v| {
        let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (n.len() == 3).then(|| (n[0], n[1], n[2]))
    }).unwrap_or((0.0, 0.0, 2.0))
}

/// Developer switch for experiments: `SUBCRAFT_SPP_SKIP=a,b` leaves those rebuild steps out.
fn skip(step: &str) -> bool {
    std::env::var("SUBCRAFT_SPP_SKIP").is_ok_and(|v| v.split(',').any(|s| s == step))
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

/// `mask` for a texture `factor` times the size (`res` texels): rasters resampled, filter radii
/// (in texels) scaled.
fn scaled_mask(mask: &Mask, factor: f32, res: u32) -> Mask {
    let mut m = mask.clone();
    m.base = m.base.resampled(res, res);
    for e in &mut m.effects {
        let e = Arc::make_mut(e);
        match &mut e.kind {
            MaskEffectKind::Paint { image } => *image = image.resampled(res, res),
            MaskEffectKind::Filter(f) => {
                *f = match *f {
                    Filter::Blur { radius } => Filter::Blur { radius: radius * factor },
                    Filter::Sharpen { radius, amount } => Filter::Sharpen { radius: (radius * factor).max(0.5), amount },
                    Filter::Median { radius } => Filter::Median { radius: radius * factor },
                    Filter::Warp { intensity, scale, seed } => Filter::Warp { intensity: intensity * factor, scale, seed },
                    Filter::DirectionalBlur { radius, angle_deg } => Filter::DirectionalBlur { radius: radius * factor, angle_deg },
                    Filter::Highpass { radius, amount } => Filter::Highpass { radius: radius * factor, amount },
                    Filter::Bevel { radius, amount, threshold } => Filter::Bevel { radius: radius * factor, amount, threshold },
                    Filter::MaskOutline { radius, threshold, position } => Filter::MaskOutline { radius: radius * factor, threshold, position },
                    other => other,
                }
            }
            _ => {}
        }
    }
    m
}

/// A mask evaluated on its own as a (value, alpha = 1) raster at the set's resolution.
pub fn eval_mask_image(mask: &Mask, view: &TextureSet, geometry: &TextureSetGeometry, resources: &ResourceStore, maps: &subcraft_materials::MeshMaps) -> TiledImage {
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

/// Painter's `fractal_sum_base` / `fractal_sum_base_colored` as SubCraft fractal noise: its
/// coarsest level `Min_Level` is 2^level cells per tile (DBAL-A2 uses 9: a grain of a few texels
/// at 2K), `Position` is the balance and `Contrast` the contrast; the colored variant is colored
/// noise (each component its own pattern).
const FRACTAL_RANGE: f32 = 0.53;

fn fractal_sum(g: &GraphRef) -> Option<ProceduralParams> {
    let name = g.name().to_ascii_lowercase();
    if !name.starts_with("fractal_sum") {
        return None;
    }
    let mut p = ProceduralParams::new(ProceduralKind::Clouds);
    p.scale = 2f32.powf(g.value("Min_Level").unwrap_or(3.0).clamp(0.0, 10.0));
    p.balance = g.value("Position").unwrap_or(0.5);
    p.contrast = g.value("Contrast").unwrap_or(0.0).clamp(0.0, 0.98);
    p.invert = g.value("invert").is_some_and(|v| v >= 0.5);
    p.colored = name.contains("colored");
    Some(p)
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
        // SubCraft's Brushed pattern drew stripes where Painter's anisotropic noises leave the
        // mask even (DBAL-A2 Metallic 0.076 -> 0.070 without it, no change elsewhere).
        return None;
    } else if n.contains("scratches") {
        ProceduralKind::Scratches
    } else {
        return None;
    })
}

/// Texels per unit of a Substance filter's resolution-relative distance parameters, at `res`.
/// Painter's filters scale with the texture, SubCraft's radii are in texels; both blur and warp are
/// calibrated against Painter's result (see docs/spp-research.md).
fn texels(res: u32, relative: f32) -> f32 {
    relative * res as f32
}

/// SubCraft filter for a Painter procedural filter graph at texture resolution `res`; the flag
/// says whether the mapping is exact.
pub fn map_filter(g: &GraphRef, res: u32) -> Option<(Filter, bool)> {
    let name = g.name().to_ascii_lowercase();
    match name.as_str() {
        "blur" => Some((Filter::Blur { radius: texels(res, g.value("intensity")? * blur_scale()).clamp(0.0, 128.0) }, false)),
        "sharpen" | "fx_sharpen" => Some((Filter::Sharpen { radius: texels(res, SHARPEN_RADIUS).max(0.5), amount: (g.value("intensity")? * SHARPEN_GAIN).clamp(0.0, 4.0) }, false)),
        "clamp" => Some((Filter::Clamp { low: g.value("min")?, high: g.value("max")? }, true)),
        "warp" => {
            let intensity = g.value("intensity").unwrap_or(0.005);
            let scale = (g.value("noise_scale").unwrap_or(1.0) * WARP_CELLS).round().clamp(1.0, 256.0) as u32;
            Some((Filter::Warp { intensity: texels(res, intensity * WARP_SCALE).clamp(0.0, 64.0), scale, seed: 0 }, false))
        }
        // Hue, saturation and luminosity are centred on 0.5 (no change).
        "hsl_perceptive" | "hsl" => Some((
            Filter::Hsl {
                hue: (g.value("hue").unwrap_or(0.5) - 0.5) * 360.0,
                saturation: (g.value("saturation").unwrap_or(0.5) * 2.0).max(0.0),
                lightness: (g.value("luminosity").or(g.value("lightness")).unwrap_or(0.5) - 0.5) * HSL_LIGHTNESS,
            },
            false,
        )),
        _ => None,
    }
}

/// Blur radius per unit of Painter's `blur.intensity`, as a fraction of the texture size
/// (`SUBCRAFT_SPP_BLUR_SCALE` overrides it for experiments).
fn blur_scale() -> f32 {
    std::env::var("SUBCRAFT_SPP_BLUR_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(BLUR_SCALE)
}
/// Substance's convention, pixels at 256: 1/16 (the first guess) blurred far too much and made a
/// 512-texel composite of DBAL-A2 take 442 s instead of seconds; 1/256 also matches Painter better
/// on DBAL-A2 and Street_cyberpunk (CarInteriro's Roughness 0.058 -> 0.062).
const BLUR_SCALE: f32 = 1.0 / 256.0;
/// Sharpen radius as a fraction of the texture size, and gain per unit of `sharpen.intensity`.
const SHARPEN_RADIUS: f32 = 1.0 / 1024.0;
const SHARPEN_GAIN: f32 = 8.0;
/// Warp displacement per unit of `warp.intensity` (fraction of the texture size) and noise cells per `noise_scale`.
const WARP_SCALE: f32 = 1.0;
const WARP_CELLS: f32 = 4.0;
/// HSL lightness offset per unit of `luminosity` away from 0.5.
const HSL_LIGHTNESS: f32 = 1.0;

/// Shorthand used by the importer for a channel kind from Painter's flag.
pub fn kind_of_flag(bits: u64) -> Option<ChannelKind> {
    channel_kind(bits)
}
