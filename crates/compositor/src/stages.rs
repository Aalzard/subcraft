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
    BlendMode, ContentEffect, ContentEffectKind, Filter, Layer, LayerContent, Mask, MaskEffect,
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
        Self::new(1024)
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
}
impl Node<'_> {
    fn primary(&self) -> Option<usize> {
        match self {
            Self::MaskEffect { input, .. }
            | Self::ContentEffect { input, .. }
            | Self::Gaussian { input, .. }
            | Self::Contour { input, .. }
            | Self::Directional { input, .. } => Some(*input),
            Self::Layer { below, .. } => Some(*below),
            _ => None,
        }
    }
}

pub(crate) struct Graph<'a> {
    nodes: Vec<Node<'a>>,
    uniform: Vec<bool>,
    masks: HashMap<LayerId, usize>,
    roots: Vec<(ChannelKind, usize)>,
    radius_scale: f32,
    ctx: EvalContext<'a>,
    albedo: subcraft_image::TextureFilter,
    cache: &'a StageCache,
}
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
            roots: Vec::new(),
            ctx,
            albedo,
            cache,
        }
    }
    pub(crate) fn new(ts: &'a TextureSet, ctx: EvalContext<'a>, cache: &'a StageCache) -> Self {
        let mut g = Self::empty(ctx, ts.albedo_filter, cache);
        for kind in ts.channel_kinds() {
            let d = kind.default_value().0;
            let root = g.push(Node::Constant([d[0], d[1], d[2], 1.0]));
            let root = g.stack(&ts.layers, root, kind);
            g.roots.push((kind, root));
        }
        g
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
                        MaskEffectKind::Generator(_) | MaskEffectKind::Image { .. } => false,
                        _ => true,
                    }
            }
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
    fn mask(&mut self, mask: &'a Mask) -> usize {
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
        }
        input
    }
    fn stack(&mut self, layers: &'a [Arc<Layer>], mut below: usize, kind: ChannelKind) -> usize {
        for layer in layers {
            below = self.layer(layer, below, kind);
        }
        below
    }
    fn layer(&mut self, layer: &'a Layer, below: usize, kind: ChannelKind) -> usize {
        if !layer.visible || !layer.blend_for(kind).is_active() {
            return below;
        }
        // Match the point evaluator's absent-source early return, including Replace mode.
        if layer.content_effects.is_empty()
            && match &layer.content {
                LayerContent::Paint(p) => !p.channels.contains_key(&kind),
                LayerContent::Fill(f) => !f.channels.get(&kind).is_some_and(|c| c.enabled),
                LayerContent::Group(_) => false,
            }
        {
            return below;
        }
        let mask = if let Some(m) = layer.mask.as_ref().filter(|m| m.enabled) {
            if let Some(&node) = self.masks.get(&layer.id) {
                Some(node)
            } else {
                let node = self.mask(m);
                self.masks.insert(layer.id, node);
                Some(node)
            }
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
    fn get(&self, node: usize, tile: usize) -> Pixels {
        if let Some(p) = self.cache.ready(self.key(node, tile)) {
            return p;
        }
        let mut chain = Vec::new();
        let mut at = node;
        let mut held = None;
        loop {
            if let Some(ready) = self.cache.ready(self.key(at, tile)) {
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
            let cell = self.cache.cell(self.key(id, tile));
            let result = cell
                .get_or_init(|| Arc::new(self.compute(id, tile)))
                .clone();
            held = Some(result);
        }
        held.unwrap_or_else(|| {
            self.cache
                .ready(self.key(node, tile))
                .expect("completed graph node")
        })
    }
    fn compute(&self, node: usize, tile: usize) -> Vec<[f32; 4]> {
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
        self.roots
            .iter()
            .map(|&(k, n)| (k, self.get(n, tile)))
            .collect()
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
        Filter::Bevel { .. } | Filter::MaskOutline { .. } => blurred,
        Filter::Blur { radius: 0.0 }
        | Filter::Sharpen { radius: 0.0, .. }
        | Filter::Sharpen { amount: 0.0, .. }
        | Filter::DirectionalBlur { radius: 0.0, .. } => source,
        Filter::Blur { .. } | Filter::DirectionalBlur { .. } => blurred,
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
pub(crate) fn has_spatial(layers: &[Arc<Layer>]) -> bool {
    layers.iter().filter(|l|l.visible).any(|l| {
        l.content_effects.iter().any(
            |e| e.visible && matches!(e.kind,ContentEffectKind::Filter{channel,filter} if filter.valid() && filter.supports_channel(channel) && e.blend_for(channel).is_active() && filter.radius().is_some()),
        ) || l.mask.as_ref().filter(|m|m.enabled).is_some_and(mask_spatial)
            || matches!(&l.content,LayerContent::Group(g) if has_spatial(&g.children))
    })
}
/// Conservative sum of supports along any possible dependency path, including mask domains.
pub(crate) fn halo(layers: &[Arc<Layer>]) -> u32 {
    let mut sum = 0u32;
    for l in layers.iter().filter(|l| l.visible) {
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
        .get(root, index(tc))
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
    graph.get(root, index(tc)).as_ref().clone()
}

fn uniform_fill(fill: &subcraft_doc::FillContent, kind: ChannelKind) -> bool {
    fill.channels
        .get(&kind)
        .is_none_or(|c| !c.enabled || matches!(c.source, subcraft_doc::FillSource::Uniform { .. }))
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
