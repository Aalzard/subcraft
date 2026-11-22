//! Displacement preview: the mesh subdivided (midpoint, like File > Export mesh with
//! displacement) and its vertices moved along their normals by Height + Displacement on the GPU,
//! so the silhouette changes as the artist paints height. Every raster pass (shading, shadows,
//! depth for ambient occlusion, emission, transparency) draws the displaced mesh.
//!
//! The vertex positions are recomputed by a compute pass each frame the preview is on (a few
//! microseconds per thousand vertices), so painting height updates the shape at once. Picking and
//! painting still use the undisplaced mesh.

use std::collections::BTreeMap;

use egui_wgpu::wgpu;
use subcraft_mesh::Mesh;
use wgpu::util::DeviceExt;

use crate::gpu_mesh::{GpuMesh, vertices};

/// Triangle budget of the automatic subdivision level.
pub const AUTO_TRIANGLE_BUDGET: usize = 1_000_000;
/// Highest subdivision level (each multiplies the triangle count by 4).
pub const MAX_LEVEL: u32 = 4;

/// The level [`crate::ViewParams::displacement`] picks for a mesh of `triangles` when the user
/// leaves it on automatic: the finest that stays within [`AUTO_TRIANGLE_BUDGET`].
pub fn auto_level(triangles: usize) -> u32 {
    let mut level = 0;
    while level < MAX_LEVEL && triangles.saturating_mul(4usize.pow(level + 1)) <= AUTO_TRIANGLE_BUDGET {
        level += 1;
    }
    level
}

/// The subdivided mesh on the GPU at one level.
pub(crate) struct Displaced {
    pub level: u32,
    /// Drawn instead of the source mesh; positions are written by the compute pass.
    pub mesh: GpuMesh,
    /// Undisplaced vertices (read by the compute pass).
    src: wgpu::Buffer,
    /// Vertex indices of each material slot (a slot's Height channel moves them).
    slots: BTreeMap<u32, (wgpu::Buffer, u32)>,
}

pub(crate) struct DisplacePass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl DisplacePass {
    pub fn new(device: &wgpu::Device) -> Self {
        let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let texture = |binding: u32, view_dimension: wgpu::TextureViewDimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("displacement"),
            entries: &[
                storage(0, true),
                storage(1, false),
                storage(2, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                texture(4, wgpu::TextureViewDimension::D2),
                texture(5, wgpu::TextureViewDimension::D2Array),
                wgpu::BindGroupLayoutEntry { binding: 6, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("displacement"), source: wgpu::ShaderSource::Wgsl(include_str!("displace.wgsl").into()) });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("displacement"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("displacement"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("displacement sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { pipeline, layout, sampler }
    }

    /// `mesh` subdivided `level` times, ready to displace.
    pub fn build(&self, device: &wgpu::Device, mesh: &Mesh, level: u32) -> Displaced {
        let mut m = mesh.clone();
        for _ in 0..level.min(MAX_LEVEL) {
            m = subcraft_mesh::subdivide::midpoint(&m);
        }
        let verts = vertices(&m);
        let gpu = GpuMesh::with_usage(device, &m, &verts, wgpu::BufferUsages::STORAGE);
        let src = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("undisplaced vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let mut by_slot: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        let mut seen = vec![false; verts.len()];
        for t in 0..m.triangle_count() {
            let slot = m.tri_material.get(t).copied().unwrap_or(0);
            for v in m.tri(t) {
                if !std::mem::replace(&mut seen[v as usize], true) {
                    by_slot.entry(slot).or_default().push(v);
                }
            }
        }
        let slots = by_slot
            .into_iter()
            .map(|(slot, ids)| {
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("displaced vertex ids"), contents: bytemuck::cast_slice(&ids), usage: wgpu::BufferUsages::STORAGE });
                (slot, (buffer, ids.len() as u32))
            })
            .collect();
        Displaced { level, mesh: gpu, src, slots }
    }

    /// Record the displacement of every slot: `inputs` gives each material slot's Height view, its
    /// extended-channel array and the Displacement layer in it (-1 = none).
    pub fn encode(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, d: &Displaced, scale: f32, inputs: &dyn Fn(u32) -> (wgpu::TextureView, wgpu::TextureView, i32)) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("displacement"), timestamp_writes: None });
        pass.set_pipeline(&self.pipeline);
        for (slot, (ids, count)) in &d.slots {
            let (height, ext, layer) = inputs(*slot);
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("displacement params"),
                contents: bytemuck::cast_slice(&[scale, *count as f32, layer as f32, 0.0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("displacement"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: d.src.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: d.mesh.vertices.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: ids.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&height) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&ext) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_level_stays_within_the_triangle_budget() {
        assert_eq!(auto_level(12), MAX_LEVEL);
        assert_eq!(auto_level(10_000), 3); // 640k triangles
        assert_eq!(auto_level(100_000), 1); // 400k
        assert_eq!(auto_level(300_000), 0); // 1.2M would exceed it
        for t in [1, 999, 50_000, 250_000, 2_000_000] {
            let l = auto_level(t);
            assert!(l == 0 || t * 4usize.pow(l) <= AUTO_TRIANGLE_BUDGET);
        }
    }
}
