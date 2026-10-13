//! Offscreen render target (MSAA color + depth, resolved to a sampleable sRGB texture).

use egui_wgpu::wgpu;

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct RenderTarget {
    pub size: [u32; 2],
    pub msaa_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    /// Resolved image (sampled by the UI, copied out by `read_rgba8`).
    pub output: wgpu::Texture,
    pub output_view: wgpu::TextureView,
}

impl RenderTarget {
    pub fn new(device: &wgpu::Device, size: [u32; 2], samples: u32) -> Self {
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let make = |label, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = make("viewport msaa", OUTPUT_FORMAT, samples, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = make("viewport depth", DEPTH_FORMAT, samples, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let output = make(
            "viewport output",
            OUTPUT_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        Self { size, msaa_view: view(&msaa), depth_view: view(&depth), output_view: view(&output), output }
    }

    /// Copy the resolved image back to the CPU as tightly packed sRGB RGBA8 rows.
    pub fn read_rgba8(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let [w, h] = self.size;
        let row = w * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            self.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback buffer"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("device poll");
        let data = slice.get_mapped_range().expect("mapped readback");
        let mut out = Vec::with_capacity((row * h) as usize);
        for y in 0..h {
            out.extend_from_slice(&data[(y * padded) as usize..][..row as usize]);
        }
        drop(data);
        buffer.unmap();
        out
    }
}
