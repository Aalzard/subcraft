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
    /// Axis the grid plane is perpendicular to: 0 X, 1 Y (the floor), 2 Z.
    pub plane: usize,
    /// Linear color of the grid lines (the axis lines keep their own colors).
    pub color: [f32; 3],
    /// Multiplier on the lines' opacity (1 = the quiet default).
    pub opacity: f32,
}

/// The gray the grid lines have by default (linear).
pub const DEFAULT_GRID_COLOR: [f32; 3] = [0.15, 0.16, 0.18];

impl GridParams {
    pub fn new(bounds: Aabb) -> Self {
        Self {
            bounds,
            axes: [true, false, true],
            plane: 1,
            color: DEFAULT_GRID_COLOR,
            opacity: 1.0,
        }
    }
}

/// Spacing of the grid lines for a model of these bounds, in scene units (1, 2 or 5 times a
/// power of ten, about an eighth of the model's largest side).
pub fn grid_step(bounds: Aabb) -> Option<f32> {
    if !bounds.is_valid() || !bounds.min.is_finite() || !bounds.max.is_finite() {
        return None;
    }
    let scale = bounds.extent().max_element().max(1e-6);
    let desired = scale / 8.0;
    let decade = 10.0f32.powf(desired.log10().floor());
    Some(decade * if desired / decade >= 5.0 { 5.0 } else if desired / decade >= 2.0 { 2.0 } else { 1.0 })
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

fn geometry(params: &GridParams) -> Option<Geometry> {
    let bounds = params.bounds;
    let step = grid_step(bounds)?;
    let radius = step * 30.0;
    // The plane's normal axis `n` and the two axes it spans (for the floor: X and Z).
    let n = params.plane.min(2);
    let (u, v) = match n {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let axis = |i: usize| [Vec3::X, Vec3::Y, Vec3::Z][i];
    const AXIS_COLORS: [[f32; 4]; 3] = [[0.30, 0.055, 0.045, 0.35], [0.055, 0.30, 0.09, 0.35], [0.045, 0.12, 0.32, 0.35]];
    let mut origin = bounds.center();
    origin[n] = bounds.min[n] - step * 0.002; // avoid z-fighting with the model's base
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
    let opacity = params.opacity.clamp(0.0, 8.0);
    let [r, g, b] = params.color;
    for i in -30i32..=30 {
        let offset = i as f32 * step;
        let strength: f32 = if i % 5 == 0 { 0.09 } else { 0.05 };
        let gray = [r, g, b, (strength * opacity).min(1.0)];
        line(
            origin + axis(u) * -radius + axis(v) * offset,
            origin + axis(u) * radius + axis(v) * offset,
            if i == 0 { AXIS_COLORS[u] } else { gray },
            if i == 0 { u as u32 + 1 } else { 0 },
        );
        line(
            origin + axis(u) * offset + axis(v) * -radius,
            origin + axis(u) * offset + axis(v) * radius,
            if i == 0 { AXIS_COLORS[v] } else { gray },
            if i == 0 { v as u32 + 1 } else { 0 },
        );
    }
    line(origin, origin + axis(n) * radius * 0.8, AXIS_COLORS[n], n as u32 + 1);
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
    /// What the cached geometry was built for (the axis toggles are a uniform, not geometry).
    cached: Option<GridParams>,
    vertices: Option<wgpu::Buffer>,
    count: u32,
    origin_radius: [f32; 4],
}

impl GridPass {
    pub fn new(device: &wgpu::Device) -> Self {
        Self::create(device)
    }
    pub fn new_guides(device: &wgpu::Device) -> Self {
        Self::create(device)
    }
    fn create(device: &wgpu::Device) -> Self {
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
            cached: None,
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
        let key = GridParams { axes: [true; 3], ..params };
        if self.cached != Some(key) {
            self.cached = Some(key);
            self.vertices = geometry(&params).map(|g| {
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
                    params.plane.min(2) as f32,
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

    pub fn set_lines(&mut self, device: &wgpu::Device, lines: &[[Vec3; 2]]) {
        self.set_lines_colored(device, lines, [0.7, 0.015, 0.01, 0.85]);
    }
    /// Unlit, depth-nudged lines of one linear RGBA color.
    pub fn set_lines_colored(&mut self, device: &wgpu::Device, lines: &[[Vec3; 2]], color: [f32; 4]) {
        let vertices: Vec<_> = lines.iter().flat_map(|line| line.iter()).map(|p| Vertex { position: p.to_array(), color, kind: 4 }).collect();
        self.count = vertices.len() as u32;
        self.vertices = (!vertices.is_empty()).then(|| device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("symmetry guides"), contents: bytemuck::cast_slice(&vertices), usage: wgpu::BufferUsages::VERTEX,
        }));
    }
    pub fn prepare_lines(&self, queue: &wgpu::Queue, view_proj: Mat4) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&Uniforms { view_proj: view_proj.to_cols_array_2d(), origin_radius: [0., 0., 0., 1.], eye: [0., 2., 0., 1.], axes: [1.; 4] }));
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
        let g = geometry(&GridParams::new(bounds)).unwrap();
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
        assert!(geometry(&GridParams::new(Aabb::EMPTY)).is_none());
        assert!(grid_step(Aabb::EMPTY).is_none());
        // Largest side 3: an eighth is 0.375, so lines every 0.2 units.
        assert!((grid_step(bounds).unwrap() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn the_grid_plane_follows_the_chosen_axis_and_lines_take_the_color_and_opacity() {
        let bounds = Aabb { min: Vec3::new(3.0, -2.0, 5.0), max: Vec3::new(5.0, 1.0, 7.0) };
        let floor = geometry(&GridParams::new(bounds)).unwrap();
        for (plane, base) in [(0usize, 3.0f32), (1, -2.0), (2, 5.0)] {
            let g = geometry(&GridParams { plane, ..GridParams::new(bounds) }).unwrap();
            assert_eq!(g.vertices.len(), 246);
            // Every grid line lies in the plane just under the model's lowest point on that axis.
            let lines: Vec<_> = g.vertices.iter().filter(|v| v.kind != plane as u32 + 1).collect();
            assert_eq!(lines.len(), 244);
            assert!(lines.iter().all(|v| v.position[plane] == g.origin[plane]), "plane {plane}");
            assert!(g.origin[plane] < base && g.origin[plane] > base - 0.01);
            // The one line leaving the plane is that axis, from the origin, in its own color.
            let up: Vec<_> = g.vertices.iter().filter(|v| v.kind == plane as u32 + 1).collect();
            assert_eq!(up.len(), 2);
            let direction = (Vec3::from_array(up[1].position) - Vec3::from_array(up[0].position)).normalize();
            assert!(direction.distance([Vec3::X, Vec3::Y, Vec3::Z][plane]) < 1e-5);
            assert!(up[0].color[plane] >= 0.30, "axis {plane} keeps its red / green / blue");
        }
        // Default color and opacity are the quiet gray the grid always had.
        let gray = floor.vertices.iter().find(|v| v.kind == 0).unwrap().color;
        assert_eq!(gray[..3], DEFAULT_GRID_COLOR);
        assert!(gray[3] == 0.05 || gray[3] == 0.09);
        let loud = geometry(&GridParams { color: [0.9, 0.1, 0.2], opacity: 4.0, ..GridParams::new(bounds) }).unwrap();
        let line = loud.vertices.iter().find(|v| v.kind == 0).unwrap().color;
        assert_eq!(line[..3], [0.9, 0.1, 0.2]);
        assert!((line[3] - gray[3] * 4.0).abs() < 1e-6, "opacity multiplies the line strength");
        let axis = loud.vertices.iter().find(|v| v.kind == 1).unwrap().color;
        assert_eq!(axis, [0.30, 0.055, 0.045, 0.35], "axis lines are not recolored");
        assert_eq!(geometry(&GridParams { opacity: 0.0, ..GridParams::new(bounds) }).unwrap().vertices.iter().find(|v| v.kind == 0).unwrap().color[3], 0.0);
    }
}
