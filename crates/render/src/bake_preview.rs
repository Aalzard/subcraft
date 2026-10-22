//! Display-only high/low depth comparison. Both meshes use their original object coordinates.
//! The nearest surface wins: blue high poly, red exposed low poly. Cage lines use the baker's
//! exact front/rear distances; this is an inspection aid, not a watertight-volume validity test.
use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use glam::Mat4;
use subcraft_mesh::Mesh;
use wgpu::util::DeviceExt;

use crate::{SAMPLE_COUNT, target};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex { position: [f32; 3], normal: [f32; 3], color: [f32; 4] }
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms { view_proj: [[f32; 4]; 4], cage: [f32; 4] }

pub(crate) struct BakePreviewPass {
    surfaces: [wgpu::RenderPipeline; 2],
    lines: [wgpu::RenderPipeline; 2],
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
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
                binding: 0, visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None,
            }],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bake preview"), layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bake preview shader"), source: wgpu::ShaderSource::Wgsl(include_str!("bake_preview.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bake preview pipeline layout"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];
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
        Self { surfaces: [make(1, false), make(SAMPLE_COUNT, false)], lines: [make(1, true), make(SAMPLE_COUNT, true)],
            uniform, group, vertices: None, indices: None, cage: None, count: 0, cage_count: 0, diagonal: 1. }
    }

    pub fn set_meshes(&mut self, device: &wgpu::Device, meshes: Option<(&Mesh, &Mesh)>) {
        self.vertices = None; self.indices = None; self.cage = None; self.count = 0;
        let Some((low, high)) = meshes else { return };
        let mut vertices = Vec::with_capacity(low.vertex_count() + high.vertex_count());
        let mut indices = low.indices.clone();
        for (mesh, color) in [(low, [0.65, 0.035, 0.025, 0.]), (high, [0.035, 0.23, 0.65, 0.])] {
            vertices.extend(mesh.positions.iter().zip(&mesh.normals).map(|(p, n)| Vertex { position: p.to_array(), normal: n.to_array(), color }));
        }
        indices.extend(high.indices.iter().map(|i| i + low.vertex_count() as u32));
        // Front and rear bounds, sharing the same normal and diagonal convention as the baker.
        let mut cage = Vec::with_capacity(low.indices.len() * 4);
        for side in [1., -1.] {
            for tri in low.indices.as_chunks::<3>().0 {
                for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                    for k in [a, b] {
                        let i = tri[k] as usize;
                        cage.push(Vertex { position: low.positions[i].to_array(), normal: low.normals[i].to_array(), color: [0.55, 0.65, 0.72, side] });
                    }
                }
            }
        }
        let buffer = |label, bytes, usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage });
        self.vertices = Some(buffer("bake surfaces", bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX));
        self.indices = Some(buffer("bake surface indices", bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX));
        self.cage = Some(buffer("bake cage", bytemuck::cast_slice(&cage), wgpu::BufferUsages::VERTEX));
        self.count = indices.len() as u32; self.cage_count = cage.len() as u32;
        self.diagonal = low.bounds.diagonal().max(high.bounds.diagonal()).max(1e-6);
    }

    pub fn prepare(&self, queue: &wgpu::Queue, view_proj: Mat4, distances: [f32; 2]) {
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&Uniforms {
            view_proj: view_proj.to_cols_array_2d(), cage: [distances[0] * self.diagonal, distances[1] * self.diagonal, 0., 0.],
        }));
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, antialias: bool, show_cage: bool) {
        let (Some(vertices), Some(indices)) = (&self.vertices, &self.indices) else { return };
        pass.set_pipeline(&self.surfaces[usize::from(antialias)]);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, 0..1);
        if show_cage && let Some(cage) = &self.cage {
            pass.set_pipeline(&self.lines[usize::from(antialias)]);
            pass.set_vertex_buffer(0, cage.slice(..));
            pass.draw(0..self.cage_count, 0..1);
        }
    }
}
