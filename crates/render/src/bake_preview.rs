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
    /// Draw the lines of `set_seam_lines` (hard edges without a UV seam).
    pub show_seams: bool,
    pub seam_color: [f32; 3],
    /// Draw the front of the cage as a see-through surface.
    pub show_cage_surface: bool,
    pub cage_surface_color: [f32; 3],
    pub cage_surface_opacity: f32,
    /// Draw the project mesh and its cage for this material slot only (the selected texture set).
    pub only_slot: Option<u32>,
    /// Show the model with a neutral material (one colour, roughness and metallic) shaded by its
    /// baked normal and ambient occlusion maps instead of its painted channels. Used by the
    /// viewport when the high / low overlay is not drawn.
    pub neutral_material: bool,
    pub neutral_color: [f32; 3],
    pub neutral_roughness: f32,
    pub neutral_metallic: f32,
}

impl Default for BakeVisualization {
    fn default() -> Self {
        Self { show_high: true, show_low: true, high_color: [0.035, 0.23, 0.65], low_color: [0.65, 0.035, 0.025], cage_color: [0.55, 0.65, 0.72], cage_opacity: 0.28, show_seams: false, seam_color: [1.0, 0.85, 0.1],
            show_cage_surface: false, cage_surface_color: [0.82, 0.74, 0.55], cage_surface_opacity: 0.3, only_slot: None,
            neutral_material: false, neutral_color: [0.5, 0.5, 0.5], neutral_roughness: 0.5, neutral_metallic: 0.0 }
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
    /// Same layout again, for the seam-check lines: their colour and a small lift off the surface.
    uniform_seams: wgpu::Buffer,
    group_seams: wgpu::BindGroup,
    seams: Option<wgpu::Buffer>,
    seam_count: u32,
    /// Surfaces drawn blended without writing depth (the cage surface).
    veils: [wgpu::RenderPipeline; 2],
    /// Same layout again, for the cage surface: the cage distances, its colour and opacity.
    uniform_veil: wgpu::Buffer,
    group_veil: wgpu::BindGroup,
    /// The low poly's vertices marked to be pushed out to the front of the cage.
    cage_surface: Option<wgpu::Buffer>,
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
        let uniform_seams = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bake preview seam uniforms"), contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let group_seams = make_group(&uniform_seams);
        let uniform_veil = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bake preview cage surface uniforms"), contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let group_veil = make_group(&uniform_veil);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bake preview shader"), source: wgpu::ShaderSource::Wgsl(include_str!("bake_preview.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bake preview pipeline layout"), bind_group_layouts: &[Some(&layout)], immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x3];
        let make = |samples, lines, depth_write: bool| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bake preview pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex, attributes: &ATTRIBUTES })],
            },
            primitive: wgpu::PrimitiveState { topology: if lines { wgpu::PrimitiveTopology::LineList } else { wgpu::PrimitiveTopology::TriangleList }, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target::DEPTH_FORMAT, depth_write_enabled: Some(depth_write), depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(), bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: target::OUTPUT_FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None, cache: None,
        });
        Self { layout, sampler, _dummy: dummy, low_ranges: Vec::new(), miss_groups: Default::default(),
            surfaces: [make(1, false, true), make(SAMPLE_COUNT, false, true)], lines: [make(1, true, false), make(SAMPLE_COUNT, true, false)],
            veils: [make(1, false, false), make(SAMPLE_COUNT, false, false)], uniform_veil, group_veil, cage_surface: None,
            uniform, group, uniform_high, group_high, uniform_seams, group_seams, seams: None, seam_count: 0, low_index_count: 0, vertices: None, indices: None, cage: None, count: 0, cage_count: 0, diagonal: 1. }
    }

    pub fn set_meshes(&mut self, device: &wgpu::Device, meshes: Option<(&Mesh, &Mesh)>) {
        self.vertices = None; self.indices = None; self.cage = None; self.cage_surface = None; self.count = 0; self.low_ranges.clear(); self.miss_groups.clear();
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
        let buffer = |label, bytes, usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage });
        self.vertices = Some(buffer("bake surfaces", bytemuck::cast_slice(&vertices), wgpu::BufferUsages::VERTEX));
        self.indices = Some(buffer("bake surface indices", bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX));
        // The cage surface: the low poly again, every vertex marked for the front cage distance.
        self.upload_cage(device, low, &low.normals);
        self.count = indices.len() as u32; self.low_index_count = low.indices.len() as u32;
        self.diagonal = low.bounds.diagonal().max(high.bounds.diagonal()).max(1e-6);
    }

    /// Draw the cage (wireframe and surface) pushed out along `normals`, one per low-poly vertex:
    /// the averaged normals when the bake uses Average normals (issue #307), `None` for the low
    /// poly's own. Needs the meshes of `set_meshes`; `low` must be the same low poly.
    pub fn set_cage_normals(&mut self, device: &wgpu::Device, low: &Mesh, normals: Option<&[glam::Vec3]>) {
        if self.cage.is_none() { return; }
        let normals = normals.filter(|n| n.len() == low.positions.len()).unwrap_or(&low.normals);
        self.upload_cage(device, low, normals);
    }

    fn upload_cage(&mut self, device: &wgpu::Device, low: &Mesh, normals: &[glam::Vec3]) {
        let buffer = |label, bytes: &[u8]| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytes, usage: wgpu::BufferUsages::VERTEX });
        let (lines, surface) = cage_vertices(low, normals);
        self.cage = Some(buffer("bake cage", bytemuck::cast_slice(&lines)));
        self.cage_surface = Some(buffer("bake cage surface", bytemuck::cast_slice(&surface)));
        self.cage_count = lines.len() as u32;
    }

    /// Lines of the seam check: `[a, b, normal]` per edge of the low poly (hard edges without a
    /// UV seam). An empty list removes them. Independent of `set_meshes`.
    pub fn set_seam_lines(&mut self, device: &wgpu::Device, lines: &[[glam::Vec3; 3]]) {
        self.seams = None;
        self.seam_count = 0;
        if lines.is_empty() { return; }
        let vertices: Vec<Vertex> = lines.iter().flat_map(|[a, b, n]| [a, b].map(|p| Vertex { position: p.to_array(), normal: n.to_array(), color: [0., 0., 0., 1.], uv_low: [0.; 3] })).collect();
        self.seams = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("bake seam lines"), contents: bytemuck::cast_slice(&vertices), usage: wgpu::BufferUsages::VERTEX }));
        self.seam_count = vertices.len() as u32;
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
        queue.write_buffer(&self.uniform_veil, 0, bytemuck::bytes_of(&Uniforms {
            view_proj: vp, model: Mat4::IDENTITY.to_cols_array_2d(), cage: [distances[0] * self.diagonal, distances[1] * self.diagonal, 1., 0.], tint: rgb(vis.cage_surface_color),
            line: [vis.cage_surface_color[0], vis.cage_surface_color[1], vis.cage_surface_color[2], vis.cage_surface_opacity.clamp(0.0, 1.0)],
        }));
        // Seam lines sit a little above the surface so the low poly does not hide them.
        queue.write_buffer(&self.uniform_seams, 0, bytemuck::bytes_of(&Uniforms {
            view_proj: vp, model: Mat4::IDENTITY.to_cols_array_2d(), cage: [0.002 * self.diagonal, 0., 1., 0.], tint: rgb(vis.seam_color), line: rgb(vis.seam_color),
        }));
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, antialias: bool, show_cage: bool, vis: &BakeVisualization) {
        let (Some(vertices), Some(indices)) = (&self.vertices, &self.indices) else { return };
        // The low poly's index ranges to draw: every slot, or the one asked for.
        let whole = [(0..self.low_index_count, u32::MAX)];
        let ranges: &[(std::ops::Range<u32>, u32)] = if self.low_ranges.is_empty() { &whole } else { &self.low_ranges };
        let wanted = |slot: u32| vis.only_slot.is_none_or(|only| only == slot || slot == u32::MAX);
        pass.set_pipeline(&self.surfaces[usize::from(antialias)]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        if vis.show_low {
            for (range, slot) in ranges.iter().filter(|(_, slot)| wanted(*slot)) {
                pass.set_bind_group(0, self.miss_groups.get(slot).unwrap_or(&self.group), &[]);
                pass.draw_indexed(range.clone(), 0, 0..1);
            }
        }
        if vis.show_high {
            pass.set_bind_group(0, &self.group_high, &[]);
            pass.draw_indexed(self.low_index_count..self.count, 0, 0..1);
        }
        if vis.show_cage_surface && vis.cage_surface_opacity > 0.0 && let Some(surface) = &self.cage_surface {
            pass.set_pipeline(&self.veils[usize::from(antialias)]);
            pass.set_bind_group(0, &self.group_veil, &[]);
            pass.set_vertex_buffer(0, surface.slice(..));
            for (range, _) in ranges.iter().filter(|(_, slot)| wanted(*slot)) {
                pass.draw_indexed(range.clone(), 0, 0..1);
            }
        }
        if show_cage && let Some(cage) = &self.cage {
            pass.set_pipeline(&self.lines[usize::from(antialias)]);
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_vertex_buffer(0, cage.slice(..));
            // Six line vertices per low-poly triangle, the front cage first, then the rear one.
            let half = self.cage_count / 2;
            for (range, _) in ranges.iter().filter(|(_, slot)| wanted(*slot)) {
                let lines = range.start * 2..range.end * 2;
                pass.draw(lines.clone(), 0..1);
                pass.draw(half + lines.start..half + lines.end, 0..1);
            }
        }
        if vis.show_seams && let Some(seams) = &self.seams {
            pass.set_pipeline(&self.lines[usize::from(antialias)]);
            pass.set_bind_group(0, &self.group_seams, &[]);
            pass.set_vertex_buffer(0, seams.slice(..));
            pass.draw(0..self.seam_count, 0..1);
        }
    }
}

/// The cage's wireframe (front then rear, two vertices per triangle edge) and surface vertices
/// (one per low-poly vertex), pushed out along `normals` by the shader's cage distances.
fn cage_vertices(low: &Mesh, normals: &[glam::Vec3]) -> (Vec<Vertex>, Vec<Vertex>) {
    let mut lines = Vec::with_capacity(low.indices.len() * 4);
    for side in [1., -1.] {
        for tri in low.indices.as_chunks::<3>().0 {
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                for k in [a, b] {
                    let i = tri[k] as usize;
                    lines.push(Vertex { position: low.positions[i].to_array(), normal: normals[i].to_array(), color: [0.55, 0.65, 0.72, side], uv_low: [0.; 3] });
                }
            }
        }
    }
    let surface = low.positions.iter().zip(normals).map(|(p, n)| Vertex { position: p.to_array(), normal: n.to_array(), color: [0., 0., 0., 1.], uv_low: [0.; 3] }).collect();
    (lines, surface)
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

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_mesh::primitives;

    /// Issue #307: with Average normals on, the cage is drawn along the same averaged normals the
    /// baker's rays follow, not the low poly's own (split) normals.
    #[test]
    fn the_cage_follows_the_normals_it_is_given() {
        let cube = primitives::cube(2.0);
        let own = cage_vertices(&cube, &cube.normals);
        let averaged = subcraft_bake::averaged_normals(&cube);
        let smooth = cage_vertices(&cube, &averaged);
        assert_eq!((own.0.len(), own.1.len()), (smooth.0.len(), smooth.1.len()));
        let corner = |v: &Vertex| glam::Vec3::from(v.position).abs().min_element() > 0.99;
        for (o, s) in own.0.iter().chain(&own.1).zip(smooth.0.iter().chain(&smooth.1)).filter(|(o, _)| corner(o)) {
            let p = glam::Vec3::from(o.position);
            let n = glam::Vec3::from(o.normal);
            // Own normals: one face's axis. Averaged: the corner's diagonal, shared by its faces.
            assert!(n.abs().max_element() > 0.999, "own normal {n:?}");
            assert!((glam::Vec3::from(s.normal) - p.normalize()).length() < 1e-3, "averaged at {p:?}: {:?}", s.normal);
            assert_eq!(o.position, s.position);
        }
    }
}
