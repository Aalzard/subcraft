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
    BlendMode, ContentEffect, ContentEffectKind, Filter, Generator, Layer, LayerContent, Mask, MaskEffect,
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
    },
    /// Edge Detect / Directional Warp / Mosaic: direct per-texel pixel reads (filter already
    /// scaled to this texture's resolution).
    Pixelwise {
        input: usize,
        filter: Filter,
        kind: ChannelKind,
        support: u32,
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
    },
    /// Micro effect: the signed height change `generator`'s mask (the running mask right after the
    /// generator effect) writes. Read by [`Node::MicroHeight`] and, through its neighbours, by
    /// [`Node::MicroNormal`]; the mask is evaluated once and shared, never recomputed.
    MicroDelta {
        mask: usize,
        generator: &'a Generator,
        opacity: f32,
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
            Self::Layer { .. } => "layer blend",
            Self::Directional { .. } => "directional blur",
            Self::Median { .. } => "median",
            Self::Warp { .. } => "warp",
            Self::Pixelwise { .. } => "pixelwise filter",
            Self::Contour { .. } => "contour",
            Self::Gaussian { .. } => "gaussian pass",
            Self::MicroDelta { .. } => "micro delta",
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
}

/// Cached accumulations at chosen top-level boundaries (see [`crate::prefix`]).
struct Prefix<'a> {
    cache: &'a crate::prefix::PrefixCache,
    /// Node -> (channel, boundary layer index) for the planned boundaries.
    marks: HashMap<usize, (ChannelKind, usize)>,
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
        }
    }
    pub(crate) fn new(ts: &'a TextureSet, ctx: EvalContext<'a>, cache: &'a StageCache) -> Self {
        Self::new_scaled(ts, ctx, cache, 1.0)
    }
    /// A graph evaluated at `scale` times the authored resolution (quick previews): spatial
    /// radii shrink with it.
    pub(crate) fn new_scaled(ts: &'a TextureSet, ctx: EvalContext<'a>, cache: &'a StageCache, scale: f32) -> Self {
        let mut g = Self::empty(ctx, ts.albedo_filter, cache);
        g.radius_scale = scale;
        for kind in ts.channel_kinds() {
            let d = kind.default_value().0;
            let root = g.push(Node::Constant([d[0], d[1], d[2], 1.0]));
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
        self
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
            | Node::Pixelwise { input, .. }
            | Node::Contour { input, .. } => self.uniform[*input],
            Node::ContentEffect {
                input,
                effect,
                kind,
                ..
            } => {
                self.uniform[*input]
                    && match &effect.kind {
                        ContentEffectKind::Fill(f) => uniform_fill(f, *kind),
                        ContentEffectKind::Paint { channels } => {
                            channels.get(kind).is_none_or(|i| i.allocated_tiles() == 0)
                        }
                        _ => true,
                    }
            }
            Node::MaskEffect { input, effect, .. } => {
                self.uniform[*input]
                    && match &effect.kind {
                        MaskEffectKind::Paint { image } => image.allocated_tiles() == 0,
                        MaskEffectKind::Fill { procedural, .. } => procedural.is_none(),
                        MaskEffectKind::Generator(_) | MaskEffectKind::Image { .. } | MaskEffectKind::IdColor(_) => false,
                        _ => true,
                    }
            }
            Node::MicroDelta { .. } | Node::MicroHeight { .. } | Node::MicroNormal { .. } => false,
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
    fn neighborhood(&mut self, input: usize, filter: Filter, kind: ChannelKind) -> Option<usize> {
        let radius = filter.radius()?;
        if !filter.valid() {
            return None;
        }
        if filter.is_contour() {
            return Some(self.push(Node::Contour {
                input,
                filter,
                kind,
                radius: radius * self.radius_scale,
            }));
        }
        if self.uniform[input]
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
            return Some(self.push(Node::Pixelwise { input, filter, kind, support: filter.support() }));
        }
        if let Filter::Warp { scale, seed, .. } = filter {
            let intensity = radius * self.radius_scale;
            return Some(self.push(Node::Warp {
                input,
                intensity,
                scale,
                seed,
                support: intensity.ceil() as u32 + 1,
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
        let h = self.push(Node::Gaussian {
            input,
            weights: weights.clone(),
            horizontal: true,
        });
        Some(self.push(Node::Gaussian {
            input: h,
            weights,
            horizontal: false,
        }))
    }
    /// The layer's mask node, built once per layer together with its micro-effect taps.
    fn layer_mask(&mut self, layer: &'a Layer, mask: &'a Mask) -> usize {
        if let Some(&node) = self.masks.get(&layer.id) {
            return node;
        }
        let (node, micro) = self.mask_with_micro(mask);
        self.masks.insert(layer.id, node);
        if !micro.is_empty() {
            self.micro_masks.insert(layer.id, micro);
        }
        node
    }
    fn mask(&mut self, mask: &'a Mask) -> usize {
        self.mask_with_micro(mask).0
    }
    fn mask_with_micro(&mut self, mask: &'a Mask) -> (usize, Vec<(usize, &'a Generator)>) {
        let mut micro = Vec::new();
        let mut input = self.push(Node::MaskBase(&mask.base));
        for effect in mask.effects.iter().filter(|e| e.visible && e.opacity > 0.0) {
            let filtered = if let MaskEffectKind::Filter(f) = effect.kind {
                self.neighborhood(input, f, ChannelKind::Roughness)
            } else {
                None
            };
            input = self.push(Node::MaskEffect {
                input,
                effect,
                filtered,
            });
            if let MaskEffectKind::Generator(g) = &effect.kind
                && g.micro_active()
            {
                micro.push((input, g));
            }
        }
        (input, micro)
    }
    fn stack(&mut self, layers: &'a [Arc<Layer>], mut below: usize, kind: ChannelKind) -> usize {
        for layer in layers {
            below = self.layer(layer, below, kind);
        }
        below
    }
    fn layer(&mut self, layer: &'a Layer, below: usize, kind: ChannelKind) -> usize {
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
        let passthrough = matches!(layer.content, LayerContent::Group(_))
            && layer.blend_for(kind).mode == BlendMode::Passthrough;
        let mut input = match &layer.content {
            LayerContent::Paint(p) => {
                if let Some(image) = p.channels.get(&kind) {
                    self.push(Node::Paint(image, kind))
                } else {
                    self.push(Node::Constant([0.0; 4]))
                }
            }
            LayerContent::Fill(f) => self.push(Node::Fill(f, kind)),
            LayerContent::Group(group) => {
                let base = if passthrough {
                    below
                } else {
                    self.push(Node::Constant([0.0; 4]))
                };
                self.stack(&group.children, base, kind)
            }
        };
        for effect in layer
            .content_effects
            .iter()
            .filter(|e| e.visible && e.blend_for(kind).is_active())
        {
            if matches!(effect.kind, ContentEffectKind::Filter {channel,..} | ContentEffectKind::Levels {channel,..} if channel != kind)
            {
                continue;
            }
            let filtered = if let ContentEffectKind::Filter { filter, .. } = effect.kind {
                if !filter.valid() || !filter.supports_channel(kind) {
                    continue;
                }
                self.neighborhood(input, filter, kind)
            } else {
                None
            };
            input = self.push(Node::ContentEffect {
                input,
                effect,
                kind,
                filtered,
            });
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
                    Node::Constant(_) | Node::Paint(..) | Node::Fill(..) | Node::MaskBase(_) => {}
                    Node::MaskEffect { input, filtered, .. } | Node::ContentEffect { input, filtered, .. } => {
                        read(*input);
                        if let Some(f) = filtered {
                            read(*f);
                        }
                    }
                    Node::Layer { below, input, mask, .. } => {
                        read(*below);
                        read(*input);
                        if let Some(m) = mask {
                            read(*m);
                        }
                    }
                    Node::Directional { input, .. }
                    | Node::Median { input, .. }
                    | Node::Warp { input, .. }
                    | Node::Pixelwise { input, .. }
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
            Node::MaskEffect {
                input,
                effect,
                filtered,
            } => {
                let source = self.get(*input, tile);
                let mut e = vec![[0.0; 4]; TILE_TEXELS];
                if let Some(filtered) = filtered {
                    let blurred = self.get(*filtered, tile);
                    let MaskEffectKind::Filter(filter) = effect.kind else {
                        unreachable!()
                    };
                    for ((e, src), b) in e.iter_mut().zip(source.iter()).zip(blurred.iter()) {
                        *e = [
                            spatial_value(filter, *src, *b, ChannelKind::Roughness)[0],
                            1.0,
                            0.0,
                            0.0,
                        ];
                    }
                } else {
                    let m: Vec<_> = source.iter().map(|v| v[0]).collect();
                    crate::mask::evaluate_effect(&effect.kind, &tc, &m, &mut e);
                }
                let mut out = source.as_ref().clone();
                for (dst, e) in out.iter_mut().zip(e) {
                    composite(
                        dst,
                        [e[0]; 3],
                        e[1] * effect.opacity,
                        effect.blend,
                        BlendTarget::MASK,
                    );
                }
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
                let mut coverage = vec![1.0; TILE_TEXELS];
                for (i, m) in coverage.iter_mut().enumerate() {
                    if let Some(mask) = &mask {
                        *m = mask[i][0].clamp(0.0, 1.0);
                    }
                    if let Some(parts) = layer.parts.as_ref().filter(|p| p.enabled) {
                        *m *= parts.factor(geo.region(i, parts.kind));
                    }
                }
                if coverage.iter().all(|&m| m <= 0.0) {
                    return out;
                }
                let content = self.get(*input, tile);
                let blend = layer.blend_for(*kind);
                let mode = effective_mode(blend.mode);
                for ((dst, src), m) in out.iter_mut().zip(content.iter()).zip(coverage) {
                    if *passthrough {
                        let f = (blend.opacity * m).clamp(0.0, 1.0);
                        for c in 0..4 {
                            dst[c] += (src[c] - dst[c]) * f;
                        }
                    } else {
                        let a = if mode == BlendMode::Replace {
                            1.0
                        } else {
                            src[3]
                        };
                        composite(
                            dst,
                            [src[0], src[1], src[2]],
                            a * blend.opacity * m,
                            mode,
                            BlendTarget::for_channel(*kind),
                        );
                    }
                }
                out
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
                    // Fine grain pits the surface: 0.55..1 of the full change.
                    let g = grain.as_ref().map_or(1.0, |f| 0.55 + 0.45 * ((f[i] - 0.5) * 2.5 + 0.5).clamp(0.0, 1.0));
                    let d = amount * m * g;
                    *o = [d, d, d, 1.0];
                }
                out
            }
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
                    Filter::Bevel { threshold, .. } | Filter::MaskOutline { threshold, .. } => {
                        *threshold
                    }
                    _ => unreachable!(),
                };
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
                let inner = (!matches!(
                    filter,
                    Filter::MaskOutline {
                        position: subcraft_doc::OutlinePosition::Outer,
                        ..
                    }
                ))
                .then(|| contour::squared_distance(&classes, width, height, false));
                let outer = matches!(
                    filter,
                    Filter::MaskOutline {
                        position: subcraft_doc::OutlinePosition::Outer
                            | subcraft_doc::OutlinePosition::Both,
                        ..
                    }
                )
                .then(|| contour::squared_distance(&classes, width, height, true));
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
            } => {
                let grid = Neighborhood::new(self, *input, &geo, *support);
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
                    let src = grid.premultiplied(x as f32 + dx, y as f32 + dy);
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
            } => {
                use subcraft_materials::filter::pixelwise as pw;
                let grid = Neighborhood::new(self, *input, &geo, *support);
                let res = self.ctx.geometry.resolution;
                let mut out = vec![[0.0; 4]; TILE_TEXELS];
                for (i, dst) in out.iter_mut().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    if x >= res || y >= res {
                        continue;
                    }
                    let here = grid.pixel(x, y);
                    *dst = match *filter {
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
                let first = (lo - r).clamp(0, res - 1) as usize / TILE_SIZE as usize;
                let last = (hi + r).clamp(0, res - 1) as usize / TILE_SIZE as usize;
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
                let mut out = vec![[0.0; 4]; TILE_TEXELS];
                for (i, dst) in out.iter_mut().enumerate() {
                    let (x, y) = geo.texel_xy(i);
                    for (w, &weight) in weights.iter().enumerate().filter(|(_, w)| **w > 0.0) {
                        let position = (if *horizontal { x as i32 } else { y as i32 } + w as i32
                            - r)
                            .clamp(0, res - 1) as usize;
                        let local = if *horizontal {
                            (y % TILE_SIZE) as usize * TILE_SIZE as usize
                                + position % TILE_SIZE as usize
                        } else {
                            position % TILE_SIZE as usize * TILE_SIZE as usize
                                + (x % TILE_SIZE) as usize
                        };
                        let src = neighbors[position / TILE_SIZE as usize - first][local];
                        for c in 0..3 {
                            dst[c] += src[c] * if *horizontal { src[3] } else { 1.0 } * weight;
                        }
                        dst[3] += src[3] * weight;
                    }
                    if !horizontal && dst[3] > 1e-12 {
                        let a = dst[3];
                        for v in &mut dst[..3] {
                            *v /= a;
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
            .map(|&(k, n)| (k, self.get(n, tile)))
            .collect();
        self.scratch_clear();
        out
    }
}

/// Preload a bounded rectangle once; no cache locks or graph traversal inside bilinear taps.
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

fn spatial_value(
    filter: Filter,
    source: [f32; 4],
    blurred: [f32; 4],
    kind: ChannelKind,
) -> [f32; 4] {
    match filter {
        Filter::Bevel { .. } | Filter::MaskOutline { .. } | Filter::Median { .. } => blurred,
        Filter::Blur { radius: 0.0 }
        | Filter::Sharpen { radius: 0.0, .. }
        | Filter::Sharpen { amount: 0.0, .. }
        | Filter::DirectionalBlur { radius: 0.0, .. } => source,
        Filter::Blur { .. } | Filter::DirectionalBlur { .. } | Filter::Warp { .. }
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
pub(crate) fn mask_spatial(mask: &Mask) -> bool {
    mask.effects
        .iter()
        .any(|e| e.visible && e.opacity > 0.0 && matches!(e.kind,MaskEffectKind::Filter(f) if f.valid() && f.supports_mask() && f.radius().is_some()))
}
/// Slope per UV unit per (height unit per texel x resolution): how strongly a micro height change
/// leans the normal.
const MICRO_NORMAL_GAIN: f32 = 0.35;
/// Whether the layer's mask holds a generator whose micro effect is on.
pub(crate) fn micro_layer(l: &Layer) -> bool {
    l.mask.as_ref().filter(|m| m.enabled).is_some_and(|m| {
        m.effects.iter().any(|e| e.visible && e.opacity > 0.0 && matches!(&e.kind, MaskEffectKind::Generator(g) if g.micro_active()))
    })
}
pub(crate) fn has_spatial(layers: &[Arc<Layer>]) -> bool {
    layers.iter().filter(|l|l.visible).any(|l| {
        micro_layer(l) ||
        l.content_effects.iter().any(
            |e| e.visible && matches!(e.kind,ContentEffectKind::Filter{channel,filter} if filter.valid() && filter.supports_channel(channel) && e.blend_for(channel).is_active() && filter.radius().is_some()),
        ) || l.mask.as_ref().filter(|m|m.enabled).is_some_and(mask_spatial)
            || matches!(&l.content,LayerContent::Group(g) if has_spatial(&g.children))
    })
}
/// Supports of the filters in a layer's own content and mask effects (not its children).
pub(crate) fn own_halo(l: &Layer) -> u32 {
    // The micro effect's normal reads one texel of its neighbours' height.
    let mut sum = micro_layer(l) as u32;
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
        }
    }
    sum
}
/// Conservative sum of supports along any possible dependency path, including mask domains.
pub(crate) fn halo(layers: &[Arc<Layer>]) -> u32 {
    let mut sum = 0u32;
    for l in layers.iter().filter(|l| l.visible) {
        sum = sum.saturating_add(micro_layer(l) as u32);
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
