//! Temporal anti-aliasing (Painter's Display Settings > Temporal anti-aliasing): while the view
//! stays still, each frame is drawn with the projection shifted by a different sub-pixel amount
//! (a Halton sequence) and averaged with the frames before it, so edges, thin specular highlights
//! and texture detail settle into a smooth image after the chosen number of accumulations.
//!
//! The caller numbers the frames ([`crate::ViewParams::taa_frame`]): 1 starts a new average
//! (after any change to the view or the scene), 0 turns it off.

use glam::{Mat4, Vec2, Vec3};

use egui_wgpu::wgpu;

/// Format of the running average (linear, more precision than the 8-bit output).
const HISTORY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Element `index` (from 1) of the Halton sequence in `base`: in [0, 1), evenly spread.
pub fn halton(mut index: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0f32, 0.0f32);
    while index > 0 {
        f /= base as f32;
        r += f * (index % base) as f32;
        index /= base;
    }
    r
}

/// Sub-pixel offset of frame `frame` (from 1), in pixels within [-0.5, 0.5). It cycles every 64
/// frames, and frame 1 is the pixel centre plus the sequence's first step.
pub fn jitter_pixels(frame: u32) -> Vec2 {
    let i = (frame.max(1) - 1) % 64 + 1;
    Vec2::new(halton(i, 2), halton(i, 3)) - Vec2::splat(0.5)
}

/// `proj` shifted by `pixels` on a `size` image (x right, y down, as screen pixels).
pub fn jittered_projection(proj: Mat4, pixels: Vec2, size: [u32; 2]) -> Mat4 {
    let ndc = Vec2::new(2.0 * pixels.x / size[0].max(1) as f32, -2.0 * pixels.y / size[1].max(1) as f32);
    Mat4::from_translation(Vec3::new(ndc.x, ndc.y, 0.0)) * proj
}

/// Weight of frame `frame` in the running average: the plain mean of the frames so far.
pub fn frame_weight(frame: u32) -> f32 {
    1.0 / frame.max(1) as f32
}

struct History {
    size: [u32; 2],
    views: [wgpu::TextureView; 2],
    _textures: [wgpu::Texture; 2],
}

pub(crate) struct Taa {
    layout: wgpu::BindGroupLayout,
    accum: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    history: Option<History>,
    /// History texture holding the latest average.
    latest: usize,
}

impl Taa {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("taa shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("taa.wgsl").into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("taa layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(1),
                tex(2),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("taa pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let fullscreen = |fs: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let accum = fullscreen("fs_accum", HISTORY_FORMAT);
        let resolve = fullscreen("fs_resolve", crate::target::OUTPUT_FORMAT);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("taa params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { layout, accum, resolve, uniform, history: None, latest: 0 }
    }

    /// Fold the frame just drawn into `target` into the average (frame `frame`, from 1) and
    /// put the average back into `target`'s output.
    pub(crate) fn encode(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &crate::target::RenderTarget, frame: u32) {
        if self.history.as_ref().is_none_or(|h| h.size != target.size) {
            let make = || device.create_texture(&wgpu::TextureDescriptor {
                label: Some("taa history"),
                size: wgpu::Extent3d { width: target.size[0], height: target.size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HISTORY_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let textures = [make(), make()];
            let views = [0, 1].map(|i| textures[i].create_view(&wgpu::TextureViewDescriptor::default()));
            self.history = Some(History { size: target.size, views, _textures: textures });
        }
        let history = self.history.as_ref().expect("created above");
        // A new average (frame 1) ignores the history, whatever size or content it had.
        let weight = frame_weight(frame);
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&[weight, 0.0f32, 0.0, 0.0]));
        let (prev, next) = (self.latest, 1 - self.latest);
        let group = |current: &wgpu::TextureView, previous: &wgpu::TextureView| device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("taa"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(current) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(previous) },
            ],
        });
        let accum_group = group(&target.output_view, &history.views[prev]);
        let resolve_group = group(&history.views[next], &history.views[next]);
        let pass = |encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, pipeline: &wgpu::RenderPipeline, bind: &wgpu::BindGroup, label| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        };
        pass(encoder, &history.views[next], &self.accum, &accum_group, "taa accumulate");
        pass(encoder, &target.output_view, &self.resolve, &resolve_group, "taa resolve");
        self.latest = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halton_points_spread_over_the_pixel() {
        assert_eq!((halton(1, 2), halton(2, 2), halton(3, 2)), (0.5, 0.25, 0.75));
        assert!((halton(1, 3) - 1.0 / 3.0).abs() < 1e-6 && (halton(2, 3) - 2.0 / 3.0).abs() < 1e-6);
        let offsets: Vec<Vec2> = (1..=16).map(jitter_pixels).collect();
        assert!(offsets.iter().all(|o| o.x >= -0.5 && o.x < 0.5 && o.y >= -0.5 && o.y < 0.5));
        // Spread: the mean is near the pixel centre and every quadrant gets samples.
        let mean = offsets.iter().copied().sum::<Vec2>() / offsets.len() as f32;
        assert!(mean.length() < 0.08, "{mean}");
        for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            assert!(offsets.iter().any(|o| o.x * sx > 0.0 && o.y * sy > 0.0), "quadrant {sx} {sy}");
        }
        assert_eq!(jitter_pixels(65), jitter_pixels(1), "the pattern repeats every 64 frames");
        assert_eq!(jitter_pixels(0), jitter_pixels(1));
    }

    #[test]
    fn the_jittered_projection_moves_points_by_exactly_the_offset() {
        let proj = Mat4::perspective_rh(0.8, 1.5, 0.1, 50.0);
        let size = [300, 200];
        let px = |m: Mat4, p: Vec3| {
            let c = m * p.extend(1.0);
            let n = c.truncate() / c.w;
            Vec2::new((n.x * 0.5 + 0.5) * size[0] as f32, (0.5 - n.y * 0.5) * size[1] as f32)
        };
        for p in [Vec3::new(0.3, -0.2, -2.0), Vec3::new(-1.0, 0.7, -9.0)] {
            let shift = Vec2::new(0.25, -0.375);
            let moved = px(jittered_projection(proj, shift, size), p) - px(proj, p);
            assert!((moved - shift).length() < 1e-3, "{moved} for {p}");
            // Depth is unchanged.
            let (a, b) = (jittered_projection(proj, shift, size) * p.extend(1.0), proj * p.extend(1.0));
            assert!((a.z / a.w - b.z / b.w).abs() < 1e-6);
        }
    }

    #[test]
    fn weights_make_the_plain_mean() {
        // Folding values in with 1/n gives their average.
        let values = [0.2f32, 0.9, 0.4, 0.7];
        let mut avg = 0.0;
        for (i, v) in values.iter().enumerate() {
            let w = frame_weight(i as u32 + 1);
            avg = if w >= 1.0 { *v } else { avg + (v - avg) * w };
        }
        assert!((avg - values.iter().sum::<f32>() / 4.0).abs() < 1e-6);
        assert_eq!(frame_weight(0), 1.0);
    }
}
