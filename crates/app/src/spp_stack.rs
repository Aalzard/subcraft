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

use subcraft_core::{ChannelKind, ChannelValue, ResourceId};
use subcraft_doc::{
    BlendMode, ChannelBlend, FillChannel, FillSource, Filter, Generator, GeneratorKind, Layer, LayerContent, Levels, Mask, MaskEffect, MaskEffectKind, ProceduralKind, ProceduralParams,
    Projection, ProjectionMode, Resource, ResourceKind, ResourceStore, TextureSet,
};
use subcraft_mesh::spp::document::{ActionKind, GraphRef, LayerInfo, LayerKind, LevelsInfo, SourceInfo};
use subcraft_mesh::spp::SppFile;

use crate::spp_import::channel_kind;

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
    /// Blend mode ids met (all treated as Normal).
    pub blend_ids: BTreeMap<i32, usize>,
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
}

impl<'a, R: Read + Seek> Rebuilder<'a, R> {
    pub fn new(spp: &'a mut SppFile<R>, ts: &'a TextureSet, resources: &'a mut ResourceStore) -> Self {
        Self { spp, ts, resources, stats: StackStats::default(), images: HashMap::new(), extract_embedded: true }
    }

    /// Layers bottom to top.
    pub fn rebuild(&mut self, infos: &[LayerInfo]) -> Vec<Layer> {
        infos.iter().map(|i| self.layer(i)).collect()
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

    /// Opacity per channel from the stored blending; unknown mode ids are counted and treated as Normal.
    fn apply_opacity(&mut self, layer: &mut Layer, info: &LayerInfo) {
        for (flags, mode, opacity) in &info.blends {
            if *mode != 25 {
                *self.stats.blend_ids.entry(*mode).or_default() += 1;
            }
            for c in &self.ts.channels {
                if channel_bit(c.kind).is_some_and(|b| flags & b != 0) {
                    if let Some(blend) = layer.blend.get_mut(&c.kind) {
                        if blend.mode != BlendMode::Disable {
                            blend.opacity = *opacity;
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
                ActionKind::Paint { strokes } => self.stats.skip(format!("paint strokes on layer content ({strokes} in one action)")),
                ActionKind::Eraser { .. } => self.stats.skip("eraser strokes on layer content"),
                ActionKind::Filter { graph } => self.stats.skip(format!("filter layer '{}'", graph.name())),
                ActionKind::Levels(_) => self.stats.skip("levels in layer content"),
                ActionKind::Decal { .. } => self.stats.skip("decals"),
                ActionKind::Anchor => self.stats.skip("anchor points"),
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
        for action in &info.mask {
            if !action.enabled {
                continue;
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
                ActionKind::Paint { .. } => self.stats.skip("painted mask strokes"),
                ActionKind::Eraser { .. } => self.stats.skip("erased mask strokes"),
                ActionKind::Decal { .. } => self.stats.skip("mask decals"),
                ActionKind::Anchor => self.stats.skip("anchor points"),
                ActionKind::Other(c) => self.stats.skip(format!("mask action {c}")),
            }
        }
        if mapped == 0 {
            // Nothing could be rebuilt: a white mask would show the layer everywhere, a black one never.
            self.stats.skip("masks with no reconstructible effect (mask left off)");
            return;
        }
        layer.mask = Some(mask);
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
