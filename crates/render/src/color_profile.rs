//! Colour profile (Painter's Display Settings > Color profile): with it on, the 3D view's
//! material shading is shown "Linear/Raw" (the values the white point clamps, not sRGB-encoded),
//! or through a 3D LUT that turns them into display values (a camera look, a calibration).
//!
//! The LUT is a float image in OpenColorIO's `ociolutimage` layout, what Painter uses: the
//! N x N x N cube flattened with red fastest, then green, then blue (entry `r + N g + N² b`),
//! reshaped row by row into a `width x height` image, so `width * height = N³` (Painter's default
//! 2048 x 128 is a 64 cube). Lookups are trilinear.

use egui_wgpu::wgpu;

/// Cube size of a `width x height` LUT image: N with N³ = width x height (at least 2), or `None`
/// when the image cannot hold a whole cube.
pub fn lut_cube_size(width: u32, height: u32) -> Option<u32> {
    subcraft_doc::display::lut_cube_size(width, height)
}

/// The identity LUT of cube size `n` in the `ociolutimage` layout (RGBA, alpha 1), as
/// `ociolutimage --generate` writes it.
pub fn identity_lut(n: u32) -> Vec<[f32; 4]> {
    let c = 1.0 / (n.max(2) - 1) as f32;
    (0..n * n * n).map(|i| [(i % n) as f32 * c, ((i / n) % n) as f32 * c, ((i / n / n) % n) as f32 * c, 1.0]).collect()
}

/// CPU reference of the shader: `rgb` (0..1) through the cube `lut` (entries in the
/// `ociolutimage` order) of size `n`, trilinear.
pub fn lut_lookup(lut: &[[f32; 4]], n: u32, rgb: [f32; 3]) -> [f32; 3] {
    let n = n as usize;
    let at = |r: usize, g: usize, b: usize| lut[r + n * (g + n * b)];
    let pos = rgb.map(|v| v.clamp(0.0, 1.0) * (n - 1) as f32);
    let i0 = pos.map(|p| p.floor() as usize);
    let i1 = i0.map(|i| (i + 1).min(n - 1));
    let f = [pos[0] - i0[0] as f32, pos[1] - i0[1] as f32, pos[2] - i0[2] as f32];
    let mix = |a: [f32; 4], b: [f32; 4], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 1.0];
    let c00 = mix(at(i0[0], i0[1], i0[2]), at(i1[0], i0[1], i0[2]), f[0]);
    let c10 = mix(at(i0[0], i1[1], i0[2]), at(i1[0], i1[1], i0[2]), f[0]);
    let c01 = mix(at(i0[0], i0[1], i1[2]), at(i1[0], i0[1], i1[2]), f[0]);
    let c11 = mix(at(i0[0], i1[1], i1[2]), at(i1[0], i1[1], i1[2]), f[0]);
    let c = mix(mix(c00, c10, f[1]), mix(c01, c11, f[1]), f[2]);
    [c[0], c[1], c[2]]
}

struct Lut {
    size: u32,
    width: u32,
    view: wgpu::TextureView,
}

pub(crate) struct ColorProfile {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    lut: Option<Lut>,
    /// 1 x 1 stand-in bound when no LUT is loaded.
    empty: wgpu::TextureView,
    /// Copy of the finished image the pass reads (it writes the output).
    scratch: Option<([u32; 2], wgpu::Texture, wgpu::TextureView)>,
}

impl ColorProfile {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("color profile shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("color_profile.wgsl").into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("color profile layout"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("color profile pipeline layout"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("color profile"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_profile"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: crate::target::OUTPUT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("color profile params"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let empty = Self::lut_texture(device, 1, 1, "color lut (none)").create_view(&wgpu::TextureViewDescriptor::default());
        Self { layout, pipeline, uniform, lut: None, empty, scratch: None }
    }

    fn lut_texture(device: &wgpu::Device, width: u32, height: u32, label: &str) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// Load a LUT image (`width x height` RGBA entries) or clear it. `Err` when the image does not
    /// hold a whole cube or is larger than the GPU allows.
    pub(crate) fn set_lut(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, lut: Option<(&[[f32; 4]], u32, u32)>) -> Result<(), String> {
        let Some((pixels, width, height)) = lut else {
            self.lut = None;
            return Ok(());
        };
        let size = lut_cube_size(width, height).ok_or_else(|| format!("a {width} x {height} image is not a 3D LUT (width x height must be a cube, such as 2048 x 128)"))?;
        let max = device.limits().max_texture_dimension_2d;
        if width > max || height > max {
            return Err(format!("a {width} x {height} LUT is larger than this graphics card allows ({max})"));
        }
        if pixels.len() != (width * height) as usize {
            return Err("the LUT image data does not match its size".into());
        }
        let texture = Self::lut_texture(device, width, height, "color lut");
        queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice(pixels),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 16), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.lut = Some(Lut { size, width, view: texture.create_view(&wgpu::TextureViewDescriptor::default()) });
        Ok(())
    }

    /// Cube size of the loaded LUT, if any.
    pub(crate) fn lut_size(&self) -> Option<u32> {
        self.lut.as_ref().map(|l| l.size)
    }

    /// Apply the profile to `target`'s finished image, in place.
    pub(crate) fn encode(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &crate::target::RenderTarget) {
        if self.scratch.as_ref().is_none_or(|s| s.0 != target.size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("color profile source"),
                size: wgpu::Extent3d { width: target.size[0], height: target.size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::target::OUTPUT_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.scratch = Some((target.size, texture, view));
        }
        let (_, scratch, scratch_view) = self.scratch.as_ref().expect("created above");
        encoder.copy_texture_to_texture(
            target.output.as_image_copy(),
            scratch.as_image_copy(),
            wgpu::Extent3d { width: target.size[0], height: target.size[1], depth_or_array_layers: 1 },
        );
        let (size, width, lut_view) = match &self.lut {
            Some(l) => (l.size as i32, l.width as i32, &l.view),
            None => (0, 1, &self.empty),
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&[size, width, 0i32, 0]));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("color profile"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(scratch_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(lut_view) },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("color profile"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.output_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_sizes_follow_the_image_size() {
        assert_eq!(lut_cube_size(2048, 128), Some(64), "Painter's default");
        assert_eq!(lut_cube_size(1024, 32), Some(32));
        assert_eq!(lut_cube_size(256, 16), Some(16));
        assert_eq!(lut_cube_size(1000, 100), None);
        assert_eq!(lut_cube_size(0, 0), None);
        assert_eq!(lut_cube_size(1, 1), None, "a single entry is not a cube");
    }

    #[test]
    fn the_identity_lut_returns_its_input_and_lookups_interpolate() {
        let n = 17;
        let id = identity_lut(n);
        assert_eq!(id.len(), (n * n * n) as usize);
        // The ociolutimage order: red fastest.
        assert_eq!(id[1], [1.0 / 16.0, 0.0, 0.0, 1.0]);
        assert_eq!(id[n as usize], [0.0, 1.0 / 16.0, 0.0, 1.0]);
        for rgb in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.3, 0.71, 0.05], [0.5, 0.25, 0.9]] {
            let out = lut_lookup(&id, n, rgb);
            assert!(out.iter().zip(rgb).all(|(a, b)| (a - b).abs() < 1e-5), "{rgb:?} -> {out:?}");
        }
        // An inverting cube inverts, between its entries too.
        let inv: Vec<[f32; 4]> = id.iter().map(|c| [1.0 - c[0], 1.0 - c[1], 1.0 - c[2], 1.0]).collect();
        let out = lut_lookup(&inv, n, [0.33, 0.6, 0.12]);
        assert!((out[0] - 0.67).abs() < 1e-5 && (out[1] - 0.4).abs() < 1e-5 && (out[2] - 0.88).abs() < 1e-5, "{out:?}");
        // Out of range input is clamped to the cube.
        assert_eq!(lut_lookup(&id, n, [-1.0, 2.0, 0.5]), [0.0, 1.0, 0.5]);
    }
}
