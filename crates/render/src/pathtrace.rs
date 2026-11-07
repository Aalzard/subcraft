//! Progressive GPU path tracing for the 3D view's Rendering mode (ADR-0001: wgpu compute; the
//! graphics library has no hardware ray tracing).
//!
//! Geometry is the session's SAH BVH flattened exactly as for the GPU baker (`ray_bake`); it is
//! uploaded only when the geometry key changes. Material channels are packed per material slot
//! into three `rgba16float` texture arrays by a compute pass, re-packed only for slots whose
//! composite was uploaded since (painting refreshes the image without touching the BVH).
//!
//! Samples accumulate while nothing that affects the image changes and restart otherwise.
//! Each frame dispatches as many one-sample passes as fit [`PathTraceSettings::frame_budget_ms`]
//! (measured, so the window never freezes), then the edge-aware denoiser and the display pass
//! write the viewport's ordinary [`RenderTarget`].

use std::collections::BTreeSet;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use glam::Mat4;
use subcraft_core::ChannelKind;
use subcraft_mesh::{Bvh, Mesh};

use crate::material::{ChannelTexture, GpuMaterial};
use crate::target::RenderTarget;
use crate::{EnvironmentParams, ViewParams};

/// Most one-sample passes in one frame (also the number of parameter slots).
const MAX_PASSES: u32 = 64;
const STACK_DEPTH: usize = 64;
/// Largest packed material texture, and the memory the three arrays may take together.
const MAX_TEXTURE: u32 = 2048;
const TEXTURE_BUDGET: u64 = 768 << 20;
/// Channels the packer reads, in binding order 1..=6.
const PACKED: [ChannelKind; 6] = [
    ChannelKind::BaseColor,
    ChannelKind::Roughness,
    ChannelKind::Metallic,
    ChannelKind::Normal,
    ChannelKind::Emissive,
    ChannelKind::Height,
];
const ARRAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// User settings of the Rendering mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathTraceSettings {
    /// Indirect bounces after the first hit.
    pub max_bounces: u32,
    /// Accumulation stops here (0 = never).
    pub max_samples: u32,
    pub denoise: bool,
    /// Gray ground disc under the model (contact shadows, floor bounce light).
    pub ground: bool,
    /// No new samples while paused; the current image stays.
    pub paused: bool,
    /// GPU time per frame spent on samples.
    pub frame_budget_ms: f32,
}

impl Default for PathTraceSettings {
    fn default() -> Self {
        Self { max_bounces: 4, max_samples: 1024, denoise: true, ground: true, paused: false, frame_budget_ms: 20.0 }
    }
}

/// Geometry of the frame: the session's mesh, its BVH and a key that changes with the geometry.
pub struct PathTraceScene<'a> {
    pub mesh: &'a Mesh,
    pub bvh: &'a Bvh,
    pub key: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PathTraceStatus {
    pub samples: u32,
    pub max_samples: u32,
    /// Samples per pixel per second over the last frame that traced.
    pub samples_per_second: f32,
    /// Passes dispatched this frame.
    pub passes: u32,
}

impl PathTraceStatus {
    pub fn converged(&self) -> bool {
        self.max_samples > 0 && self.samples >= self.max_samples
    }
}

/// `Err` with a message for the user when the device lacks what the path tracer needs.
pub fn check_support(device: &wgpu::Device) -> Result<(), String> {
    if crate::caps::safe_mode() { return Err("Path tracing is off in safe mode (SubCraft did not start cleanly last time). Restart normally to use it.".into()); }
    let l = device.limits();
    let mut missing = Vec::new();
    if l.max_storage_buffers_per_shader_stage < 7 {
        missing.push(format!("7 storage buffers per shader (this GPU: {})", l.max_storage_buffers_per_shader_stage));
    }
    if l.max_storage_textures_per_shader_stage < 3 {
        missing.push(format!("3 storage textures per shader (this GPU: {})", l.max_storage_textures_per_shader_stage));
    }
    if l.max_compute_invocations_per_workgroup < 64 || l.max_compute_workgroup_size_x < 8 || l.max_compute_workgroup_size_y < 8 {
        missing.push("8x8 compute workgroups".to_string());
    }
    if l.max_bind_groups < 2 || l.max_sampled_textures_per_shader_stage < 6 {
        missing.push("2 bind groups with 6 textures".to_string());
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("Path tracing needs GPU compute support this adapter lacks: {}", missing.join(", ")))
    }
}

/// Trace resolution whose per-pixel buffers (16 bytes per pixel) fit `max_buffer` bytes: `size`
/// scaled down with its aspect kept (the view stretches it, like the resolution setting).
pub fn fit_trace_size(size: [u32; 2], max_buffer: u64) -> [u32; 2] {
    let pixels = size[0] as u64 * size[1] as u64;
    let max_pixels = (max_buffer / 16).max(1);
    if pixels <= max_pixels {
        return size;
    }
    let scale = (max_pixels as f64 / pixels as f64).sqrt() * 0.999;
    size.map(|s| ((s as f64 * scale).floor() as u32).max(1))
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TraceParams {
    inv_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    light_dir: [f32; 4],
    light_color: [f32; 4],
    background: [f32; 4],
    ground: [f32; 4],
    horizon: [f32; 4],
    zenith: [f32; 4],
    lights: [f32; 4],
    env: [f32; 4],
    floor: [f32; 4],
    size: [u32; 4],
    misc: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DisplayParams {
    size: [u32; 4],
    look: [f32; 4],
}

/// Everything that changes the traced image (display-only settings are not in here).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Signature {
    view: Mat4,
    proj: Mat4,
    light: [f32; 8],
    environment: EnvironmentParams,
    environment_intensity: f32,
    height_scale: f32,
    background: [f32; 3],
    size: [u32; 2],
    bounces: u32,
    ground: bool,
    geometry: u64,
    revision: u64,
}

struct Geometry {
    key: u64,
    nodes: wgpu::Buffer,
    tris: wgpu::Buffer,
    tri_info: wgpu::Buffer,
    verts: wgpu::Buffer,
    /// Material slots (texture array layers).
    layers: u32,
    floor: [f32; 4],
    eps: f32,
}

struct Packed {
    size: u32,
    layers: u32,
    textures: [wgpu::Texture; 3],
    views: [wgpu::TextureView; 3],
}

/// Per-resolution buffers.
struct Frame {
    size: [u32; 2],
    accum: wgpu::Buffer,
    albedo: wgpu::Buffer,
    normal: wgpu::Buffer,
    ping: wgpu::Buffer,
    pong: wgpu::Buffer,
}

impl Frame {
    fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let bytes = (size[0] as u64 * size[1] as u64 * 16).max(16);
        let make = |label| device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        Self { size, accum: make("pt accum"), albedo: make("pt albedo"), normal: make("pt normal"), ping: make("pt ping"), pong: make("pt pong") }
    }
}

fn storage_entry(binding: u32, read_only: bool, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn uniform_entry(binding: u32, dynamic: bool, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: dynamic, min_binding_size: None },
        count: None,
    }
}

fn texture_entry(binding: u32, dimension: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: dimension, multisampled: false },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }
}

fn compute_pipeline(device: &wgpu::Device, label: &str, module: &wgpu::ShaderModule, entry: &str, layouts: &[&wgpu::BindGroupLayout]) -> wgpu::ComputePipeline {
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(|l| Some(*l)).collect();
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &layouts, immediate_size: 0 });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pl),
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

/// The edge-aware à-trous filter (separate so it can be tested on its own).
pub struct Denoiser {
    prepare: wgpu::ComputePipeline,
    atrous: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    stride: u64,
}

/// À-trous iterations (strides 1, 2, 4, 8, 16).
const ATROUS_PASSES: u32 = 5;

impl Denoiser {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pt denoise"),
            source: wgpu::ShaderSource::Wgsl(include_str!("denoise.wgsl").into()),
        });
        let c = wgpu::ShaderStages::COMPUTE;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pt denoise"),
            entries: &[storage_entry(0, true, c), storage_entry(1, true, c), storage_entry(2, true, c), storage_entry(3, true, c), storage_entry(4, false, c), uniform_entry(5, true, c)],
        });
        let stride = (device.limits().min_uniform_buffer_offset_alignment as u64).max(16);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pt denoise params"),
            size: stride * (ATROUS_PASSES as u64 + 2),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            prepare: compute_pipeline(device, "pt denoise prepare", &module, "prepare", &[&layout]),
            atrous: compute_pipeline(device, "pt denoise atrous", &module, "atrous", &[&layout]),
            finish: compute_pipeline(device, "pt denoise finish", &module, "finish", &[&layout]),
            layout,
            params,
            stride,
        }
    }

    /// Denoise the mean of `samples` accumulated samples into `frame.ping`.
    fn encode(&self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, frame: &Frame, samples: u32) {
        let [w, h] = frame.size;
        let group = |src: &wgpu::Buffer, dst: &wgpu::Buffer| device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pt denoise"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: frame.accum.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: frame.albedo.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: frame.normal.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: src.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: dst.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.params, offset: 0, size: wgpu::BufferSize::new(16) }),
                },
            ],
        });
        let to_ping = group(&frame.pong, &frame.ping);
        let to_pong = group(&frame.ping, &frame.pong);
        for k in 0..ATROUS_PASSES + 2 {
            let stride = if (1..=ATROUS_PASSES).contains(&k) { 1u32 << (k - 1) } else { 1 };
            queue.write_buffer(&self.params, k as u64 * self.stride, bytemuck::cast_slice(&[w, h, stride, samples]));
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("pt denoise"), timestamp_writes: None });
        let groups = [w.div_ceil(8), h.div_ceil(8)];
        let offset = |k: u32| (k as u64 * self.stride) as u32;
        // prepare: -> ping; à-trous ping -> pong -> ping ...; finish: last -> other.
        pass.set_pipeline(&self.prepare);
        pass.set_bind_group(0, &to_ping, &[offset(0)]);
        pass.dispatch_workgroups(groups[0], groups[1], 1);
        pass.set_pipeline(&self.atrous);
        for k in 1..=ATROUS_PASSES {
            pass.set_bind_group(0, if k % 2 == 1 { &to_pong } else { &to_ping }, &[offset(k)]);
            pass.dispatch_workgroups(groups[0], groups[1], 1);
        }
        // An odd number of passes leaves the result in pong; finish writes ping.
        debug_assert!(ATROUS_PASSES % 2 == 1);
        pass.set_pipeline(&self.finish);
        pass.set_bind_group(0, &to_ping, &[offset(ATROUS_PASSES + 1)]);
        pass.dispatch_workgroups(groups[0], groups[1], 1);
    }

    /// Denoise an image given as the per-pixel sum of `samples` samples (`accum`, w = sum of
    /// squared luminance), albedo (w = depth) and normal (w = 0 for background) guides.
    pub fn denoise_image(&self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2], samples: u32, accum: &[[f32; 4]], albedo: &[[f32; 4]], normal: &[[f32; 4]]) -> Vec<[f32; 4]> {
        let frame = Frame::new(device, size);
        queue.write_buffer(&frame.accum, 0, bytemuck::cast_slice(accum));
        queue.write_buffer(&frame.albedo, 0, bytemuck::cast_slice(albedo));
        queue.write_buffer(&frame.normal, 0, bytemuck::cast_slice(normal));
        let mut encoder = device.create_command_encoder(&Default::default());
        self.encode(device, queue, &mut encoder, &frame, samples);
        queue.submit([encoder.finish()]);
        read_vec4(device, queue, &frame.ping, size)
    }
}

fn read_vec4(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer, size: [u32; 2]) -> Vec<[f32; 4]> {
    let bytes = size[0] as u64 * size[1] as u64 * 16;
    let read = device.create_buffer(&wgpu::BufferDescriptor { label: Some("pt readback"), size: bytes, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &read, 0, bytes);
    queue.submit([encoder.finish()]);
    let slice = read.slice(..);
    slice.map_async(wgpu::MapMode::Read, |r| r.expect("map path tracer readback"));
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let out = bytemuck::cast_slice::<u8, [f32; 4]>(&slice.get_mapped_range().expect("mapped readback")).to_vec();
    read.unmap();
    out
}

/// Deepest root-to-leaf path (nodes) of a flattened tree.
fn tree_depth(nodes: &[[u32; 8]]) -> usize {
    let mut deepest = 0;
    let mut todo = vec![(0usize, 1usize)];
    while let Some((i, d)) = todo.pop() {
        if i >= nodes.len() {
            continue;
        }
        deepest = deepest.max(d);
        if nodes[i][7] == 0 {
            let l = nodes[i][3] as usize;
            todo.push((l, d + 1));
            todo.push((l + 1, d + 1));
        }
    }
    deepest
}

pub(crate) struct PathTracer {
    trace: wgpu::ComputePipeline,
    trace_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    pack: wgpu::ComputePipeline,
    pack_layout: wgpu::BindGroupLayout,
    display: wgpu::RenderPipeline,
    display_layout: wgpu::BindGroupLayout,
    denoiser: Denoiser,
    params: wgpu::Buffer,
    param_stride: u64,
    display_params: wgpu::Buffer,
    sampler: wgpu::Sampler,
    pack_sampler: wgpu::Sampler,
    geometry: Option<Geometry>,
    packed: Option<Packed>,
    /// Material slots to re-pack; `all_dirty` re-packs every slot.
    dirty: BTreeSet<u32>,
    all_dirty: bool,
    /// Bumped when packed textures or the environment change.
    revision: u64,
    frame: Option<Frame>,
    signature: Option<Signature>,
    samples: u32,
    seed: u32,
    passes_per_frame: u32,
    samples_per_second: f32,
    /// Exposure, tone map and denoise of the image last written to the target.
    shown: Option<([f32; 2], bool, u32)>,
}

impl PathTracer {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let c = wgpu::ShaderStages::COMPUTE;
        let trace_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("path trace"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pathtrace.wgsl").into()),
        });
        let trace_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("path trace"),
            entries: &[
                storage_entry(0, true, c),
                storage_entry(1, true, c),
                storage_entry(2, true, c),
                storage_entry(3, true, c),
                storage_entry(4, false, c),
                storage_entry(5, false, c),
                storage_entry(6, false, c),
                uniform_entry(7, true, c),
            ],
        });
        let d2a = wgpu::TextureViewDimension::D2Array;
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("path trace textures"),
            entries: &[
                sampler_entry(0),
                texture_entry(1, d2a),
                texture_entry(2, d2a),
                texture_entry(3, d2a),
                texture_entry(4, wgpu::TextureViewDimension::D2),
                sampler_entry(5),
            ],
        });
        let trace = compute_pipeline(device, "path trace", &trace_module, "main", &[&trace_layout, &texture_layout]);

        let pack_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pt pack"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pathtrace_pack.wgsl").into()),
        });
        let mut pack_entries = vec![sampler_entry(0)];
        pack_entries.extend((1..=6).map(|b| texture_entry(b, wgpu::TextureViewDimension::D2)));
        pack_entries.extend((7..=9).map(|b| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: c,
            ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: ARRAY_FORMAT, view_dimension: wgpu::TextureViewDimension::D2 },
            count: None,
        }));
        let pack_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("pt pack"), entries: &pack_entries });
        let pack = compute_pipeline(device, "pt pack", &pack_module, "main", &[&pack_layout]);

        let display_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pt display"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pathtrace_display.wgsl").into()),
        });
        let f = wgpu::ShaderStages::FRAGMENT;
        let display_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pt display"),
            entries: &[storage_entry(0, true, f), storage_entry(1, true, f), uniform_entry(2, false, f)],
        });
        let display_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pt display"), bind_group_layouts: &[Some(&display_layout)], immediate_size: 0 });
        let display = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pt display"),
            layout: Some(&display_pl),
            vertex: wgpu::VertexState { module: &display_module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &display_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: crate::OUTPUT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let param_stride = (device.limits().min_uniform_buffer_offset_alignment as u64).max(std::mem::size_of::<TraceParams>() as u64);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pt params"),
            size: param_stride * MAX_PASSES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let display_params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pt display params"),
            size: std::mem::size_of::<DisplayParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pt material"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pack_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pt pack"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            trace,
            trace_layout,
            texture_layout,
            pack,
            pack_layout,
            display,
            display_layout,
            denoiser: Denoiser::new(device),
            params,
            param_stride,
            display_params,
            sampler,
            pack_sampler,
            geometry: None,
            packed: None,
            dirty: BTreeSet::new(),
            all_dirty: true,
            revision: 0,
            frame: None,
            signature: None,
            samples: 0,
            seed: 0,
            passes_per_frame: 1,
            samples_per_second: 0.0,
            shown: None,
        }
    }

    pub(crate) fn mark_slot(&mut self, slot: u32) {
        self.dirty.insert(slot);
    }

    pub(crate) fn mark_all(&mut self) {
        self.all_dirty = true;
    }

    pub(crate) fn environment_changed(&mut self) {
        self.revision += 1;
    }

    /// Mean of the accumulated samples, linear RGB (tests and diagnostics).
    pub(crate) fn read_mean(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<[f32; 4]>> {
        let frame = self.frame.as_ref()?;
        let n = self.samples.max(1) as f32;
        Some(read_vec4(device, queue, &frame.accum, frame.size).into_iter().map(|c| [c[0] / n, c[1] / n, c[2] / n, c[3] / n]).collect())
    }

    pub(crate) fn read_denoised(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<[f32; 4]>> {
        let frame = self.frame.as_ref()?;
        Some(read_vec4(device, queue, &frame.ping, frame.size))
    }

    fn upload_geometry(&mut self, device: &wgpu::Device, scene: &PathTraceScene) -> Result<(), String> {
        let flat = scene.bvh.flatten();
        if tree_depth(&flat.nodes) + 2 >= STACK_DEPTH {
            return Err("This model's BVH is too deep for GPU path tracing".into());
        }
        let mesh = scene.mesh;
        let layers = mesh.materials.len().max(mesh.tri_material.iter().map(|&m| m as usize + 1).max().unwrap_or(1)).max(1) as u32;
        let max_layers = device.limits().max_texture_array_layers;
        let tri_info: Vec<[u32; 4]> = flat
            .tri_ids
            .iter()
            .map(|&t| {
                let t = t as usize;
                let i = &mesh.indices[t * 3..t * 3 + 3];
                [i[0], i[1], i[2], mesh.tri_material.get(t).copied().unwrap_or(0).min(max_layers - 1)]
            })
            .collect();
        let verts: Vec<[f32; 12]> = (0..mesh.vertex_count())
            .map(|i| {
                let n = mesh.normals[i];
                let t = mesh.tangents.get(i).copied().unwrap_or(glam::Vec4::ZERO);
                let uv = mesh.uvs[i];
                [n.x, n.y, n.z, uv.x, t.x, t.y, t.z, t.w, uv.y, 0.0, 0.0, 0.0]
            })
            .collect();
        let limits = device.limits();
        let max = (limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size);
        for (what, bytes) in [("BVH nodes", flat.nodes.len() * 32), ("triangles", flat.tris.len() * 48), ("vertices", verts.len() * 48)] {
            if bytes as u64 > max {
                return Err(format!("Path tracing: the model's {what} need {} MB of GPU buffer, this GPU allows {} MB", bytes >> 20, max >> 20));
            }
        }
        let buffer = |label: &str, data: &[u8]| {
            use wgpu::util::DeviceExt;
            let mut padded = data.to_vec();
            padded.resize(data.len().max(16), 0);
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: &padded, usage: wgpu::BufferUsages::STORAGE })
        };
        let b = mesh.bounds;
        let center = (b.min + b.max) * 0.5;
        let diag = (b.max - b.min).length().max(1e-6);
        self.geometry = Some(Geometry {
            key: scene.key,
            nodes: buffer("pt nodes", bytemuck::cast_slice(&flat.nodes)),
            tris: buffer("pt tris", bytemuck::cast_slice(&flat.tris)),
            tri_info: buffer("pt tri info", bytemuck::cast_slice(&tri_info)),
            verts: buffer("pt verts", bytemuck::cast_slice(&verts)),
            layers: layers.min(max_layers),
            floor: [center.x, b.min.y - diag * 1e-4, center.z, diag * 1.5],
            eps: diag * 2e-5,
        });
        Ok(())
    }

    /// Packed texture size: the largest material channel (capped), shrunk to the memory budget.
    fn texture_size(device: &wgpu::Device, materials: &std::collections::HashMap<u32, GpuMaterial>, layers: u32) -> u32 {
        let largest = materials.values().flat_map(|m| PACKED.iter().filter_map(|k| m.channels.get(k).map(|t| t.size))).max().unwrap_or(16);
        let mut size = largest.clamp(16, MAX_TEXTURE.min(device.limits().max_texture_dimension_2d)).next_power_of_two().min(MAX_TEXTURE);
        while size > 256 && 3 * 8 * (size as u64).pow(2) * layers as u64 > TEXTURE_BUDGET {
            size /= 2;
        }
        size
    }

    fn pack(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, materials: &std::collections::HashMap<u32, GpuMaterial>, defaults: &std::collections::HashMap<ChannelKind, ChannelTexture>) -> bool {
        let layers = self.geometry.as_ref().map_or(1, |g| g.layers);
        let size = Self::texture_size(device, materials, layers);
        if self.packed.as_ref().is_none_or(|p| p.size != size || p.layers != layers) {
            let make = |label| device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: layers },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: ARRAY_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            });
            let textures = [make("pt base+rough"), make("pt normal+metal"), make("pt emissive+height")];
            let views = [0, 1, 2].map(|i| textures[i].create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() }));
            self.packed = Some(Packed { size, layers, textures, views });
            self.all_dirty = true;
        }
        let slots: Vec<u32> = if self.all_dirty { (0..layers).collect() } else { self.dirty.iter().copied().filter(|&s| s < layers).collect() };
        self.all_dirty = false;
        self.dirty.clear();
        if slots.is_empty() {
            return false;
        }
        let packed = self.packed.as_ref().expect("created above");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pt pack") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("pt pack"), timestamp_writes: None });
            pass.set_pipeline(&self.pack);
            for slot in slots {
                let material = materials.get(&slot);
                let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.pack_sampler) }];
                for (i, k) in PACKED.iter().enumerate() {
                    let view = material.and_then(|m| m.channels.get(k)).map_or(&defaults[k].view, |t| &t.view);
                    entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: wgpu::BindingResource::TextureView(view) });
                }
                let layer_views: Vec<wgpu::TextureView> = packed
                    .textures
                    .iter()
                    .map(|t| t.create_view(&wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_array_layer: slot,
                        array_layer_count: Some(1),
                        ..Default::default()
                    }))
                    .collect();
                for (i, v) in layer_views.iter().enumerate() {
                    entries.push(wgpu::BindGroupEntry { binding: 7 + i as u32, resource: wgpu::BindingResource::TextureView(v) });
                }
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("pt pack"), layout: &self.pack_layout, entries: &entries });
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(packed.size.div_ceil(8), packed.size.div_ceil(8), 1);
            }
        }
        queue.submit([encoder.finish()]);
        true
    }

    /// Trace, denoise and display one frame into `target` (single-sample, `size`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget,
        params: &ViewParams,
        scene: &PathTraceScene,
        settings: &PathTraceSettings,
        materials: &std::collections::HashMap<u32, GpuMaterial>,
        defaults: &std::collections::HashMap<ChannelKind, ChannelTexture>,
        environment: (&wgpu::TextureView, &wgpu::Sampler, bool),
    ) -> Result<PathTraceStatus, String> {
        let size = target.size;
        if self.geometry.as_ref().is_none_or(|g| g.key != scene.key) {
            self.geometry = None;
            self.upload_geometry(device, scene)?;
            self.all_dirty = true;
        }
        if self.pack(device, queue, materials, defaults) {
            self.revision += 1;
        }
        if self.frame.as_ref().is_none_or(|f| f.size != size) {
            self.frame = Some(Frame::new(device, size));
        }
        let geometry = self.geometry.as_ref().expect("uploaded above");
        let e = params.environment;
        let (env_view, env_sampler, hdri) = environment;
        let signature = Signature {
            view: params.view,
            proj: params.proj,
            light: [params.light_dir.x, params.light_dir.y, params.light_dir.z, params.light_intensity, params.light_color[0], params.light_color[1], params.light_color[2], 0.0],
            environment: e,
            environment_intensity: params.environment_intensity,
            height_scale: params.height_scale,
            background: params.background,
            size,
            bounces: settings.max_bounces,
            ground: settings.ground,
            geometry: geometry.key,
            revision: self.revision,
        };
        if self.signature != Some(signature) {
            self.signature = Some(signature);
            self.samples = 0;
        }
        let remaining = if settings.max_samples == 0 { u32::MAX } else { settings.max_samples.saturating_sub(self.samples) };
        let passes = if settings.paused { 0 } else { self.passes_per_frame.min(remaining).min(MAX_PASSES) };
        // Never show an empty accumulation, even when paused right at a restart.
        let passes = if self.samples == 0 { passes.max(1) } else { passes };
        let denoise = settings.denoise && self.samples + passes > 0;
        let look = [params.exposure, params.post.tone_map.id()];
        if passes == 0 && self.shown == Some((look, denoise, self.samples)) {
            return Ok(self.status(settings, 0));
        }

        let frame = self.frame.as_ref().expect("created above");
        let inv_view_proj = (params.proj * params.view).inverse();
        let rotated = |d: glam::Vec3| d.normalize_or(glam::Vec3::Y);
        let v4 = |c: [f32; 3], w: f32| [c[0], c[1], c[2], w];
        let start = self.samples;
        for k in 0..passes {
            self.seed = self.seed.wrapping_add(1);
            let p = TraceParams {
                inv_view_proj: inv_view_proj.to_cols_array_2d(),
                eye: params.eye.extend(0.0).to_array(),
                light_dir: rotated(params.light_dir).extend(params.light_intensity).to_array(),
                light_color: v4(params.light_color, params.environment_intensity),
                background: v4(params.background, params.height_scale),
                ground: v4(e.ground, 0.0),
                horizon: v4(e.horizon, 0.0),
                zenith: v4(e.zenith, 0.0),
                lights: [e.panels, e.top, e.key_softbox, if hdri { 1.0 } else { 0.0 }],
                env: [e.rotation, e.hdri_intensity, geometry.eps, 20.0],
                floor: if settings.ground { geometry.floor } else { [0.0; 4] },
                size: [size[0], size[1], start + k, settings.max_bounces.min(16)],
                misc: [self.seed, (start + k == 0) as u32, 0, 0],
            };
            queue.write_buffer(&self.params, k as u64 * self.param_stride, bytemuck::bytes_of(&p));
        }
        let trace_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("path trace"),
            layout: &self.trace_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: geometry.nodes.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: geometry.tris.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: geometry.tri_info.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: geometry.verts.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: frame.accum.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: frame.albedo.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: frame.normal.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.params,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<TraceParams>() as u64),
                    }),
                },
            ],
        });
        let packed = self.packed.as_ref().expect("packed above");
        let texture_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("path trace textures"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&packed.views[0]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&packed.views[1]) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&packed.views[2]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(env_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(env_sampler) },
            ],
        });
        let started = Instant::now();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("path trace") });
        if passes > 0 {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("path trace"), timestamp_writes: None });
            pass.set_pipeline(&self.trace);
            pass.set_bind_group(1, &texture_group, &[]);
            for k in 0..passes {
                pass.set_bind_group(0, &trace_group, &[(k as u64 * self.param_stride) as u32]);
                pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
            }
        }
        let samples = self.samples + passes;
        if denoise {
            self.denoiser.encode(device, queue, &mut encoder, frame, samples);
        }
        queue.write_buffer(&self.display_params, 0, bytemuck::bytes_of(&DisplayParams { size: [size[0], size[1], samples, denoise as u32], look: [look[0], look[1], 0.0, 0.0] }));
        let display_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pt display"),
            layout: &self.display_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: frame.accum.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: frame.ping.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: self.display_params.as_entire_binding() },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pt display"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.display);
            pass.set_bind_group(0, &display_group, &[]);
            pass.draw(0..3, 0..1);
        }
        target.encode_display(&mut encoder);
        queue.submit([encoder.finish()]);
        // Wait so the next frame is sized from this one's real cost and the queue never backs up
        // (which is what would freeze the window).
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("Path tracing: GPU wait failed: {e}"))?;
        let took = started.elapsed().as_secs_f32().max(1e-5);
        self.samples = samples;
        self.shown = Some((look, denoise, samples));
        if passes > 0 {
            let per_pass = took / passes as f32;
            self.samples_per_second = passes as f32 / took;
            let fit = (settings.frame_budget_ms.max(1.0) * 1e-3 / per_pass).floor() as u32;
            self.passes_per_frame = fit.clamp(1, (self.passes_per_frame * 2).min(MAX_PASSES));
        }
        Ok(self.status(settings, passes))
    }

    fn status(&self, settings: &PathTraceSettings, passes: u32) -> PathTraceStatus {
        PathTraceStatus { samples: self.samples, max_samples: settings.max_samples, samples_per_second: self.samples_per_second, passes }
    }
}

#[cfg(test)]
mod tests {
    use super::{fit_trace_size, tree_depth};

    #[test]
    fn trace_size_fits_the_buffer_limit_keeping_the_aspect() {
        assert_eq!(fit_trace_size([1920, 1080], 256 << 20), [1920, 1080]);
        let [w, h] = fit_trace_size([8192, 5434], 256 << 20);
        assert!(w as u64 * h as u64 * 16 <= 256 << 20, "{w}x{h}");
        assert!(((w as f32 / h as f32) - 8192.0 / 5434.0).abs() < 0.01);
        assert_eq!(fit_trace_size([5, 5], 0), [1, 1]);
    }

    #[test]
    fn tree_depth_counts_the_longest_path_and_ignores_empty_trees() {
        assert_eq!(tree_depth(&[]), 0);
        // root -> (leaf, interior -> (leaf, leaf))
        let leaf = [0, 0, 0, 0, 0, 0, 0, 1];
        let nodes = [[0, 0, 0, 1, 0, 0, 0, 0], leaf, [0, 0, 0, 3, 0, 0, 0, 0], leaf, leaf];
        assert_eq!(tree_depth(&nodes), 3);
    }
}
