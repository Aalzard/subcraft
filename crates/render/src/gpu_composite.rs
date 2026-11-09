//! GPU layer compositing backend (ADR-0044): implements `subcraft_compositor::gpu::GpuCompositeBackend`
//! with one WGSL compute pass per channel on a headless device of its own, so the viewport device
//! is never stalled.
//!
//! Paint and mask tiles are uploaded once into a pool (32 KiB slots, f16, exactly the stored
//! texels) and found again by the identity of the `Arc<Tile>` (the cache holds a clone, so
//! copy-on-write guarantees an edited tile has a new key): between edits only new or edited tiles
//! travel. The pool is up to four storage buffers (one binding is limited by the device), grown on
//! demand. Tiles the pool cannot hold at once make the batch report `TooLarge`, and the compositor
//! splits it. CPU-evaluated content travels as f32 blocks for the batch only. Scratch buffers
//! are kept and reused between batches. The pool is a derived cache; nothing stored depends on it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use half::f16;
use half::slice::HalfFloatSliceExt;
use subcraft_compositor::gpu::{
    EffectKind, GpuCompositeBackend, GpuError, GpuOutput, GpuProgram, GpuSrc, GpuTile, GpuTiming, OpKind,
};
use subcraft_doc::BlendMode;
use subcraft_image::{TILE_TEXELS, Tile};

const SLOT_BYTES: u64 = 32 * 1024;
const REF_NONE: u32 = 0;
const REF_CONST: u32 = 1 << 28;
const REF_TILE: u32 = 2 << 28;
const REF_DYN: u32 = 3 << 28;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    ops_off: u32,
    nops: u32,
    refs_off: u32,
    out_off: u32,
    flags: u32,
    tiles: u32,
    eff_off: u32,
    /// Slots per pool buffer.
    spb: u32,
    default_value: [f32; 4],
}

/// Pool buffers the shader binds (`pool0` .. `pool3`).
const POOL_BUFFERS: usize = 4;

struct Entry {
    _tile: Arc<Tile>,
    slot: u32,
    used: u64,
}

struct Pool {
    buffers: Vec<wgpu::Buffer>,
    /// Slots per buffer.
    per_buffer: u32,
    /// Buffers the pool may grow to.
    max_buffers: usize,
    map: HashMap<usize, Entry>,
    free: Vec<u32>,
    stamp: u64,
}

impl Pool {
    fn capacity(&self) -> usize {
        self.per_buffer as usize * self.max_buffers
    }
}

/// Buffers reused between batches (grown, never shrunk).
#[derive(Default)]
struct Scratch {
    bufs: HashMap<&'static str, wgpu::Buffer>,
}

pub struct GpuCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    pool: Mutex<Pool>,
    scratch: Mutex<Scratch>,
    dummy: wgpu::Buffer,
    timestamps: bool,
    max_binding: u64,
}

/// Registered with `subcraft_compositor::gpu::register_gpu_compositor` by the application.
/// Software adapters (llvmpipe, WARP) are refused: the CPU path is faster than they are.
/// `SUBCRAFT_GPU_COMPOSITE_SOFTWARE=1` accepts them (tests, CI).
pub fn create_backend() -> Option<Arc<dyn GpuCompositeBackend>> {
    create(std::env::var_os("SUBCRAFT_GPU_COMPOSITE_SOFTWARE").is_some()).map(|b| Arc::new(b) as Arc<dyn GpuCompositeBackend>)
}

/// Like [`create_backend`] but accepts a software adapter (for equivalence tests).
pub fn create_any_backend() -> Option<Arc<GpuCompositor>> {
    create(true).map(Arc::new)
}

fn create(allow_software: bool) -> Option<GpuCompositor> {
    // Same graphics API and adapter as the window (see `headless::secondary_adapter`).
    let (_instance, adapter) = crate::headless::secondary_adapter()?;
    if adapter.get_info().device_type == wgpu::DeviceType::Cpu && !allow_software {
        return None;
    }
    let supported = adapter.limits();
    let big: u64 = u64::from(supported.max_storage_buffer_binding_size).min(supported.max_buffer_size).min(1 << 30);
    let mut limits = wgpu::Limits::default();
    limits.max_storage_buffer_binding_size = big.max(limits.max_storage_buffer_binding_size);
    limits.max_buffer_size = big.max(limits.max_buffer_size);
    // prog, refs, consts, four pool buffers, CPU content, CPU masks, output.
    if supported.max_storage_buffers_per_shader_stage < 10 {
        return None;
    }
    limits.max_storage_buffers_per_shader_stage = limits.max_storage_buffers_per_shader_stage.max(10);
    let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("subcraft gpu composite"),
        required_limits: limits,
        required_features: if timestamps { wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() },
        ..Default::default()
    }))
    .ok()?;
    Some(GpuCompositor::new(&adapter, device, queue, timestamps))
}

fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

impl GpuCompositor {
    fn new(adapter: &wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue, timestamps: bool) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }];
        entries.extend((1..=6).map(|b| storage(b, true)));
        entries.push(storage(7, false));
        entries.extend((8..=10).map(|b| storage(b, true)));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("composite"), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("composite"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let limits = device.limits();
        let max_binding = (limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size);
        let pool_mb: u64 = std::env::var("SUBCRAFT_GPU_POOL_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(2048);
        let per_buffer = (max_binding.min(512 << 20) / SLOT_BYTES).clamp(64, 1 << 20) as u32;
        let max_buffers = ((pool_mb << 20).div_ceil(per_buffer as u64 * SLOT_BYTES) as usize).clamp(1, POOL_BUFFERS);
        let pool = Pool { buffers: Vec::new(), per_buffer, max_buffers, map: HashMap::new(), free: Vec::new(), stamp: 0 };
        let dummy = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite pool placeholder"),
            size: 256,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Self {
            device,
            queue,
            adapter_name: adapter.get_info().name,
            pipeline,
            layout,
            pool: Mutex::new(pool),
            scratch: Mutex::default(),
            dummy,
            timestamps,
            max_binding,
        }
    }

    fn buffer(&self, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage, mapped_at_creation: false })
    }

    /// A kept buffer of at least `size` bytes (grown by half again when too small).
    fn kept(&self, scratch: &mut Scratch, label: &'static str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        let size = size.max(16).next_multiple_of(256);
        if let Some(b) = scratch.bufs.get(label).filter(|b| b.size() >= size) {
            return b.clone();
        }
        let b = self.buffer(label, (size + size / 2).min(self.max_binding.max(size)), usage);
        scratch.bufs.insert(label, b.clone());
        b
    }

    fn upload(&self, scratch: &mut Scratch, label: &'static str, data: &[u8]) -> wgpu::Buffer {
        let buf = self.kept(scratch, label, data.len() as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        if !data.is_empty() {
            self.queue.write_buffer(&buf, 0, data);
        }
        buf
    }

    /// Add one pool buffer; its slots become free (lowest first).
    fn grow(&self, pool: &mut Pool) -> bool {
        if pool.buffers.len() >= pool.max_buffers {
            return false;
        }
        let first = pool.buffers.len() as u32 * pool.per_buffer;
        pool.buffers.push(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite tile pool"),
            size: pool.per_buffer as u64 * SLOT_BYTES,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        pool.free.extend((first..first + pool.per_buffer).rev());
        true
    }

    /// Pool slots for every stored tile the batch reads; uploads the ones not resident.
    fn resolve_tiles(&self, pool: &mut Pool, tiles: &[GpuTile], timing: &mut GpuTiming) -> Result<HashMap<usize, u32>, GpuError> {
        pool.stamp += 1;
        let stamp = pool.stamp;
        let mut slots: HashMap<usize, u32> = HashMap::new();
        let mut misses: Vec<(usize, Arc<Tile>)> = Vec::new();
        for t in tiles {
            for ch in &t.ops {
                for op in ch {
                    for src in [&op.src, &op.mask] {
                        let GpuSrc::Tile { tile, .. } = src else { continue };
                        let key = Arc::as_ptr(tile) as usize;
                        if slots.contains_key(&key) {
                            continue;
                        }
                        if let Some(e) = pool.map.get_mut(&key) {
                            e.used = stamp;
                            slots.insert(key, e.slot);
                            timing.cache_hits += 1;
                        } else {
                            slots.insert(key, u32::MAX);
                            misses.push((key, tile.clone()));
                            timing.cache_misses += 1;
                        }
                    }
                }
            }
        }
        if slots.len() > pool.capacity() {
            return Err(GpuError::TooLarge);
        }
        while misses.len() > pool.free.len() && self.grow(pool) {}
        // Evict least recently used entries (never ones this batch needs).
        let need = misses.len().saturating_sub(pool.free.len());
        if need > 0 {
            let mut old: Vec<(u64, usize)> = pool.map.iter().filter(|(_, e)| e.used < stamp).map(|(k, e)| (e.used, *k)).collect();
            old.sort_unstable();
            for (_, key) in old.into_iter().take(need) {
                let e = pool.map.remove(&key).expect("listed");
                pool.free.push(e.slot);
            }
        }
        let mut placed: Vec<(u32, Arc<Tile>)> = Vec::with_capacity(misses.len());
        for (key, tile) in misses {
            let slot = pool.free.pop().ok_or(GpuError::TooLarge)?;
            slots.insert(key, slot);
            pool.map.insert(key, Entry { _tile: tile.clone(), slot, used: stamp });
            placed.push((slot, tile));
        }
        // One write per run of neighboring slots instead of one per 32 KiB tile: far fewer
        // queue submissions and staging allocations.
        placed.sort_unstable_by_key(|p| p.0);
        let mut staging: Vec<u8> = Vec::new();
        let mut run_start = 0u32;
        let mut next = 0u32;
        for (slot, tile) in placed.iter() {
            let bits = tile.data.reinterpret_cast();
            let new_buffer = slot % pool.per_buffer == 0;
            if !staging.is_empty() && (*slot != next || new_buffer || staging.len() >= (64 << 20)) {
                self.write_slots(pool, run_start, &staging);
                staging.clear();
            }
            if staging.is_empty() {
                run_start = *slot;
            }
            let bytes: &[u8] = bytemuck::cast_slice(bits);
            debug_assert!(bytes.len() <= SLOT_BYTES as usize);
            staging.extend_from_slice(bytes);
            // Tiles with fewer components than a full slot leave the rest of the slot unread.
            let pad = SLOT_BYTES as usize - bytes.len();
            staging.resize(staging.len() + pad, 0);
            timing.uploaded_bytes += bytes.len() as u64;
            next = slot + 1;
        }
        if !staging.is_empty() {
            self.write_slots(pool, run_start, &staging);
        }
        Ok(slots)
    }

    fn write_slots(&self, pool: &Pool, first: u32, data: &[u8]) {
        let (b, local) = (first / pool.per_buffer, first % pool.per_buffer);
        self.queue.write_buffer(&pool.buffers[b as usize], local as u64 * SLOT_BYTES, data);
    }
}

fn mode_code(mode: BlendMode) -> u32 {
    use BlendMode::*;
    match mode {
        Normal => 0,
        Passthrough => 1,
        Disable => 2,
        Replace => 3,
        Multiply => 4,
        Divide => 5,
        InverseDivide => 6,
        Darken => 7,
        Lighten => 8,
        LinearDodge => 9,
        Subtract => 10,
        InverseSubtract => 11,
        Difference => 12,
        Exclusion => 13,
        SignedAddition => 14,
        Overlay => 15,
        Screen => 16,
        LinearBurn => 17,
        ColorBurn => 18,
        ColorDodge => 19,
        SoftLight => 20,
        HardLight => 21,
        VividLight => 22,
        LinearLight => 23,
        PinLight => 24,
        Tint => 25,
        Saturation => 26,
        Color => 27,
        Value => 28,
        NormalMapCombine => 29,
        NormalMapDetail => 30,
        NormalMapInverseDetail => 31,
    }
}

fn limit_err<T>(e: Option<T>) -> Result<T, GpuError> {
    e.ok_or_else(|| GpuError::Failed("index overflow".into()))
}

impl GpuCompositeBackend for GpuCompositor {
    fn name(&self) -> String {
        format!("wgpu compute on {}", self.adapter_name)
    }

    fn composite(&self, program: &GpuProgram, tiles: &[GpuTile]) -> Result<GpuOutput, GpuError> {
        let start = Instant::now();
        let mut timing = GpuTiming::default();
        let n = tiles.len();
        if n == 0 || program.channels.is_empty() {
            return Ok(GpuOutput { channels: vec![Vec::new(); program.channels.len()], timing });
        }
        if n > 4096 {
            return Err(GpuError::TooLarge);
        }
        let mut pool = self.pool.lock().unwrap();
        let slots = self.resolve_tiles(&mut pool, tiles, &mut timing)?;

        // Program words: all channels' ops, then all channels' effects.
        let mut prog: Vec<u32> = Vec::new();
        let mut op_offsets = Vec::new();
        for ch in &program.channels {
            op_offsets.push(prog.len() as u32);
            for op in &ch.ops {
                let kind = match op.kind {
                    OpKind::Leaf => 0,
                    OpKind::Begin => 1,
                    OpKind::End => 2,
                };
                prog.extend([kind | (mode_code(op.mode) << 8) | (u32::from(op.passthrough) << 16), op.opacity.to_bits(), op.effects.0, op.effects.1]);
            }
        }
        let mut eff_offsets = Vec::new();
        for ch in &program.channels {
            eff_offsets.push(prog.len() as u32);
            for e in &ch.effects {
                let code = match e.kind {
                    EffectKind::Levels => 1,
                    EffectKind::Contrast => 2,
                    EffectKind::Threshold => 3,
                    EffectKind::Clamp => 4,
                    EffectKind::HistogramScan => 5,
                    EffectKind::HistogramRange => 6,
                };
                prog.extend([code | (mode_code(e.mode) << 8), e.opacity.to_bits()]);
                prog.extend(e.p.map(f32::to_bits));
                prog.push(0);
            }
        }

        // Per-tile bases into the shared CPU-evaluated blocks.
        let mut dyn_px: Vec<[f32; 4]> = Vec::new();
        let mut dyn_mask: Vec<f32> = Vec::new();
        let mut bases = Vec::with_capacity(n);
        for t in tiles {
            bases.push(((dyn_px.len() / TILE_TEXELS) as u32, (dyn_mask.len() / TILE_TEXELS) as u32));
            dyn_px.extend_from_slice(&t.dyn_px);
            dyn_mask.extend_from_slice(&t.dyn_mask);
            if (dyn_px.len() * 16) as u64 > self.max_binding || (dyn_mask.len() * 4) as u64 > self.max_binding {
                return Err(GpuError::TooLarge);
            }
        }
        let mut consts: Vec<[f32; 4]> = vec![[0.0; 4]];
        let mut refs: Vec<u32> = Vec::new();
        let mut refs_offsets = Vec::new();
        for (ci, ch) in program.channels.iter().enumerate() {
            refs_offsets.push(refs.len() as u32);
            for (t, tile) in tiles.iter().enumerate() {
                debug_assert_eq!(tile.ops[ci].len(), ch.ops.len());
                for op in &tile.ops[ci] {
                    for (src, is_mask) in [(&op.src, false), (&op.mask, true)] {
                        let word = match src {
                            GpuSrc::None => REF_NONE,
                            GpuSrc::Const(v) => {
                                consts.push(*v);
                                REF_CONST | limit_err(u32::try_from(consts.len() - 1).ok().filter(|&i| i < 1 << 24))?
                            }
                            GpuSrc::Tile { tile, comps } => {
                                let slot = slots[&(Arc::as_ptr(tile) as usize)];
                                REF_TILE | (u32::from(*comps) << 24) | slot
                            }
                            GpuSrc::Dyn(block) => {
                                let base = if is_mask { bases[t].1 } else { bases[t].0 };
                                REF_DYN | limit_err(Some(base + block).filter(|&i| i < 1 << 24))?
                            }
                        };
                        refs.push(word);
                    }
                }
            }
        }

        let mut scratch = self.scratch.lock().unwrap();
        let scratch = &mut *scratch;
        let prog_buf = self.upload(scratch, "composite program", bytemuck::cast_slice(&prog));
        let refs_buf = self.upload(scratch, "composite refs", bytemuck::cast_slice(&refs));
        let consts_buf = self.upload(scratch, "composite consts", bytemuck::cast_slice(&consts));
        let px_buf = self.upload(scratch, "composite cpu content", bytemuck::cast_slice(&dyn_px));
        let mask_buf = self.upload(scratch, "composite cpu masks", bytemuck::cast_slice(&dyn_mask));
        let channel_words = (n * TILE_TEXELS * 2) as u64;
        let out_bytes = channel_words * 4 * program.channels.len() as u64;
        if out_bytes > self.max_binding {
            return Err(GpuError::TooLarge);
        }
        let out_buf = self.kept(scratch, "composite out", out_bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let read_buf = self.kept(scratch, "composite readback", out_bytes, wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        let nch = program.channels.len() as u32;
        let queries = self.timestamps.then(|| {
            let set = self.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("composite timing"), ty: wgpu::QueryType::Timestamp, count: nch * 2 });
            let resolve = self.buffer("composite timing", nch as u64 * 16, wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
            let read = self.buffer("composite timing read", nch as u64 * 16, wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
            (set, resolve, read)
        });
        let pool_buf = |i: usize| pool.buffers.get(i).unwrap_or(&self.dummy).as_entire_binding();

        let mut enc = self.device.create_command_encoder(&Default::default());
        let mut keep = Vec::new();
        for (ci, ch) in program.channels.iter().enumerate() {
            let params = Params {
                ops_off: op_offsets[ci],
                nops: ch.ops.len() as u32,
                refs_off: refs_offsets[ci],
                out_off: (ci as u64 * channel_words) as u32,
                flags: u32::from(ch.target.signed) | (u32::from(ch.target.normal) << 1) | (u32::from(ch.color) << 2),
                tiles: n as u32,
                eff_off: eff_offsets[ci],
                spb: pool.per_buffer,
                default_value: ch.default,
            };
            let ubuf = self.buffer("composite params", std::mem::size_of::<Params>() as u64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
            self.queue.write_buffer(&ubuf, 0, bytemuck::bytes_of(&params));
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("composite"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: prog_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: refs_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: consts_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: pool_buf(0) },
                    wgpu::BindGroupEntry { binding: 8, resource: pool_buf(1) },
                    wgpu::BindGroupEntry { binding: 9, resource: pool_buf(2) },
                    wgpu::BindGroupEntry { binding: 10, resource: pool_buf(3) },
                    wgpu::BindGroupEntry { binding: 5, resource: px_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: mask_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: out_buf.as_entire_binding() },
                ],
            });
            let timestamp_writes = queries.as_ref().map(|(set, _, _)| wgpu::ComputePassTimestampWrites {
                query_set: set,
                beginning_of_pass_write_index: Some(ci as u32 * 2),
                end_of_pass_write_index: Some(ci as u32 * 2 + 1),
            });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("composite"), timestamp_writes });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((TILE_TEXELS / 64) as u32, n as u32, 1);
            drop(pass);
            keep.push((ubuf, group));
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &read_buf, 0, out_bytes);
        if let Some((set, resolve, read)) = &queries {
            enc.resolve_query_set(set, 0..nch * 2, resolve, 0);
            enc.copy_buffer_to_buffer(resolve, 0, read, 0, nch as u64 * 16);
        }
        timing.upload_ms = start.elapsed().as_secs_f64() * 1e3;
        let t_gpu = Instant::now();
        self.queue.submit([enc.finish()]);
        let slice = read_buf.slice(..out_bytes);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map composite output"));
        if let Some((_, _, read)) = &queries {
            read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map composite timing"));
        }
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| GpuError::Failed(format!("GPU wait failed: {e}")))?;
        timing.gpu_ms = t_gpu.elapsed().as_secs_f64() * 1e3;
        if let Some((_, _, read)) = &queries {
            {
                let data = read.slice(..).get_mapped_range().map_err(|e| GpuError::Failed(format!("GPU timing readback failed: {e}")))?;
                let ticks: &[u64] = bytemuck::cast_slice(&data);
                let period = f64::from(self.queue.get_timestamp_period());
                timing.kernel_ms = ticks.chunks(2).map(|p| p[1].saturating_sub(p[0]) as f64 * period * 1e-6).sum();
            }
            read.unmap();
        }
        let t_read = Instant::now();
        let mut channels = Vec::with_capacity(program.channels.len());
        {
            let data = slice.get_mapped_range().map_err(|e| GpuError::Failed(format!("GPU readback failed: {e}")))?;
            let bits: &[u16] = bytemuck::cast_slice(&data);
            for chunk in bits.chunks((n * TILE_TEXELS * 4).max(1)) {
                channels.push(chunk.iter().map(|&b| f16::from_bits(b)).collect::<Vec<f16>>());
            }
        }
        read_buf.unmap();
        drop(keep);
        timing.readback_ms = t_read.elapsed().as_secs_f64() * 1e3;
        Ok(GpuOutput { channels, timing })
    }
}
