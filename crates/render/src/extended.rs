//! The extra material channels the viewport shades beyond metal/roughness: clear coat, sheen,
//! anisotropy, subsurface scattering, translucency, transmission and displacement.
//!
//! Each channel arrives like any other (a mipmapped [`ChannelTexture`] per channel, uploaded tile
//! by tile). The viewport shader cannot bind sixteen more textures (devices guarantee only 16
//! sampled textures per stage, and the shader already uses 12), so a GPU pass packs them into the
//! layers of one RGBA16F array texture, one layer per [`GROUPS`] entry the texture set uses. Only
//! the groups a set has get a layer, only the groups whose channels changed are repacked, and
//! only inside the changed rectangle.

use egui_wgpu::wgpu;
use subcraft_core::ChannelKind;

use crate::composite_comps;
use crate::material::{ChannelTexture, GpuMaterial};

/// Extra channels the viewport shades, in the binding order of `extended.wgsl` (bindings 1..=16).
pub const EXTENDED_CHANNELS: [ChannelKind; 16] = [
    ChannelKind::CoatColor,
    ChannelKind::CoatOpacity,
    ChannelKind::CoatRoughness,
    ChannelKind::CoatSpecularLevel,
    ChannelKind::CoatNormal,
    ChannelKind::SheenColor,
    ChannelKind::SheenOpacity,
    ChannelKind::SheenRoughness,
    ChannelKind::AnisotropyLevel,
    ChannelKind::AnisotropyAngle,
    ChannelKind::Translucency,
    ChannelKind::ScatteringColor,
    ChannelKind::Scattering,
    ChannelKind::Transmissive,
    ChannelKind::Ior,
    ChannelKind::Displacement,
];

/// Packed layers and the channels each holds (see `extended.wgsl` for the component layout):
/// 0 coat colour + opacity, 1 coat roughness + specular level + normal XY, 2 sheen colour +
/// opacity, 3 sheen roughness + anisotropy vector + translucency, 4 scattering colour + amount,
/// 5 transmissive + IOR, 6 displacement.
pub const GROUPS: [&[ChannelKind]; 7] = [
    &[ChannelKind::CoatColor, ChannelKind::CoatOpacity],
    &[
        ChannelKind::CoatRoughness,
        ChannelKind::CoatSpecularLevel,
        ChannelKind::CoatNormal,
    ],
    &[ChannelKind::SheenColor, ChannelKind::SheenOpacity],
    &[
        ChannelKind::SheenRoughness,
        ChannelKind::AnisotropyLevel,
        ChannelKind::AnisotropyAngle,
        ChannelKind::Translucency,
    ],
    &[ChannelKind::ScatteringColor, ChannelKind::Scattering],
    &[ChannelKind::Transmissive, ChannelKind::Ior],
    &[ChannelKind::Displacement],
];

/// The packed group that holds `kind`, if the viewport shades it.
pub fn group_of(kind: ChannelKind) -> Option<usize> {
    GROUPS.iter().position(|g| g.contains(&kind))
}

/// Uniform slots: one per (group, mip level), 256 bytes apart for dynamic offsets.
const MAX_LEVELS: usize = 16;
const SLOT: u64 = 256;

/// Packed state of one texture set.
#[derive(Default)]
pub struct Extended {
    /// The array texture (one layer per used group), its size and layer count.
    pub(crate) texture: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    /// Layer of each group, -1 when the set has none of its channels.
    pub(crate) layer_of: [i32; 7],
    /// Region of each group still to repack, in 0..1 texture coordinates (x0, y0, x1, y1).
    pub(crate) dirty: [Option<[f32; 4]>; 7],
    /// Sources bind group of the pack pass (rebuilt when a channel texture is recreated).
    pub(crate) sources: Option<wgpu::BindGroup>,
}

impl Extended {
    pub fn new() -> Self {
        Self {
            texture: None,
            layer_of: [-1; 7],
            dirty: [None; 7],
            sources: None,
        }
    }

    /// Mark `rect` (0..1 coordinates) of `kind`'s group for repacking.
    pub(crate) fn touch(&mut self, kind: ChannelKind, rect: [f32; 4]) {
        let Some(g) = group_of(kind) else { return };
        self.dirty[g] = Some(match self.dirty[g] {
            Some(d) => [
                d[0].min(rect[0]),
                d[1].min(rect[1]),
                d[2].max(rect[2]),
                d[3].max(rect[3]),
            ],
            None => rect,
        });
    }

    /// `MaterialParams` fields: layers of groups 0..4 and 4..7, and which effects are on
    /// (x anisotropy, y subsurface scattering, z translucency).
    pub(crate) fn params(&self, mat: &GpuMaterial) -> [[f32; 4]; 3] {
        let l = self.layer_of.map(|v| v as f32);
        let has = |k: ChannelKind| {
            if mat.channels.contains_key(&k) {
                1.0
            } else {
                0.0
            }
        };
        let sss = if mat.channels.contains_key(&ChannelKind::Scattering)
            || mat.channels.contains_key(&ChannelKind::ScatteringColor)
        {
            1.0
        } else {
            0.0
        };
        [
            [l[0], l[1], l[2], l[3]],
            [l[4], l[5], l[6], 0.0],
            [
                has(ChannelKind::AnisotropyLevel),
                sss,
                has(ChannelKind::Translucency),
                0.0,
            ],
        ]
    }
}

pub(crate) struct PackPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    /// 1x1 textures holding each channel's default, read where a set lacks the channel.
    defaults: Vec<ChannelTexture>,
    /// Bound where a set uses none of the extra channels (never sampled: every layer is -1).
    pub(crate) empty: wgpu::TextureView,
}

pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

impl PackPass {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let tex = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        }];
        entries.extend((1..=16).map(tex));
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 17,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(16),
            },
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("extended channels pack"),
            entries: &entries,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("extended channels pack"),
            source: wgpu::ShaderSource::Wgsl(include_str!("extended.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("extended channels pack"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("extended channels pack"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("extended pack sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("extended pack uniforms"),
            size: SLOT * (GROUPS.len() * MAX_LEVELS) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let defaults = EXTENDED_CHANNELS
            .iter()
            .map(|&k| {
                let d = k.default_value().0;
                ChannelTexture::constant(
                    device,
                    queue,
                    [d[0], d[1], d[2], 1.0],
                    composite_comps(k),
                    k.key(),
                )
            })
            .collect();
        let empty = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("no extended channels"),
                // Two layers: OpenGL makes a one-layer texture a plain 2D texture, which cannot be
                // viewed as an array.
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 2,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
        Self {
            pipeline,
            layout,
            sampler,
            uniforms,
            defaults,
            empty,
        }
    }

    /// Bring `mat`'s packed texture up to date with its channel textures: (re)allocate it when
    /// the groups in use or the size changed, then repack the dirty regions. Returns true when
    /// the material's bind group and parameters must be rebuilt.
    pub fn update(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mat: &mut GpuMaterial,
    ) -> bool {
        let used: Vec<bool> = GROUPS
            .iter()
            .map(|g| g.iter().any(|k| mat.channels.contains_key(k)))
            .collect();
        let size = EXTENDED_CHANNELS
            .iter()
            .filter_map(|k| mat.channels.get(k))
            .map(|t| t.size)
            .max()
            .unwrap_or(0);
        let layers = used.iter().filter(|u| **u).count() as u32;
        let mut wanted = [-1; 7];
        let mut next = 0;
        for (g, u) in used.iter().enumerate() {
            if *u {
                wanted[g] = next;
                next += 1;
            }
        }
        let current = mat.extended.texture.as_ref().map(|t| (t.2, t.3));
        let mut rebuild = false;
        if layers == 0 {
            if mat.extended.texture.is_some() || mat.extended.layer_of != [-1; 7] {
                mat.extended = Extended::new();
                rebuild = true;
            }
            return rebuild;
        }
        // OpenGL cannot view a one-layer texture as an array (see `empty`).
        let layers = if crate::caps::get().backend == "Gl" {
            layers.max(2)
        } else {
            layers
        };
        if current != Some((size, layers)) || mat.extended.layer_of != wanted {
            let levels = size.ilog2() + 1;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("extended channels"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: layers,
                },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            });
            mat.extended.texture = Some((texture, view, size, layers));
            mat.extended.layer_of = wanted;
            mat.extended.dirty = used
                .iter()
                .map(|u| u.then_some([0.0, 0.0, 1.0, 1.0]))
                .collect::<Vec<_>>()
                .try_into()
                .expect("seven groups");
            rebuild = true;
        }
        if mat.extended.dirty.iter().all(Option::is_none) {
            return rebuild;
        }
        if mat.extended.sources.is_none() {
            let mut entries = vec![wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            }];
            for (i, k) in EXTENDED_CHANNELS.iter().enumerate() {
                let view = mat
                    .channels
                    .get(k)
                    .map_or(&self.defaults[i].view, |t| &t.view);
                entries.push(wgpu::BindGroupEntry {
                    binding: i as u32 + 1,
                    resource: wgpu::BindingResource::TextureView(view),
                });
            }
            entries.push(wgpu::BindGroupEntry {
                binding: 17,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &self.uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(16),
                }),
            });
            mat.extended.sources = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("extended pack sources"),
                layout: &self.layout,
                entries: &entries,
            }));
        }
        let (texture, _, size, _) = mat.extended.texture.as_ref().expect("allocated above");
        let levels = (size.ilog2() + 1).min(MAX_LEVELS as u32);
        let sources = mat.extended.sources.as_ref().expect("created above");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("extended channels pack"),
        });
        for g in 0..GROUPS.len() {
            let (Some(rect), layer) = (mat.extended.dirty[g], mat.extended.layer_of[g]) else {
                continue;
            };
            if layer < 0 {
                continue;
            }
            for level in 0..levels {
                let w = (size >> level).max(1);
                let slot = (g * MAX_LEVELS + level as usize) as u64 * SLOT;
                queue.write_buffer(
                    &self.uniforms,
                    slot,
                    bytemuck::cast_slice(&[g as f32, w as f32, 0.0, 0.0]),
                );
                let x0 = ((rect[0] * w as f32).floor() as u32).min(w - 1);
                let y0 = ((rect[1] * w as f32).floor() as u32).min(w - 1);
                let x1 = ((rect[2] * w as f32).ceil() as u32).clamp(x0 + 1, w);
                let y1 = ((rect[3] * w as f32).ceil() as u32).clamp(y0 + 1, w);
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    base_array_layer: layer as u32,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("extended channels pack"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, sources, &[slot as u32]);
                pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
                pass.draw(0..3, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
        mat.extended.dirty = [None; 7];
        rebuild
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extended_channel_is_in_exactly_one_group() {
        for k in EXTENDED_CHANNELS {
            assert_eq!(GROUPS.iter().filter(|g| g.contains(&k)).count(), 1, "{k:?}");
        }
        assert_eq!(
            GROUPS.iter().map(|g| g.len()).sum::<usize>(),
            EXTENDED_CHANNELS.len()
        );
        assert_eq!(group_of(ChannelKind::Displacement), Some(6));
        assert_eq!(group_of(ChannelKind::BaseColor), None);
    }

    #[test]
    fn touched_regions_grow_to_their_union() {
        let mut e = Extended::new();
        e.touch(ChannelKind::CoatRoughness, [0.1, 0.2, 0.3, 0.4]);
        e.touch(ChannelKind::CoatNormal, [0.0, 0.5, 0.2, 0.9]);
        assert_eq!(e.dirty[1], Some([0.0, 0.2, 0.3, 0.9]));
        assert!(
            e.dirty
                .iter()
                .enumerate()
                .all(|(g, d)| g == 1 || d.is_none())
        );
    }
}
