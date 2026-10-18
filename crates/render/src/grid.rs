//! A quiet, depth-tested floor grid. Geometry is cached per whole-model bounds, not per frame.
use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use glam::{Mat4, Vec3};
use subcraft_mesh::Aabb;
use wgpu::util::DeviceExt;

use crate::{SAMPLE_COUNT, target};

/// Display-only guides in world-axis directions, centered beneath the whole model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridParams {
    pub bounds: Aabb,
    /// X red, Y green (up), Z blue. Y is deliberately absent by default.
    pub axes: [bool; 3],
}

impl GridParams {
    pub fn new(bounds: Aabb) -> Self {
        Self {
            bounds,
            axes: [true, false, true],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
    kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    origin_radius: [f32; 4],
    eye: [f32; 4],
    axes: [f32; 4],
}

struct Geometry {
    vertices: Vec<Vertex>,
    origin: Vec3,
    radius: f32,
}

fn geometry(bounds: Aabb) -> Option<Geometry> {
    if !bounds.is_valid() || !bounds.min.is_finite() || !bounds.max.is_finite() {
        return None;
    }
    let scale = bounds.extent().max_element().max(1e-6);
    let desired = scale / 8.0;
    let decade = 10.0f32.powf(desired.log10().floor());
    let step = decade
        * if desired / decade >= 5.0 {
            5.0
        } else if desired / decade >= 2.0 {
            2.0
        } else {
            1.0
        };
    let radius = step * 30.0;
    let mut origin = bounds.center();
    origin.y = bounds.min.y - step * 0.002; // avoid z-fighting with the model's base
    let mut vertices = Vec::with_capacity(246);
    let mut line = |a: Vec3, b: Vec3, color: [f32; 4], kind| {
        vertices.extend([
            Vertex {
                position: a.to_array(),
                color,
                kind,
            },
            Vertex {
                position: b.to_array(),
                color,
                kind,
            },
        ]);
    };
    for i in -30i32..=30 {
        let offset = i as f32 * step;
        let gray = [0.15, 0.16, 0.18, if i % 5 == 0 { 0.09 } else { 0.05 }];
        line(
            origin + Vec3::new(-radius, 0.0, offset),
            origin + Vec3::new(radius, 0.0, offset),
            if i == 0 {
                [0.30, 0.055, 0.045, 0.35]
            } else {
                gray
            },
            if i == 0 { 1 } else { 0 },
        );
        line(
            origin + Vec3::new(offset, 0.0, -radius),
            origin + Vec3::new(offset, 0.0, radius),
            if i == 0 {
                [0.045, 0.12, 0.32, 0.35]
            } else {
                gray
            },
            if i == 0 { 3 } else { 0 },
        );
    }
    line(
        origin,
        origin + Vec3::Y * radius * 0.8,
        [0.055, 0.30, 0.09, 0.35],
        2,
    );
    Some(Geometry {
        vertices,
        origin,
        radius,
    })
}

pub(crate) struct GridPass {
    pipeline: wgpu::RenderPipeline,
    pipeline_no_aa: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    group: wgpu::BindGroup,
    cached_bounds: Option<Aabb>,
    vertices: Option<wgpu::Buffer>,
    count: u32,
    origin_radius: [f32; 4],
}

impl GridPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid uniforms"),
            contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("grid.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("grid pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Uint32];
        let pipeline = |samples| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("grid pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &ATTRIBUTES,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: target::DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target::OUTPUT_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            pipeline: pipeline(SAMPLE_COUNT),
            pipeline_no_aa: pipeline(1),
            uniforms,
            group,
            cached_bounds: None,
            vertices: None,
            count: 0,
            origin_radius: [0.0; 4],
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: Mat4,
        eye: Vec3,
        params: GridParams,
    ) {
        if self.cached_bounds != Some(params.bounds) {
            self.cached_bounds = Some(params.bounds);
            self.vertices = geometry(params.bounds).map(|g| {
                self.count = g.vertices.len() as u32;
                self.origin_radius = g.origin.extend(g.radius).to_array();
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cached floor grid"),
                    contents: bytemuck::cast_slice(&g.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            });
        }
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                view_proj: view_proj.to_cols_array_2d(),
                origin_radius: self.origin_radius,
                eye: eye.extend(1.0).to_array(),
                axes: [
                    f32::from(params.axes[0]),
                    f32::from(params.axes[1]),
                    f32::from(params.axes[2]),
                    0.0,
                ],
            }),
        );
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, antialias: bool) {
        if let Some(vertices) = &self.vertices {
            pass.set_pipeline(if antialias {
                &self.pipeline
            } else {
                &self.pipeline_no_aa
            });
            pass.set_bind_group(0, &self.group, &[]);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..self.count, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_is_bounded_at_model_base_and_has_world_axis_directions() {
        let bounds = Aabb {
            min: Vec3::new(3.0, -2.0, 5.0),
            max: Vec3::new(5.0, 1.0, 7.0),
        };
        let g = geometry(bounds).unwrap();
        assert_eq!(g.vertices.len(), 246);
        assert!(g.origin.y < bounds.min.y && g.origin.y > bounds.min.y - 0.01);
        assert_eq!((g.origin.x, g.origin.z), (4.0, 6.0));
        for axis in 1..=3 {
            let pair = g
                .vertices
                .iter()
                .filter(|v| v.kind == axis)
                .collect::<Vec<_>>();
            assert_eq!(pair.len(), 2);
            let direction = (Vec3::from_array(pair[1].position)
                - Vec3::from_array(pair[0].position))
            .normalize();
            assert!(direction.distance([Vec3::X, Vec3::Y, Vec3::Z][axis as usize - 1]) < 1e-5);
        }
        assert_eq!(GridParams::new(bounds).axes, [true, false, true]);
        assert!(geometry(Aabb::EMPTY).is_none());
    }
}
