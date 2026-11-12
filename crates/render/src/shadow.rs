//! Raster shadows of the key light (Painter's Display Settings > Shadows): a depth map of the
//! model seen from the light, fitted around the model's bounds, that the viewport shader compares
//! each pixel against with a percentage-closer filter. Only the key light's direct contribution
//! is shadowed; the environment light keeps its ambient-occlusion shading.

use egui_wgpu::wgpu;
use glam::{Mat4, Vec3};

use crate::gpu_mesh::{GpuMesh, Vertex};

/// Painter's shadow computation modes, as shadow-map size and filter width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ShadowQuality {
    #[default]
    Lightweight,
    Average,
    Intensive,
}

impl ShadowQuality {
    /// Side of the square shadow map in texels.
    pub fn map_size(self) -> u32 {
        match self {
            ShadowQuality::Lightweight => 1024,
            ShadowQuality::Average => 2048,
            ShadowQuality::Intensive => 4096,
        }
    }

    /// Filter radius in texels: (2r + 1)^2 comparisons per pixel.
    pub fn filter_radius(self) -> u32 {
        match self {
            ShadowQuality::Lightweight => 1,
            ShadowQuality::Average => 1,
            ShadowQuality::Intensive => 2,
        }
    }
}

/// Shadows of one frame. Off by default, which leaves the image exactly as before.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShadowParams {
    pub enabled: bool,
    /// How dark a fully shadowed spot gets: 0 (no shadow) .. 1 (no key light at all).
    pub opacity: f32,
    pub quality: ShadowQuality,
}

impl ShadowParams {
    pub fn active(&self) -> bool {
        self.enabled && self.opacity > 0.0
    }
}

/// View-projection of the key light looking at the model: an orthographic box around the
/// bounding sphere of `bounds`, seen from the side `towards_light` points to. Every point of the
/// bounds lands inside the box (x, y in -1..1, depth in 0..1).
pub fn light_view_proj(towards_light: Vec3, bounds: (Vec3, Vec3)) -> Mat4 {
    let dir = towards_light.normalize_or(Vec3::Y);
    let center = (bounds.0 + bounds.1) * 0.5;
    let radius = ((bounds.1 - bounds.0).length() * 0.5).max(1e-4) * 1.02;
    let eye = center + dir * radius * 2.0;
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let view = Mat4::look_at_rh(eye, center, up);
    // The sphere spans eye distances radius..3 radius; keep a margin on both sides.
    let proj = Mat4::orthographic_rh(-radius, radius, -radius, radius, radius * 0.5, radius * 3.5);
    proj * view
}

/// World size of one shadow-map texel for these bounds and map size (the normal offset that
/// keeps lit surfaces from shadowing themselves is a few of these).
pub fn texel_world_size(bounds: (Vec3, Vec3), map_size: u32) -> f32 {
    let radius = ((bounds.1 - bounds.0).length() * 0.5).max(1e-4) * 1.02;
    2.0 * radius / map_size.max(1) as f32
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub(crate) struct ShadowPass {
    pipeline: wgpu::RenderPipeline,
    light: wgpu::Buffer,
    light_bg: wgpu::BindGroup,
    /// The map in use and its size, made on first use.
    map: Option<(u32, wgpu::TextureView)>,
    /// 1x1 map bound while shadows are off (the bind group always needs a depth texture).
    placeholder: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
}

fn depth_view(device: &wgpu::Device, size: u32, label: &str) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl ShadowPass {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("shadow shader"), source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shadow pipeline layout"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &[Some(Vertex::layout())] },
            fragment: None,
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                // Slope-scaled bias against shadow acne on surfaces facing the light at an angle.
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        use wgpu::util::DeviceExt;
        let light = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shadow light"),
            contents: bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let light_bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("shadow light"), layout: &layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: light.as_entire_binding() }] });
        let placeholder = depth_view(device, 1, "shadow placeholder");
        // Clear the placeholder to the far plane: nothing in it ever shadows.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("shadow placeholder clear") });
        Self::clear(&mut encoder, &placeholder);
        queue.submit([encoder.finish()]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        Self { pipeline, light, light_bg, map: None, placeholder, sampler }
    }

    fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow clear"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    /// The view to bind: the shadow map when one exists, the placeholder otherwise.
    pub(crate) fn view(&self) -> &wgpu::TextureView {
        self.map.as_ref().map_or(&self.placeholder, |m| &m.1)
    }

    /// Make sure the map has `size` texels per side. Returns true when the view changed (the
    /// globals bind group must then be rebuilt).
    pub(crate) fn ensure(&mut self, device: &wgpu::Device, size: u32) -> bool {
        if self.map.as_ref().is_some_and(|m| m.0 == size) {
            return false;
        }
        self.map = Some((size, depth_view(device, size, "shadow map")));
        true
    }

    /// Render the depth of every visible range of `mesh` as seen through `view_proj`.
    pub(crate) fn encode(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, mesh: &GpuMesh, hidden: &std::collections::HashSet<u32>, view_proj: Mat4) {
        let Some((_, view)) = &self.map else { return };
        queue.write_buffer(&self.light, 0, bytemuck::cast_slice(&view_proj.to_cols_array()));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.light_bg, &[]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        for r in &mesh.ranges {
            if hidden.contains(&r.material_slot) {
                continue;
            }
            pass.draw_indexed(r.first_index..r.first_index + r.index_count, 0, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_box_holds_the_whole_model_from_any_direction() {
        let bounds = (Vec3::new(-1.0, -0.5, -2.0), Vec3::new(3.0, 1.5, 0.5));
        for dir in [Vec3::Y, Vec3::new(0.65, 0.75, 0.35), Vec3::NEG_Y, Vec3::X, Vec3::new(-0.2, 0.1, -0.9), Vec3::new(0.0, 1.0, 0.0001)] {
            let m = light_view_proj(dir, bounds);
            for i in 0..8 {
                let p = Vec3::new(
                    if i & 1 == 0 { bounds.0.x } else { bounds.1.x },
                    if i & 2 == 0 { bounds.0.y } else { bounds.1.y },
                    if i & 4 == 0 { bounds.0.z } else { bounds.1.z },
                );
                let c = m.project_point3(p);
                assert!(c.x.abs() <= 1.0 && c.y.abs() <= 1.0 && (0.0..=1.0).contains(&c.z), "{dir:?}: corner {p:?} at {c:?}");
            }
            // Nearer the light is smaller depth.
            let center = (bounds.0 + bounds.1) * 0.5;
            assert!(m.project_point3(center + dir.normalize()).z < m.project_point3(center).z);
        }
    }

    #[test]
    fn quality_sets_map_size_and_filter_and_the_default_is_off() {
        assert_eq!(ShadowQuality::default(), ShadowQuality::Lightweight);
        assert!(ShadowQuality::Lightweight.map_size() < ShadowQuality::Average.map_size() && ShadowQuality::Average.map_size() < ShadowQuality::Intensive.map_size());
        assert!(ShadowQuality::Intensive.filter_radius() >= ShadowQuality::Lightweight.filter_radius());
        assert!(!ShadowParams::default().active());
        assert!(!ShadowParams { enabled: true, opacity: 0.0, ..Default::default() }.active());
        assert!(ShadowParams { enabled: true, opacity: 0.5, ..Default::default() }.active());
        let b = (Vec3::splat(-1.0), Vec3::splat(1.0));
        assert!((texel_world_size(b, 1024) - 2.0 * 3f32.sqrt() * 1.02 / 1024.0).abs() < 1e-6);
    }
}
