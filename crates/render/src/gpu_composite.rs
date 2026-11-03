//! GPU layer compositing backend (ADR-0044): implements `subcraft_compositor::gpu::GpuCompositeBackend`
//! with one WGSL compute pass per channel on a headless device of its own, so the viewport device
//! is never stalled.
//!
//! Paint and mask tiles are uploaded once into a pool buffer (32 KiB slots, f16, exactly the
//! stored texels) and found again by the identity of the `Arc<Tile>` (the cache holds a clone, so
//! copy-on-write guarantees an edited tile has a new key). Tiles the pool cannot hold at once
//! make the batch report `TooLarge`, and the compositor splits it. CPU-evaluated content travels
//! as f32 blocks for the batch only. The pool is a derived cache; nothing stored depends on it.

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
    pad: u32,
    default_value: [f32; 4],
}

struct Entry {
    _tile: Arc<Tile>,
    slot: u32,
    used: u64,
}

struct Pool {
    buffer: wgpu::Buffer,
    slots: u32,
    map: HashMap<usize, Entry>,
    free: Vec<u32>,
    stamp: u64,
}

pub struct GpuCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    pool: Mutex<Pool>,
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
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    if adapter.get_info().device_type == wgpu::DeviceType::Cpu && !allow_software {
        return None;
    }
    let supported = adapter.limits();
    let big: u64 = u64::from(supported.max_storage_buffer_binding_size).min(supported.max_buffer_size).min(1 << 30);
    let mut limits = wgpu::Limits::default();
    limits.max_storage_buffer_binding_size = big.max(limits.max_storage_buffer_binding_size);
    limits.max_buffer_size = big.max(limits.max_buffer_size);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("subcraft gpu composite"),
        required_limits: limits,
        ..Default::default()
    }))
    .ok()?;
    Some(GpuCompositor::new(&adapter, device, queue))
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
    fn new(adapter: &wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
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
        let pool_mb: u64 = std::env::var("SUBCRAFT_GPU_POOL_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
        let slots = ((pool_mb << 20).min(max_binding) / SLOT_BYTES).clamp(64, 1 << 20) as u32;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite tile pool"),
            size: slots as u64 * SLOT_BYTES,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pool = Pool { buffer, slots, map: HashMap::new(), free: (0..slots).rev().collect(), stamp: 0 };
        Self { device, queue, adapter_name: adapter.get_info().name, pipeline, layout, pool: Mutex::new(pool), max_binding }
    }

    fn buffer(&self, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage, mapped_at_creation: false })
    }

    fn upload(&self, label: &str, data: &[u8]) -> wgpu::Buffer {
        let buf = self.buffer(label, data.len() as u64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        if !data.is_empty() {
            self.queue.write_buffer(&buf, 0, data);
        }
        buf
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
        if slots.len() > pool.slots as usize {
            return Err(GpuError::TooLarge);
        }
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
        for (key, tile) in misses {
            let slot = pool.free.pop().ok_or(GpuError::TooLarge)?;
            let bits = tile.data.reinterpret_cast();
            self.queue.write_buffer(&pool.buffer, slot as u64 * SLOT_BYTES, bytemuck::cast_slice(bits));
            timing.uploaded_bytes += (bits.len() * 2) as u64;
            slots.insert(key, slot);
            pool.map.insert(key, Entry { _tile: tile, slot, used: stamp });
        }
        Ok(slots)
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

        let prog_buf = self.upload("composite program", bytemuck::cast_slice(&prog));
        let refs_buf = self.upload("composite refs", bytemuck::cast_slice(&refs));
        let consts_buf = self.upload("composite consts", bytemuck::cast_slice(&consts));
        let px_buf = self.upload("composite cpu content", bytemuck::cast_slice(&dyn_px));
        let mask_buf = self.upload("composite cpu masks", bytemuck::cast_slice(&dyn_mask));
        let channel_words = (n * TILE_TEXELS * 2) as u64;
        let out_bytes = channel_words * 4 * program.channels.len() as u64;
        if out_bytes > self.max_binding {
            return Err(GpuError::TooLarge);
        }
        let out_buf = self.buffer("composite out", out_bytes, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let read_buf = self.buffer("composite readback", out_bytes, wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);

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
                pad: 0,
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
                    wgpu::BindGroupEntry { binding: 4, resource: pool.buffer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: px_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: mask_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: out_buf.as_entire_binding() },
                ],
            });
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((TILE_TEXELS / 64) as u32, n as u32, 1);
            drop(pass);
            keep.push((ubuf, group));
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &read_buf, 0, out_bytes);
        timing.upload_ms = start.elapsed().as_secs_f64() * 1e3;
        let t_gpu = Instant::now();
        self.queue.submit([enc.finish()]);
        let slice = read_buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map composite output"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| GpuError::Failed(format!("GPU wait failed: {e}")))?;
        timing.gpu_ms = t_gpu.elapsed().as_secs_f64() * 1e3;
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
