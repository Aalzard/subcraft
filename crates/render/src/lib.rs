//! wgpu viewport renderer (ADR-0001/0003): draws the mesh with its composite channel textures
//! into an offscreen target that the UI shows as an image (and tests read back).
//!
//! Textures are updated per dirty tile straight from the composite `TiledImage`s (f16 data is
//! uploaded as-is, no conversion). Lighting: metal/roughness GGX with one key light and a
//! procedural sky environment (analytic irradiance + split-sum approximation); ACES tone map;
//! sRGB output.

pub mod gpu_mesh;
pub mod headless;
pub mod material;
pub mod preview;
pub mod target;

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use glam::{Mat4, Vec3};
use subcraft_core::ChannelKind;
use subcraft_image::TiledImage;
use subcraft_mesh::Mesh;
use wgpu::util::DeviceExt;

pub use egui_wgpu;
pub use egui_wgpu::wgpu as wgpu_api;
pub use gpu_mesh::GpuMesh;
pub use material::{ChannelTexture, GpuMaterial, SHADED_CHANNELS};
pub use preview::{PreviewPass, PreviewTarget};
pub use target::{DISPLAY_FORMAT, OUTPUT_FORMAT, RenderTarget};

/// Camera and lighting for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewParams {
    pub view: Mat4,
    pub proj: Mat4,
    pub eye: Vec3,
    /// Direction towards the key light (world space).
    pub light_dir: Vec3,
    pub light_intensity: f32,
    pub environment_intensity: f32,
    pub exposure: f32,
    /// Height-to-bump strength (UV units of slope per height unit; 0 disables bumps).
    pub height_scale: f32,
    /// Linear background color.
    pub background: [f32; 3],
}

impl ViewParams {
    pub fn new(view: Mat4, proj: Mat4) -> Self {
        Self {
            view,
            proj,
            eye: view.inverse().w_axis.truncate(),
            light_dir: Vec3::new(0.65, 0.75, 0.35).normalize(),
            light_intensity: 2.6,
            environment_intensity: 1.0,
            exposure: 1.0,
            height_scale: 0.02,
            background: [0.018, 0.019, 0.022],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    light_dir: [f32; 4],
    params: [f32; 4],
}

pub const SAMPLE_COUNT: u32 = 4;

pub struct ViewportRenderer {
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    defaults: HashMap<ChannelKind, ChannelTexture>,
    mesh: Option<GpuMesh>,
    materials: HashMap<u32, GpuMaterial>,
    target: Option<RenderTarget>,
    preview: PreviewPass,
    preview_target: Option<PreviewTarget>,
}

impl ViewportRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("viewport.wgsl").into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        }];
        for i in 0..SHADED_CHANNELS.len() as u32 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: i + 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        let material_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("material layout"), entries: &entries });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(gpu_mesh::Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target::DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLE_COUNT, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: OUTPUT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::bytes_of(&Globals::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let defaults = SHADED_CHANNELS
            .iter()
            .map(|&k| {
                let d = k.default_value().0;
                (k, ChannelTexture::constant(device, queue, [d[0], d[1], d[2], 1.0], composite_comps(k), k.key()))
            })
            .collect();
        Self { pipeline, globals, globals_bg, material_layout, sampler, defaults, mesh: None, materials: HashMap::new(), target: None, preview: PreviewPass::new(device), preview_target: None }
    }

    /// Replace the mesh (drops all material textures).
    pub fn set_mesh(&mut self, device: &wgpu::Device, mesh: &Mesh) {
        self.mesh = Some(GpuMesh::new(device, mesh));
        self.materials.clear();
    }

    pub fn has_mesh(&self) -> bool {
        self.mesh.is_some()
    }

    /// Upload `tiles` of a channel's composite (all tiles when `None`, or when the texture had
    /// to be (re)created because size or format changed).
    pub fn upload_channel(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material_slot: u32,
        kind: ChannelKind,
        image: &TiledImage,
        tiles: Option<&[usize]>,
    ) {
        let mat = self.materials.entry(material_slot).or_default();
        let recreate = mat.channels.get(&kind).is_none_or(|t| t.size != image.width() || t.comps != image.comps());
        if recreate {
            mat.channels.insert(kind, ChannelTexture::new(device, image.width(), image.comps(), kind.key()));
            mat.bind_group = None;
        }
        let tex = &mat.channels[&kind];
        if recreate || tiles.is_none() {
            let all: Vec<usize> = (0..image.tile_count()).collect();
            tex.upload(queue, image, &all);
        } else if let Some(t) = tiles {
            tex.upload(queue, image, t);
        }
    }

    /// View of an uploaded channel texture (UV view display).
    pub fn channel_view(&self, material_slot: u32, kind: ChannelKind) -> Option<&wgpu::TextureView> {
        self.materials.get(&material_slot)?.channels.get(&kind).map(|t| &t.view)
    }

    /// Render a display preview (sRGB 8-bit, egui-ready `display_view`) of an uploaded channel.
    pub fn channel_preview(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, material_slot: u32, kind: ChannelKind) -> Option<&PreviewTarget> {
        let tex = self.materials.get(&material_slot)?.channels.get(&kind)?;
        if self.preview_target.as_ref().is_none_or(|t| t.size != tex.size) {
            self.preview_target = Some(PreviewPass::target(device, tex.size));
        }
        let dst = self.preview_target.as_ref().expect("created above");
        self.preview.run(device, queue, &tex.view, kind.is_color(), dst);
        Some(dst)
    }

    fn ensure_bind_groups(&mut self, device: &wgpu::Device) {
        for mat in self.materials.values_mut() {
            if mat.bind_group.is_some() {
                continue;
            }
            let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) }];
            for (i, k) in SHADED_CHANNELS.iter().enumerate() {
                let view = mat.channels.get(k).map_or(&self.defaults[k].view, |t| &t.view);
                entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: wgpu::BindingResource::TextureView(view) });
            }
            mat.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material"),
                layout: &self.material_layout,
                entries: &entries,
            }));
        }
    }

    /// Render a frame into the offscreen target (resized as needed) and return the target.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2], params: &ViewParams) -> &RenderTarget {
        let size = [size[0].max(1), size[1].max(1)];
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            self.target = Some(RenderTarget::new(device, size, SAMPLE_COUNT));
        }
        self.ensure_bind_groups(device);
        let globals = Globals {
            view_proj: (params.proj * params.view).to_cols_array_2d(),
            eye: params.eye.extend(1.0).to_array(),
            light_dir: params.light_dir.normalize_or(Vec3::Y).extend(0.0).to_array(),
            params: [params.exposure, params.environment_intensity, params.light_intensity, params.height_scale],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let target = self.target.as_ref().expect("created above");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport") });
        {
            let bg = params.background;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&target.output_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: 1.0 }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(mesh) = &self.mesh {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                for r in &mesh.ranges {
                    let Some(bg) = self.materials.get(&r.material_slot).and_then(|m| m.bind_group.as_ref()) else { continue };
                    pass.set_bind_group(1, bg, &[]);
                    pass.draw_indexed(r.first_index..r.first_index + r.index_count, 0, 0..1);
                }
            }
        }
        queue.submit([encoder.finish()]);
        target
    }

    pub fn target(&self) -> Option<&RenderTarget> {
        self.target.as_ref()
    }
}

/// Stored components of a composite channel image (mirrors `subcraft_compositor`).
pub fn composite_comps(kind: ChannelKind) -> usize {
    if kind.components() == 3 { 4 } else { 1 }
}
