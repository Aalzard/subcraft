//! Offscreen render target (MSAA color + depth, resolved to a sampleable sRGB texture).

use egui_wgpu::wgpu;

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Non-sRGB view format of the output for display in egui.
pub const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct RenderTarget {
    display_copy: Option<crate::display::DisplayCopy>,
    pub size: [u32; 2],
    pub msaa_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    /// Resolved image (copied out by `read_rgba8`).
    pub output: wgpu::Texture,
    /// sRGB view used as the render-pass resolve target.
    pub output_view: wgpu::TextureView,
    /// The same texels viewed as plain `Rgba8Unorm`: sampling returns the sRGB-encoded values,
    /// which is what egui expects from its textures (it composites in gamma space).
    pub display_view: wgpu::TextureView,
}

impl RenderTarget {
    pub fn new(device: &wgpu::Device, size: [u32; 2], samples: u32) -> Self {
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let make = |label, format, samples, usage, view_formats: &[wgpu::TextureFormat]| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats,
            })
        };
        let msaa = make("viewport msaa", OUTPUT_FORMAT, samples, wgpu::TextureUsages::RENDER_ATTACHMENT, &[]);
        let depth = make("viewport depth", DEPTH_FORMAT, samples, wgpu::TextureUsages::RENDER_ATTACHMENT, &[]);
        let output = make(
            "viewport output",
            OUTPUT_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            crate::display::view_formats(device),
        );
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (display_view, display_copy) = crate::display::view(device, &output, size);
        Self { size, msaa_view: view(&msaa), depth_view: view(&depth), output_view: view(&output), display_view, output, display_copy }
    }

    pub fn encode_display(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(copy) = &self.display_copy { copy.encode(encoder); }
    }

    /// Start copying the resolved image back to the CPU without waiting (works in the browser,
    /// where the GPU can't be waited on). The target can be re-rendered right away.
    pub fn read_rgba8_async(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> PendingReadback {
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
        let state = std::sync::Arc::new(std::sync::Mutex::new(None));
        let s = state.clone();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| *s.lock().expect("readback state") = Some(r.is_ok()));
        PendingReadback { buffer, state, size: [w, h], padded }
    }

    /// Copy the resolved image back to the CPU as tightly packed sRGB RGBA8 rows.
    pub fn read_rgba8(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        read_texture_rgba8(device, queue, &self.output, self.size)
    }
}

pub(crate) fn read_texture_rgba8(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture, size: [u32; 2]) -> Vec<u8> {
        let [w, h] = size;
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
            texture.as_image_copy(),
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

/// A GPU → CPU image copy in flight (see [`RenderTarget::read_rgba8_async`]).
pub struct PendingReadback {
    buffer: wgpu::Buffer,
    state: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
    size: [u32; 2],
    padded: u32,
}

impl PendingReadback {
    /// The tightly packed sRGB RGBA8 rows once the copy is done (`Some(None)` if it failed);
    /// `None` while still in flight. Never blocks.
    pub fn try_take(&self, device: &wgpu::Device) -> Option<Option<Vec<u8>>> {
        let _ = device.poll(wgpu::PollType::Poll);
        let ok = (*self.state.lock().expect("readback state"))?;
        if !ok {
            return Some(None);
        }
        let [w, h] = self.size;
        let row = (w * 4) as usize;
        let data = self.buffer.slice(..).get_mapped_range().ok()?;
        let mut out = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            out.extend_from_slice(&data[y * self.padded as usize..][..row]);
        }
        drop(data);
        self.buffer.unmap();
        Some(Some(out))
    }
}
