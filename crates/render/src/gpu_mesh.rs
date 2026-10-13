//! Mesh vertex/index buffers grouped by material slot.

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use subcraft_mesh::Mesh;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    pub uv: [f32; 2],
}

impl Vertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Index range of one material slot.
#[derive(Clone, Copy, Debug)]
pub struct DrawRange {
    pub material_slot: u32,
    pub first_index: u32,
    pub index_count: u32,
}

pub struct GpuMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub ranges: Vec<DrawRange>,
}

impl GpuMesh {
    pub fn new(device: &wgpu::Device, mesh: &Mesh) -> Self {
        let verts: Vec<Vertex> = (0..mesh.vertex_count())
            .map(|i| Vertex {
                position: mesh.positions[i].to_array(),
                normal: mesh.normals[i].to_array(),
                tangent: mesh.tangents.get(i).map_or([1.0, 0.0, 0.0, 1.0], |t| t.to_array()),
                uv: mesh.uvs[i].to_array(),
            })
            .collect();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let ranges = mesh
            .submeshes
            .iter()
            .map(|s| DrawRange { material_slot: s.material, first_index: s.first_triangle * 3, index_count: s.triangle_count * 3 })
            .collect();
        Self { vertices, indices, ranges }
    }
}
