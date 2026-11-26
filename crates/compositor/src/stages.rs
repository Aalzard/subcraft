//! Immutable previous-stage graph for neighborhood filters. Tiles remain sparse and bounded.
use crate::{
    EvalContext, TileContext,
    blend::{BlendTarget, composite},
    tile::{effective_mode, eval_fill, read_paint},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, OnceLock},
};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{
    BlendMode, CompareMask, ContentEffect, ContentEffectKind, Filter, Generator, Layer, LayerContent, Mask, MaskEffect,
    MaskEffectKind, TextureSet,
};
use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};
use subcraft_materials::SampleContext;

type Pixels = Arc<Vec<[f32; 4]>>;
type Cell = Arc<OnceLock<Pixels>>;
type Key = (usize, usize);
struct Entries {
    cells: HashMap<Key, Cell>,
    fifo: VecDeque<Key>,
}
/// 64 MiB of completed RGBA tiles per set, plus tiles in active dependency frontiers.
/// Cells are never evicted while computing or borrowed, so parallel evaluation shares work.
pub(crate) struct StageCache {
    entries: Mutex<Entries>,
    capacity: usize,
}
impl Default for StageCache {
    fn default() -> Self {
        // Neighboring tiles of a blur or median reuse each other's inputs while they stay cached.
        // Each thread works on a block of tiles, so a few hundred 64 KiB tiles per thread (256 MiB
        // for 16 threads) keeps most of them; more only trades memory for a few percent.
        let threads = rayon::current_num_threads();
        Self::new(std::env::var("SUBCRAFT_STAGE_CACHE").ok().and_then(|v| v.parse().ok()).unwrap_or((threads * 256).clamp(1024, 8192)))
    }
}
impl StageCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(Entries {
                cells: HashMap::new(),
                fifo: VecDeque::new(),
            }),
            capacity: capacity.max(1),
        }
    }
    pub(crate) fn clear(&self) {
        let mut e = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        e.cells.clear();
        e.fifo.clear();
    }
    pub(crate) fn trim(&self) {
        let mut e = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        for _ in 0..e.fifo.len() {
            if e.cells.len() <= self.capacity {
                break;
            }
            let old = e.fifo.pop_front().unwrap();
            if e.cells.get(&old).is_some_and(|c| {
                Arc::strong_count(c) == 1 && c.get().is_some_and(|p| Arc::strong_count(p) == 1)
            }) {
                e.cells.remove(&old);
            } else {
                e.fifo.push_back(old);
            }
        }
    }
    fn ready(&self, key: Key) -> Option<Pixels> {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .cells
            .get(&key)?
            .get()
            .cloned()
    }
    fn cell(&self, key: Key) -> Cell {
        let mut e = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cell) = e.cells.get(&key) {
            return cell.clone();
        }
        let cell = Arc::new(OnceLock::new());
        e.cells.insert(key, cell.clone());
        e.fifo.push_back(key);
        for _ in 0..e.fifo.len() {
            if e.cells.len() <= self.capacity {
                break;
            }
            let old = e.fifo.pop_front().unwrap();
            let removable = e.cells.get(&old).is_some_and(|c| {
                Arc::strong_count(c) == 1 && c.get().is_some_and(|p| Arc::strong_count(p) == 1)
            });
            if removable {
                e.cells.remove(&old);
            } else {
                e.fifo.push_back(old);
            }
        }
        cell
    }
}

enum Node<'a> {
    Constant([f32; 4]),
    Paint(&'a TiledImage, ChannelKind),
    Fill(&'a subcraft_doc::FillContent, ChannelKind),
    MaskBase(&'a TiledImage),
    MaskEffect {
        input: usize,
        effect: &'a MaskEffect,
        filtered: Option<usize>,
        /// Anchor input (ADR-0045): the anchor's node and its channel (`None`: a mask anchor).
        anchor: Option<(usize, Option<ChannelKind>)>,
        /// Compare Mask: the composite of the layers below and this layer's own content, in the
        /// compared channel (`None` when the effect does not read it).
        compare: [Option<usize>; 2],
        /// Micro details: the generator's [`Node::MicroDetail`] (`None` when off).
        details: Option<usize>,
    },
    /// A fill channel reading an anchor (ADR-0045): `source`'s `from` channel (`None`: a mask
    /// anchor) as `to`, scaled by the channel's strength.
    AnchorRead {
        source: usize,
        from: Option<ChannelKind>,
        to: ChannelKind,
        strength: f32,
    },
    ContentEffect {
        input: usize,
        effect: &'a Arc<ContentEffect>,
        kind: ChannelKind,
        filtered: Option<usize>,
    },
    Layer {
        below: usize,
        input: usize,
        mask: Option<usize>,
        layer: &'a Layer,
        kind: ChannelKind,
        passthrough: bool,
    },
    Directional {
        input: usize,
        taps: Arc<Vec<[f32; 3]>>,
        support: u32,
    },
    Median {
        input: usize,
        radius: usize,
    },
    Warp {
        input: usize,
        intensity: f32,
        scale: u32,
        seed: u32,
        support: u32,
        /// Reads per texel along the offset: 1 is the Warp filter (the far end only), more is
        /// Blur Slope (the average from the texel to the far end).
        samples: u32,
    },
    /// Edge Detect / Directional Warp / Mosaic: direct per-texel pixel reads (filter already
    /// scaled to this texture's resolution).
    Pixelwise {
        input: usize,
        filter: Filter,
        kind: ChannelKind,
        support: u32,
        /// A smoothed copy of `input` (Blur Slope on Previous input measures its slope there).
        slope: Option<usize>,
        /// The anchors in the filter's image slots, already converted to `kind` (ADR-0045).
        images: [Option<usize>; 2],
    },
    Contour {
        input: usize,
        filter: Filter,
        kind: ChannelKind,
        radius: f32,
    },
    Gaussian {
        input: usize,
        weights: Arc<Vec<f32>>,
        horizontal: bool,
        /// Taps past a texture border continue from the opposite border (Blur with Wrap).
        wrap: bool,
    },
    /// Mask Editor with a global blur: its blended contributions before the blur (the blurred
    /// result is finished by the generator's [`Node::MaskEffect`]).
    EditorRaw {
        generator: &'a Generator,
    },
    /// Micro effect: the signed height change `generator`'s mask (the running mask right after the
    /// generator effect) writes. Read by [`Node::MicroHeight`] and, through its neighbours, by
    /// [`Node::MicroNormal`]; the mask is evaluated once and shared, never recomputed.
    MicroDelta {
        mask: usize,
        generator: &'a Generator,
        opacity: f32,
    },
    /// Micro details (Painter's Use Micro Details): `[curvature, occlusion, 0, 1]` per texel from
    /// the Normal and Height below a layer. `chain` holds the `[normal, height]` accumulations
    /// below each level from the top-level ancestor down to the layer itself (inside folders the
    /// accumulation starts transparent, so the outer levels show through it).
    MicroDetail {
        chain: Arc<Vec<[usize; 2]>>,
        settings: subcraft_doc::MicroDetails,
        /// The last level is a layer's own content and adds its relief to the levels before it
        /// (Height sums, Normal leans further) instead of covering them: a material filter
        /// reading the detail at its own place in the stack.
        own_adds: bool,
        /// Replace the occlusion by a horizon scan of the height ([`horizon_occlusion_tile`]):
        /// smooth over relief of any size, for the HBAO filter. The default occlusion is a
        /// cheap 16-tap average meant for fine detail under a generator.
        horizon: bool,
    },
    /// Height channel: adds the micro delta.
    MicroHeight {
        below: usize,
        delta: usize,
    },
    /// Normal channel: whiteout-blends the normal of the micro delta's slope.
    MicroNormal {
        below: usize,
        delta: usize,
    },
}
impl Node<'_> {
    /// Profile bucket name (`SUBCRAFT_COMPOSITE_PROFILE=1`).
    fn label(&self) -> &'static str {
        match self {
            Self::Constant(_) => "constant",
            Self::Paint(..) => "paint",
            Self::Fill(..) => "fill",
            Self::MaskBase(_) => "mask base",
            Self::MaskEffect { effect, .. } => match &effect.kind {
                MaskEffectKind::Generator(_) => "mask generator",
                MaskEffectKind::Filter(_) => "mask filter",
                _ => "mask effect",
            },
            Self::ContentEffect { .. } => "content effect",
            Self::AnchorRead { .. } => "anchor read",
            Self::Layer { .. } => "layer blend",
            Self::Directional { .. } => "directional blur",
            Self::Median { .. } => "median",
            Self::Warp { .. } => "warp",
            Self::Pixelwise { .. } => "pixelwise filter",
            Self::Contour { .. } => "contour",
            Self::Gaussian { .. } => "gaussian pass",
            Self::EditorRaw { .. } => "mask editor",
            Self::MicroDelta { .. } => "micro delta",
            Self::MicroDetail { .. } => "micro details",
            Self::MicroHeight { .. } => "micro height",
            Self::MicroNormal { .. } => "micro normal",
        }
    }
    fn primary(&self) -> Option<usize> {
        match self {
            Self::MaskEffect { input, .. }
            | Self::ContentEffect { input, .. }
            | Self::Gaussian { input, .. }
            | Self::Contour { input, .. }
            | Self::Median { input, .. }
            | Self::Warp { input, .. }
            | Self::Pixelwise { input, .. }
            | Self::Directional { input, .. } => Some(*input),
            Self::Layer { below, .. } | Self::MicroHeight { below, .. } | Self::MicroNormal { below, .. } => Some(*below),
            Self::MicroDelta { mask, .. } => Some(*mask),
            Self::AnchorRead { source, .. } => Some(*source),
            _ => None,
        }
    }
}

pub(crate) struct Graph<'a> {
    nodes: Vec<Node<'a>>,
    uniform: Vec<bool>,
    masks: HashMap<LayerId, usize>,
    /// Per layer: the mask node right after each micro-effect generator.
    micro_masks: HashMap<LayerId, Vec<(usize, &'a Generator)>>,
    /// (layer, micro generator index) -> its shared delta node.
    micro_deltas: HashMap<(LayerId, usize), usize>,
    roots: Vec<(ChannelKind, usize)>,
    radius_scale: f32,
    ctx: EvalContext<'a>,
    albedo: subcraft_image::TextureFilter,
    cache: &'a StageCache,
    id: u64,
    modes: OnceLock<Vec<Mode>>,
    /// Per channel and top-level layer index: the layer index that produced the accumulation
    /// after it and that accumulation's node (`None` while still the channel's base value).
    tops: Vec<(ChannelKind, Vec<Option<(usize, usize)>>)>,
    prefix: Option<Prefix<'a>>,
    /// Evaluate only these channels' roots (`None`: all); see `PrefixCache::set_only`.
    only: Option<Vec<ChannelKind>>,
    content: Option<Content<'a>>,
    /// Anchor points and references (ADR-0045); `None` when no layer reads an anchor, so sets
    /// without references build exactly the graph they always did.
    anchors: Option<Anchors<'a>>,
    /// Compare Mask operands: per (layer, channel), the accumulation below the layer and the
    /// layer's own content, recorded while the channel's stack is built.
    compare_below: HashMap<(LayerId, ChannelKind), usize>,
    /// (layer, content effect index) -> the micro details node of that material filter.
    effect_details: HashMap<(LayerId, usize), usize>,
    compare_content: HashMap<(LayerId, ChannelKind), usize>,
    /// The top-level layers, to rebuild a stack below a layer on demand.
    top: &'a [Arc<Layer>],
    /// Quantize palettes per node: picked once from the whole input texture, then shared by
    /// every tile.
    palettes: Mutex<HashMap<usize, Arc<OnceLock<Arc<subcraft_materials::filter::quantize::Palette>>>>>,
    /// Fill Area results per node: the enclosed areas of the whole input, found once.
    fills: Mutex<HashMap<usize, Arc<OnceLock<Arc<FilledAreas>>>>>,
    /// Color Match averages per node: the mean colour of the whole input, found once.
    averages: Mutex<HashMap<usize, Arc<OnceLock<Option<[f32; 3]>>>>>,
}

/// Anchor bookkeeping of a graph whose texture set has references.
struct Anchors<'a> {
    index: subcraft_doc::AnchorIndex,
    layers: HashMap<LayerId, &'a Layer>,
    /// Per (layer, channel): the isolated content chain, `[base, after effect 0, ...]`.
    content: HashMap<(LayerId, ChannelKind), Vec<usize>>,
    /// Per layer: the mask chain, `[base, after effect 0, ...]`.
    masks: HashMap<LayerId, Vec<usize>>,
    /// Per (layer, channel): the content node the layer blends (for the GPU preparation).
    sources: HashMap<(LayerId, ChannelKind), usize>,
}

/// Cached accumulations at chosen top-level boundaries (see [`crate::prefix`]).
struct Prefix<'a> {
    cache: &'a crate::prefix::PrefixCache,
    /// Node -> (channel, boundary layer index) for the planned boundaries.
    marks: HashMap<usize, (ChannelKind, usize)>,
}

/// Results of top-level layers that depend on nothing but the layer (see
/// [`crate::prefix::ContentCache`]).
struct Content<'a> {
    cache: &'a crate::prefix::ContentCache,
    /// Node -> (layer address, slot).
    marks: HashMap<usize, (usize, crate::prefix::ContentSlot)>,
}

/// Where a node's per-tile result lives.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Read by neighboring tiles (blur, median, ...) or tile independent: the shared [`StageCache`].
    Shared,
    /// Used more than once, always for its own tile: kept until the tile that needed it is done.
    Tile,
    /// Used exactly once: handed straight to its consumer, never stored.
    Once,
}

/// Results of the current tile's [`Mode::Tile`] nodes and the in-flight chain, per thread (no
/// locks). Tagged with the graph it belongs to; cleared when a root tile is finished.
struct Scratch {
    graph: u64,
    pixels: HashMap<Key, Pixels>,
}
thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::new(Scratch { graph: 0, pixels: HashMap::new() });
}
static NEXT_GRAPH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl<'a> Graph<'a> {
    fn empty(
        ctx: EvalContext<'a>,
        albedo: subcraft_image::TextureFilter,
        cache: &'a StageCache,
    ) -> Self {
        Self {
            radius_scale: 1.0,
            nodes: Vec::new(),
            uniform: Vec::new(),
            masks: HashMap::new(),
            micro_masks: HashMap::new(),
            micro_deltas: HashMap::new(),
            roots: Vec::new(),
            ctx,
            albedo,
            cache,
            id: NEXT_GRAPH.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            modes: OnceLock::new(),
            tops: Vec::new(),
            prefix: None,
            only: None,
            content: None,
            anchors: None,
            compare_below: HashMap::new(),
            effect_details: HashMap::new(),
            compare_content: HashMap::new(),
            top: &[],
            palettes: Mutex::new(HashMap::new()),
            averages: Mutex::new(HashMap::new()),
            fills: Mutex::new(HashMap::new()),
        }
    }
    /// Index the anchors of `layers` when any layer reads one.
    fn with_anchors(&mut self, layers: &'a [Arc<Layer>]) {
        if !subcraft_doc::anchor::has_references(layers) {
            return;
        }
        fn collect<'a>(layers: &'a [Arc<Layer>], out: &mut HashMap<LayerId, &'a Layer>) {
            for l in layers {
                out.insert(l.id, l);
                collect(l.children(), out);
            }
        }
        let mut map = HashMap::new();
        collect(layers, &mut map);
        self.anchors = Some(Anchors {
            index: subcraft_doc::AnchorIndex::build(layers),
            layers: map,
            content: HashMap::new(),
            masks: HashMap::new(),
            sources: HashMap::new(),
        });
    }
    /// The node a valid reference at `site` reads, with the anchor's channel (`None`: mask).
    fn anchor_node(&mut self, site: subcraft_doc::RefSite) -> Option<(usize, Option<ChannelKind>)> {
        let anchors = self.anchors.as_ref()?;
        let input = anchors.index.reference(site).filter(|r| r.status.is_ok())?.input;
        let info = anchors.index.anchor(input.anchor)?;
        let (domain, at) = (info.domain, info.index);
        let layer: &'a Layer = *anchors.layers.get(&info.layer)?;
        match domain {
            subcraft_doc::AnchorDomain::Content => {
                let steps = self.isolated_content(layer, input.channel);
                Some((steps[at], Some(input.channel)))
            }
            subcraft_doc::AnchorDomain::Mask => {
                let mask = layer.mask.as_ref()?;
                self.layer_mask(layer, mask);
                let steps = self.anchors.as_ref()?.masks.get(&layer.id)?;
                Some((steps[at], None))
            }
        }
    }
    /// For a reference at `site` in `owner`'s mask: `Some(node)` when it reads an anchor of that
    /// same mask (`Some(None)` if that anchor's step is not built yet), `None` when the anchor is
    /// elsewhere.
    fn own_mask_anchor(&self, site: subcraft_doc::RefSite, owner: LayerId, steps: &[usize]) -> Option<Option<(usize, Option<ChannelKind>)>> {
        let anchors = self.anchors.as_ref()?;
        let input = anchors.index.reference(site).filter(|r| r.status.is_ok())?.input;
        let info = anchors.index.anchor(input.anchor)?;
        (info.layer == owner && info.domain == subcraft_doc::AnchorDomain::Mask).then(|| steps.get(info.index).map(|&node| (node, None)))
    }
    /// A layer's own content chain for `kind` on a transparent base, built once and shared.
    fn isolated_content(&mut self, layer: &'a Layer, kind: ChannelKind) -> Vec<usize> {
        if let Some(steps) = self.anchors.as_ref().and_then(|a| a.content.get(&(layer.id, kind))) {
            return steps.clone();
        }
        let base = match &layer.content {
            LayerContent::Group(_) => self.push(Node::Constant([0.0; 4])),
            _ => usize::MAX,
        };
        let base = self.content_base(layer, kind, base);
        let mut steps = vec![base];
        self.content_effects(layer, kind, base, Some(&mut steps));
        if let Some(a) = self.anchors.as_mut() {
            a.content.insert((layer.id, kind), steps.clone());
        }
        steps
    }
    /// The layer's content before its content effects: its paint raster, fill channel, or
    /// folder children composited on `folder_base`.
    fn content_base(&mut self, layer: &'a Layer, kind: ChannelKind, folder_base: usize) -> usize {
        match &layer.content {
            LayerContent::Paint(p) => {
                if let Some(image) = p.channels.get(&kind) {
                    self.push(Node::Paint(image, kind))
                } else {
                    self.push(Node::Constant([0.0; 4]))
                }
            }
            LayerContent::Fill(f) => {
                if self.anchors.is_some()
                    && let Some(ch) = f.channels.get(&kind).filter(|c| c.enabled)
                    && let subcraft_doc::FillSource::Anchor { .. } = ch.source
                {
                    let strength = ch.strength;
                    return match self.anchor_node(subcraft_doc::RefSite::Fill { layer: layer.id, channel: kind }) {
                        Some((source, from)) => self.push(Node::AnchorRead { source, from, to: kind, strength }),
                        None => self.push(Node::Constant([0.0; 4])),
                    };
                }
                self.push(Node::Fill(f, kind))
            }
            LayerContent::Group(group) => self.stack(&group.children, folder_base, kind),
        }
    }
    /// The layer's content effects for `kind` on `input`. `steps` (anchors only) receives the
    /// node after every effect, including those that change nothing for `kind`.
    fn content_effects(&mut self, layer: &'a Layer, kind: ChannelKind, input: usize, steps: Option<&mut Vec<usize>>) -> usize {
        self.content_effects_until(layer, kind, input, steps, usize::MAX)
    }
    /// The Metallic content of `layer` right below its effect number `index`: what a material
    /// filter there that reads Metallic sees (PBR Validate).
    fn below_effect(&mut self, layer: &'a Layer, index: usize, kind: ChannelKind) -> usize {
        let blend = layer.blend_for(kind);
        let passthrough = blend.mode == BlendMode::Passthrough;
        let base = if passthrough && !layer.is_folder() {
            self.layer_below(layer, kind)
        } else {
            let folder_base = match &layer.content {
                LayerContent::Group(_) if passthrough => self.layer_below(layer, kind),
                LayerContent::Group(_) => self.push(Node::Constant([0.0; 4])),
                _ => usize::MAX,
            };
            self.content_base(layer, kind, folder_base)
        };
        self.content_effects_until(layer, kind, base, None, index)
    }
    /// [`Graph::content_effects`] for the first `limit` effects only.
    fn content_effects_until(&mut self, layer: &'a Layer, kind: ChannelKind, mut input: usize, mut steps: Option<&mut Vec<usize>>, limit: usize) -> usize {
        // The node below every effect so far: what an anchor at that place holds.
        let mut below = Vec::new();
        for (index, effect) in layer.content_effects.iter().enumerate().take(limit) {
            below.push(input);
            let usable = effect.visible
                && effect.blend_for(kind).is_active()
                && !matches!(effect.kind, ContentEffectKind::Anchor)
                && effect.applies_to(kind);
            let filtered = match effect.kind {
                ContentEffectKind::Filter { filter, .. } if usable => {
                    if !filter.valid() || !filter.supports_channel(kind) {
                        None
                    } else {
                        let images = self.filter_images(layer, effect.id, filter, kind, &below);
                        Some(self.neighborhood(input, filter, kind, images))
                    }
                }
                // PBR Validate: the Base Color verdict needs the Metallic at the same point.
                ContentEffectKind::Material(m) if usable && m.valid() && m.reads_metallic() && kind == ChannelKind::BaseColor => {
                    Some(Some(self.below_effect(layer, index, ChannelKind::Metallic)))
                }
                // Detail Edge Wear: the micro curvature of the Normal and Height at the same point.
                ContentEffectKind::Material(m) if usable && m.writes(kind) && let Some(settings) = m.micro_details() => Some(self.effect_detail_node(layer, index, settings)),
                // Shut Line: the blurred coverage of the layer's strokes.
                ContentEffectKind::Material(m) if usable && m.writes(kind) && let Some(radius) = m.coverage_blur() => Some(Some(self.effect_coverage_node(layer, index, radius))),
                _ => Some(None),
            };
            if usable && let Some(filtered) = filtered {
                input = self.push(Node::ContentEffect {
                    input,
                    effect,
                    kind,
                    filtered,
                });
            }
            if let Some(s) = steps.as_deref_mut() {
                s.push(input);
            }
        }
        input
    }
    /// The anchors in the image slots of a content filter of `layer`, each converted to `kind`.
    /// `below[i]` is the node under the layer's effect `i` in the chain being built.
    fn filter_images(&mut self, layer: &'a Layer, effect: subcraft_core::EffectId, filter: Filter, kind: ChannelKind, below: &[usize]) -> [Option<usize>; 2] {
        let mut images = [None; 2];
        if self.anchors.is_none() {
            return images;
        }
        for (slot, _) in filter.anchor_inputs() {
            let site = subcraft_doc::RefSite::Content { layer: layer.id, effect, slot };
            let own = self.anchors.as_ref().and_then(|a| {
                let input = a.index.reference(site).filter(|r| r.status.is_ok())?.input;
                let info = a.index.anchor(input.anchor)?;
                (info.layer == layer.id && info.domain == subcraft_doc::AnchorDomain::Content).then_some((info.index, input.channel))
            });
            let read = match own {
                // An anchor lower in this same chain: the node is already built.
                Some((at, channel)) if channel == kind => below.get(at).map(|&node| (node, Some(channel))),
                // The same anchor in another channel: only the effects below the anchor, so that
                // two channels reading each other's anchors cannot build each other endlessly.
                Some((at, channel)) => {
                    let base = match &layer.content {
                        LayerContent::Group(_) => self.push(Node::Constant([0.0; 4])),
                        _ => usize::MAX,
                    };
                    let base = self.content_base(layer, channel, base);
                    Some((self.content_effects_until(layer, channel, base, None, at), Some(channel)))
                }
                None => self.anchor_node(site),
            };
            if let (Some((source, from)), Some(image)) = (read, images.get_mut(slot)) {
                *image = Some(self.push(Node::AnchorRead { source, from, to: kind, strength: 1.0 }));
            }
        }
        images
    }
    /// The content node `layer` blends for `kind` (after its content effects), when the set has
    /// anchor references.
    pub(crate) fn layer_content_node(&self, layer: LayerId, kind: ChannelKind) -> Option<usize> {
        self.anchors.as_ref()?.sources.get(&(layer, kind)).copied()
    }
    /// The final mask node of `layer` (built when the layer takes part in a channel).
    pub(crate) fn layer_mask_node(&self, layer: LayerId) -> Option<usize> {
        self.masks.get(&layer).copied()
    }
    pub(crate) fn new(ts: &'a TextureSet, ctx: EvalContext<'a>, cache: &'a StageCache) -> Self {
        Self::new_scaled(ts, ctx, cache, 1.0)
    }
    /// A graph evaluated at `scale` times the authored resolution (quick previews): spatial
    /// radii shrink with it.
    pub(crate) fn new_scaled(ts: &'a TextureSet, ctx: EvalContext<'a>, cache: &'a StageCache, scale: f32) -> Self {
        let mut g = Self::empty(ctx, ts.albedo_filter, cache);
        g.radius_scale = scale;
        g.with_anchors(&ts.layers);
        g.top = &ts.layers;
        // Channels a Compare Mask reads are built first, so the stacks below each layer exist
        // when its mask is built in a later channel.
        let compared = compared_channels(&ts.layers);
        let mut kinds: Vec<ChannelKind> = ts.channel_kinds().into_iter().collect();
        kinds.sort_by_key(|k| !compared.contains(k));
        for kind in kinds {
            let d = kind.default_value().0;
            let root = g.push(Node::Constant([d[0], d[1], d[2], ts.base_alpha()]));
            let mut below = root;
            let mut tops = Vec::with_capacity(ts.layers.len());
            let mut last = None;
            for (j, layer) in ts.layers.iter().enumerate() {
                let next = g.layer(layer, below, kind);
                if next != below {
                    last = Some((j, next));
                }
                below = next;
                tops.push(last);
            }
            g.tops.push((kind, tops));
            g.roots.push((kind, below));
        }
        g
    }
    /// Read and write the accumulations after the top-level layers `plan` (indices) through `cache`.
    pub(crate) fn with_prefix(mut self, cache: &'a crate::prefix::PrefixCache, plan: &[usize]) -> Self {
        let mut marks = HashMap::new();
        for (kind, tops) in &self.tops {
            // The topmost result is the composite itself: never worth a copy here.
            let top = self.roots.iter().find(|(k, _)| k == kind).map(|r| r.1);
            for &j in plan {
                if let Some(Some((at, node))) = tops.get(j) {
                    if Some(*node) != top && !self.uniform[*node] {
                        marks.insert(*node, (*kind, *at));
                    }
                }
            }
        }
        if !marks.is_empty() {
            self.prefix = Some(Prefix { cache, marks });
        }
        self.mark_content(&cache.content);
        self.only = cache.only();
        self
    }
    /// Every input of a node (not only the primary one).
    fn inputs(node: &Node) -> Vec<usize> {
        match node {
            Node::Constant(_) | Node::Paint(_, _) | Node::Fill(_, _) | Node::MaskBase(_) | Node::EditorRaw { generator: _ } => Vec::new(),
            Node::MaskEffect { input, effect: _, filtered, anchor, compare, details } => {
                let mut v = vec![*input];
                v.extend(*filtered);
                v.extend(anchor.map(|a| a.0));
                v.extend(compare.iter().flatten().copied());
                v.extend(*details);
                v
            }
            Node::MicroDetail { chain, settings: _, own_adds: _, horizon: _ } => chain.iter().flatten().copied().collect(),
            Node::AnchorRead { source, from: _, to: _, strength: _ } => vec![*source],
            Node::ContentEffect { input, effect: _, kind: _, filtered } => {
                let mut v = vec![*input];
                v.extend(*filtered);
                v
            }
            Node::Layer { below, input, mask, layer: _, kind: _, passthrough: _ } => {
                let mut v = vec![*below, *input];
                v.extend(*mask);
                v
            }
            Node::Pixelwise { input, filter: _, kind: _, support: _, slope, images } => {
                let mut v = vec![*input];
                v.extend(*slope);
                v.extend(images.iter().flatten());
                v
            }
            Node::Directional { input, taps: _, support: _ }
            | Node::Median { input, radius: _ }
            | Node::Warp { input, intensity: _, scale: _, seed: _, support: _, samples: _ }
            | Node::Contour { input, filter: _, kind: _, radius: _ }
            | Node::Gaussian { input, weights: _, horizontal: _, wrap: _ } => vec![*input],
            Node::MicroDelta { mask, generator: _, opacity: _ } => vec![*mask],
            Node::MicroHeight { below, delta } | Node::MicroNormal { below, delta } => vec![*below, *delta],
        }
    }
    /// Mark the content and mask nodes of the top-level layers whose results depend on the layer
    /// alone, so they are read from / written to `cache`.
    ///
    /// A node qualifies when nothing it reads, directly or not, is an accumulation of the
    /// top-level stack, an anchor, or a compare-mask operand. Sets with anchor references are
    /// left alone altogether, and so are passthrough layers (their content is the stack below).
    /// Cheap leaves (a constant, a paint image, a plain fill) are not worth a copy.
    fn mark_content(&mut self, cache: &'a crate::prefix::ContentCache) {
        if !cache.enabled() || self.anchors.is_some() || self.radius_scale != 1.0 {
            return;
        }
        let n = self.nodes.len();
        // Accumulations of the top-level stack: results that depend on other layers.
        let mut pure = vec![true; n];
        for (_, tops) in &self.tops {
            for &(_, node) in tops.iter().flatten() {
                pure[node] = false;
            }
        }
        // Inputs are pushed before the nodes that read them.
        for i in 0..n {
            let own = match &self.nodes[i] {
                Node::AnchorRead { .. } => false,
                Node::MaskEffect { anchor, compare, details, .. } => anchor.is_none() && compare.iter().all(Option::is_none) && details.is_none(),
                Node::MicroDetail { .. } => false,
                _ => true,
            };
            let inputs = Self::inputs(&self.nodes[i]);
            if !own || inputs.iter().any(|&j| j >= i || !pure[j]) {
                pure[i] = false;
            }
        }
        let top: std::collections::HashSet<usize> = self.top.iter().map(|l| Arc::as_ptr(l) as usize).collect();
        let worth = |g: &Self, node: usize| {
            pure[node] && !g.uniform[node] && !matches!(g.nodes[node], Node::Constant(_) | Node::Paint(_, _) | Node::Fill(_, _) | Node::MaskBase(_))
        };
        let mut marks = HashMap::new();
        for node in &self.nodes {
            let Node::Layer { input, mask, layer, kind, passthrough: false, .. } = node else { continue };
            let address = *layer as *const Layer as usize;
            if !top.contains(&address) {
                continue;
            }
            if worth(self, *input) {
                marks.insert(*input, (address, *kind as crate::prefix::ContentSlot));
            }
            if let Some(m) = mask.filter(|&m| worth(self, m)) {
                marks.insert(m, (address, crate::prefix::CONTENT_MASK));
            }
        }
        if !marks.is_empty() {
            self.content = Some(Content { cache, marks });
        }
    }
    fn push(&mut self, node: Node<'a>) -> usize {
        // Tile-independent stages share one result. Most default material channels are uniform;
        // caching thousands of identical RGBA tiles would evict useful neighborhood inputs.
        let uniform = match &node {
            Node::Constant(_) => true,
            Node::Fill(fill, kind) => uniform_fill(fill, *kind),
            Node::Paint(image, _) | Node::MaskBase(image) => image.allocated_tiles() == 0,
            Node::Gaussian { input, .. }
            | Node::Directional { input, .. }
            | Node::Median { input, .. }
            | Node::Warp { input, .. }
            | Node::Contour { input, .. } => self.uniform[*input],
            // Edge Detect, Watercolor, Oil Paint and a gap-leaving Transform turn a flat input into an image.
            Node::Pixelwise { input, filter, images, .. } => {
                self.uniform[*input]
                    // A Color Match recolours a flat input per texel from its images.
                    && !filter.match_reads_images()
                    && images.iter().flatten().all(|n| self.uniform[*n])
                    && !matches!(filter, Filter::EdgeDetect { .. })
                    // Paper grain and stroke tones: a flat input comes out textured.
                    && !matches!(filter, Filter::Watercolor { .. } | Filter::OilPaint { .. })
                    && !matches!(filter, Filter::Transform { repeat, .. } if *repeat != subcraft_doc::RepeatMode::Both)
            }
            Node::ContentEffect {
                input,
                effect,
                kind,
                filtered,
            } => {
                // A uniform result is computed for one tile and reused for all: the filtered
                // input must be uniform too (a gap-leaving Transform of a flat layer is not).
                self.uniform[*input]
                    && filtered.is_none_or(|f| self.uniform[f])
                    && match &effect.kind {
                        ContentEffectKind::Fill(f) => uniform_fill(f, *kind),
                        ContentEffectKind::Generator(_) => false,
                        // Only PBR Validate is a function of the texel's values alone.
                        ContentEffectKind::Material(m) => m.reads_metallic(),
                        ContentEffectKind::Paint { channels } => {
                            channels.get(kind).is_none_or(|i| i.allocated_tiles() == 0)
                        }
                        _ => true,
                    }
            }
            Node::MaskEffect { input, effect, anchor, filtered, compare, details } => {
                self.uniform[*input]
                    && filtered.is_none_or(|f| self.uniform[f])
                    && details.is_none_or(|d| self.uniform[d])
                    && anchor.is_none_or(|(a, _)| self.uniform[a])
                    && compare.iter().flatten().all(|&c| self.uniform[c])
                    && match &effect.kind {
                        MaskEffectKind::Paint { image } => image.allocated_tiles() == 0,
                        MaskEffectKind::Fill { procedural, .. } => procedural.is_none(),
                        MaskEffectKind::Generator(_) | MaskEffectKind::Image { .. } | MaskEffectKind::IdColor(_) => false,
                        MaskEffectKind::Compare(c) => !c.reads_layers() || compare.iter().any(Option::is_some),
                        _ => true,
                    }
            }
            Node::EditorRaw { .. } | Node::MicroDelta { .. } | Node::MicroHeight { .. } | Node::MicroNormal { .. } => false,
            // Flat Normal and Height below: the same flat details everywhere.
            Node::MicroDetail { chain, .. } => chain.iter().flatten().all(|&n| self.uniform[n]),
            Node::AnchorRead { source, .. } => self.uniform[*source],
            Node::Layer {
                below,
                input,
                mask,
                layer,
                ..
            } => {
                self.uniform[*below]
                    && self.uniform[*input]
                    && mask.is_none_or(|m| self.uniform[m])
                    && layer.parts.as_ref().is_none_or(|p| !p.enabled)
            }
        };
        let id = self.nodes.len();
        self.uniform.push(uniform);
        self.nodes.push(node);
        id
    }
    fn neighborhood(&mut self, input: usize, filter: Filter, kind: ChannelKind, images: [Option<usize>; 2]) -> Option<usize> {
        let radius = filter.radius()?;
        if !filter.valid() {
            return None;
        }
        if filter.is_contour() {
            // QA 2026-11: the Mask Outline shape kept its full-resolution band in a scaled
            // composite (cut off where the read area ended, or half as thick at scale 2).
            return Some(self.push(Node::Contour {
                input,
                filter: filter.scaled(self.radius_scale),
                kind,
                radius: radius * self.radius_scale,
            }));
        }
        // Drop Shadow / Glow: the coverage moved by the offset, then blurred; `spatial_value`
        // puts it under the content.
        if let Some(([dx, dy], size)) = filter.shadow_geometry() {
            let (dx, dy, size) = (dx * self.radius_scale, dy * self.radius_scale, size * self.radius_scale);
            let mut node = input;
            if dx != 0.0 || dy != 0.0 {
                let res = self.ctx.geometry.resolution.max(1) as f32;
                let shift = Filter::Transform {
                    repeat: subcraft_doc::RepeatMode::None,
                    flip: subcraft_doc::FlipMode::None,
                    scale: [1.0; 2],
                    offset: [dx / res, dy / res],
                    rotation_deg: 0.0,
                    tile_safe: false,
                };
                let support = dx.abs().max(dy.abs()).ceil() as u32 + 1;
                node = self.push(Node::Pixelwise { input: node, filter: shift, kind, support, slope: None, images: [None; 2] });
            }
            if size > 0.0 {
                let weights = Arc::new(subcraft_materials::filter::gaussian_weights(size));
                let h = self.push(Node::Gaussian { input: node, weights: weights.clone(), horizontal: true, wrap: false });
                node = self.push(Node::Gaussian { input: h, weights, horizontal: false, wrap: false });
            }
            return Some(node);
        }
        // A uniform image is unchanged by any of these, except by Edge Detect (0 on a flat input)
        // and by a Transform that leaves part of the texture empty: those must still be evaluated.
        let leaves_gaps = matches!(filter, Filter::Transform { repeat, .. } if repeat != subcraft_doc::RepeatMode::Both);
        // Color Match in Average mode recolours a flat input too, and so do the stylization
        // filters (saturation, paper, stroke colour variation).
        if (self.uniform[input] && !leaves_gaps && !matches!(filter, Filter::EdgeDetect { .. } | Filter::ColorMatch { .. } | Filter::ComicBook { .. } | Filter::Watercolor { .. } | Filter::OilPaint { .. }))
            || radius == 0.0
            || matches!(
                filter,
                Filter::Sharpen { amount: 0.0, .. } | Filter::Highpass { amount: 0.0, .. }
            )
        {
            return Some(input);
        }
        if filter.is_pixelwise() {
            let filter = filter.scaled(self.radius_scale);
            let mut input = input;
            // Quantize Contour Smoothing: a Gaussian blur of the input, then the palette.
            if let Filter::Quantize { contour_smoothing, .. } = filter {
                if contour_smoothing > 0.0 {
                    let weights = Arc::new(subcraft_materials::filter::gaussian_weights(contour_smoothing));
                    let h = self.push(Node::Gaussian { input, weights: weights.clone(), horizontal: true, wrap: false });
                    input = self.push(Node::Gaussian { input: h, weights, horizontal: false, wrap: false });
                }
            }
            // Comic Book / Watercolor: a Kuwahara pass paints the flat areas first.
            if let Some(smoothing) = filter.style_smoothing().filter(|_| !self.uniform[input]) {
                input = self.push(Node::Pixelwise { input, filter: smoothing, kind, support: smoothing.support(), slope: None, images: [None; 2] });
            }
            // Blur Slope on Previous input: the slope is measured on a Gaussian-smoothed copy.
            let mut slope = None;
            if let Filter::BlurSlopeMode { intensity, source: subcraft_doc::SlopeSource::Input, .. } = filter {
                let weights = Arc::new(subcraft_materials::filter::gaussian_weights(intensity * subcraft_doc::SLOPE_SMOOTHING));
                let h = self.push(Node::Gaussian { input, weights: weights.clone(), horizontal: true, wrap: false });
                slope = Some(self.push(Node::Gaussian { input: h, weights, horizontal: false, wrap: false }));
            }
            let support = match filter {
                // The node itself only walks `intensity` pixels; the smoothing has its own nodes.
                Filter::BlurSlopeMode { intensity, .. } => intensity.ceil() as u32 + 1,
                _ => filter.support(),
            };
            return Some(self.push(Node::Pixelwise { input, filter, kind, support, slope, images }));
        }
        if let Filter::Warp { scale, seed, .. } = filter {
            let intensity = radius * self.radius_scale;
            return Some(self.push(Node::Warp {
                input,
                intensity,
                scale,
                seed,
                support: intensity.ceil() as u32 + 1,
                samples: 1,
            }));
        }
        if let Filter::BlurSlope { scale, seed, samples, .. } = filter {
            let intensity = radius * self.radius_scale;
            return Some(self.push(Node::Warp {
                input,
                intensity,
                scale,
                seed,
                support: intensity.ceil() as u32 + 1,
                samples: samples.clamp(2, subcraft_doc::MAX_SLOPE_SAMPLES),
            }));
        }
        if matches!(filter, Filter::Median { .. }) {
            let radius = subcraft_materials::filter::median::window_radius(radius * self.radius_scale);
            return Some(if radius == 0 { input } else { self.push(Node::Median { input, radius }) });
        }
        if let Filter::DirectionalBlur { angle_deg, .. } = filter {
            let radius = radius * self.radius_scale;
            let taps = Arc::new(subcraft_materials::filter::directional_taps(
                radius, angle_deg,
            ));
            return Some(self.push(Node::Directional {
                input,
                taps,
                support: radius.ceil() as u32,
            }));
        }
        let weights = Arc::new(subcraft_materials::filter::gaussian_weights(
            radius * self.radius_scale,
        ));
        let wrap = matches!(filter, Filter::BlurWrap { .. });
        let h = self.push(Node::Gaussian {
            input,
            weights: weights.clone(),
            horizontal: true,
            wrap,
        });
        Some(self.push(Node::Gaussian {
            input: h,
            weights,
            horizontal: false,
            wrap,
        }))
    }
    /// The layer's mask node, built once per layer together with its micro-effect taps.
    fn layer_mask(&mut self, layer: &'a Layer, mask: &'a Mask) -> usize {
        if let Some(&node) = self.masks.get(&layer.id) {
            return node;
        }
        let (node, micro) = self.mask_with_micro(mask, Some(layer));
        self.masks.insert(layer.id, node);
        if !micro.is_empty() {
            self.micro_masks.insert(layer.id, micro);
        }
        node
    }
    fn mask(&mut self, mask: &'a Mask) -> usize {
        self.mask_with_micro(mask, None).0
    }
    /// The nodes a Compare Mask of `layer` reads: the accumulation below the layer and the
    /// layer's own content in the compared channel.
    fn compare_operands(&mut self, layer: &'a Layer, c: &CompareMask) -> [Option<usize>; 2] {
        use subcraft_doc::CompareOperand as O;
        let reads = |o: O| c.left == o || c.right == o;
        let kind = c.channel;
        let below = reads(O::LayersBelow).then(|| self.layer_below(layer, kind));
        let own = reads(O::ThisLayer).then(|| self.layer_content(layer, kind));
        [below, own]
    }
    /// The accumulation of `kind` right below `layer`: recorded while that channel was built, or
    /// rebuilt for a top-level layer; the channel's default value otherwise.
    fn layer_below(&mut self, layer: &'a Layer, kind: ChannelKind) -> usize {
        if let Some(&n) = self.compare_below.get(&(layer.id, kind)) {
            return n;
        }
        let d = kind.default_value().0;
        let base = self.push(Node::Constant([d[0], d[1], d[2], 1.0]));
        let top = self.top;
        match top.iter().position(|l| l.id == layer.id) {
            Some(at) => self.stack(&top[..at], base, kind),
            None => base,
        }
    }
    /// This layer's own content in `kind` after its content effects (transparent when it has none).
    fn layer_content(&mut self, layer: &'a Layer, kind: ChannelKind) -> usize {
        if let Some(&n) = self.compare_content.get(&(layer.id, kind)) {
            return n;
        }
        let base = match &layer.content {
            LayerContent::Group(_) => self.push(Node::Constant([0.0; 4])),
            _ => usize::MAX,
        };
        let base = self.content_base(layer, kind, base);
        let n = self.content_effects(layer, kind, base, None);
        self.compare_content.insert((layer.id, kind), n);
        n
    }
    /// The mask chain. With its `owner` known, anchor references in it resolve (ADR-0045).
    fn mask_with_micro(&mut self, mask: &'a Mask, owner: Option<&'a Layer>) -> (usize, Vec<(usize, &'a Generator)>) {
        let mut micro = Vec::new();
        let mut input = self.push(Node::MaskBase(&mask.base));
        let record = owner.filter(|_| self.anchors.is_some());
        let mut steps = record.map(|_| vec![input]);
        for effect in mask.effects.iter() {
            if effect.visible && effect.opacity > 0.0 && !matches!(effect.kind, MaskEffectKind::Anchor) {
                let anchor = match (record, subcraft_doc::anchor::mask_effect_input(&effect.kind)) {
                    (Some(layer), Some(_)) => {
                        let site = subcraft_doc::RefSite::Mask { layer: layer.id, effect: effect.id };
                        // An anchor earlier in this same mask: its step is already built here.
                        // Going through `anchor_node` would build this mask again, endlessly.
                        match self.own_mask_anchor(site, layer.id, steps.as_deref().unwrap_or(&[])) {
                            Some(local) => local,
                            None => self.anchor_node(site),
                        }
                    }
                    _ => None,
                };
                // An unresolved Anchor Reference contributes nothing: no node at all.
                if !(anchor.is_none() && matches!(effect.kind, MaskEffectKind::AnchorRef { .. })) {
                    let filtered = match &effect.kind {
                        MaskEffectKind::Filter(f) => {
                            // The anchor in the filter's image slot, as a gray image.
                            let image = anchor.map(|(source, from)| self.push(Node::AnchorRead { source, from, to: ChannelKind::Roughness, strength: 1.0 }));
                            let slot = f.anchor_inputs().first().map_or(0, |(slot, _)| *slot);
                            let mut images = [None; 2];
                            images[slot.min(1)] = image;
                            self.neighborhood(input, *f, ChannelKind::Roughness, images)
                        }
                        // Mask Editor with a global blur: blur its contributions, finish after.
                        MaskEffectKind::Generator(g) if g.editor_blur() > 0.0 => {
                            let raw = self.push(Node::EditorRaw { generator: g });
                            self.neighborhood(raw, Filter::Blur { radius: g.editor_blur() }, ChannelKind::Roughness, [None; 2])
                        }
                        _ => None,
                    };
                    let compare = match (&effect.kind, owner) {
                        (MaskEffectKind::Compare(c), Some(layer)) => self.compare_operands(layer, c),
                        _ => [None, None],
                    };
                    let details = match (&effect.kind, owner) {
                        (MaskEffectKind::Generator(g), Some(layer)) => g.micro_details_active().and_then(|d| self.micro_detail_node(layer, d)),
                        _ => None,
                    };
                    // A filter reads its anchor through `filtered`.
                    let anchor = anchor.filter(|_| !matches!(effect.kind, MaskEffectKind::Filter(_)));
                    input = self.push(Node::MaskEffect {
                        input,
                        effect,
                        filtered,
                        anchor,
                        compare,
                        details,
                    });
                    if let MaskEffectKind::Generator(g) = &effect.kind
                        && g.micro_active()
                    {
                        micro.push((input, g));
                    }
                }
            }
            if let Some(s) = steps.as_mut() {
                s.push(input);
            }
        }
        if let (Some(layer), Some(steps), Some(a)) = (record, steps, self.anchors.as_mut()) {
            a.masks.insert(layer.id, steps);
        }
        (input, micro)
    }
    /// Micro details of a generator in `layer`'s mask: a [`Node::MicroDetail`] over the Normal and
    /// Height below the layer, at every folder level from the top down.
    fn micro_detail_node(&mut self, layer: &'a Layer, settings: subcraft_doc::MicroDetails) -> Option<usize> {
        let path = ancestry(self.top, layer.id)?;
        let mut chain = Vec::with_capacity(path.len());
        for (depth, level) in path.iter().enumerate() {
            let pair = [ChannelKind::Normal, ChannelKind::Height].map(|kind| {
                if depth == 0 {
                    // Top level: recorded, or rebuilt from the layers below.
                    Some(self.layer_below(level, kind))
                } else {
                    // Inside a folder: only what that channel's build recorded (a folder that does
                    // not take part in a channel never builds its children for it).
                    self.compare_below.get(&(level.id, kind)).copied()
                }
            });
            // Nothing recorded: a transparent level, so the one above shows through.
            let normal = match pair[0] { Some(n) => n, None => self.push(Node::Constant([0.0; 4])) };
            let height = match pair[1] { Some(n) => n, None => self.push(Node::Constant([0.0; 4])) };
            chain.push([normal, height]);
        }
        Some(self.push(Node::MicroDetail { chain: Arc::new(chain), settings, own_adds: false, horizon: false }))
    }
    /// Micro details for the material filter at `index` of `layer`'s content effects: over the
    /// Normal and Height below the layer (as a generator's micro details) and then the layer's
    /// own content up to that effect, laid over them by its coverage. One node per effect,
    /// shared by every channel the filter writes.
    fn effect_detail_node(&mut self, layer: &'a Layer, index: usize, settings: subcraft_doc::MicroDetails) -> Option<usize> {
        if let Some(&n) = self.effect_details.get(&(layer.id, index)) {
            return Some(n);
        }
        let below = self.micro_detail_node(layer, settings)?;
        let Node::MicroDetail { chain, .. } = &self.nodes[below] else { unreachable!() };
        let mut chain = chain.as_ref().clone();
        let own = [ChannelKind::Normal, ChannelKind::Height].map(|kind| self.below_effect(layer, index, kind));
        chain.push(own);
        // A Passthrough paint or fill layer's content already is the stack below, processed.
        let own_adds = layer.is_folder() || layer.blend_for(ChannelKind::Height).mode != BlendMode::Passthrough;
        let horizon = matches!(layer.content_effects[index].kind, ContentEffectKind::Material(subcraft_doc::MaterialFilter::Hbao { .. }));
        let n = self.push(Node::MicroDetail { chain: Arc::new(chain), settings, own_adds, horizon });
        self.effect_details.insert((layer.id, index), n);
        Some(n)
    }
    /// The layer's own Base Color content before the effect at `index`, Gaussian-blurred over
    /// `radius` texels at 2048: its alpha is the soft coverage of what is painted there. One
    /// node per effect, shared by every channel the filter writes.
    fn effect_coverage_node(&mut self, layer: &'a Layer, index: usize, radius: f32) -> usize {
        if let Some(&n) = self.effect_details.get(&(layer.id, index)) {
            return n;
        }
        let input = self.below_effect(layer, index, ChannelKind::BaseColor);
        let size = radius * self.ctx.geometry.resolution as f32 / 2048.0 * self.radius_scale;
        let weights = Arc::new(subcraft_materials::filter::gaussian_weights(size.max(0.5)));
        let h = self.push(Node::Gaussian { input, weights: weights.clone(), horizontal: true, wrap: false });
        let n = self.push(Node::Gaussian { input: h, weights, horizontal: false, wrap: false });
        self.effect_details.insert((layer.id, index), n);
        n
    }
    /// Micro curvature and occlusion for one tile (see [`Node::MicroDetail`]).
    fn micro_detail(&self, chain: &[[usize; 2]], settings: &subcraft_doc::MicroDetails, own_adds: bool, horizon: bool, geo: &subcraft_materials::TileGeometry) -> Vec<[f32; 4]> {
        let res = self.ctx.geometry.resolution;
        let scale = res as f32 / 2048.0;
        let support = settings.support();
        // Occlusion radius in texels, never beyond the support the dirty-tile expansion assumes.
        let ao_r = ((1.0 + settings.ao_radius.clamp(0.0, 1.0) * (subcraft_doc::MicroDetails::MAX_AO_TEXELS - 1.0)) * scale * self.radius_scale)
            .clamp(1.0, support.saturating_sub(2).max(1) as f32);
        let reach = ao_r.ceil() as u32 + 2;
        let grids: Vec<[Neighborhood; 2]> = chain.iter().map(|pair| pair.map(|n| Neighborhood::new(self, n, geo, reach))).collect();
        // The Normal and Height a texel sees: each level over the one above it, by its coverage.
        let sample = |x: i32, y: i32| -> ([f32; 3], f32) {
            let px = x.clamp(0, res as i32 - 1) as u32;
            let py = y.clamp(0, res as i32 - 1) as u32;
            let mut n = [0.5, 0.5, 1.0];
            let mut h = 0.0;
            for (depth, [gn, gh]) in grids.iter().enumerate() {
                let a = gn.pixel(px, py);
                let b = gh.pixel(px, py);
                let (wa, wb) = if depth == 0 { (1.0, 1.0) } else { (a[3].clamp(0.0, 1.0), b[3].clamp(0.0, 1.0)) };
                if own_adds && depth + 1 == grids.len() {
                    // Height's default blend adds; a normal leans the one below by its own tilt.
                    n[0] += (a[0] - 0.5) * wa;
                    n[1] += (a[1] - 0.5) * wa;
                    h += b[0] * wb;
                    continue;
                }
                for c in 0..3 {
                    n[c] += (a[c] - n[c]) * wa;
                }
                h += (b[0] - h) * wb;
            }
            (n, h)
        };
        let mut out = micro_detail_tile(settings, res, ao_r, geo, &sample);
        if horizon {
            let open = horizon_occlusion_tile(res, ao_r, settings.ao_depth, geo, |x, y| sample(x, y).1);
            for (o, v) in out.iter_mut().zip(open) {
                o[1] = v;
            }
        }
        out
    }
    fn stack(&mut self, layers: &'a [Arc<Layer>], mut below: usize, kind: ChannelKind) -> usize {
        for layer in layers {
            below = self.layer(layer, below, kind);
        }
        below
    }
    fn layer(&mut self, layer: &'a Layer, below: usize, kind: ChannelKind) -> usize {
        self.compare_below.entry((layer.id, kind)).or_insert(below);
        let out = self.layer_inner(layer, below, kind);
        if layer.visible && matches!(kind, ChannelKind::Normal | ChannelKind::Height) && micro_layer(layer) {
            return self.micro(layer, out, kind);
        }
        out
    }
    /// Micro effect: the layer's micro-effect generators also write into the Normal / Height
    /// accumulated so far. Nothing is added to a channel nothing has written to (no channel is
    /// created), and the generator layer's own per-channel blends do not matter: the generator
    /// is a mask, and the layer need not carry height or normal content itself.
    fn micro(&mut self, layer: &'a Layer, mut out: usize, kind: ChannelKind) -> usize {
        if matches!(self.nodes[out], Node::Constant(_)) {
            return out;
        }
        let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) else { return out };
        self.layer_mask(layer, mask);
        let taps = self.micro_masks.get(&layer.id).cloned().unwrap_or_default();
        let base = layer.blend_for(ChannelKind::BaseColor);
        let opacity = if base.is_active() { base.opacity.clamp(0.0, 1.0) } else { 1.0 };
        for (index, (mask_node, generator)) in taps.into_iter().enumerate() {
            let delta = match self.micro_deltas.get(&(layer.id, index)) {
                Some(&d) => d,
                None => {
                    let d = self.push(Node::MicroDelta { mask: mask_node, generator, opacity });
                    self.micro_deltas.insert((layer.id, index), d);
                    d
                }
            };
            out = self.push(if kind == ChannelKind::Height {
                Node::MicroHeight { below: out, delta }
            } else {
                Node::MicroNormal { below: out, delta }
            });
        }
        out
    }
    fn layer_inner(&mut self, layer: &'a Layer, below: usize, kind: ChannelKind) -> usize {
        if !layer.visible || !layer.blend_for(kind).is_active() {
            return below;
        }
        // Match the point evaluator's absent-source early return, including Replace mode.
        if !crate::util::effects_touch(layer, kind)
            && match &layer.content {
                LayerContent::Paint(p) => !p.channels.contains_key(&kind),
                LayerContent::Fill(f) => !f.channels.get(&kind).is_some_and(|c| c.enabled),
                LayerContent::Group(_) => false,
            }
        {
            return below;
        }
        let mask = if let Some(m) = layer.mask.as_ref().filter(|m| m.enabled) {
            Some(self.layer_mask(layer, m))
        } else {
            None
        };
        // Folders and, like Painter, paint / fill layers: in Passthrough the stack below is the
        // layer's content (its own paint or fill is not used) and its effects process it.
        let passthrough = layer.blend_for(kind).mode == BlendMode::Passthrough;
        let input = if self.anchors.is_some() && !passthrough {
            // Shared with any anchor captured in this stack.
            *self.isolated_content(layer, kind).last().expect("chain has a base")
        } else {
            let base = match &layer.content {
                _ if passthrough => below,
                LayerContent::Group(_) => self.push(Node::Constant([0.0; 4])),
                _ => usize::MAX,
            };
            let base = if passthrough && !layer.is_folder() { base } else { self.content_base(layer, kind, base) };
            self.content_effects(layer, kind, base, None)
        };
        if let Some(a) = self.anchors.as_mut() {
            a.sources.insert((layer.id, kind), input);
        }
        if !passthrough {
            self.compare_content.entry((layer.id, kind)).or_insert(input);
        }
        self.push(Node::Layer {
            below,
            input,
            mask,
            layer,
            kind,
            passthrough,
        })
    }
    /// Prime long same-tile chains iteratively. Neighbors only depend on strictly earlier stages.
    fn key(&self, node: usize, tile: usize) -> Key {
        (node, if self.uniform[node] { usize::MAX } else { tile })
    }
    /// Storage class of every node, from how often and from where it is read.
    fn modes(&self) -> &[Mode] {
        self.modes.get_or_init(|| {
            let n = self.nodes.len();
            let mut uses = vec![0u32; n];
            let mut shared = vec![false; n];
            for &(_, root) in &self.roots {
                uses[root] += 1;
            }
            for node in &self.nodes {
                let mut read = |i: usize| uses[i] += 1;
                match node {
                    Node::Constant(_) | Node::Paint(..) | Node::Fill(..) | Node::MaskBase(_) | Node::EditorRaw { .. } => {}
                    Node::MaskEffect { input, filtered, anchor, compare, details, .. } => {
                        read(*input);
                        if let Some(d) = details {
                            read(*d);
                        }
                        if let Some(f) = filtered {
                            read(*f);
                        }
                        if let Some((a, _)) = anchor {
                            read(*a);
                        }
                        for &c in compare.iter().flatten() {
                            read(c);
                        }
                    }
                    Node::ContentEffect { input, filtered, .. } => {
                        read(*input);
                        if let Some(f) = filtered {
                            read(*f);
                        }
                    }
                    Node::AnchorRead { source, .. } => read(*source),
                    Node::Layer { below, input, mask, .. } => {
                        read(*below);
                        read(*input);
                        if let Some(m) = mask {
                            read(*m);
                        }
                    }
                    Node::Pixelwise { input, slope, images, .. } => {
                        for n in [Some(*input), *slope, images[0], images[1]].into_iter().flatten() {
                            shared[n] = true;
                            uses[n] += 2;
                        }
                    }
                    Node::Directional { input, .. }
                    | Node::Median { input, .. }
                    | Node::Warp { input, .. }
                    | Node::Contour { input, .. }
                    | Node::Gaussian { input, .. } => {
                        shared[*input] = true;
                        uses[*input] += 2;
                    }
                    Node::MicroDelta { mask, .. } => {
                        // The generator mask feeds the delta of this tile and, later, the layer
                        // itself: keep it so the generator runs once per tile.
                        shared[*mask] = true;
                        uses[*mask] += 2;
                    }
                    Node::MicroDetail { chain, .. } => {
                        for &n in chain.iter().flatten() {
                            shared[n] = true;
                            uses[n] += 2;
                        }
                    }
                    Node::MicroHeight { below, delta } | Node::MicroNormal { below, delta } => {
                        read(*below);
                        shared[*delta] = true;
                        uses[*delta] += 2;
                    }
                }
            }
            (0..n)
                .map(|i| {
                    if shared[i] || self.uniform[i] {
                        Mode::Shared
                    } else if uses[i] > 1 {
                        Mode::Tile
                    } else {
                        Mode::Once
                    }
                })
                .collect()
        })
    }
    fn lookup(&self, node: usize, tile: usize) -> Option<Pixels> {
        let key = self.key(node, tile);
        let hit = if self.modes()[node] == Mode::Shared {
            self.cache.ready(key)
        } else {
            SCRATCH.with(|s| {
                let s = s.borrow();
                if s.graph == self.id { s.pixels.get(&key).cloned() } else { None }
            })
        };
        hit.or_else(|| {
            let p = self.prefix.as_ref()?;
            let &(kind, j) = p.marks.get(&node)?;
            p.cache.get(kind, j, tile).map(|packed| Arc::new(packed.unpack()))
        })
        .or_else(|| {
            let c = self.content.as_ref()?;
            let &(layer, slot) = c.marks.get(&node)?;
            c.cache.get(layer, slot, tile).map(|packed| Arc::new(packed.unpack()))
        })
    }
    fn scratch_put(&self, key: Key, value: Pixels) {
        SCRATCH.with(|s| {
            let mut s = s.borrow_mut();
            if s.graph != self.id {
                s.graph = self.id;
                s.pixels.clear();
            }
            s.pixels.insert(key, value);
        });
    }
    fn scratch_clear(&self) {
        SCRATCH.with(|s| {
            let mut s = s.borrow_mut();
            if s.graph == self.id {
                s.pixels.clear();
            }
        });
    }
    fn get(&self, node: usize, tile: usize) -> Pixels {
        if let Some(p) = self.lookup(node, tile) {
            return p;
        }
        // Walk down the primary inputs and evaluate bottom-up, so a 40-layer stack is a loop and
        // not 40 nested calls.
        let mut chain = Vec::new();
        let mut at = node;
        let mut held = None;
        loop {
            if let Some(ready) = self.lookup(at, tile) {
                held = Some(ready);
                break;
            }
            chain.push(at);
            match self.nodes[at].primary() {
                Some(next) => at = next,
                None => break,
            }
        }
        for &id in chain.iter().rev() {
            let key = self.key(id, tile);
            let result = match self.modes()[id] {
                // `compute` must not wait on rayon jobs (par_iter, join): while it waits, this
                // thread can steal a tile that blocks on this same cell, which never completes.
                Mode::Shared => self.cache.cell(key).get_or_init(|| Arc::new(self.compute(id, tile))).clone(),
                Mode::Tile => {
                    let p = Arc::new(self.compute(id, tile));
                    self.scratch_put(key, p.clone());
                    p
                }
                Mode::Once => {
                    let p = Arc::new(self.compute(id, tile));
                    // Only a chain link has a reader waiting in the loop; the requested node does not.
                    if id != node {
                        self.scratch_put(key, p.clone());
                    }
                    p
                }
            };
            if let Some(p) = &self.prefix {
                if let Some(&(kind, j)) = p.marks.get(&id) {
                    p.cache.put(kind, j, tile, &result);
                }
            }
            if let Some(c) = &self.content {
                if let Some(&(layer, slot)) = c.marks.get(&id) {
                    c.cache.put(layer, slot, tile, &result);
                }
            }
            // The primary of a node used once was read by this node alone.
            if let Some(below) = self.nodes[id].primary().filter(|&b| self.modes()[b] == Mode::Once) {
                SCRATCH.with(|s| {
                    let mut s = s.borrow_mut();
                    if s.graph == self.id {
                        s.pixels.remove(&self.key(below, tile));
                    }
                });
            }
            held = Some(result);
        }
        held.expect("chain evaluates at least the requested node")
    }
    /// Entry point for a whole tile: evaluate `node`, then free what only this tile needed.
    pub(crate) fn root(&self, node: usize, tile: usize) -> Pixels {
        let p = self.get(node, tile);
        self.scratch_clear();
        p
    }
    fn compute(&self, node: usize, tile: usize) -> Vec<[f32; 4]> {
        if !crate::profile::enabled() {
            return self.compute_node(node, tile);
        }
        let label = self.nodes[node].label();
        crate::profile::timed(label, || self.compute_node(node, tile))
    }
    fn compute_node(&self, node: usize, tile: usize) -> Vec<[f32; 4]> {
        let geo = self.ctx.geometry.tile(tile);
        let tc = TileContext {
            albedo_filter: self.albedo,
            geo: &geo,
            set_geometry: self.ctx.geometry,
            sample: SampleContext {
                resources: self.ctx.resources,
                mesh_maps: self.ctx.mesh_maps,
            },
        };
        match &self.nodes[node] {
            Node::Constant(v) => vec![*v; TILE_TEXELS],
            Node::Paint(image, kind) => {
                let mut p = vec![[0.0; 4]; TILE_TEXELS];
                read_paint(image, *kind, &tc, &mut p);
                p
            }
            Node::Fill(fill, kind) => {
                let mut p = vec![[0.0; 4]; TILE_TEXELS];
                eval_fill(fill, *kind, &tc, &mut p);
                p
            }
            Node::MaskBase(image) => {
                let mut p = vec![[0.0; 4]; TILE_TEXELS];
                crate::util::read_tile_rgba(image, &geo, &mut p);
                for v in &mut p {
                    *v = [v[0], v[0], v[0], 1.0];
                }
                p
            }
            Node::ContentEffect {
                input,
                effect,
                kind,
                filtered,
            } => {
                let source = self.get(*input, tile);
                let mut out = source.as_ref().clone();
                if let ContentEffectKind::Material(m) = &effect.kind {
                    // The one extra input is the Metallic below (PBR Validate) or the micro details.
                    let extra = filtered.map(|n| self.get(n, tile));
                    let extra = extra.as_deref().map(|p| p.as_slice());
                    let (metallic, details) = if m.reads_metallic() { (extra, None) } else { (None, extra) };
                    crate::material::apply(m, *kind, effect.blend_for(*kind), &mut out, &tc, metallic, details);
                    return out;
                }
                if let Some(filtered) = filtered {
                    let blurred = self.get(*filtered, tile);
                    let ContentEffectKind::Filter { filter, .. } = effect.kind else {
                        unreachable!()
                    };
                    let blend = effect.blend_for(*kind);
                    if effective_mode(blend.mode) == BlendMode::Normal && filter.spatial_identity()
                    {
                        return out;
                    }
                    let target = BlendTarget::for_channel(*kind);
                    for ((dst, src), blur) in out.iter_mut().zip(source.iter()).zip(blurred.iter())
                    {
                        let value = spatial_value(filter, *src, *blur, *kind);
                        let mut mixed = [src[0], src[1], src[2], 1.0];
                        composite(
                            &mut mixed,
                            [value[0], value[1], value[2]],
                            1.0,
                            effective_mode(blend.mode),
                            target,
                        );
                        if filter.softens_coverage() {
                            let t = blend.opacity.clamp(0.0, 1.0);
                            let a = src[3] + (value[3] - src[3]) * t;
                            for c in 0..3 {
                                dst[c] = if a > 1e-12 {
                                    (src[c] * src[3] * (1.0 - t) + mixed[c] * value[3] * t) / a
                                } else {
                                    0.0
                                };
                            }
                            dst[3] = a;
                        } else {
                            for c in 0..3 {
                                dst[c] =
                                    src[c] + (mixed[c] - src[c]) * blend.opacity.clamp(0.0, 1.0);
                            }
                        }
                    }
                } else {
                    crate::tile::point_effect(effect, *kind, &mut out, &tc);
                }
                out
            }
            Node::AnchorRead { source, from, to, strength } => {
                let mut p = self.get(*source, tile).as_ref().clone();
                anchor_as_channel(&mut p, *from, *to);
                crate::strength::apply_strength(*to, *strength, &mut p);
                if to.components() == 1 {
                    for px in p.iter_mut() {
                        *px = [px[0], px[0], px[0], px[3]];
                    }
                }
                p
            }
            Node::MaskEffect {
                input,
                effect,
                filtered,
                anchor,
                compare,
                details,
            } => {
                let source = self.get(*input, tile);
                let mut e = vec![[0.0; 4]; TILE_TEXELS];
                if let Some(filtered) = filtered {
                    let blurred = self.get(*filtered, tile);
                    match &effect.kind {
                        MaskEffectKind::Filter(filter) => {
                            for ((e, src), b) in e.iter_mut().zip(source.iter()).zip(blurred.iter()) {
                                *e = [
                                    spatial_value(*filter, *src, *b, ChannelKind::Roughness)[0],
                                    1.0,
                                    0.0,
                                    0.0,
                                ];
                            }
                        }
                        // Mask Editor: `blurred` is its blurred contributions.
                        MaskEffectKind::Generator(g) => {
                            let editor = g.editor_settings();
                            for (e, b) in e.iter_mut().zip(blurred.iter()) {
                                *e = [editor.finish(b[0]), 1.0, 0.0, 0.0];
                            }
                        }
                        _ => unreachable!(),
                    }
                } else if let MaskEffectKind::Compare(c) = &effect.kind {
                    let m: Vec<_> = source.iter().map(|v| v[0]).collect();
                    let [below, own] = (*compare).map(|n| n.map(|n| channel_gray(&self.get(n, tile), c.channel)));
                    crate::mask::eval_compare(c, &m, below.as_deref(), own.as_deref(), &mut e);
                } else {
                    let m: Vec<_> = source.iter().map(|v| v[0]).collect();
                    let input = anchor.map(|(node, from)| anchor_gray(&self.get(node, tile), from));
                    let details = details.map(|d| self.get(d, tile));
                    crate::mask::evaluate_effect(&effect.kind, &tc, &m, &mut e, input.as_deref(), details.as_deref().map(|d| d.as_slice()));
                }
                let mut out = source.as_ref().clone();
                // `[value; 3]` with the effect's coverage in alpha, blended with the mode chosen once.
                let src: Vec<[f32; 4]> = e.iter().map(|e| [e[0], e[0], e[0], e[1]]).collect();
                let opacity = effect.opacity;
                crate::blend::composite_run(&mut out, &src, effect.blend, BlendTarget::MASK, |_, s| s[3] * opacity);
                out
            }
            Node::Layer {
                below,
                input,
                mask,
                layer,
                kind,
                passthrough,
            } => {
                let mut out = self.get(*below, tile).as_ref().clone();
                let mask = mask.map(|m| self.get(m, tile));
                let mut coverage: Vec<f32> = match &mask {
                    Some(mask) => mask.iter().map(|m| m[0].clamp(0.0, 1.0)).collect(),
                    None => vec![1.0; TILE_TEXELS],
                };
                if let Some(parts) = layer.parts.as_ref().filter(|p| p.enabled) {
                    for (i, m) in coverage.iter_mut().enumerate() {
                        *m *= parts.factor(geo.region(i, parts.kind));
                    }
                }
                if coverage.iter().all(|&m| m <= 0.0) {
                    return out;
                }
                let content = self.get(*input, tile);
                let blend = layer.blend_for(*kind);
                let mode = effective_mode(blend.mode);
                if *passthrough {
                    for ((dst, src), m) in out.iter_mut().zip(content.iter()).zip(coverage) {
                        let f = (blend.opacity * m).clamp(0.0, 1.0);
                        crate::blend::passthrough_mix(dst, src, f);
                    }
                } else {
                    let (replace, opacity) = (mode == BlendMode::Replace, blend.opacity);
                    crate::blend::composite_run(&mut out, &content, mode, BlendTarget::for_channel(*kind), |i, s| {
                        (if replace { 1.0 } else { s[3] }) * opacity * coverage[i]
                    });
                }
                out
            }
            Node::EditorRaw { generator } => {
                let mut raw = vec![0.0f32; TILE_TEXELS];
                crate::mask::editor_raw(generator, &tc, &mut raw);
                raw.into_iter().map(|v| [v, v, v, 1.0]).collect()
            }
            Node::MicroDelta { mask, generator, opacity } => {
                let mask = self.get(*mask, tile);
                let micro = generator.micro.unwrap_or_default();
                let amount = micro.height() * opacity;
                let grain = (micro.grain_follows && amount != 0.0).then(|| crate::mask::fine_noise(generator, &geo));
                let mut out = vec![[0.0; 4]; TILE_TEXELS];
                for (i, o) in out.iter_mut().enumerate() {
                    let m = mask[i][0].clamp(0.0, 1.0);
                    if m <= 0.0 {
                        continue;
                    }
                    // Fine grain pits the surface: 0.7..1 of the full change.
                    let g = grain.as_ref().map_or(1.0, |f| 0.7 + 0.3 * ((f[i] - 0.5) * 2.5 + 0.5).clamp(0.0, 1.0));
                    // Soft knee: the faint tail of a mask (Dirt's grunge floor) leaves no relief.
                    let t = ((m - 0.1) / 0.5).clamp(0.0, 1.0);
                    let d = amount * (t * t * (3.0 - 2.0 * t)) * g;
                    *o = [d, d, d, 1.0];
                }
                out
            }
            Node::MicroDetail { chain, settings, own_adds, horizon } => self.micro_detail(chain, settings, *own_adds, *horizon, &geo),
            Node::MicroHeight { below, delta } => {
                let mut out = self.get(*below, tile).as_ref().clone();
                let delta = self.get(*delta, tile);
                for (o, d) in out.iter_mut().zip(delta.iter()) {
                    if d[0] != 0.0 {
                        for c in &mut o[..3] {
                            *c += d[0];
                        }
                    }
                }
                out
            }
            Node::MicroNormal { below, delta } => {
                let mut out = self.get(*below, tile).as_ref().clone();
                let grid = Neighborhood::new(self, *delta, &geo, 1);
                let res = self.ctx.geometry.resolution;
                // Height per texel -> slope per UV unit, so the look is resolution independent.
                let gain = res as f32 * MICRO_NORMAL_GAIN;
                for (i, o) in out.iter_mut().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    if x >= res || y >= res {
                        continue;
                    }
                    let h = |dx: i32, dy: i32| {
                        let px = (x as i32 + dx).clamp(0, res as i32 - 1) as u32;
                        let py = (y as i32 + dy).clamp(0, res as i32 - 1) as u32;
                        grid.pixel(px, py)[0]
                    };
                    let gx = (h(1, 0) - h(-1, 0)) * 0.5;
                    let gy = (h(0, 1) - h(0, -1)) * 0.5;
                    if gx == 0.0 && gy == 0.0 {
                        continue;
                    }
                    // OpenGL (+Y up) normal map, texel y grows downwards: the normal leans away
                    // from rising height.
                    let n = glam::Vec3::new(-gx * gain, gy * gain, 1.0).normalize();
                    let detail = [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5];
                    let r = crate::blend::whiteout([o[0], o[1], o[2]], detail);
                    o[..3].copy_from_slice(&r);
                }
                out
            }
            Node::Contour {
                input,
                filter,
                kind,
                radius,
            } => {
                use subcraft_materials::filter::contour;
                let threshold = match filter {
                    Filter::Bevel { threshold, .. }
                    | Filter::MaskOutline { threshold, .. }
                    | Filter::BevelShape { threshold, .. }
                    | Filter::BevelSmooth { threshold, .. }
                    | Filter::MaskOutlineShape { threshold, .. } => *threshold,
                    _ => unreachable!(),
                };
                let outline = filter.outline_parts().map(|o| o.position);
                // Angular bevels measure the larger axis distance (square corners).
                let angular = matches!(filter, Filter::BevelShape { corner: subcraft_doc::BevelCorner::Angular, .. });
                let source = self.get(*input, tile);
                if self.uniform[*input] {
                    let v = contour::value(
                        *filter,
                        contour::inside(source[0], *kind, threshold),
                        f32::INFINITY,
                        *radius,
                        *kind,
                    );
                    return vec![[v, v, v, source[0][3]]; TILE_TEXELS];
                }
                let support = (radius + 0.5).ceil() as u32;
                let grid = Neighborhood::new(self, *input, &geo, support);
                let res = self.ctx.geometry.resolution;
                let x0 = geo.x0.saturating_sub(support);
                let y0 = geo.y0.saturating_sub(support);
                let x1 = (geo.x0 + TILE_SIZE + support).min(res);
                let y1 = (geo.y0 + TILE_SIZE + support).min(res);
                let width = (x1 - x0) as usize;
                let height = (y1 - y0) as usize;
                let classes: Vec<_> = (y0..y1)
                    .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                    .map(|(x, y)| contour::inside(grid.pixel(x, y), *kind, threshold))
                    .collect();
                // Only the requested side(s) need a distance field. No full-resolution dense cache.
                let field = |feature: bool| {
                    if angular {
                        contour::squared_chebyshev(&classes, width, height, feature, support as usize + 1)
                    } else {
                        contour::squared_distance(&classes, width, height, feature)
                    }
                };
                let inner = (outline != Some(subcraft_doc::OutlinePosition::Outer)).then(|| field(false));
                let outer = matches!(outline, Some(subcraft_doc::OutlinePosition::Outer | subcraft_doc::OutlinePosition::Both)).then(|| field(true));
                let mut out = source.as_ref().clone();
                for (i, p) in out.iter_mut().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    if x >= res || y >= res {
                        continue;
                    }
                    let at = ((y - y0) * (x1 - x0) + x - x0) as usize;
                    let inside = classes[at];
                    let d = if inside {
                        inner.as_ref()
                    } else {
                        outer.as_ref()
                    }
                    .map_or(f32::INFINITY, |d| d[at]);
                    let v = contour::value(*filter, inside, d, *radius, *kind);
                    p[..3].fill(v);
                }
                out
            }
            Node::Warp {
                input,
                intensity,
                scale,
                seed,
                support,
                samples,
            } => {
                let grid = Neighborhood::new(self, *input, &geo, *support);
                let count = (*samples).max(1);
                let res = self.ctx.geometry.resolution;
                let mut out = vec![[0.0; 4]; TILE_TEXELS];
                for (i, dst) in out.iter_mut().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    if x >= res || y >= res {
                        continue;
                    }
                    let [dx, dy] = subcraft_materials::filter::warp::offset(
                        x, y, res, *intensity, *scale, *seed,
                    );
                    let mut src = [0.0f32; 4];
                    for k in 0..count {
                        // One read: the far end (Warp). Several: evenly from the texel to it.
                        let t = if count == 1 { 1.0 } else { k as f32 / (count - 1) as f32 };
                        let s = grid.premultiplied(x as f32 + dx * t, y as f32 + dy * t);
                        for c in 0..4 {
                            src[c] += s[c] / count as f32;
                        }
                    }
                    *dst = src;
                    if dst[3] > 1e-12 {
                        let a = dst[3];
                        for c in &mut dst[..3] {
                            *c /= a;
                        }
                    }
                }
                out
            }
            Node::Pixelwise {
                input,
                filter,
                kind,
                support,
                slope,
                images,
            } => {
                use subcraft_materials::filter::pixelwise as pw;
                let grid = Neighborhood::new(self, *input, &geo, *support);
                let slope = slope.map(|n| Neighborhood::new(self, n, &geo, 1));
                // Anchors in the image slots: gray is the value times the anchor's coverage.
                let anchors = images.map(|n| n.map(|n| Neighborhood::new(self, n, &geo, 1)));
                let bitmap = |s: Option<subcraft_doc::ImageSource>| s.and_then(subcraft_doc::ImageSource::resource).and_then(|id| self.ctx.resources.bitmap(id));
                let res = self.ctx.geometry.resolution;
                let mut out = vec![[0.0; 4]; TILE_TEXELS];
                if let Filter::Quantize { colors, dithering, pattern, lab, apply_to_alpha, alpha_threshold, .. } = *filter {
                    use subcraft_materials::filter::quantize as qz;
                    let lab = qz::uses_lab(lab, *kind);
                    let cell = self.palettes.lock().unwrap().entry(node).or_default().clone();
                    let palette = cell.get_or_init(|| Arc::new(qz::palette(&quantize_samples(&grid, res), colors as usize, lab))).clone();
                    let settings = qz::Quantize { dithering, pattern, lab, apply_to_alpha, alpha_threshold };
                    for (i, dst) in out.iter_mut().enumerate() {
                        let (x, y) = geo.texel_xy(i);
                        if x < res && y < res {
                            *dst = qz::map(grid.pixel(x, y), x, y, &palette, settings);
                        }
                    }
                    return out;
                }
                if let Filter::FillAreaMask { threshold, closed_borders } | Filter::FillAreaColor { threshold, closed_borders, .. } = *filter {
                    // Masks carry their value in the first component, content its coverage in alpha.
                    let mask = matches!(filter, Filter::FillAreaMask { .. });
                    let border_colors = matches!(filter, Filter::FillAreaColor { mode: subcraft_doc::FillColorMode::Border, .. });
                    let cell = self.fills.lock().unwrap().entry(node).or_default().clone();
                    let areas = cell.get_or_init(|| Arc::new(filled_areas(&grid, self.ctx.geometry, threshold, closed_borders, mask, border_colors))).clone();
                    let fill = match *filter {
                        Filter::FillAreaColor { color, .. } if kind.is_color() => color,
                        Filter::FillAreaColor { color, .. } => [color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722; 3],
                        _ => [1.0; 3],
                    };
                    let r = res as usize;
                    for (i, dst) in out.iter_mut().enumerate() {
                        let (x, y) = geo.texel_xy(i);
                        if x >= res || y >= res {
                            continue;
                        }
                        let p = grid.pixel(x, y);
                        let at = y as usize * r + x as usize;
                        let mut area = areas.labels[at];
                        let own = area != 0;
                        if !own {
                            // An outline texel next to a filled area: the fill shows through
                            // its soft edge.
                            let (x, y) = (x as usize, y as usize);
                            for (ok, j) in [(x > 0, at.wrapping_sub(1)), (x + 1 < r, at + 1), (y > 0, at.wrapping_sub(r)), (y + 1 < r, at + r)] {
                                if ok && areas.labels[j] != 0 {
                                    area = areas.labels[j];
                                    break;
                                }
                            }
                        }
                        *dst = if area == 0 {
                            p
                        } else if mask {
                            [1.0, 1.0, 1.0, p[3]]
                        } else {
                            let c = if border_colors { areas.colors[area as usize - 1] } else { fill };
                            let a = if own { 0.0 } else { p[3].clamp(0.0, 1.0) };
                            [p[0] * a + c[0] * (1.0 - a), p[1] * a + c[1] * (1.0 - a), p[2] * a + c[2] * (1.0 - a), 1.0]
                        };
                    }
                    return out;
                }
                if let Filter::ColorMatch { source_mode, source, target, hue, chroma, luma, source_image, target_image } = *filter {
                    // Average mode: the mean of the input.
                    let source = if source_mode == subcraft_doc::MatchSource::Average {
                        let cell = self.averages.lock().unwrap().entry(node).or_default().clone();
                        cell.get_or_init(|| average_color(&grid, res)).unwrap_or(source)
                    } else {
                        source
                    };
                    let per_texel = source_mode == subcraft_doc::MatchSource::Image;
                    let (source_image, target_image) = (bitmap(source_image).filter(|_| per_texel), bitmap(target_image));
                    // A slot's colour at a texel: the anchor (where it has coverage), the image,
                    // or the picked colour.
                    let colour = |slot: usize, image: Option<&Arc<subcraft_image::Bitmap>>, fallback: [f32; 3], x: u32, y: u32| {
                        if let Some(a) = &anchors[slot] {
                            let p = a.pixel(x, y);
                            return if p[3] > 0.0 { [p[0], p[1], p[2]] } else { fallback };
                        }
                        image.map_or(fallback, |b| crate::util::image_color(b, x, y, res))
                    };
                    for (i, dst) in out.iter_mut().enumerate() {
                        let (x, y) = geo.texel_xy(i);
                        if x < res && y < res {
                            let p = grid.pixel(x, y);
                            let from = if per_texel { colour(0, source_image, source, x, y) } else { source };
                            let to = colour(1, target_image, target, x, y);
                            let c = subcraft_materials::filter::color_match([p[0], p[1], p[2]], from, to, hue, chroma, luma);
                            *dst = [c[0], c[1], c[2], p[3]];
                        }
                    }
                    return out;
                }
                for (i, dst) in out.iter_mut().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    if x >= res || y >= res {
                        continue;
                    }
                    let here = grid.pixel(x, y);
                    *dst = match *filter {
                        Filter::BlurSlopeMode { intensity, scale, seed, samples, mode, source, image } => {
                            let sample = |dx: f32, dy: f32| grid.premultiplied(x as f32 + dx, y as f32 + dy);
                            let texel = |dx: i32, dy: i32| grid.pixel((x as i32 + dx).clamp(0, res as i32 - 1) as u32, (y as i32 + dy).clamp(0, res as i32 - 1) as u32);
                            match (source, &slope) {
                                // Previous input: across the nearest edge, both ways.
                                (subcraft_doc::SlopeSource::Input, Some(smooth)) => {
                                    let v = |dx: i32, dy: i32| {
                                        let p = smooth.pixel((x as i32 + dx).clamp(0, res as i32 - 1) as u32, (y as i32 + dy).clamp(0, res as i32 - 1) as u32);
                                        pw::scalar(p, *kind) * p[3].min(1.0)
                                    };
                                    let g = [(v(1, 0) - v(-1, 0)) * 0.5, (v(0, 1) - v(0, -1)) * 0.5];
                                    pw::slope_blur(sample, texel, pw::slope_offset(g, intensity), true, samples, mode, *kind)
                                }
                                (subcraft_doc::SlopeSource::Input, None) => here,
                                // Custom noise: uphill on the picked image, tiled like the noise.
                                (subcraft_doc::SlopeSource::Custom, _) if anchors[0].is_some() => {
                                    // An anchor: uphill on its gray value, one texel to one texel.
                                    let v = |dx: i32, dy: i32| {
                                        let p = anchors[0].as_ref().unwrap().pixel((x as i32 + dx).clamp(0, res as i32 - 1) as u32, (y as i32 + dy).clamp(0, res as i32 - 1) as u32);
                                        pw::scalar(p, *kind) * p[3].min(1.0)
                                    };
                                    let g = [(v(1, 0) - v(-1, 0)) * 0.5, (v(0, 1) - v(0, -1)) * 0.5];
                                    pw::slope_blur(sample, texel, pw::slope_offset(g, intensity), false, samples, mode, *kind)
                                }
                                (subcraft_doc::SlopeSource::Custom, _) => match bitmap(image) {
                                    Some(bmp) => {
                                        let v = |dx: f32, dy: f32| crate::util::image_gray(bmp, x as f32 + dx, y as f32 + dy, res, scale);
                                        let g = [(v(1.0, 0.0) - v(-1.0, 0.0)) * 0.5, (v(0.0, 1.0) - v(0.0, -1.0)) * 0.5];
                                        pw::slope_blur(sample, texel, pw::slope_offset(g, intensity), false, samples, mode, *kind)
                                    }
                                    None => here,
                                },
                                (subcraft_doc::SlopeSource::Noise, _) => pw::slope_blur(sample, texel, subcraft_materials::filter::warp::offset(x, y, res, intensity, scale, seed), false, samples, mode, *kind),
                            }
                        }
                        Filter::NonUniformBlur { radius, map, scale, seed, samples, image } => {
                            let image = match &anchors[0] {
                                Some(a) => {
                                    let p = a.pixel(x, y);
                                    Some(pw::scalar(p, *kind) * p[3].min(1.0))
                                }
                                None => bitmap(image).map(|b| crate::util::image_gray(b, x as f32, y as f32, res, 1)),
                            };
                            let m = pw::blur_map(map, pw::scalar(here, *kind), image, x, y, res, scale, seed);
                            pw::disc_blur(|dx, dy| grid.premultiplied(x as f32 + dx, y as f32 + dy), radius * m, samples)
                        }
                        Filter::EdgeDetect { radius, strength } => {
                            let read = |dx: i32, dy: i32| {
                                let px = (x as i32 + dx).clamp(0, res as i32 - 1) as u32;
                                let py = (y as i32 + dy).clamp(0, res as i32 - 1) as u32;
                                pw::scalar(grid.pixel(px, py), *kind)
                            };
                            let v = pw::edge_detect(read, radius, strength);
                            [v, v, v, here[3]]
                        }
                        Filter::DirectionalWarp { intensity, angle_deg } => {
                            let [dx, dy] = pw::directional_offset(pw::scalar(here, *kind), intensity, angle_deg, *kind);
                            let mut p = grid.premultiplied(x as f32 + dx, y as f32 + dy);
                            if p[3] > 1e-12 {
                                let a = p[3];
                                for c in &mut p[..3] {
                                    *c /= a;
                                }
                            }
                            p
                        }
                        Filter::Mosaic { size } => {
                            let (sx, sy) = pw::mosaic_source(x, y, size, res);
                            grid.pixel(sx, sy)
                        }
                        Filter::Transform { repeat, flip, scale, offset, rotation_deg, tile_safe } => {
                            let uv = [(x as f32 + 0.5) / res as f32, (y as f32 + 0.5) / res as f32];
                            let repeats = repeat.repeats();
                            match pw::transform_source(uv, scale, offset, rotation_deg, flip.flips(), repeats, tile_safe) {
                                Some(q) => unpremultiply(grid.premultiplied_wrapped(q[0] * res as f32 - 0.5, q[1] * res as f32 - 0.5, repeats)),
                                None => [0.0; 4],
                            }
                        }
                        Filter::Fxaa { alpha } => {
                            let read = |dx: i32, dy: i32| {
                                let px = (x as i32 + dx).clamp(0, res as i32 - 1) as u32;
                                let py = (y as i32 + dy).clamp(0, res as i32 - 1) as u32;
                                grid.pixel(px, py)
                            };
                            pw::fxaa(read, alpha, *kind)
                        }
                        Filter::AnisotropicKuwahara { radius, smoothness, sharpness, tensor_smoothness, anisotropy, angle, extract_direction } => {
                            let read = |dx: i32, dy: i32| {
                                let px = (x as i32 + dx).clamp(0, res as i32 - 1) as u32;
                                let py = (y as i32 + dy).clamp(0, res as i32 - 1) as u32;
                                grid.pixel(px, py)
                            };
                            pw::kuwahara(read, pw::Kuwahara { radius, smoothness, sharpness, tensor_smoothness, anisotropy, angle, extract_direction }, *kind)
                        }
                        Filter::ComicBook { tones, color_intensity, border_opacity, border_tolerance, border_smoothness, border_width, input_contrast, input_luminosity, .. } => {
                            let read = |dx: i32, dy: i32| {
                                let px = (x as i32 + dx).clamp(0, res as i32 - 1) as u32;
                                let py = (y as i32 + dy).clamp(0, res as i32 - 1) as u32;
                                grid.pixel(px, py)
                            };
                            let comic = subcraft_materials::filter::stylize::Comic { tones, color_intensity, border_opacity, border_tolerance, border_smoothness, border_width, input_contrast, input_luminosity };
                            subcraft_materials::filter::stylize::comic_book(read, comic, *kind)
                        }
                        Filter::Watercolor { paint_opacity, paper_absorption, borders_spread, borders_density, hide_paper, seed, .. } => {
                            let read = |dx: i32, dy: i32| {
                                let px = (x as i32 + dx).clamp(0, res as i32 - 1) as u32;
                                let py = (y as i32 + dy).clamp(0, res as i32 - 1) as u32;
                                grid.pixel(px, py)
                            };
                            let uv = [(x as f32 + 0.5) / res as f32, (y as f32 + 0.5) / res as f32];
                            let wash = subcraft_materials::filter::stylize::Watercolor { paint_opacity, paper_absorption, borders_spread, borders_density, hide_paper, seed };
                            subcraft_materials::filter::stylize::watercolor(read, wash, uv, *kind)
                        }
                        Filter::OilPaint { effect_intensity, stroke_size, stroke_length, size_random, color_random, bristles, angle, seed } => {
                            let read = |dx: i32, dy: i32| {
                                let px = (x as i32 + dx).clamp(0, res as i32 - 1) as u32;
                                let py = (y as i32 + dy).clamp(0, res as i32 - 1) as u32;
                                grid.pixel(px, py)
                            };
                            let oil = subcraft_materials::filter::stylize::OilPaint { effect_intensity, stroke_size, stroke_length, size_random, color_random, bristles, angle, seed };
                            subcraft_materials::filter::stylize::oil_paint(read, x, y, oil, *kind)
                        }
                        Filter::DirectionalDistance { distance, angle_deg, contrast } => {
                            // Beyond the texture there is nothing to spread from.
                            let read = |dx: i32, dy: i32| {
                                let (px, py) = (x as i32 + dx, y as i32 + dy);
                                if px < 0 || py < 0 || px >= res as i32 || py >= res as i32 { [0.0; 4] } else { grid.pixel(px as u32, py as u32) }
                            };
                            pw::directional_distance(read, distance, angle_deg, contrast, *kind)
                        }
                        Filter::Mirror { mode, offset, invert_x, invert_y, corner } => {
                            let uv = [(x as f32 + 0.5) / res as f32, (y as f32 + 0.5) / res as f32];
                            let (axes, far) = pw::mirror_axes(mode, invert_x, invert_y, corner);
                            let q = pw::mirror_source(uv, axes, offset, far);
                            if q == uv { here } else { unpremultiply(grid.premultiplied(q[0] * res as f32 - 0.5, q[1] * res as f32 - 0.5)) }
                        }
                        _ => here,
                    };
                }
                out
            }
            Node::Median { input, radius } => {
                let r = *radius as u32;
                let source = self.get(*input, tile);
                if self.uniform[*input] {
                    return source.as_ref().clone();
                }
                let grid = Neighborhood::new(self, *input, &geo, r);
                let res = self.ctx.geometry.resolution;
                let x0 = geo.x0.saturating_sub(r);
                let y0 = geo.y0.saturating_sub(r);
                let x1 = (geo.x0 + TILE_SIZE + r).min(res);
                let y1 = (geo.y0 + TILE_SIZE + r).min(res);
                let (width, height) = ((x1 - x0) as usize, (y1 - y0) as usize);
                let rect: Vec<[f32; 3]> = (y0..y1)
                    .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                    .map(|(x, y)| {
                        let p = grid.pixel(x, y);
                        [p[0], p[1], p[2]]
                    })
                    .collect();
                let (tw, th) = (
                    (res - geo.x0).min(TILE_SIZE) as usize,
                    (res - geo.y0).min(TILE_SIZE) as usize,
                );
                let med = subcraft_materials::filter::median::median_rect(
                    &rect,
                    width,
                    height,
                    (geo.x0 - x0) as usize,
                    (geo.y0 - y0) as usize,
                    tw,
                    th,
                    *radius,
                );
                let mut out = source.as_ref().clone();
                for (i, p) in out.iter_mut().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    if x >= res || y >= res {
                        continue;
                    }
                    let m = med[(y - geo.y0) as usize * tw + (x - geo.x0) as usize];
                    p[..3].copy_from_slice(&m);
                }
                out
            }
            Node::Directional {
                input,
                taps,
                support,
            } => {
                let grid = Neighborhood::new(self, *input, &geo, *support);
                let mut out = vec![[0.0; 4]; TILE_TEXELS];
                for (i, dst) in out.iter_mut().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    for &[dx, dy, w] in taps.iter() {
                        let src = grid.premultiplied(x as f32 + dx, y as f32 + dy);
                        for c in 0..4 {
                            dst[c] += src[c] * w;
                        }
                    }
                    if dst[3] > 1e-12 {
                        let a = dst[3];
                        for c in &mut dst[..3] {
                            *c /= a;
                        }
                    }
                }
                out
            }
            Node::Gaussian {
                input,
                weights,
                horizontal,
                wrap,
            } => {
                let r = (weights.len() / 2) as i32;
                let res = self.ctx.geometry.resolution as i32;
                let n = (res as u32).div_ceil(TILE_SIZE) as usize;
                let lo = if *horizontal {
                    geo.x0 as i32
                } else {
                    geo.y0 as i32
                };
                let hi = lo + TILE_SIZE as i32 - 1;
                // Wrapped: every tile of the row / column, so taps can continue from the far border.
                let (first, last) = if *wrap {
                    (0, n - 1)
                } else {
                    ((lo - r).clamp(0, res - 1) as usize / TILE_SIZE as usize, (hi + r).clamp(0, res - 1) as usize / TILE_SIZE as usize)
                };
                let neighbors: Vec<_> = (first..=last)
                    .map(|v| {
                        self.get(
                            *input,
                            if *horizontal {
                                (geo.y0 / TILE_SIZE) as usize * n + v
                            } else {
                                v * n + (geo.x0 / TILE_SIZE) as usize
                            },
                        )
                    })
                    .collect();
                // Same taps, in the same order, as summing `weights` texel by texel; only the
                // index arithmetic moves out of the inner loops (results are bit-identical).
                let taps: Vec<(usize, f32)> = weights.iter().copied().enumerate().filter(|(_, w)| *w > 0.0).collect();
                let ts = TILE_SIZE as usize;
                let fetch = |position: i32, across: usize| -> [f32; 4] {
                    let position = if *wrap { position.rem_euclid(res) } else { position.clamp(0, res - 1) } as usize;
                    let local = if *horizontal { across * ts + position % ts } else { position % ts * ts + across };
                    neighbors[position / ts - first][local]
                };
                let mut out = vec![[0.0; 4]; TILE_TEXELS];
                if *horizontal {
                    // One row at a time: the row's texels with color premultiplied by alpha (the
                    // `src[c] * src[3]` factor of every tap), padded by the radius on both sides.
                    let mut line = vec![[0.0f32; 4]; ts + 2 * r as usize];
                    for row in 0..ts {
                        for (k, l) in line.iter_mut().enumerate() {
                            let src = fetch(lo - r + k as i32, row);
                            *l = [src[0] * src[3], src[1] * src[3], src[2] * src[3], src[3]];
                        }
                        // Eight texels at a time: each still adds its taps in order, but the eight
                        // sums are independent, so the adds overlap instead of waiting in a chain.
                        let dst_row = &mut out[row * ts..(row + 1) * ts];
                        for (x8, dst8) in dst_row.chunks_exact_mut(8).enumerate() {
                            let x = x8 * 8;
                            let mut acc = [[0.0f32; 4]; 8];
                            for &(w, weight) in &taps {
                                for (k, a) in acc.iter_mut().enumerate() {
                                    let p = line[x + k + w];
                                    for c in 0..3 {
                                        a[c] += p[c] * weight;
                                    }
                                    a[3] += p[3] * weight;
                                }
                            }
                            dst8.copy_from_slice(&acc);
                        }
                    }
                } else {
                    // Whole source rows: for each output row, add every tap's row in order.
                    let rows: Vec<Vec<[f32; 4]>> = (0..ts + 2 * r as usize)
                        .map(|k| (0..ts).map(|x| fetch(lo - r + k as i32, x)).collect())
                        .collect();
                    for (y, dst_row) in out.chunks_exact_mut(ts).enumerate() {
                        // Eight texels at a time, summed in registers (taps in order per texel).
                        for (x8, dst8) in dst_row.chunks_exact_mut(8).enumerate() {
                            let x = x8 * 8;
                            let mut acc = [[0.0f32; 4]; 8];
                            for &(w, weight) in &taps {
                                for (a, src) in acc.iter_mut().zip(&rows[y + w][x..x + 8]) {
                                    for c in 0..3 {
                                        a[c] += src[c] * 1.0 * weight;
                                    }
                                    a[3] += src[3] * weight;
                                }
                            }
                            dst8.copy_from_slice(&acc);
                        }
                        for dst in dst_row.iter_mut() {
                            if dst[3] > 1e-12 {
                                let a = dst[3];
                                for v in &mut dst[..3] {
                                    *v /= a;
                                }
                            }
                        }
                    }
                }
                out
            }
        }
    }
    pub(crate) fn tile(&self, tile: usize) -> Vec<(ChannelKind, Pixels)> {
        let out = self
            .roots
            .iter()
            .filter(|(k, _)| self.only.as_ref().is_none_or(|only| only.contains(k)))
            .map(|&(k, n)| (k, self.get(n, tile)))
            .collect();
        self.scratch_clear();
        out
    }
}

/// Preload a bounded rectangle once; no cache locks or graph traversal inside bilinear taps.
/// Texels the Quantize palette is picked from: a regular grid of at most
/// `MAX_PALETTE_SAMPLES` over the whole texture, covered texels only.
fn quantize_samples(grid: &Neighborhood, res: u32) -> Vec<[f32; 3]> {
    let side = (subcraft_materials::filter::quantize::MAX_PALETTE_SAMPLES as f64).sqrt() as u32;
    let step = res.div_ceil(side).max(1);
    let mut samples = Vec::new();
    for y in (0..res).step_by(step as usize) {
        for x in (0..res).step_by(step as usize) {
            let p = grid.pixel(x, y);
            if p[3] > 0.0 {
                samples.push([p[0], p[1], p[2]]);
            }
        }
    }
    samples
}

/// The areas a Fill Area filter fills, for the whole texture.
struct FilledAreas {
    /// Per texel: 0, or the 1-based id of the area it is in.
    labels: Vec<u32>,
    /// Outline colour mode: per area the mean colour of the outline texels around it.
    colors: Vec<[f32; 3]>,
}

/// Flood-fills the whole input once: outline texels are those whose value (`mask`) or paint
/// coverage reaches `threshold`; the UV islands come from the texture set's geometry.
fn filled_areas(grid: &Neighborhood, set: &subcraft_materials::TextureSetGeometry, threshold: f32, closed_borders: bool, mask: bool, border_colors: bool) -> FilledAreas {
    let res = grid.res as usize;
    let amount = |p: [f32; 4]| if mask { p[0] } else { p[3] };
    let mut wall = vec![false; res * res];
    for y in 0..res {
        for x in 0..res {
            wall[y * res + x] = amount(grid.pixel(x as u32, y as u32)) >= threshold;
        }
    }
    let mut inside = vec![false; res * res];
    for index in 0..set.tile_count() {
        let geo = set.tile(index);
        if geo.is_empty() {
            continue;
        }
        for i in 0..TILE_TEXELS {
            let (x, y) = geo.texel_xy(i);
            if (x as usize) < res && (y as usize) < res && geo.coverage(i) > 0.0 {
                inside[y as usize * res + x as usize] = true;
            }
        }
    }
    let areas = subcraft_materials::filter::fill_area::enclosed(res, &wall, &inside, closed_borders);
    let mut colors = Vec::new();
    if border_colors {
        let mut sums = vec![([0.0f64; 3], 0.0f64); areas.count as usize];
        for y in 0..res {
            for x in 0..res {
                let at = y * res + x;
                if !wall[at] {
                    continue;
                }
                let p = grid.pixel(x as u32, y as u32);
                for (ok, j) in [(x > 0, at.wrapping_sub(1)), (x + 1 < res, at + 1), (y > 0, at.wrapping_sub(res)), (y + 1 < res, at + res)] {
                    if ok && areas.labels[j] != 0 {
                        let sum = &mut sums[areas.labels[j] as usize - 1];
                        for c in 0..3 {
                            sum.0[c] += p[c] as f64;
                        }
                        sum.1 += 1.0;
                    }
                }
            }
        }
        colors = sums.into_iter().map(|(sum, n)| if n > 0.0 { sum.map(|v| (v / n) as f32) } else { [0.0; 3] }).collect();
    }
    FilledAreas { labels: areas.labels, colors }
}

/// Mean colour of the covered texels of the whole texture, weighted by coverage, from the
/// same regular grid the Quantize palette uses. `None` when nothing is covered.
fn average_color(grid: &Neighborhood, res: u32) -> Option<[f32; 3]> {
    let side = (subcraft_materials::filter::quantize::MAX_PALETTE_SAMPLES as f64).sqrt() as u32;
    let step = res.div_ceil(side).max(1);
    let (mut sum, mut total) = ([0.0f64; 3], 0.0f64);
    for y in (0..res).step_by(step as usize) {
        for x in (0..res).step_by(step as usize) {
            let p = grid.pixel(x, y);
            let a = p[3].clamp(0.0, 1.0) as f64;
            for c in 0..3 {
                sum[c] += p[c] as f64 * a;
            }
            total += a;
        }
    }
    (total > 0.0).then(|| sum.map(|v| (v / total) as f32))
}

struct Neighborhood {
    x0: usize,
    y0: usize,
    cols: usize,
    res: u32,
    tiles: Vec<Pixels>,
}
impl Neighborhood {
    fn new(
        graph: &Graph<'_>,
        input: usize,
        geo: &subcraft_materials::TileGeometry,
        r: u32,
    ) -> Self {
        let res = graph.ctx.geometry.resolution;
        let n = res.div_ceil(TILE_SIZE);
        let x0 = geo.x0.saturating_sub(r) / TILE_SIZE;
        let y0 = geo.y0.saturating_sub(r) / TILE_SIZE;
        let x1 = (geo.x0 + TILE_SIZE - 1 + r).min(res - 1) / TILE_SIZE;
        let y1 = (geo.y0 + TILE_SIZE - 1 + r).min(res - 1) / TILE_SIZE;
        let mut tiles = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                tiles.push(graph.get(input, (y * n + x) as usize));
            }
        }
        Self {
            x0: x0 as usize,
            y0: y0 as usize,
            cols: (x1 - x0 + 1) as usize,
            res,
            tiles,
        }
    }
    fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
        let tile = (y / TILE_SIZE) as usize - self.y0;
        let tile = tile * self.cols + (x / TILE_SIZE) as usize - self.x0;
        self.tiles[tile][((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize]
    }
    /// [`Neighborhood::premultiplied`] where an axis that repeats wraps around the texture
    /// (bilinear across the border) instead of clamping.
    fn premultiplied_wrapped(&self, x: f32, y: f32, repeat: [bool; 2]) -> [f32; 4] {
        let res = self.res as i64;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let fix = |i: i64, repeat: bool| if repeat { i.rem_euclid(res) } else { i.clamp(0, res - 1) } as u32;
        let mut out = [0.0; 4];
        for (yy, wy) in [(fix(y0 as i64, repeat[1]), 1.0 - fy), (fix(y0 as i64 + 1, repeat[1]), fy)] {
            for (xx, wx) in [(fix(x0 as i64, repeat[0]), 1.0 - fx), (fix(x0 as i64 + 1, repeat[0]), fx)] {
                let w = wx * wy;
                if w == 0.0 {
                    continue;
                }
                let src = self.pixel(xx, yy);
                for c in 0..3 {
                    out[c] += src[c] * src[3] * w;
                }
                out[3] += src[3] * w;
            }
        }
        out
    }
    fn premultiplied(&self, x: f32, y: f32) -> [f32; 4] {
        let x = x.clamp(0.0, (self.res - 1) as f32);
        let y = y.clamp(0.0, (self.res - 1) as f32);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let mut out = [0.0; 4];
        for (yy, wy) in [(y0, 1.0 - fy), ((y0 + 1).min(self.res - 1), fy)] {
            for (xx, wx) in [(x0, 1.0 - fx), ((x0 + 1).min(self.res - 1), fx)] {
                let w = wx * wy;
                if w == 0.0 {
                    continue;
                }
                let src = self.pixel(xx, yy);
                for c in 0..3 {
                    out[c] += src[c] * src[3] * w;
                }
                out[3] += src[3] * w;
            }
        }
        out
    }
}

/// Straight-alpha texel of a premultiplied sample.
fn unpremultiply(mut p: [f32; 4]) -> [f32; 4] {
    if p[3] > 1e-12 {
        let a = p[3];
        for c in &mut p[..3] {
            *c /= a;
        }
    }
    p
}

fn spatial_value(
    filter: Filter,
    source: [f32; 4],
    blurred: [f32; 4],
    kind: ChannelKind,
) -> [f32; 4] {
    match filter {
        Filter::Bevel { .. } | Filter::MaskOutline { .. } | Filter::Median { .. } => blurred,
        Filter::BevelShape { .. } | Filter::MaskOutlineShape { .. } | Filter::Fxaa { .. } => blurred,
        Filter::BevelSmooth { .. } | Filter::DirectionalDistance { .. } => blurred,
        // Pixelwise nodes whose output is the final value (a filter missing here is ignored).
        Filter::AnisotropicKuwahara { .. } | Filter::Quantize { .. } => blurred,
        Filter::ComicBook { .. } | Filter::Watercolor { .. } | Filter::OilPaint { .. } => blurred,
        Filter::FillAreaMask { .. } | Filter::FillAreaColor { .. } => blurred,
        Filter::BlurSlopeMode { .. } | Filter::NonUniformBlur { .. } | Filter::ColorMatch { .. } => blurred,
        Filter::DropShadow { spread, opacity, color, .. } => under(source, blurred, spread, opacity, false, color, kind),
        Filter::OuterGlow { spread, opacity, color, precise, .. } => under(source, blurred, spread, opacity, precise, color, kind),
        Filter::Blur { radius: 0.0 }
        | Filter::BlurWrap { radius: 0.0 }
        | Filter::Sharpen { radius: 0.0, .. }
        | Filter::Sharpen { amount: 0.0, .. }
        | Filter::DirectionalBlur { radius: 0.0, .. } => source,
        Filter::Blur { .. } | Filter::BlurWrap { .. } | Filter::Transform { .. } | Filter::Mirror { .. }
        | Filter::DirectionalBlur { .. } | Filter::Warp { .. } | Filter::BlurSlope { .. }
        | Filter::EdgeDetect { .. } | Filter::DirectionalWarp { .. } | Filter::Mosaic { .. } => blurred,
        Filter::Glow { radius: 0.0, .. } | Filter::Glow { amount: 0.0, .. } => source,
        Filter::Glow { amount, threshold, .. } => {
            let mut out = source;
            for c in 0..3 {
                let add = amount * (blurred[c] - threshold).max(0.0) / (1.0 - threshold);
                let v = source[c] + add;
                out[c] = if kind.is_signed() { v.clamp(-1.0, 1.0) } else { v.clamp(0.0, 1.0) };
            }
            out
        }
        Filter::Highpass { amount, .. } => {
            let mut out = source;
            let v = subcraft_materials::filter::highpass(
                [source[0], source[1], source[2]],
                [blurred[0], blurred[1], blurred[2]],
                amount,
                kind,
            );
            out[..3].copy_from_slice(&v);
            out
        }
        Filter::Sharpen { amount, .. } => {
            let mut out = source;
            for c in 0..3 {
                let v = source[c] + amount * (source[c] - blurred[c]);
                out[c] = if kind.is_signed() {
                    v
                } else {
                    v.clamp(0.0, 1.0)
                };
            }
            out
        }
        _ => source,
    }
}
/// Drop Shadow / Glow: `shape` (the moved and blurred source) widened by `spread`, at
/// `opacity`, placed under `source`. Colour channels use the coverage and `color`; scalar
/// channels (and masks) use the blurred value, brightening toward the colour's luminance.
fn under(source: [f32; 4], shape: [f32; 4], spread: f32, opacity: f32, precise: bool, color: [f32; 3], kind: ChannelKind) -> [f32; 4] {
    let gain = 1.0 / (1.0 - spread.clamp(0.0, 0.99)) * if precise { 2.0 } else { 1.0 };
    if kind.is_color() {
        let a = (shape[3] * gain).clamp(0.0, 1.0) * opacity;
        let out_a = source[3] + a * (1.0 - source[3]);
        let mut out = [0.0, 0.0, 0.0, out_a];
        if out_a > 1e-12 {
            for c in 0..3 {
                out[c] = (source[c] * source[3] + color[c] * a * (1.0 - source[3])) / out_a;
            }
        }
        out
    } else {
        let a = (shape[0] * shape[3] * gain).clamp(0.0, 1.0) * opacity;
        let g = color[0] * LUMA[0] + color[1] * LUMA[1] + color[2] * LUMA[2];
        let v = source[0] + (g - source[0]).max(0.0) * a;
        [v, v, v, source[3]]
    }
}

/// Rec. 709 weights for reading a color channel as gray.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// An anchor's texels as one grayscale value per texel, premultiplied by coverage (mask anchors
/// are opaque and hold the mask value).
fn anchor_gray(px: &[[f32; 4]], from: Option<ChannelKind>) -> Vec<f32> {
    match from {
        None => px.iter().map(|p| p[0]).collect(),
        Some(k) if k.components() == 1 => px.iter().map(|p| p[0] * p[3]).collect(),
        Some(_) => px.iter().map(|p| (p[0] * LUMA[0] + p[1] * LUMA[1] + p[2] * LUMA[2]) * p[3]).collect(),
    }
}

/// Convert an anchor's `(value, alpha)` texels from channel `from` (`None`: a mask anchor) to
/// channel `to`: same component count as is, gray to color broadcast, color to gray by luminance.
fn anchor_as_channel(px: &mut [[f32; 4]], from: Option<ChannelKind>, to: ChannelKind) {
    let from_comps = from.map_or(1, |k| k.components());
    if from_comps == 3 && to.components() == 1 {
        for p in px.iter_mut() {
            let l = p[0] * LUMA[0] + p[1] * LUMA[1] + p[2] * LUMA[2];
            *p = [l, l, l, p[3]];
        }
    } else if from_comps == 1 {
        for p in px.iter_mut() {
            *p = [p[0], p[0], p[0], p[3]];
        }
    }
}

/// Whether evaluation needs the stage graph: neighborhood filters, micro effects or anchor
/// references (ADR-0045).
pub(crate) fn needs_graph(layers: &[Arc<Layer>]) -> bool {
    has_spatial(layers) || subcraft_doc::anchor::has_references(layers) || has_compare(layers) || crate::material::reads_across(layers)
}

/// The Compare Mask effects of visible layers that read other layers (anywhere in the tree).
fn compares(layers: &[Arc<Layer>], out: &mut Vec<CompareMask>) {
    for l in layers.iter().filter(|l| l.visible) {
        if let Some(m) = l.mask.as_ref().filter(|m| m.enabled) {
            for e in m.effects.iter().filter(|e| e.visible && e.opacity > 0.0) {
                if let MaskEffectKind::Compare(c) = &e.kind
                    && c.reads_layers()
                {
                    out.push(*c);
                }
            }
        }
        if let LayerContent::Group(g) = &l.content {
            compares(&g.children, out);
        }
    }
}

/// Whether a Compare Mask reads the layer stack: the stage graph (and the CPU) must evaluate it.
pub(crate) fn has_compare(layers: &[Arc<Layer>]) -> bool {
    let mut out = Vec::new();
    compares(layers, &mut out);
    !out.is_empty()
}

/// The channels Compare Masks read.
fn compared_channels(layers: &[Arc<Layer>]) -> Vec<ChannelKind> {
    let mut out = Vec::new();
    compares(layers, &mut out);
    let mut kinds: Vec<ChannelKind> = out.iter().map(|c| c.channel).collect();
    // Micro details read the Normal and Height below their layers in every other channel.
    if has_micro_details(layers) {
        kinds.extend([ChannelKind::Normal, ChannelKind::Height]);
    }
    // PBR Validate reads the Metallic below its layer for the Base Color.
    if crate::material::reads_across(layers) {
        kinds.push(ChannelKind::Metallic);
    }
    kinds.sort();
    kinds.dedup();
    kinds
}

/// One gray value per texel of a straight channel tile: the value of a scalar channel, the
/// luminance of a colour one.
fn channel_gray(px: &[[f32; 4]], kind: ChannelKind) -> Vec<f32> {
    if kind.components() >= 3 {
        px.iter().map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).collect()
    } else {
        px.iter().map(|p| p[0]).collect()
    }
}

pub(crate) fn mask_spatial(mask: &Mask) -> bool {
    mask.effects
        .iter()
        .any(|e| e.visible && e.opacity > 0.0 && (matches!(e.kind,MaskEffectKind::Filter(f) if f.valid() && f.supports_mask() && f.radius().is_some())
            || matches!(&e.kind, MaskEffectKind::Generator(g) if g.editor_blur() > 0.0)))
}
/// The layers from the top-level one down to the layer `id` (inclusive), or `None` when it is
/// not in `layers`.
fn ancestry(layers: &[Arc<Layer>], id: LayerId) -> Option<Vec<&Layer>> {
    for l in layers {
        if l.id == id {
            return Some(vec![l]);
        }
        if let LayerContent::Group(g) = &l.content
            && let Some(mut path) = ancestry(&g.children, id)
        {
            path.insert(0, l);
            return Some(path);
        }
    }
    None
}

/// Curvature gain of micro details at 2048: a 45 degree normal-map bevel one texel wide reaches
/// full convexity.
const MICRO_CURVATURE_GAIN: f32 = 1.5;
/// Occlusion of micro details per height unit a texel lies below its surroundings, at AO Depth 1.
const MICRO_OCCLUSION_GAIN: f32 = 12.0;

/// Micro details of one tile: `[curvature (0.5 flat), occlusion (1 open), 0, 1]` per texel.
/// `sample(x, y)` gives the encoded tangent-space normal and the height below at a texel (clamped
/// to the texture); `ao_r` is the occlusion radius in texels.
fn micro_detail_tile(settings: &subcraft_doc::MicroDetails, res: u32, ao_r: f32, geo: &subcraft_materials::TileGeometry, sample: impl Fn(i32, i32) -> ([f32; 3], f32)) -> Vec<[f32; 4]> {
    use glam::{Vec2, Vec3};
    let t = TILE_SIZE as i32;
    let pad = ao_r.ceil() as i32 + 2;
    let side = t + 2 * pad;
    let (ox, oy) = (geo.x0 as i32 - pad, geo.y0 as i32 - pad);
    let at = |x: i32, y: i32| (y * side + x) as usize;
    let len = (side * side) as usize;
    let (mut nb, mut hb) = (vec![Vec3::Z; len], vec![0.0f32; len]);
    for y in 0..side {
        for x in 0..side {
            let (n, h) = sample(ox + x, oy + y);
            nb[at(x, y)] = Vec3::new(n[0] * 2.0 - 1.0, n[1] * 2.0 - 1.0, n[2] * 2.0 - 1.0);
            hb[at(x, y)] = if h.is_finite() { h } else { 0.0 };
        }
    }
    // The micro normal: the normal below, the height's slope whiteout-blended on top of it.
    let gain = res as f32 * MICRO_NORMAL_GAIN * settings.height_intensity.clamp(0.0, 4.0);
    let use_height = settings.micro_height && gain > 0.0;
    let mut cn = vec![Vec3::Z; len];
    for y in 1..side - 1 {
        for x in 1..side - 1 {
            let mut n = if settings.micro_normal { nb[at(x, y)].normalize_or(Vec3::Z) } else { Vec3::Z };
            if use_height {
                let gx = (hb[at(x + 1, y)] - hb[at(x - 1, y)]) * 0.5;
                let gy = (hb[at(x, y + 1)] - hb[at(x, y - 1)]) * 0.5;
                if gx != 0.0 || gy != 0.0 {
                    // OpenGL normal map, texel y grows downwards (as the micro effect).
                    let hn = Vec3::new(-gx * gain, gy * gain, 1.0).normalize();
                    n = Vec3::new(n.x + hn.x, n.y + hn.y, n.z * hn.z).normalize_or(Vec3::Z);
                }
            }
            cn[at(x, y)] = n;
        }
    }
    let k = MICRO_CURVATURE_GAIN * settings.curvature_intensity.clamp(0.0, 4.0) * res as f32 / 2048.0;
    let depth = settings.ao_depth.clamp(0.0, 1.0);
    // Occlusion taps: eight directions at the full and half radius.
    let taps: Vec<(i32, i32)> = (0..16)
        .map(|i| {
            let angle = (i % 8) as f32 * std::f32::consts::FRAC_PI_4 + if i >= 8 { std::f32::consts::FRAC_PI_8 } else { 0.0 };
            let r = if i >= 8 { ao_r * 0.5 } else { ao_r };
            let d = Vec2::from_angle(angle) * r;
            (d.x.round() as i32, d.y.round() as i32)
        })
        .collect();
    let mut out = vec![[0.5, 1.0, 0.0, 1.0]; TILE_TEXELS];
    for (i, o) in out.iter_mut().enumerate() {
        let (tx, ty) = geo.texel_xy(i);
        if tx >= res || ty >= res {
            continue;
        }
        let (x, y) = (tx as i32 - ox, ty as i32 - oy);
        // Divergence of the normal's slope: positive on ridges (convex), negative in grooves.
        let div = match settings.curvature_type {
            subcraft_doc::MicroCurvatureType::Standard => (cn[at(x + 1, y)].x - cn[at(x - 1, y)].x) * 0.5 - (cn[at(x, y + 1)].y - cn[at(x, y - 1)].y) * 0.5,
            subcraft_doc::MicroCurvatureType::Sobel => {
                let dx = |dy: i32| cn[at(x + 1, y + dy)].x - cn[at(x - 1, y + dy)].x;
                let dy = |dx: i32| cn[at(x + dx, y + 1)].y - cn[at(x + dx, y - 1)].y;
                (dx(-1) + 2.0 * dx(0) + dx(1)) / 8.0 - (dy(-1) + 2.0 * dy(0) + dy(1)) / 8.0
            }
            subcraft_doc::MicroCurvatureType::Smooth => {
                let mut d = 0.0;
                for j in -1..=1 {
                    d += (cn[at(x + 2, y + j)].x - cn[at(x - 2, y + j)].x) * 0.25 - (cn[at(x + j, y + 2)].y - cn[at(x + j, y - 2)].y) * 0.25;
                }
                d / 3.0
            }
        };
        let curvature = (0.5 + div * k).clamp(0.0, 1.0);
        // Height occlusion: how far the texel lies below its surroundings.
        let open_height = if settings.micro_height && depth > 0.0 {
            let h = hb[at(x, y)];
            let avg = taps.iter().map(|&(dx, dy)| hb[at(x + dx, y + dy)]).sum::<f32>() / taps.len() as f32;
            1.0 - ((avg - h) * MICRO_OCCLUSION_GAIN * depth).clamp(0.0, 1.0)
        } else {
            1.0
        };
        // Grooves of the micro normal hold dirt too, with or without a height map.
        let concave = ((0.5 - curvature) * 2.0).clamp(0.0, 1.0);
        let occlusion = open_height * (1.0 - concave * depth);
        *o = [curvature, occlusion, 0.0, 1.0];
    }
    out
}

/// Texels of rise per height unit and texel of resolution, for the horizon scan: five times the
/// viewport's bump scale (0.02), so relief that reads clearly when lit also occludes clearly.
const HORIZON_HEIGHT_GAIN: f32 = 0.1;
const HORIZON_DIRECTIONS: usize = 12;
const HORIZON_STEPS: usize = 8;

/// Horizon-based occlusion of a height field for one tile: per texel 1 (open) down to 0. In
/// [`HORIZON_DIRECTIONS`] directions it walks out to `radius` texels and keeps the steepest rise
/// it meets, faded with distance; the occlusion is the mean sine of those horizon angles times
/// twice `depth`. `height(x, y)` is the height at a texel (clamped to the texture).
fn horizon_occlusion_tile(res: u32, radius: f32, depth: f32, geo: &subcraft_materials::TileGeometry, height: impl Fn(i32, i32) -> f32) -> Vec<f32> {
    let t = TILE_SIZE as i32;
    let radius = radius.max(1.0);
    let pad = radius.ceil() as i32 + 1;
    let side = t + 2 * pad;
    let (ox, oy) = (geo.x0 as i32 - pad, geo.y0 as i32 - pad);
    let gain = res as f32 * HORIZON_HEIGHT_GAIN;
    let mut z = vec![0.0f32; (side * side) as usize];
    for y in 0..side {
        for x in 0..side {
            let h = height(ox + x, oy + y);
            z[(y * side + x) as usize] = if h.is_finite() { h * gain } else { 0.0 };
        }
    }
    let mut out = vec![1.0f32; TILE_TEXELS];
    let (lo, hi) = z.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let strength = depth.clamp(0.0, 1.0) * 2.0;
    if hi - lo <= 1e-9 || strength <= 0.0 {
        return out;
    }
    // Bilinear, so the scan has no texel-grid pattern.
    let at = |x: f32, y: f32| -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let i = (y0 as i32 * side + x0 as i32) as usize;
        let s = side as usize;
        (z[i] * (1.0 - fx) + z[i + 1] * fx) * (1.0 - fy) + (z[i + s] * (1.0 - fx) + z[i + s + 1] * fx) * fy
    };
    let dirs: Vec<(f32, f32)> = (0..HORIZON_DIRECTIONS).map(|i| (i as f32 / HORIZON_DIRECTIONS as f32 * std::f32::consts::TAU).sin_cos()).collect();
    // Steps crowd near the texel, where the horizon changes fastest.
    let steps: Vec<f32> = (1..=HORIZON_STEPS).map(|k| (radius * (k as f32 / HORIZON_STEPS as f32).powf(1.5)).max(0.75)).collect();
    for (i, o) in out.iter_mut().enumerate() {
        let (tx, ty) = geo.texel_xy(i);
        if tx >= res || ty >= res {
            continue;
        }
        let (x, y) = ((tx as i32 - ox) as f32, (ty as i32 - oy) as f32);
        let z0 = z[((ty as i32 - oy) * side + (tx as i32 - ox)) as usize];
        let mut sum = 0.0;
        for &(dy, dx) in &dirs {
            let mut horizon = 0.0f32;
            for &d in &steps {
                let rise = at(x + dx * d, y + dy * d) - z0;
                // Above the bilinear read's rounding, so level ground stays exactly open.
                if rise > 1e-4 {
                    let fade = 1.0 - (d / radius) * (d / radius) * 0.75;
                    horizon = horizon.max(rise / (rise * rise + d * d).sqrt() * fade);
                }
            }
            sum += horizon;
        }
        *o = 1.0 - (sum / HORIZON_DIRECTIONS as f32 * strength).clamp(0.0, 1.0);
    }
    out
}

/// Whether a visible layer (anywhere in the tree) has a generator with micro details on.
pub(crate) fn has_micro_details(layers: &[Arc<Layer>]) -> bool {
    layers.iter().filter(|l| l.visible).any(|l| micro_details_support(l) > 0 || matches!(&l.content, LayerContent::Group(g) if has_micro_details(&g.children)))
}

/// Texels the micro details of a layer's own mask read beyond its own (0 without any).
fn micro_details_support(l: &Layer) -> u32 {
    // Material filters that read the detail below them (Detail Edge Wear).
    let content = l.content_effects.iter().filter(|e| e.visible).filter_map(|e| match e.kind {
        ContentEffectKind::Material(m) => Some(m.support()).filter(|s| *s > 0),
        _ => None,
    });
    let mask = l.mask.as_ref().filter(|m| m.enabled).into_iter().flat_map(|m| {
        m.effects.iter().filter(|e| e.visible && e.opacity > 0.0).filter_map(|e| match &e.kind {
            MaskEffectKind::Generator(g) => g.micro_details_active().map(|d| d.support()),
            _ => None,
        })
    });
    content.chain(mask).max().unwrap_or(0)
}

/// Slope per UV unit per (height unit per texel x resolution): how strongly a micro height change
/// leans the normal.
const MICRO_NORMAL_GAIN: f32 = 0.25;
/// Whether the layer's mask holds a generator whose micro effect is on.
pub(crate) fn micro_layer(l: &Layer) -> bool {
    l.mask.as_ref().filter(|m| m.enabled).is_some_and(|m| {
        m.effects.iter().any(|e| e.visible && e.opacity > 0.0 && matches!(&e.kind, MaskEffectKind::Generator(g) if g.micro_active()))
    })
}
pub(crate) fn has_spatial(layers: &[Arc<Layer>]) -> bool {
    layers.iter().filter(|l|l.visible).any(|l| {
        micro_layer(l) || micro_details_support(l) > 0 ||
        l.content_effects.iter().any(
            |e| e.visible && matches!(e.kind,ContentEffectKind::Filter{filter,..} if filter.valid() && filter.radius().is_some())
                && e.filter_channels().into_iter().any(|c| e.blend_for(c).is_active()),
        ) || l.mask.as_ref().filter(|m|m.enabled).is_some_and(mask_spatial)
            || matches!(&l.content,LayerContent::Group(g) if has_spatial(&g.children))
    })
}
/// Supports of the filters in a layer's own content and mask effects (not its children).
pub(crate) fn own_halo(l: &Layer) -> u32 {
    // The micro effect's normal reads one texel of its neighbours' height.
    let mut sum = (micro_layer(l) as u32).saturating_add(micro_details_support(l));
    for e in l.content_effects.iter().filter(|e| e.visible) {
        if let ContentEffectKind::Filter { filter, .. } = e.kind {
            sum = sum.saturating_add(filter.support());
        }
    }
    if let Some(m) = l.mask.as_ref().filter(|m| m.enabled) {
        for e in m.effects.iter().filter(|e| e.visible) {
            if let MaskEffectKind::Filter(f) = e.kind {
                sum = sum.saturating_add(f.support());
            }
            sum = sum.saturating_add(editor_support(&e.kind));
        }
    }
    sum
}
/// Texels a Mask Editor's global blur reads beyond its own (0 for anything else).
fn editor_support(kind: &MaskEffectKind) -> u32 {
    match kind {
        MaskEffectKind::Generator(g) => g.editor_blur().ceil() as u32,
        _ => 0,
    }
}
/// Conservative sum of supports along any possible dependency path, including mask domains.
pub(crate) fn halo(layers: &[Arc<Layer>]) -> u32 {
    let mut sum = 0u32;
    for l in layers.iter().filter(|l| l.visible) {
        sum = sum.saturating_add(micro_layer(l) as u32).saturating_add(micro_details_support(l));
        for e in l.content_effects.iter().filter(|e| e.visible) {
            if let ContentEffectKind::Filter { filter, .. } = e.kind {
                sum = sum.saturating_add(filter.support());
            }
        }
        if let Some(m) = l.mask.as_ref().filter(|m| m.enabled) {
            for e in m.effects.iter().filter(|e| e.visible) {
                if let MaskEffectKind::Filter(f) = e.kind {
                    sum = sum.saturating_add(f.support());
                }
                sum = sum.saturating_add(editor_support(&e.kind));
            }
        }
        if let LayerContent::Group(g) = &l.content {
            sum = sum.saturating_add(halo(&g.children));
        }
    }
    sum
}
fn context<'a>(tc: &'a TileContext<'a>) -> EvalContext<'a> {
    EvalContext {
        geometry: tc.set_geometry,
        resources: tc.sample.resources,
        mesh_maps: tc.sample.mesh_maps,
    }
}
fn index(tc: &TileContext) -> usize {
    (tc.geo.y0 / TILE_SIZE * tc.set_geometry.resolution.div_ceil(TILE_SIZE) + tc.geo.x0 / TILE_SIZE)
        as usize
}
pub(crate) fn mask_preview(mask: &Mask, tc: &TileContext, scale: f32) -> Vec<f32> {
    let cache = StageCache::default();
    let mut graph = Graph::empty(context(tc), tc.albedo_filter, &cache);
    graph.radius_scale = scale;
    let root = graph.mask(mask);
    graph
        .root(root, index(tc))
        .iter()
        .map(|v| v[0].clamp(0.0, 1.0))
        .collect()
}
pub(crate) fn content_preview(
    layer: &Layer,
    kind: ChannelKind,
    tc: &TileContext,
    scale: f32,
) -> Vec<[f32; 4]> {
    let cache = StageCache::default();
    let mut graph = Graph::empty(context(tc), tc.albedo_filter, &cache);
    graph.radius_scale = scale;
    let root = graph.push(Node::Constant([0.0; 4]));
    let root = graph.layer(layer, root, kind);
    graph.root(root, index(tc)).as_ref().clone()
}

fn uniform_fill(fill: &subcraft_doc::FillContent, kind: ChannelKind) -> bool {
    fill.channels
        .get(&kind)
        .is_none_or(|c| !c.enabled || matches!(c.source, subcraft_doc::FillSource::Uniform { .. }))
}

/// Find the accumulation immediately below a target, honoring ancestor isolation. Parent
/// masks/effects run after that target and therefore do not belong in its stage preview.
fn before<'a>(
    graph: &mut Graph<'a>,
    layers: &'a [Arc<Layer>],
    target: LayerId,
    mut below: usize,
    kind: ChannelKind,
) -> Option<usize> {
    for layer in layers {
        if layer.id == target {
            return Some(below);
        }
        if let LayerContent::Group(group) = &layer.content {
            let contains = group
                .children
                .iter()
                .any(|l| l.id == target || contains_layer(l, target));
            if contains {
                let base = if layer.blend_for(kind).mode == BlendMode::Passthrough {
                    below
                } else {
                    graph.push(Node::Constant([0.0; 4]))
                };
                return before(graph, &group.children, target, base, kind);
            }
        }
        below = graph.layer(layer, below, kind);
    }
    None
}
fn contains_layer(layer: &Layer, target: LayerId) -> bool {
    matches!(&layer.content,LayerContent::Group(g) if g.children.iter().any(|l|l.id==target || contains_layer(l,target)))
}
pub(crate) fn content_preview_in_stack(
    layer: &Layer,
    kind: ChannelKind,
    tc: &TileContext,
    scale: f32,
    stack: &[Arc<Layer>],
) -> Vec<[f32; 4]> {
    let cache = StageCache::default();
    let mut graph = Graph::empty(context(tc), tc.albedo_filter, &cache);
    graph.radius_scale = scale;
    let d = kind.default_value().0;
    let base = graph.push(Node::Constant([d[0], d[1], d[2], 1.0]));
    let below = if layer.blend_for(kind).mode == BlendMode::Passthrough {
        before(&mut graph, stack, layer.id, base, kind).unwrap_or(base)
    } else {
        graph.push(Node::Constant([0.0; 4]))
    };
    let root = graph.layer(layer, below, kind);
    graph.root(root, index(tc)).as_ref().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rayon::prelude::*;
    /// Micro details from a normal map alone: a dome (normals leaning outward) is convex, a bowl
    /// (normals leaning inward) concave and occluded; a flat normal gives neutral values.
    #[test]
    fn horizon_occlusion_is_smooth_darkest_at_the_foot_of_a_wall_and_one_on_flat_ground() {
        let tile = subcraft_materials::TileGeometry::empty(0, 0, 0, 64);
        let flat = horizon_occlusion_tile(64, 8.0, 1.0, &tile, |_, _| 0.3);
        assert!(flat.iter().all(|v| *v == 1.0));
        // A step up at x = 32: the low side is occluded near the wall, the top stays open.
        let step = horizon_occlusion_tile(64, 8.0, 1.0, &tile, |x, _| if x >= 32 { 1.0 } else { 0.0 });
        let row = |x: usize| step[32 * 64 + x];
        assert!(row(31) < 0.6 && row(31) < row(28) && row(28) < row(25), "{} {} {}", row(31), row(28), row(25));
        assert_eq!((row(10), row(40), row(33)), (1.0, 1.0, 1.0));
        // A wide smooth bowl is occluded all over, with no tap pattern (neighbouring texels
        // never differ by much).
        let bowl = |x: i32, y: i32| {
            let (dx, dy) = ((x - 32) as f32 / 32.0, (y - 32) as f32 / 32.0);
            (dx * dx + dy * dy).min(1.0) * 0.5
        };
        let ao = horizon_occlusion_tile(64, 8.0, 1.0, &tile, bowl);
        let v = |x: usize, y: usize| ao[y * 64 + x];
        assert!(v(32, 32) < 0.99 && v(48, 32) < 0.99, "{} {}", v(32, 32), v(48, 32));
        let mut worst = 0.0f32;
        // Well inside the bowl: its rim is a real crease, where the occlusion may step.
        for y in 18..46 {
            for x in 18..46 {
                worst = worst.max((v(x, y) - v(x + 1, y)).abs()).max((v(x, y) - v(x, y + 1)).abs());
            }
        }
        assert!(worst < 0.03, "smooth: {worst}");
        // Depth scales it; zero switches it off.
        assert!(horizon_occlusion_tile(64, 8.0, 0.0, &tile, bowl).iter().all(|v| *v == 1.0));
        let half = horizon_occlusion_tile(64, 8.0, 0.5, &tile, |x, _| if x >= 32 { 1.0 } else { 0.0 });
        assert!(half[32 * 64 + 31] > row(31));
    }

    #[test]
    fn micro_details_measure_curvature_from_the_normal_below() {
        let geo = subcraft_materials::TextureSetGeometry::new(std::sync::Arc::new(subcraft_mesh::primitives::plane(2.0, 1)), 0, 64);
        let tile = geo.tile(0);
        let settings = subcraft_doc::MicroDetails { enabled: true, micro_height: false, ..Default::default() };
        // Slope field of a dome (s = 1) or bowl (s = -1) centred at texel 32: OpenGL normal, texel y down.
        let field = |s: f32| move |x: i32, y: i32| {
            let (dx, dy) = ((x as f32 - 32.0) / 24.0, (y as f32 - 32.0) / 24.0);
            let n = glam::Vec3::new(s * dx, -s * dy, 1.0).normalize();
            ([n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5], 0.0)
        };
        let centre = 32 * 64 + 32;
        for kind in subcraft_doc::MicroCurvatureType::ALL {
            let s = subcraft_doc::MicroDetails { curvature_type: kind, ..settings };
            let dome = micro_detail_tile(&s, 64, 2.0, &tile, field(1.0));
            let bowl = micro_detail_tile(&s, 64, 2.0, &tile, field(-1.0));
            let flat = micro_detail_tile(&s, 64, 2.0, &tile, |_, _| ([0.5, 0.5, 1.0], 0.0));
            assert!(dome[centre][0] > 0.5, "{kind:?} dome {:?}", dome[centre]);
            assert!(bowl[centre][0] < 0.5 && bowl[centre][1] < 1.0, "{kind:?} bowl {:?}", bowl[centre]);
            assert!((dome[centre][0] - 0.5 + bowl[centre][0] - 0.5).abs() < 1e-4, "{kind:?} symmetric");
            assert!(flat.iter().all(|p| (p[0] - 0.5).abs() < 1e-6 && p[1] == 1.0), "{kind:?} flat");
        }
        // Micro Normal off: the normal below is ignored.
        let off = subcraft_doc::MicroDetails { micro_normal: false, ..settings };
        assert!((micro_detail_tile(&off, 64, 2.0, &tile, field(1.0))[centre][0] - 0.5).abs() < 1e-6);
    }
    #[test]
    fn eviction_and_parallel_neighbor_requests_preserve_exact_results() {
        let mut ts = TextureSet::new("Cache", 0, 128);
        ts.channels.retain(|c| c.kind == ChannelKind::Roughness);
        let mut layer = Layer::new_paint("P", &ts);
        let LayerContent::Paint(p) = &mut layer.content else {
            unreachable!()
        };
        for y in 0..128 {
            for x in 0..128 {
                p.channel_mut(ChannelKind::Roughness, 128).set(
                    x,
                    y,
                    [((x + y) % 7) as f32 / 8.0, 1.0, 0.0, 0.0],
                );
            }
        }
        layer.content_effects.push(Arc::new(ContentEffect::new(
            ContentEffectKind::Filter {
                channel: ChannelKind::Roughness,
                filter: Filter::Blur { radius: 4.0 },
            },
            &ts,
        )));
        ts.insert_above(None, layer);
        let geometry = subcraft_materials::TextureSetGeometry::new(
            Arc::new(subcraft_mesh::primitives::plane(2.0, 1)),
            0,
            128,
        );
        let resources = subcraft_doc::ResourceStore::default();
        let maps = subcraft_materials::MeshMaps::new();
        let ctx = EvalContext {
            geometry: &geometry,
            resources: &resources,
            mesh_maps: &maps,
        };
        for filter in [
            Filter::Bevel {
                radius: 8.0,
                amount: 0.4,
                threshold: 0.5,
            },
            Filter::MaskOutline {
                radius: 8.0,
                threshold: 0.5,
                position: subcraft_doc::OutlinePosition::Both,
            },
            Filter::Blur { radius: 4.0 },
            Filter::DirectionalBlur {
                radius: 4.0,
                angle_deg: 35.0,
            },
            Filter::Highpass {
                radius: 4.0,
                amount: 1.0,
            },
        ] {
            let layer = Arc::make_mut(&mut ts.layers[0]);
            Arc::make_mut(&mut layer.content_effects[0]).kind = ContentEffectKind::Filter {
                channel: ChannelKind::Roughness,
                filter,
            };
            let large = StageCache::default();
            let reference = Graph::new(&ts, ctx, &large);
            let expected: Vec<_> = (0..4).map(|t| reference.tile(t)[0].1.clone()).collect();
            let tiny = StageCache::new(2);
            let graph = Graph::new(&ts, ctx, &tiny);
            (0..12usize).into_par_iter().for_each(|i| {
                assert_eq!(graph.tile(i % 4)[0].1, expected[i % 4]);
            });
            tiny.trim();
            assert!(tiny.entries.lock().unwrap().cells.len() <= 2);
        }
    }
}
