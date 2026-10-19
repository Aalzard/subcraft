//! Per-texture-set channel textures, updated tile by tile from composite `TiledImage`s.

use std::collections::BTreeMap;

use egui_wgpu::wgpu;
use half::f16;
use subcraft_core::ChannelKind;
use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};

/// Channels the viewport shader reads, in binding order (after the sampler).
pub const SHADED_CHANNELS: [ChannelKind; 7] = [
    ChannelKind::BaseColor,
    ChannelKind::Roughness,
    ChannelKind::Metallic,
    ChannelKind::Normal,
    ChannelKind::Emissive,
    ChannelKind::Height,
    ChannelKind::AmbientOcclusion,
];

pub struct ChannelTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub size: u32,
    pub comps: usize,
}

fn format_for(comps: usize) -> wgpu::TextureFormat {
    match comps {
        1 => wgpu::TextureFormat::R16Float,
        2 => wgpu::TextureFormat::Rg16Float,
        _ => wgpu::TextureFormat::Rgba16Float,
    }
}

impl ChannelTexture {
    pub fn new(device: &wgpu::Device, size: u32, comps: usize, label: &str) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: format_for(comps),
            // Render attachment: a full upload clears to the image's fill on the GPU.
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view, size, comps }
    }

    /// Upload all of `image`: clear to its fill value on the GPU, then upload only the allocated
    /// tiles (an unallocated tile is the fill). A composite still warming up is mostly fill, so
    /// creating its texture costs a clear instead of hundreds of megabytes of uploads.
    pub fn upload_all(&self, device: &wgpu::Device, queue: &wgpu::Queue, image: &TiledImage) {
        let mut fill = image.fill();
        if image.comps() == 3 {
            fill[3] = 1.0;
        }
        // The f16 value a fill tile upload would store, exactly representable for the clear.
        let c = |i: usize| f16::from_f32(fill[i]).to_f64();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("channel clear") });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("channel clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: c(0), g: c(1), b: c(2), a: c(3) }), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        // Submitted before the tile writes below: queued writes run ahead of the next submission.
        queue.submit([encoder.finish()]);
        let allocated: Vec<usize> = (0..image.tile_count()).filter(|&t| image.is_allocated(t)).collect();
        self.upload(queue, image, &allocated);
    }

    /// 1×1 texture holding a constant (channel default when a texture set lacks the channel).
    pub fn constant(device: &wgpu::Device, queue: &wgpu::Queue, value: [f32; 4], comps: usize, label: &str) -> Self {
        let t = Self::new(device, 1, comps, label);
        let data: Vec<f16> = value[..comps].iter().map(|&v| f16::from_f32(v)).collect();
        queue.write_texture(
            t.texture.as_image_copy(),
            bytemuck::cast_slice(&data),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(comps as u32 * 2), rows_per_image: None },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        t
    }

    /// Upload tiles of `image` (same size; 3-component images are padded to RGBA).
    pub fn upload(&self, queue: &wgpu::Queue, image: &TiledImage, tiles: &[usize]) {
        let src_comps = image.comps();
        // GPU texel layout: there is no 3-component float format, RGB is stored as RGBA.
        let comps = if src_comps == 3 { 4 } else { src_comps };
        let mut fill_tile: Option<Vec<f16>> = None;
        let mut padded: Vec<f16> = Vec::new();
        for &t in tiles {
            let r = image.tile_rect(t);
            let data: &[f16] = match image.tile(t) {
                Some(tile) if src_comps == 3 => {
                    padded.clear();
                    for px in tile.data.chunks_exact(3) {
                        padded.extend_from_slice(&[px[0], px[1], px[2], f16::ONE]);
                    }
                    &padded
                }
                Some(tile) => &tile.data,
                None => fill_tile.get_or_insert_with(|| {
                    let mut f = image.fill();
                    if src_comps == 3 {
                        f[3] = 1.0;
                    }
                    (0..TILE_TEXELS * comps).map(|i| f16::from_f32(f[i % comps])).collect()
                }),
            };
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: r.x0 as u32, y: r.y0 as u32, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(data),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(TILE_SIZE * comps as u32 * 2),
                    rows_per_image: Some(TILE_SIZE),
                },
                wgpu::Extent3d { width: r.width() as u32, height: r.height() as u32, depth_or_array_layers: 1 },
            );
        }
    }
}

/// Channel textures + bind group of one texture set (material slot).
pub struct GpuMaterial {
    pub albedo_filter: subcraft_image::TextureFilter,
    pub channels: BTreeMap<ChannelKind, ChannelTexture>,
    /// Baked mesh maps (2D view display).
    pub mesh_maps: BTreeMap<subcraft_core::MeshMapKind, ChannelTexture>,
    pub bind_group: Option<wgpu::BindGroup>,
}

impl GpuMaterial {
    pub fn new() -> Self {
        Self { albedo_filter: Default::default(), channels: BTreeMap::new(), mesh_maps: BTreeMap::new(), bind_group: None }
    }
}

impl Default for GpuMaterial {
    fn default() -> Self {
        Self::new()
    }
}
