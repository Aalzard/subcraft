//! Optional GPU compositing backend (ADR-0044).
//!
//! The CPU evaluator stays the reference and the fallback. This module (no wgpu here) does the
//! planning and the per-tile CPU preparation, and hands a [`GpuProgram`] plus per-tile sources to
//! a [`GpuCompositeBackend`] (implemented with wgpu compute in `subcraft-render`). The GPU only
//! blends: layer stack walk, opacity, masks, folders (isolated and passthrough), all 32 blend
//! modes and the per-pixel content effects (Levels, Contrast, Threshold, Clamp, histogram scan /
//! range). Everything else is evaluated by the CPU for that layer and tile and passed as a
//! source: procedural / bitmap / mesh-map fills, generator masks, part masks, gradient maps,
//! HSL, paint / fill effects. Texture sets with neighborhood filters (blur, ...) are not handled
//! (they need other tiles) and use the CPU for the whole set.
//!
//! Stored data is untouched: paint tiles are uploaded to a GPU-side cache keyed by tile identity
//! (an `Arc<Tile>` the cache holds, so copy-on-write guarantees an edited tile has a new key).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use half::f16;
use rayon::prelude::*;
use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, ContentEffect, ContentEffectKind, Filter, Layer, LayerContent, TextureSet};
use subcraft_image::{TILE_SIZE, TILE_TEXELS, Tile, slot_data_mut};

use crate::blend::BlendTarget;
use crate::mask::eval_mask;
use crate::tile::{eval_fill, point_effect, read_paint};
use crate::{ChangedTiles, Composite, EvalContext, TileContext};

/// Deepest folder nesting the shader keeps accumulators for.
pub const MAX_GROUP_DEPTH: usize = 4;

#[derive(Debug, Clone)]
pub enum GpuError {
    /// The batch needs more GPU memory than the backend has; the caller splits it.
    TooLarge,
    Failed(String),
}

pub trait GpuCompositeBackend: Send + Sync {
    fn name(&self) -> String;
    /// Blend `tiles` with `program`. The output holds, per program channel, the f16 texels
    /// `rgb + alpha` of every tile in order.
    fn composite(&self, program: &GpuProgram, tiles: &[GpuTile]) -> Result<GpuOutput, GpuError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Leaf,
    Begin,
    End,
}

/// One step of a channel's program. `effects` indexes [`GpuChannel::effects`].
#[derive(Clone, Copy, Debug)]
pub struct GpuOp {
    pub kind: OpKind,
    pub mode: BlendMode,
    pub passthrough: bool,
    pub opacity: f32,
    pub effects: (u32, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    Levels = 1,
    Contrast = 2,
    Threshold = 3,
    Clamp = 4,
    HistogramScan = 5,
    HistogramRange = 6,
}

#[derive(Clone, Copy, Debug)]
pub struct GpuEffect {
    pub kind: EffectKind,
    pub mode: BlendMode,
    pub opacity: f32,
    pub p: [f32; 5],
}

#[derive(Clone, Debug)]
pub struct GpuChannel {
    pub kind: ChannelKind,
    pub target: BlendTarget,
    pub color: bool,
    pub default: [f32; 4],
    pub ops: Vec<GpuOp>,
    pub effects: Vec<GpuEffect>,
}

#[derive(Clone, Debug, Default)]
pub struct GpuProgram {
    pub channels: Vec<GpuChannel>,
}

/// Where the texels of a leaf's content or a mask come from, for one tile.
#[derive(Clone)]
pub enum GpuSrc {
    None,
    /// The same value on every texel (masks use `[0]`).
    Const([f32; 4]),
    /// A stored tile with `comps` interleaved f16 values per texel.
    Tile { tile: Arc<Tile>, comps: u8 },
    /// Block `n` of the tile's `dyn_px` (content) or `dyn_mask` (mask).
    Dyn(u32),
}

#[derive(Clone)]
pub struct GpuOpData {
    pub src: GpuSrc,
    pub mask: GpuSrc,
}

/// Everything the GPU needs for one tile. `ops[channel][op]` matches the program.
pub struct GpuTile {
    pub tile: usize,
    pub ops: Vec<Vec<GpuOpData>>,
    /// Blocks of `TILE_TEXELS` straight `[v0, v1, v2, alpha]` texels evaluated on the CPU.
    pub dyn_px: Vec<[f32; 4]>,
    /// Blocks of `TILE_TEXELS` mask values.
    pub dyn_mask: Vec<f32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpuTiming {
    pub upload_ms: f64,
    /// Submit to results ready (transfers and kernels).
    pub gpu_ms: f64,
    /// Kernel time from GPU timestamps (0 when the device has none).
    pub kernel_ms: f64,
    pub readback_ms: f64,
    pub uploaded_bytes: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

pub struct GpuOutput {
    pub channels: Vec<Vec<f16>>,
    pub timing: GpuTiming,
}

// ---------------------------------------------------------------------------------------------
// Registry, switches and statistics.

type Factory = fn() -> Option<Arc<dyn GpuCompositeBackend>>;

static FACTORY: Mutex<Option<Factory>> = Mutex::new(None);
static BACKEND: OnceLock<Option<Arc<dyn GpuCompositeBackend>>> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(true);
static MIN_TILES: AtomicUsize = AtomicUsize::new(DEFAULT_MIN_TILES);
static MIN_RESOLUTION: AtomicU32 = AtomicU32::new(DEFAULT_MIN_RESOLUTION);
static STATS: Mutex<GpuStats> = Mutex::new(GpuStats::new());

/// Edits touching fewer tiles than this stay on the CPU (a brush stroke is a handful of tiles
/// and the CPU answers sooner than a round trip to the GPU).
pub const DEFAULT_MIN_TILES: usize = 64;

/// Texture sets below this resolution stay on the CPU: measured (docs/progress.md) the GPU path
/// wins whole-set edits at 2048² and 4096² but loses at 1024², where the CPU needs only ~16 ms.
pub const DEFAULT_MIN_RESOLUTION: u32 = 2048;

/// Above this share of CPU-evaluated sources the CPU keeps the whole set (see `evaluate_tiles`).
const MAX_DYNAMIC_SHARE: f64 = 0.25;
/// Tiles per GPU batch before the memory-based cap.
const MAX_BATCH: usize = 256;

/// Register the backend constructor (called once at start-up by the UI).
pub fn register_gpu_compositor(factory: Factory) {
    *FACTORY.lock().unwrap() = Some(factory);
}

/// The preference "Use GPU for compositing".
pub fn set_gpu_compositing_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn gpu_compositing_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Smallest number of tiles worth a GPU round trip (tests lower it).
pub fn set_gpu_min_tiles(n: usize) {
    MIN_TILES.store(n, Ordering::Relaxed);
}

/// Smallest texture set resolution that uses the GPU (tests lower it).
pub fn set_gpu_min_resolution(res: u32) {
    MIN_RESOLUTION.store(res, Ordering::Relaxed);
}

/// Whether a backend is registered and the preference is on (the device is created lazily).
pub fn gpu_compositing_possible() -> bool {
    gpu_compositing_enabled() && match BACKEND.get() {
        Some(b) => b.is_some(),
        None => FACTORY.lock().unwrap().is_some(),
    }
}

/// Tiles per call the cache should ask for when the GPU is in use, to amortize the round trip.
pub fn preferred_batch() -> usize {
    if gpu_compositing_possible() { MAX_BATCH } else { 0 }
}

fn backend() -> Option<Arc<dyn GpuCompositeBackend>> {
    BACKEND
        .get_or_init(|| {
            let factory = (*FACTORY.lock().unwrap())?;
            let b = factory();
            match &b {
                Some(b) => log::info!("GPU compositing backend: {}", b.name()),
                None => log::info!("GPU compositing unavailable; using the CPU compositor"),
            }
            b
        })
        .clone()
}

/// Name of the backend in use, creating it if needed (diagnostics, benchmarks).
pub fn gpu_backend_name() -> Option<String> {
    backend().map(|b| b.name())
}

#[derive(Clone, Debug, Default)]
pub struct GpuStats {
    pub batches: u64,
    pub tiles: u64,
    pub fallback_tiles: u64,
    pub errors: u64,
    pub prep_ms: f64,
    pub apply_ms: f64,
    pub timing: GpuTiming,
}

impl GpuStats {
    const fn new() -> Self {
        Self {
            batches: 0,
            tiles: 0,
            fallback_tiles: 0,
            errors: 0,
            prep_ms: 0.0,
            apply_ms: 0.0,
            timing: GpuTiming { upload_ms: 0.0, gpu_ms: 0.0, kernel_ms: 0.0, readback_ms: 0.0, uploaded_bytes: 0, cache_hits: 0, cache_misses: 0 },
        }
    }
}

/// Counters since the last call (which path ran, how long each part took).
pub fn take_gpu_stats() -> GpuStats {
    std::mem::replace(&mut *STATS.lock().unwrap(), GpuStats::new())
}

// ---------------------------------------------------------------------------------------------
// Planning: a texture set's layer tree as one flat program per channel.

#[derive(Debug)]
pub enum Unsupported {
    Spatial,
    Depth,
    GroupEffect,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaf {
    No,
    Paint,
    Fill,
}

struct PlanOp {
    layer: usize,
    leaf: Leaf,
    /// Content effects run on the CPU (the content is evaluated after them).
    cpu_effects: bool,
}

struct Plan {
    layers: Vec<Arc<Layer>>,
    program: GpuProgram,
    ops: Vec<Vec<PlanOp>>,
    resolution: u32,
}

/// Translate a visible, active content effect for `kind`. `Ok(None)` = ignored by the CPU too,
/// `Err(())` = needs the CPU.
fn translate_effect(e: &ContentEffect, kind: ChannelKind) -> Result<Option<GpuEffect>, ()> {
    if !e.visible {
        return Ok(None);
    }
    let blend = e.blend_for(kind);
    if !blend.is_active() {
        return Ok(None);
    }
    let mode = crate::tile::effective_mode(blend.mode);
    let (k, p) = match &e.kind {
        ContentEffectKind::Paint { .. } | ContentEffectKind::Fill(_) => return Err(()),
        ContentEffectKind::Levels { channel, levels } => {
            if *channel != kind || kind == ChannelKind::Normal {
                return Ok(None);
            }
            (EffectKind::Levels, [levels.in_low, levels.in_high, levels.gamma, levels.out_low, levels.out_high])
        }
        ContentEffectKind::Filter { channel, filter } => {
            if *channel != kind || !filter.supports_channel(kind) || !filter.valid() {
                return Ok(None);
            }
            match *filter {
                Filter::Contrast { gain, pivot } => (EffectKind::Contrast, [gain, pivot, 0.0, 0.0, 0.0]),
                Filter::Threshold { cutoff } => (EffectKind::Threshold, [cutoff, 0.0, 0.0, 0.0, 0.0]),
                Filter::Clamp { low, high } => (EffectKind::Clamp, [low, high, 0.0, 0.0, 0.0]),
                Filter::HistogramScan { position, contrast, invert } => {
                    (EffectKind::HistogramScan, [position, contrast, f32::from(u8::from(invert)), 0.0, 0.0])
                }
                Filter::HistogramRange { position, range, softness, invert } => {
                    (EffectKind::HistogramRange, [position, range, softness, f32::from(u8::from(invert)), 0.0])
                }
                _ => return Err(()),
            }
        }
    };
    Ok(Some(GpuEffect { kind: k, mode, opacity: blend.opacity, p }))
}

/// Effects of `layer` for `kind`: GPU effects, or `None` when any needs the CPU.
fn translate_effects(layer: &Layer, kind: ChannelKind, out: &mut Vec<GpuEffect>) -> Option<(u32, u32)> {
    let first = out.len();
    for e in &layer.content_effects {
        match translate_effect(e, kind) {
            Ok(Some(g)) => out.push(g),
            Ok(None) => {}
            Err(()) => {
                out.truncate(first);
                return None;
            }
        }
    }
    Some((first as u32, (out.len() - first) as u32))
}

struct Planner<'a> {
    kind: ChannelKind,
    layers: &'a mut Vec<Arc<Layer>>,
    channel: GpuChannel,
    ops: Vec<PlanOp>,
}

impl Planner<'_> {
    fn stack(&mut self, list: &[Arc<Layer>], depth: usize) -> Result<(), Unsupported> {
        let kind = self.kind;
        for layer in list {
            if !layer.visible {
                continue;
            }
            let blend = layer.blend_for(kind);
            if !blend.is_active() {
                continue;
            }
            if layer.parts.as_ref().is_some_and(|p| p.enabled && p.parts.is_empty()) {
                continue;
            }
            let mode = crate::tile::effective_mode(blend.mode);
            let index = match self.layers.iter().position(|l| Arc::ptr_eq(l, layer)) {
                Some(i) => i,
                None => {
                    self.layers.push(layer.clone());
                    self.layers.len() - 1
                }
            };
            match &layer.content {
                LayerContent::Paint(paint) => {
                    if !paint.channels.contains_key(&kind) && !crate::util::effects_touch(layer, kind) {
                        continue;
                    }
                    self.leaf(layer, index, Leaf::Paint, mode, blend.opacity);
                }
                LayerContent::Fill(fill) => {
                    if !fill.channels.get(&kind).is_some_and(|c| c.enabled) && !crate::util::effects_touch(layer, kind) {
                        continue;
                    }
                    self.leaf(layer, index, Leaf::Fill, mode, blend.opacity);
                }
                LayerContent::Group(group) => {
                    if depth + 1 >= MAX_GROUP_DEPTH {
                        return Err(Unsupported::Depth);
                    }
                    let passthrough = blend.mode == BlendMode::Passthrough;
                    let begin = GpuOp { kind: OpKind::Begin, mode, passthrough, opacity: blend.opacity, effects: (0, 0) };
                    self.channel.ops.push(begin);
                    self.ops.push(PlanOp { layer: index, leaf: Leaf::No, cpu_effects: false });
                    self.stack(&group.children, depth + 1)?;
                    let effects = translate_effects(layer, kind, &mut self.channel.effects).ok_or(Unsupported::GroupEffect)?;
                    let end = GpuOp { kind: OpKind::End, mode, passthrough, opacity: blend.opacity, effects };
                    self.channel.ops.push(end);
                    self.ops.push(PlanOp { layer: index, leaf: Leaf::No, cpu_effects: false });
                }
            }
        }
        Ok(())
    }

    fn leaf(&mut self, layer: &Layer, index: usize, leaf: Leaf, mode: BlendMode, opacity: f32) {
        let translated = translate_effects(layer, self.kind, &mut self.channel.effects);
        let effects = translated.unwrap_or((0, 0));
        self.channel.ops.push(GpuOp { kind: OpKind::Leaf, mode, passthrough: false, opacity, effects });
        self.ops.push(PlanOp { layer: index, leaf, cpu_effects: translated.is_none() });
    }
}

fn plan(ts: &TextureSet) -> Result<Plan, Unsupported> {
    if crate::stages::has_spatial(&ts.layers) {
        return Err(Unsupported::Spatial);
    }
    let mut layers = Vec::new();
    let mut program = GpuProgram::default();
    let mut ops = Vec::new();
    for kind in ts.channel_kinds() {
        let d = kind.default_value().0;
        let channel = GpuChannel {
            kind,
            target: BlendTarget::for_channel(kind),
            color: kind.is_color(),
            default: [d[0], d[1], d[2], 1.0],
            ops: Vec::new(),
            effects: Vec::new(),
        };
        let mut p = Planner { kind, layers: &mut layers, channel, ops: Vec::new() };
        p.stack(&ts.layers, 0)?;
        program.channels.push(p.channel);
        ops.push(p.ops);
    }
    Ok(Plan { layers, program, ops, resolution: ts.resolution })
}

// ---------------------------------------------------------------------------------------------
// Per-tile preparation (CPU, parallel over tiles).

fn mask_source(layer: &Layer, tc: &TileContext, res: u32, dyn_mask: &mut Vec<f32>) -> GpuSrc {
    let mask = layer.mask.as_ref().filter(|m| m.enabled);
    let parts = layer.parts.as_ref().filter(|p| p.enabled);
    if mask.is_none() && parts.is_none() {
        return GpuSrc::None;
    }
    if let (Some(m), None) = (mask, parts)
        && m.base.width() == res
        && m.base.height() == res
        && !m.effects.iter().any(|e| e.visible && e.opacity > 0.0)
    {
        let t = m.base.tile_index(tc.geo.x0 / TILE_SIZE, tc.geo.y0 / TILE_SIZE);
        return match m.base.tile(t) {
            Some(tile) => GpuSrc::Tile { tile: tile.clone(), comps: m.base.comps() as u8 },
            None => GpuSrc::Const([m.base.fill()[0].clamp(0.0, 1.0); 4]),
        };
    }
    let mut values = eval_mask(layer.mask.as_ref(), tc);
    if let Some(parts) = parts {
        let m = values.get_or_insert_with(|| vec![1.0; TILE_TEXELS]);
        for (i, v) in m.iter_mut().enumerate() {
            *v *= parts.factor(tc.geo.region(i, parts.kind));
        }
    }
    let Some(values) = values else { return GpuSrc::None };
    if values.iter().all(|&v| v <= 0.0) {
        return GpuSrc::Const([0.0; 4]);
    }
    let block = (dyn_mask.len() / TILE_TEXELS) as u32;
    dyn_mask.extend_from_slice(&values);
    GpuSrc::Dyn(block)
}

fn push_dyn(dyn_px: &mut Vec<[f32; 4]>, content: &[[f32; 4]]) -> GpuSrc {
    let block = (dyn_px.len() / TILE_TEXELS) as u32;
    dyn_px.extend_from_slice(content);
    GpuSrc::Dyn(block)
}

fn apply_cpu_effects(layer: &Layer, kind: ChannelKind, content: &mut [[f32; 4]], tc: &TileContext) {
    for e in &layer.content_effects {
        point_effect(e, kind, content, tc);
    }
}

fn content_source(layer: &Layer, kind: ChannelKind, leaf: Leaf, cpu_effects: bool, res: u32, tc: &TileContext, dyn_px: &mut Vec<[f32; 4]>) -> GpuSrc {
    let mut content = vec![[0.0f32; 4]; TILE_TEXELS];
    match (&layer.content, leaf) {
        (LayerContent::Paint(paint), Leaf::Paint) => match paint.channels.get(&kind) {
            None => {
                if cpu_effects {
                    apply_cpu_effects(layer, kind, &mut content, tc);
                    return push_dyn(dyn_px, &content);
                }
                GpuSrc::Const([0.0; 4])
            }
            Some(img) => {
                let n = kind.components();
                if !cpu_effects && img.width() == res && img.height() == res && img.comps() == n + 1 {
                    let t = img.tile_index(tc.geo.x0 / TILE_SIZE, tc.geo.y0 / TILE_SIZE);
                    return match img.tile(t) {
                        Some(tile) => GpuSrc::Tile { tile: tile.clone(), comps: img.comps() as u8 },
                        None => {
                            let f = img.fill();
                            GpuSrc::Const(if n == 1 { [f[0], f[0], f[0], f[1]] } else { f })
                        }
                    };
                }
                read_paint(img, kind, tc, &mut content);
                if cpu_effects {
                    apply_cpu_effects(layer, kind, &mut content, tc);
                }
                push_dyn(dyn_px, &content)
            }
        },
        (LayerContent::Fill(fill), Leaf::Fill) => {
            if !eval_fill(fill, kind, tc, &mut content) {
                content.fill([0.0; 4]); // effects touch this channel (checked by the plan)
                if cpu_effects {
                    apply_cpu_effects(layer, kind, &mut content, tc);
                    return push_dyn(dyn_px, &content);
                }
                return GpuSrc::Const([0.0; 4]);
            }
            if cpu_effects {
                apply_cpu_effects(layer, kind, &mut content, tc);
                return push_dyn(dyn_px, &content);
            }
            let uniform = fill.channels.get(&kind).is_some_and(|c| matches!(c.source, subcraft_doc::FillSource::Uniform { .. }));
            if uniform { GpuSrc::Const(content[0]) } else { push_dyn(dyn_px, &content) }
        }
        _ => GpuSrc::None,
    }
}

fn prepare_tile(plan: &Plan, ts: &TextureSet, ctx: &EvalContext, t: usize) -> GpuTile {
    let geo = ctx.geometry.tile(t);
    let tc = TileContext {
        albedo_filter: ts.albedo_filter,
        geo: &geo,
        set_geometry: ctx.geometry,
        sample: subcraft_materials::SampleContext { resources: ctx.resources, mesh_maps: ctx.mesh_maps },
    };
    let mut dyn_px = Vec::new();
    let mut dyn_mask = Vec::new();
    let mut masks: Vec<Option<GpuSrc>> = vec![None; plan.layers.len()];
    let mut ops = Vec::with_capacity(plan.ops.len());
    for (ch, list) in plan.program.channels.iter().zip(&plan.ops) {
        let mut out = Vec::with_capacity(list.len());
        for (op, gop) in list.iter().zip(&ch.ops) {
            let layer = &*plan.layers[op.layer];
            let mask = match gop.kind {
                OpKind::Begin => GpuSrc::None,
                _ => masks[op.layer].get_or_insert_with(|| mask_source(layer, &tc, plan.resolution, &mut dyn_mask)).clone(),
            };
            let src = if gop.kind == OpKind::Leaf {
                content_source(layer, ch.kind, op.leaf, op.cpu_effects, plan.resolution, &tc, &mut dyn_px)
            } else {
                GpuSrc::None
            };
            out.push(GpuOpData { src, mask });
        }
        ops.push(out);
    }
    GpuTile { tile: t, ops, dyn_px, dyn_mask }
}

/// Bytes of CPU-evaluated data one tile may need (for batch sizing).
fn dyn_bytes_per_tile(plan: &Plan) -> usize {
    let leaves: usize = plan.program.channels.iter().map(|c| c.ops.iter().filter(|o| o.kind == OpKind::Leaf).count()).sum();
    leaves * TILE_TEXELS * 16 + plan.layers.len() * TILE_TEXELS * 4
}

// ---------------------------------------------------------------------------------------------
// Driver.

/// Evaluate as many of `tiles` as possible on the GPU and store them in `composite`. Returns
/// what changed plus the tiles the caller must still evaluate on the CPU.
pub(crate) fn evaluate_tiles(composite: &mut Composite, ts: &TextureSet, ctx: &EvalContext, tiles: &[usize]) -> (ChangedTiles, Vec<usize>) {
    let none = |t: &[usize]| (ChangedTiles::new(), t.to_vec());
    if !gpu_compositing_enabled() || tiles.len() < MIN_TILES.load(Ordering::Relaxed) || ts.resolution < MIN_RESOLUTION.load(Ordering::Relaxed) {
        return none(tiles);
    }
    let Some(backend) = backend() else { return none(tiles) };
    let plan = match plan(ts) {
        Ok(p) => p,
        Err(why) => {
            log::debug!("GPU compositing skipped for this set: {why:?}");
            return none(tiles);
        }
    };
    let batch = (MAX_BATCH).min((192usize << 20) / dyn_bytes_per_tile(&plan).max(1)).max(4);
    let mut changed = ChangedTiles::new();
    let mut left = Vec::new();
    let mut work: Vec<&[usize]> = tiles.chunks(batch).collect();
    work.reverse();
    let mut first = true;
    while let Some(chunk) = work.pop() {
        let t0 = Instant::now();
        let prepared: Vec<GpuTile> = chunk.par_iter().map(|&t| prepare_tile(&plan, ts, ctx, t)).collect();
        let prep = t0.elapsed().as_secs_f64() * 1e3;
        if first {
            first = false;
            // When most leaves need CPU-evaluated blocks (procedural fills, generator masks), the
            // CPU work is the cost and the GPU only adds upload and readback: leave the rest of
            // the set to the CPU (the batch in hand is still blended on the GPU).
            let (mut dynamic, mut leaves) = (0usize, 0usize);
            for op in prepared.iter().flat_map(|t| t.ops.iter().flatten()) {
                if let GpuSrc::Dyn(_) = op.src {
                    dynamic += 1;
                }
                if !matches!(op.src, GpuSrc::None) {
                    leaves += 1;
                }
            }
            if leaves > 0 && dynamic as f64 > MAX_DYNAMIC_SHARE * leaves as f64 {
                log::debug!("GPU compositing: {dynamic}/{leaves} sources are CPU-evaluated; the rest of the set uses the CPU");
                while let Some(rest) = work.pop() {
                    left.extend_from_slice(rest);
                }
            }
        }
        match backend.composite(&plan.program, &prepared) {
            Ok(out) => {
                let t1 = Instant::now();
                for (kind, t) in apply(composite, &plan.program, chunk, &out) {
                    changed.entry(kind).or_default().extend(t);
                }
                let mut s = STATS.lock().unwrap();
                s.batches += 1;
                s.tiles += chunk.len() as u64;
                s.prep_ms += prep;
                s.apply_ms += t1.elapsed().as_secs_f64() * 1e3;
                let (a, b) = (&mut s.timing, out.timing);
                a.upload_ms += b.upload_ms;
                a.gpu_ms += b.gpu_ms;
                a.kernel_ms += b.kernel_ms;
                a.readback_ms += b.readback_ms;
                a.uploaded_bytes += b.uploaded_bytes;
                a.cache_hits += b.cache_hits;
                a.cache_misses += b.cache_misses;
            }
            Err(GpuError::TooLarge) if chunk.len() > 1 => {
                let (a, b) = chunk.split_at(chunk.len() / 2);
                work.push(b);
                work.push(a);
            }
            Err(e) => {
                log::warn!("GPU compositing failed ({e:?}); {} tiles use the CPU", chunk.len());
                let mut s = STATS.lock().unwrap();
                s.errors += 1;
                s.fallback_tiles += chunk.len() as u64;
                left.extend_from_slice(chunk);
            }
        }
    }
    for t in changed.values_mut() {
        t.sort_unstable();
    }
    (changed, left)
}

/// Store the GPU result in the composite; returns per channel the tiles whose texels changed.
fn apply(composite: &mut Composite, program: &GpuProgram, tiles: &[usize], out: &GpuOutput) -> Vec<(ChannelKind, Vec<usize>)> {
    let mut result = Vec::new();
    for (ci, ch) in program.channels.iter().enumerate() {
        let Some(img) = composite.channels.get_mut(&ch.kind) else { continue };
        let slots = img.slots_mut();
        let (comps, fill) = (slots.comps, slots.fill);
        let data = &out.channels[ci];
        let mut index = vec![usize::MAX; slots.slots.len()];
        for (i, &t) in tiles.iter().enumerate() {
            index[t] = i;
        }
        let changed: Vec<usize> = slots
            .slots
            .par_iter_mut()
            .enumerate()
            .filter(|(t, _)| index[*t] != usize::MAX)
            .filter_map(|(t, slot)| {
                let src = &data[index[t] * TILE_TEXELS * 4..][..TILE_TEXELS * 4];
                let narrow: Vec<f16>;
                let encoded: &[f16] = if comps == 4 {
                    src
                } else {
                    let mut v = vec![f16::ZERO; TILE_TEXELS * comps];
                    for (o, px) in v.chunks_exact_mut(comps).zip(src.chunks_exact(4)) {
                        o.copy_from_slice(&px[..comps]);
                    }
                    narrow = v;
                    &narrow
                };
                if slot.as_ref().is_some_and(|tile| tile.data[..] == *encoded) {
                    return None;
                }
                slot_data_mut(slot, comps, &fill).copy_from_slice(encoded);
                Some(t)
            })
            .collect();
        result.push((ch.kind, changed));
    }
    result
}
