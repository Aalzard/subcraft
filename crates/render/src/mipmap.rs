//! Render-owned, incremental linear-space mipmaps. CPU composites/export remain authoritative.
use crate::material::{ChannelTexture, format_for};
use egui_wgpu::wgpu;
use subcraft_image::TiledImage;

pub(crate) struct MipGenerator {
    pub layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::RenderPipeline; 3],
}

// Half-open destination bounds touched by a source edit, including odd-sized footprints.
fn down_bounds(r: [u32; 4], src: u32) -> [u32; 4] {
    let dst = (src / 2).max(1);
    [
        r[0] * dst / src,
        r[1] * dst / src,
        (r[2] * dst).div_ceil(src),
        (r[3] * dst).div_ceil(src),
    ]
}

impl MipGenerator {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mip source"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("area mipmap"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mipmap.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mipmap"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = [1, 2, 4].map(|comps| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mipmap"),
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
                        format: format_for(comps),
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
            })
        });
        Self { layout, pipelines }
    }

    pub fn update(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &ChannelTexture,
        image: &TiledImage,
        tiles: Option<&[usize]>,
    ) {
        if texture.mip_inputs.is_empty() || tiles.is_some_and(|t| t.is_empty()) {
            return;
        }
        let mut bounds = [0, 0, texture.size, texture.size];
        if let Some(tiles) = tiles {
            bounds = [texture.size, texture.size, 0, 0];
            for &t in tiles {
                let r = image.tile_rect(t);
                bounds[0] = bounds[0].min(r.x0 as u32);
                bounds[1] = bounds[1].min(r.y0 as u32);
                bounds[2] = bounds[2].max(r.x1 as u32);
                bounds[3] = bounds[3].max(r.y1 as u32);
            }
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("channel mipmaps"),
        });
        let pipeline = &self.pipelines[match texture.comps {
            1 => 0,
            2 => 1,
            _ => 2,
        }];
        let mut src = texture.size;
        for (i, input) in texture.mip_inputs.iter().enumerate() {
            bounds = down_bounds(bounds, src);
            src = (src / 2).max(1);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("channel mipmap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture.mip_views[i + 1],
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
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, input, &[]);
            pass.set_scissor_rect(
                bounds[0],
                bounds[1],
                bounds[2] - bounds[0],
                bounds[3] - bounds[1],
            );
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_bounds_include_odd_footprints_and_stay_local() {
        assert_eq!(down_bounds([0, 0, 64, 64], 4096), [0, 0, 32, 32]);
        assert_eq!(down_bounds([4, 4, 5, 5], 5), [1, 1, 2, 2]);
        assert_eq!(down_bounds([2, 2, 3, 3], 5), [0, 0, 2, 2]);
        assert_eq!(down_bounds([0, 0, 4096, 4096], 4096), [0, 0, 2048, 2048]);
    }

    fn read(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        t: &ChannelTexture,
        level: u32,
    ) -> Vec<f32> {
        let size = (t.size >> level).max(1);
        let comps = if t.comps == 3 { 4 } else { t.comps };
        let row = size * comps as u32 * 2;
        let padded = row.div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (padded * size) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &t.texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(size),
                },
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        let out = mapped
            .chunks_exact(padded as usize)
            .flat_map(|row| {
                row[..row.len().min(size as usize * comps * 2)]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| half::f16::from_le_bytes([b[0], b[1]]).to_f32())
            })
            .collect();
        drop(mapped);
        buffer.unmap();
        out
    }

    #[test]
    fn gpu_mips_average_all_formats_and_incremental_edits_match_full() {
        let Some((adapter, device, queue)) = crate::headless::device() else {
            eprintln!("no adapter: skipping");
            return;
        };
        eprintln!("mipmap adapter: {:?}", adapter.get_info());
        let generator = MipGenerator::new(&device);
        for comps in [1, 2, 3, 4] {
            let mut image = TiledImage::new(128, 128, comps, [0.25; 4]);
            for y in 0..64 {
                for x in 0..64 {
                    image.set(x, y, [0.75; 4]);
                }
            }
            let t = ChannelTexture::new_mipmapped(&device, 128, comps, "test", &generator.layout);
            t.upload_all(&device, &queue, &image);
            generator.update(&device, &queue, &t, &image, None);
            let last = read(&device, &queue, &t, 7);
            for (i, v) in last.iter().enumerate() {
                assert!(
                    (v - if comps == 3 && i == 3 { 1. } else { 0.375 }).abs() < 0.001,
                    "{comps}: {last:?}"
                );
            }
            // Real dirty-tile path.
            for y in 64..128 {
                for x in 64..128 {
                    image.set(x, y, [1.; 4]);
                }
            }
            t.upload(&queue, &image, &[3]);
            generator.update(&device, &queue, &t, &image, Some(&[3]));
            let full =
                ChannelTexture::new_mipmapped(&device, 128, comps, "full", &generator.layout);
            full.upload_all(&device, &queue, &image);
            generator.update(&device, &queue, &full, &image, None);
            for level in 0..8 {
                assert_eq!(
                    read(&device, &queue, &t, level),
                    read(&device, &queue, &full, level),
                    "comps {comps}, level {level}"
                );
            }
            generator.update(&device, &queue, &t, &image, Some(&[]));
            assert_eq!(
                read(&device, &queue, &t, 7),
                read(&device, &queue, &full, 7)
            );
            // Allocated tiles becoming implicit fill must invalidate their lower mips too.
            let cleared = TiledImage::new(128, 128, comps, [0.25; 4]);
            t.upload(&queue, &cleared, &[0, 3]);
            generator.update(&device, &queue, &t, &cleared, Some(&[0, 3]));
            for level in 0..8 {
                for (i, value) in read(&device, &queue, &t, level).iter().enumerate() {
                    let expected = if comps == 3 && i % 4 == 3 { 1. } else { 0.25 };
                    assert_eq!(*value, expected, "cleared {comps}, mip {level}");
                }
            }
        }
        // Odd dimensions must retain the final row/column and conserve their area.
        let mut odd = TiledImage::new(5, 5, 1, [0.; 4]);
        for y in 0..5 {
            odd.set(4, y, [1.; 4]);
        }
        let t = ChannelTexture::new_mipmapped(&device, 5, 1, "odd", &generator.layout);
        t.upload_all(&device, &queue, &odd);
        generator.update(&device, &queue, &t, &odd, None);
        assert!((read(&device, &queue, &t, 2)[0] - 0.2).abs() < 0.001);
        // Profile a sparse 4K single-tile edit (includes submission + completion, not FPS).
        let image = TiledImage::new(4096, 4096, 1, [0.5; 4]);
        let t = ChannelTexture::new_mipmapped(&device, 4096, 1, "4K", &generator.layout);
        t.upload_all(&device, &queue, &image);
        generator.update(&device, &queue, &t, &image, None);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mut times = Vec::new();
        for _ in 0..20 {
            let start = std::time::Instant::now();
            t.upload(&queue, &image, &[100]);
            generator.update(&device, &queue, &t, &image, Some(&[100]));
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "4K scalar tile upload+mips+GPU wait: median {:.3} ms, max {:.3} ms",
            times[10], times[19]
        );
        assert!((read(&device, &queue, &t, 12)[0] - 0.5).abs() < 0.001);
    }
}
