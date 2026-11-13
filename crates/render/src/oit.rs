//! Order-independent transparency (Painter's Display Settings > Render settings): alpha-blended
//! texture sets are drawn in any order into two targets, the weighted sum of their colours and
//! how much of the background they leave visible, then averaged over the opaque image in one
//! full-screen pass (weighted blended OIT, McGuire and Bavoil 2013). Overlapping transparent
//! surfaces then look the same whatever order the mesh lists them in, which plain blending
//! (drawn in mesh order) cannot promise.

use egui_wgpu::wgpu;

/// Weighted colour sum (rgb premultiplied by alpha and weight, a: weighted alpha).
const ACCUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Product of (1 - alpha) of every layer: the share of the background still visible.
const REVEAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

/// Weight of a layer at window depth `z` (0 near .. 1 far) with coverage `alpha`: nearer and
/// more opaque layers count more (the depth weight of McGuire and Bavoil, clamped).
pub fn layer_weight(alpha: f32, z: f32) -> f32 {
    let z = z.clamp(0.0, 1.0);
    (alpha * (3e3 * (1.0 - z).powi(3)).max(1e-2)).clamp(1e-2, 3e3)
}

/// What the composite shows for layers `(color, alpha, z)`: the weighted average colour and the
/// total coverage, the CPU reference of the two passes.
pub fn composite_reference(layers: &[([f32; 3], f32, f32)]) -> ([f32; 3], f32) {
    let (mut sum, mut weight, mut reveal) = ([0.0f32; 3], 0.0f32, 1.0f32);
    for &(c, a, z) in layers {
        let w = layer_weight(a, z);
        for k in 0..3 {
            sum[k] += c[k] * a * w;
        }
        weight += a * w;
        reveal *= 1.0 - a;
    }
    let d = weight.max(1e-5);
    ([sum[0] / d, sum[1] / d, sum[2] / d], 1.0 - reveal)
}

struct Targets {
    size: [u32; 2],
    samples: u32,
    /// Multisampled draw targets (resolved into the plain ones), when `samples` > 1.
    accum_ms: Option<wgpu::TextureView>,
    reveal_ms: Option<wgpu::TextureView>,
    accum: wgpu::TextureView,
    reveal: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

pub(crate) struct Oit {
    /// Draw pipelines: multisampled (as the main pass) and single-sample.
    draw: [wgpu::RenderPipeline; 2],
    draw_samples: u32,
    composite: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    targets: Option<Targets>,
}

impl Oit {
    /// `layout` and `shader` are the viewport's (the draw pipelines run its `fs_oit`).
    pub(crate) fn new(device: &wgpu::Device, layout: &wgpu::PipelineLayout, shader: &wgpu::ShaderModule, vertex: wgpu::VertexBufferLayout<'static>, msaa_samples: u32) -> Self {
        let add = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let keep_share = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::OneMinusSrc, operation: wgpu::BlendOperation::Add };
        let draw = |samples: u32| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oit draw"),
            layout: Some(layout),
            vertex: wgpu::VertexState { module: shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Some(vertex.clone())] },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            // Hidden behind opaque surfaces, never hiding each other.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::target::DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_oit"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: ACCUM_FORMAT, blend: Some(wgpu::BlendState { color: add, alpha: add }), write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: REVEAL_FORMAT, blend: Some(wgpu::BlendState { color: keep_share, alpha: keep_share }), write_mask: wgpu::ColorWrites::RED }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("oit composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("oit.wgsl").into()),
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
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("oit composite"), entries: &[tex(0), tex(1)] });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("oit composite layout"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("oit composite"),
            layout: Some(&composite_layout),
            vertex: wgpu::VertexState { module: &composite_shader, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_composite"),
                compilation_options: Default::default(),
                // Over the opaque image, keeping its alpha (as the plain blended pass does).
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::target::OUTPUT_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { draw: [draw(msaa_samples), draw(1)], draw_samples: msaa_samples, composite, layout: bind_layout, targets: None }
    }

    /// Targets for a `size` image drawn with `samples` per pixel.
    pub(crate) fn ensure(&mut self, device: &wgpu::Device, size: [u32; 2], samples: u32) {
        if self.targets.as_ref().is_some_and(|t| t.size == size && t.samples == samples) {
            return;
        }
        let make = |label, format, samples, usage| device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        }).create_view(&wgpu::TextureViewDescriptor::default());
        let plain = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let accum = make("oit accum", ACCUM_FORMAT, 1, plain);
        let reveal = make("oit reveal", REVEAL_FORMAT, 1, plain);
        let (accum_ms, reveal_ms) = if samples > 1 {
            (Some(make("oit accum msaa", ACCUM_FORMAT, samples, wgpu::TextureUsages::RENDER_ATTACHMENT)), Some(make("oit reveal msaa", REVEAL_FORMAT, samples, wgpu::TextureUsages::RENDER_ATTACHMENT)))
        } else {
            (None, None)
        };
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("oit composite"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&accum) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&reveal) },
            ],
        });
        self.targets = Some(Targets { size, samples, accum_ms, reveal_ms, accum, reveal, bind });
    }

    /// The pass the blended texture sets are drawn in (over `depth`, the main pass's depth),
    /// with the draw pipeline set. Call [`Self::ensure`] first.
    pub(crate) fn begin<'e>(&self, encoder: &'e mut wgpu::CommandEncoder, depth: &wgpu::TextureView) -> wgpu::RenderPass<'e> {
        let t = self.targets.as_ref().expect("ensure first");
        fn attach<'a>(ms: &'a Option<wgpu::TextureView>, plain: &'a wgpu::TextureView, clear: f64) -> Option<wgpu::RenderPassColorAttachment<'a>> {
            Some(wgpu::RenderPassColorAttachment {
                view: ms.as_ref().unwrap_or(plain),
                depth_slice: None,
                resolve_target: ms.as_ref().map(|_| plain),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: clear, g: clear, b: clear, a: clear }),
                    store: if ms.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                },
            })
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("oit pass"),
            color_attachments: &[attach(&t.accum_ms, &t.accum, 0.0), attach(&t.reveal_ms, &t.reveal, 1.0)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.draw[if t.samples > 1 && t.samples == self.draw_samples { 0 } else { 1 }]);
        pass
    }

    /// Lay the averaged transparent layers over `output`.
    pub(crate) fn composite(&self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
        let t = self.targets.as_ref().expect("ensure first");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("oit composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &t.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_average_does_not_depend_on_the_order() {
        let red = ([1.0, 0.0, 0.0], 0.5, 0.3);
        let blue = ([0.0, 0.0, 1.0], 0.5, 0.6);
        let (a, cov_a) = composite_reference(&[red, blue]);
        let (b, cov_b) = composite_reference(&[blue, red]);
        assert_eq!((a, cov_a), (b, cov_b));
        // Two half-transparent layers cover three quarters; the nearer (red) one weighs more.
        assert!((cov_a - 0.75).abs() < 1e-6);
        assert!(a[0] > a[2] && a[0] + a[2] > 0.999, "{a:?}");
        // One layer alone shows its own colour and coverage.
        let (one, cov) = composite_reference(&[([0.2, 0.4, 0.6], 0.3, 0.5)]);
        assert!(one.iter().zip([0.2, 0.4, 0.6]).all(|(x, y)| (x - y).abs() < 1e-5) && (cov - 0.3).abs() < 1e-6);
    }

    #[test]
    fn weights_favour_near_and_opaque_layers_within_bounds() {
        assert!(layer_weight(0.5, 0.1) > layer_weight(0.5, 0.9));
        assert!(layer_weight(0.9, 0.5) > layer_weight(0.2, 0.5));
        for (a, z) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.5, -3.0), (0.5, 7.0)] {
            let w = layer_weight(a, z);
            assert!((1e-2..=3e3).contains(&w), "{w} for {a} {z}");
        }
    }
}
