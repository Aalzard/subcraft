//! Preserve encoded display colors on APIs without alternate texture view formats (OpenGL).
use egui_wgpu::wgpu;
use crate::target::DISPLAY_FORMAT;

pub fn view_formats(device: &wgpu::Device) -> &'static [wgpu::TextureFormat] {
    if device.adapter_info().backend == wgpu::Backend::Gl { &[] } else { &[DISPLAY_FORMAT] }
}
pub fn view(device: &wgpu::Device, source: &wgpu::Texture, size: [u32; 2]) -> (wgpu::TextureView, Option<DisplayCopy>) {
    if !view_formats(device).is_empty() {
        (source.create_view(&wgpu::TextureViewDescriptor { format: Some(DISPLAY_FORMAT), ..Default::default() }), None)
    } else {
        let copy = DisplayCopy::new(device, source, size);
        (copy.texture.create_view(&Default::default()), Some(copy))
    }
}

pub struct DisplayCopy { texture: wgpu::Texture, pipeline: wgpu::RenderPipeline, group: wgpu::BindGroup }
impl DisplayCopy {
    fn new(device: &wgpu::Device, source: &wgpu::Texture, size: [u32; 2]) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("encoded display"), size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: DISPLAY_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("display copy"), entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
        ] });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("display copy"), layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&source.create_view(&Default::default())) }] });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("display copy"), source: wgpu::ShaderSource::Wgsl(include_str!("display.wgsl").into()) });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("display copy"), layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("display copy"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 })),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: DISPLAY_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None, cache: None,
        });
        Self { texture, pipeline, group }
    }
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("encoded display copy"), color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.texture.create_view(&Default::default()), depth_slice: None, resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })], depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline); pass.set_bind_group(0, &self.group, &[]); pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatible_display_copy_preserves_every_encoded_level_and_image_orientation() {
        let Some((_, device, queue)) = crate::headless::device() else { return };
        let size = [256, 2];
        let source = device.create_texture(&wgpu::TextureDescriptor { label: Some("display swatches"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: crate::OUTPUT_FORMAT, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        let bytes: Vec<u8> = (0..512u32).flat_map(|i| { let x = (i % 256) as u8; [x, 255-x, (i/256) as u8 * 255, 255] }).collect();
        queue.write_texture(source.as_image_copy(), &bytes, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024), rows_per_image: Some(2) }, source.size());
        // Force the fallback even on DX12/Vulkan so CI exercises its shader numerically.
        let copy = DisplayCopy::new(&device, &source, size);
        let mut encoder = device.create_command_encoder(&Default::default()); copy.encode(&mut encoder); queue.submit([encoder.finish()]);
        let actual = crate::target::read_texture_rgba8(&device, &queue, &copy.texture, size);
        assert_eq!(actual.len(), bytes.len());
        assert!(actual.iter().zip(&bytes).all(|(a,b)| a.abs_diff(*b) <= 1), "gamma or orientation changed");
    }
}
