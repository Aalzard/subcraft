//! Display-only high/low depth comparison. Both meshes use their original object coordinates.
//! The nearest surface wins: blue high poly, red exposed low poly (colours and what is drawn
//! come from [`BakeVisualization`]). Cage lines use the baker's
//! exact front/rear distances; this is an inspection aid, not a watertight-volume validity test.
//! With the ray-miss heatmap on, the low poly is drawn from per-texture-set miss maps on its UVs:
//! gray where the baker's projection ray reaches the high poly, red where it misses.
use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use glam::Mat4;
use subcraft_image::TiledImage;
use subcraft_mesh::Mesh;
use wgpu::util::DeviceExt;

use crate::{SAMPLE_COUNT, target};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex { position: [f32; 3], normal: [f32; 3], color: [f32; 4], uv_low: [f32; 3] }
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms { view_proj: [[f32; 4]; 4], model: [[f32; 4]; 4], cage: [f32; 4], tint: [f32; 4], line: [f32; 4] }

/// What the bake overlay shows and in which colours (the Baking visualization settings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakeVisualization {
    /// Draw the high-poly mesh.
    pub show_high: bool,
    /// Draw the project (low-poly) mesh.
    pub show_low: bool,
    pub high_color: [f32; 3],
    /// The low poly where it shows through the high poly: the matching error colour.
    pub low_color: [f32; 3],
    pub cage_color: [f32; 3],
    pub cage_opacity: f32,
}

impl Default for BakeVisualization {
    fn default() -> Self {
        Self { show_high: true, show_low: true, high_color: [0.035, 0.23, 0.65], low_color: [0.65, 0.035, 0.025], cage_color: [0.55, 0.65, 0.72], cage_opacity: 0.28 }
    }
}

pub(crate) struct BakePreviewPass {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// 1×1 "no misses" map bound where a slot has no miss map yet (kept with the pass).
    _dummy: wgpu::TextureView,
    /// Low-poly index ranges per material slot (from the submeshes).
    low_ranges: Vec<(std::ops::Range<u32>, u32)>,
    /// Bind group with the low uniforms and the miss map of each material slot.
    miss_groups: std::collections::HashMap<u32, wgpu::BindGroup>,
    surfaces: [wgpu::RenderPipeline; 2],
    lines: [wgpu::RenderPipeline; 2],
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// Same layout as `uniform`, for the high poly: its placement matrix and opacity.
    uniform_high: wgpu::Buffer,
    group_high: wgpu::BindGroup,
    low_index_count: u32,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    cage: Option<wgpu::Buffer>,
    count: u32,
    cage_count: u32,
    diagonal: f32,
}

impl BakePreviewPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bake preview uniforms"), contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bake preview layout"), entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None,
            }, wgpu::BindGroupLayoutEntry {
                binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None,
            }, wgpu::BindGroupLayoutEntry {
                binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None,
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bake miss sampler"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default()
        });
        let dummy = miss_texture(device, None, 1, 1, &[0]);
        let make_group = |buffer: &wgpu::Buffer| bind_group(device, &layout, buffer, &dummy, &sampler);
        let group = make_group(&uniform);
        let uniform_high = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bake preview high uniforms"), contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let group_high = make_group(&uniform_high);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bake preview shader"), source: wgpu::ShaderSource::Wgsl(include_str!("bake_preview.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bake preview pipeline layout"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x3];
        let make = |samples, lines| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bake preview pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRIBUTES })],
            },
            primitive: wgpu::PrimitiveState { topology: if lines { wgpu::PrimitiveTopology::LineList } else { wgpu::PrimitiveTopology::TriangleList }, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target::DEPTH_FORMAT, depth_write_enabled: Some(!lines), depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(), bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: target::OUTPUT_FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None, cache: None,
        });
        Self { layout, sampler, _dummy: dummy, low_ranges: Vec::new(), miss_groups: Default::default(),
            surfaces: [make(1, false), make(SAMPLE_COUNT, false)], lines: [make(1, true), make(SAMPLE_COUNT, true)],
            uniform, group, uniform_high, group_high, low_index_count: 0, vertices: None, indices: None, cage: None, count: 0, cage_count: 0, diagonal: 1. }
    }

    pub fn set_meshes(&mut self, device: &wgpu::Device, meshes: Option<(&Mesh, &Mesh)>) {
        self.vertices = None; self.indices = None; self.cage = None; self.count = 0; self.low_ranges.clear(); self.miss_groups.clear();
        let Some((low, high)) = meshes else { return };
        let mut vertices = Vec::with_capacity(low.vertex_count() + high.vertex_count());
        let mut indices = low.indices.clone();
        for (mesh, color, is_low) in [(low, [0.65, 0.035, 0.025, 0.], 1.0), (high, [0.035, 0.23, 0.65, 0.], 0.0)] {
            vertices.extend(mesh.positions.iter().zip(&mesh.normals).enumerate().map(|(i, (p, n))| {
                let uv = mesh.uvs.get(i).copied().unwrap_or_default();
                Vertex { position: p.to_array(), normal: n.to_array(), color, uv_low: [uv.x, uv.y, is_low] }
            }));
        }
        self.low_ranges = low.submeshes.iter().map(|s| (s.first_triangle * 3..(s.first_triangle + s.triangle_count) * 3, s.material)).collect();
        indices.extend(high.indices.iter().map(|i| i + low.vertex_count() as u32));
        // Front and rear bounds, sharing the same normal and diagonal convention as the baker.
        let mut cage = Vec::with_capacity(low.indices.len() * 4);
        for side in [1., -1.] {
            for tri in low.indices.as_chunks::<3>().0 {
                for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                    for k in [a, b] {
                        let i = tri[k] as usize;
                        cage.push(Vertex { position: low.positions[i].to_array(), normal: low.normals[i].to_array(), color: [0.55, 0.65, 0.72, side], uv_low: [0.; 3] });
                    }
                }
            }
        }
        let buffer = |label, bytes, usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage });
        self.vertices = Some(buffer("bake surfaces", bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX));
        self.indices = Some(buffer("bake surface indices", bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX));
        self.cage = Some(buffer("bake cage", bytemuck::cast_slice(&cage), wgpu::BufferUsages::VERTEX));
        self.count = indices.len() as u32; self.low_index_count = low.indices.len() as u32; self.cage_count = cage.len() as u32;
        self.diagonal = low.bounds.diagonal().max(high.bounds.diagonal()).max(1e-6);
    }

    /// `high_model` places the high poly (object space of the loaded file to the low poly's space);
    /// `high_opacity` < 1 draws it see-through over the low poly.
    /// Miss maps (1 component, 1 = miss) per low-poly material slot for the ray-miss heatmap;
    /// an empty list removes them.
    pub fn set_miss_maps(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, maps: &[(u32, &TiledImage)]) {
        self.miss_groups.clear();
        for (slot, img) in maps {
            let (w, h) = (img.width(), img.height());
            let bytes: Vec<u8> = img.to_dense().iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();
            let view = miss_texture(device, Some(queue), w, h, &bytes);
            self.miss_groups.insert(*slot, bind_group(device, &self.layout, &self.uniform, &view, &self.sampler));
        }
    }

    pub fn prepare(&self, queue: &wgpu::Queue, view_proj: Mat4, distances: [f32; 2], high_model: Mat4, high_opacity: f32, heatmap: bool, vis: &BakeVisualization) {
        let vp = view_proj.to_cols_array_2d();
        let rgb = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
        let line = [vis.cage_color[0], vis.cage_color[1], vis.cage_color[2], vis.cage_opacity.clamp(0.0, 1.0)];
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&Uniforms {
            view_proj: vp, model: Mat4::IDENTITY.to_cols_array_2d(),
            cage: [distances[0] * self.diagonal, distances[1] * self.diagonal, 1., if heatmap { 1. } else { 0. }],
            tint: rgb(vis.low_color), line,
        }));
        queue.write_buffer(&self.uniform_high, 0, bytemuck::bytes_of(&Uniforms {
            view_proj: vp, model: high_model.to_cols_array_2d(), cage: [0., 0., high_opacity.clamp(0.05, 1.), 0.], tint: rgb(vis.high_color), line,
        }));
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, antialias: bool, show_cage: bool, vis: &BakeVisualization) {
        let (Some(vertices), Some(indices)) = (&self.vertices, &self.indices) else { return };
        pass.set_pipeline(&self.surfaces[usize::from(antialias)]);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        if !vis.show_low {
            // The project mesh is hidden.
        } else if self.miss_groups.is_empty() {
            pass.draw_indexed(0..self.low_index_count, 0, 0..1);
        } else {
            for (range, slot) in &self.low_ranges {
                pass.set_bind_group(0, self.miss_groups.get(slot).unwrap_or(&self.group), &[]);
                pass.draw_indexed(range.clone(), 0, 0..1);
            }
        }
        if vis.show_high {
            pass.set_bind_group(0, &self.group_high, &[]);
            pass.draw_indexed(self.low_index_count..self.count, 0, 0..1);
        }
        if show_cage && let Some(cage) = &self.cage {
            pass.set_pipeline(&self.lines[usize::from(antialias)]);
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_vertex_buffer(0, cage.slice(..));
            pass.draw(0..self.cage_count, 0..1);
        }
    }
}

/// An R8 texture (`queue` given: filled with `bytes`, row-major `w`x`h`).
fn miss_texture(device: &wgpu::Device, queue: Option<&wgpu::Queue>, w: u32, h: u32, bytes: &[u8]) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 };
    let desc = wgpu::TextureDescriptor {
        label: Some("bake miss map"), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[],
    };
    let texture = match queue {
        Some(queue) => device.create_texture_with_data(queue, &desc, wgpu::util::TextureDataOrder::LayerMajor, bytes),
        None => device.create_texture(&desc),
    };
    texture.create_view(&Default::default())
}

fn bind_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniform: &wgpu::Buffer, view: &wgpu::TextureView, sampler: &wgpu::Sampler) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bake preview"), layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}
