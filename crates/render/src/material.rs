//! Per-texture-set channel textures, updated tile by tile from composite `TiledImage`s.

use std::collections::BTreeMap;

use egui_wgpu::wgpu;
use half::f16;
use subcraft_core::ChannelKind;
use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};

/// Channels the viewport shader reads, in binding order (after the sampler).
pub const SHADED_CHANNELS: [ChannelKind; 6] = [
    ChannelKind::BaseColor,
    ChannelKind::Roughness,
    ChannelKind::Metallic,
    ChannelKind::Normal,
    ChannelKind::Emissive,
    ChannelKind::Height,
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
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view, size, comps }
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

    /// Upload tiles of `image` (same size and component count).
    pub fn upload(&self, queue: &wgpu::Queue, image: &TiledImage, tiles: &[usize]) {
        let comps = image.comps();
        let mut fill_tile: Option<Vec<f16>> = None;
        for &t in tiles {
            let r = image.tile_rect(t);
            let data: &[f16] = match image.tile(t) {
                Some(tile) => &tile.data,
                None => fill_tile.get_or_insert_with(|| {
                    let f = image.fill();
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
    pub channels: BTreeMap<ChannelKind, ChannelTexture>,
    pub bind_group: Option<wgpu::BindGroup>,
}

impl GpuMaterial {
    pub fn new() -> Self {
        Self { channels: BTreeMap::new(), bind_group: None }
    }
}

impl Default for GpuMaterial {
    fn default() -> Self {
        Self::new()
    }
}
