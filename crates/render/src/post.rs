//! Viewport post-processing: tone-map choice, screen-space ambient occlusion and bloom.
//!
//! The scene pass already writes the tone-mapped image into the (sRGB) output texture. When bloom
//! or AO is on, that image is copied aside, a depth-only pass feeds the AO, a half-resolution
//! bright pass is blurred into a glow, and one composite pass writes `scene * AO + glow` back.
//! With both off nothing here runs and the frame is bit-identical to the plain render.

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use glam::Mat4;
use std::collections::HashMap;

use crate::{gpu_mesh, target};

pub use subcraft_doc::display::{GlareShape as GlareMode, ToneMap};

/// How a glare shape is built: streak directions (unit steps), whether the streaks split into
/// colors, the weight of a round glow, the number of ghost images and a streak-length multiplier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlareRecipe {
    pub directions: &'static [[f32; 2]],
    pub spectral: bool,
    pub glow: f32,
    pub ghosts: u32,
    pub length: f32,
}

/// Recipe of a glare shape.
pub fn glare_recipe(shape: GlareMode) -> GlareRecipe {
    const D: f32 = std::f32::consts::FRAC_1_SQRT_2;
    // 60 degree steps for the six-pointed snow cross.
    const S: f32 = 0.866_025_4;
    const H: [f32; 2] = [1.0, 0.0];
    const V: [f32; 2] = [0.0, 1.0];
    let streaks = |directions: &'static [[f32; 2]], spectral: bool| GlareRecipe { directions, spectral, glow: 0.0, ghosts: 0, length: 1.0 };
    match shape {
        GlareMode::Horizontal => streaks(&[H], false),
        GlareMode::Vertical => streaks(&[V], false),
        GlareMode::Both => streaks(&[H, V], false),
        GlareMode::CrossScreenSpectral => streaks(&[H, V], true),
        GlareMode::Star => streaks(&[H, V, [D, D], [D, -D]], false),
        GlareMode::SunnyCrossSpectral => streaks(&[H, V, [D, D], [D, -D]], true),
        GlareMode::SnowCross => streaks(&[H, [0.5, S], [0.5, -S]], false),
        GlareMode::SnowCrossSpectral => streaks(&[H, [0.5, S], [0.5, -S]], true),
        GlareMode::Bloom => GlareRecipe { directions: &[], spectral: false, glow: 1.0, ghosts: 0, length: 1.0 },
        GlareMode::LensFlare => GlareRecipe { directions: &[], spectral: false, glow: 0.6, ghosts: 3, length: 1.0 },
        GlareMode::Standard => GlareRecipe { directions: &[H, V], spectral: false, glow: 0.6, ghosts: 0, length: 1.0 },
        GlareMode::CheapLens => GlareRecipe { directions: &[[D, D], [D, -D]], spectral: false, glow: 0.0, ghosts: 1, length: 0.5 },
        GlareMode::AfterImage => GlareRecipe { directions: &[H], spectral: false, glow: 0.15, ghosts: 0, length: 1.5 },
    }
}

/// Post-processing settings for a frame. The defaults leave the image untouched.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostParams {
    pub tone_map: ToneMap,
    /// Glow added around bright areas (0 = off).
    pub bloom: f32,
    /// Brightness (after tone mapping, 0..1) above which pixels glow.
    pub bloom_threshold: f32,
    /// Reach of the bloom, 0 (tight) .. 1 (wide); 0.5 is the original width.
    pub bloom_falloff: f32,
    /// Mapping Factor of the Reinhard / Log tone functions (1 = the plain curve).
    pub mapping_factor: f32,
    /// Strength of the ambient-occlusion darkening in creases (0 = off).
    pub ssao: f32,
    /// AO reach in object units.
    pub ssao_radius: f32,
    /// Soft glow around emissive areas only (0 = off). Models without emission never change.
    pub emission_glow: f32,
    /// Glow reach: 1 is a few percent of the view, larger spreads it wider.
    pub glow_radius: f32,
    /// Streaks from bright emission (0 = off).
    pub glare: f32,
    pub glare_mode: GlareMode,
    /// Streak length, 0..1 (fraction of the longest reach).
    pub glare_length: f32,
    /// Emission below this brightness throws no glare (0 = all of it does).
    pub glare_threshold: f32,
    /// Exponent on the brightness above the threshold (1 = proportional).
    pub glare_remap: f32,
    /// Lens flare (0 = off): ghost images of the bright areas, mirrored through the view centre.
    pub flare: f32,
    /// Brightness (after tone mapping, 0..1) above which pixels throw ghosts.
    pub flare_threshold: f32,
    /// Number of ghost images, 1..8.
    pub flare_ghosts: u32,
    /// Spacing of the ghosts (1 = spread over the view, smaller keeps them near the centre).
    pub flare_scale: f32,
    /// Lateral aberration, vignette, sharpen, film grain, color correction and gamma.
    pub grade: crate::grade::Grade,
    /// Depth of field: largest blur radius as a share of the maximum (0 = off; 1 = a radius of 5% of
    /// the image height, about 54 px at 1080p). Needs a model in view.
    pub dof: f32,
    /// Distance from the camera (view space, scene units) that stays sharp.
    pub dof_focus: f32,
    /// Depth of the sharp band around the focus distance, in scene units.
    pub dof_range: f32,
}

impl Default for PostParams {
    fn default() -> Self {
        Self {
            tone_map: ToneMap::Aces,
            bloom: 0.0,
            bloom_threshold: 0.8,
            bloom_falloff: 0.5,
            mapping_factor: 1.0,
            ssao: 0.0,
            ssao_radius: 0.1,
            emission_glow: 0.35,
            glow_radius: 1.0,
            glare: 0.0,
            glare_mode: GlareMode::Horizontal,
            glare_length: 0.5,
            glare_threshold: 0.0,
            glare_remap: 1.0,
            flare: 0.0,
            flare_threshold: 0.8,
            flare_ghosts: 4,
            flare_scale: 1.0,
            grade: crate::grade::Grade::default(),
            dof: 0.0,
            dof_focus: 1.0,
            dof_range: 0.2,
        }
    }
}

impl PostParams {
    /// Whether any effect needs extra passes over the scene.
    pub fn active(&self) -> bool {
        self.bloom > 0.0 || self.ssao > 0.0 || self.dof > 0.0 || self.flare > 0.0 || self.grade.active()
    }

    /// Whether the emission glow / glare passes would add anything for a model with emission.
    pub fn wants_emission(&self) -> bool {
        self.emission_glow > 0.0 || self.glare > 0.0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostUniform {
    inv_proj: [[f32; 4]; 4],
    params: [f32; 4],
    size: [f32; 4],
    misc: [f32; 4],
    /// Rows of the lens and color stage (see `fs_grade`).
    grade: [[f32; 4]; 4],
    /// Lens distortion: power, zoom, edge roundness, edge smoothness.
    lens: [f32; 4],
}

const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// Emission and the effects buffer keep values above 1 until the final composite.
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const UNIFORMS: usize = 56;
const U_AO: usize = 0;
const U_AO_H: usize = 1;
const U_AO_V: usize = 2;
const U_BRIGHT: usize = 3;
const U_BLOOM_H: usize = 4;
const U_BLOOM_V: usize = 5;
const U_COMPOSITE: usize = 6;
const U_DOWN: usize = 7;
const U_GLOW_H: usize = 8;
const U_GLOW_V: usize = 9;
const U_FX_BLOOM: usize = 10;
const U_FX_GLOW: usize = 11;
const U_FX_GLARE: usize = 12;
const U_FX_GLARE_ADD: usize = 13;
const U_DOF: usize = 14;
const U_GRADE: usize = 15;
const U_GLARE_SRC: usize = 40;
const U_GLARE_GLOW_H: usize = 41;
const U_GLARE_GLOW_V: usize = 42;
const U_GLARE_GLOW_ADD: usize = 43;
const U_GLARE_GHOST: usize = 44;
const U_FLARE_BRIGHT: usize = 45;
const U_FLARE_H: usize = 46;
const U_FLARE_V: usize = 47;
const U_FLARE: usize = 48;
/// Streak stages: `U_STREAK + direction * STAGES + stage` (four directions, six stages).
const U_STREAK: usize = 16;
const STAGES: usize = 6;

struct Targets {
    size: [u32; 2],
    depth: wgpu::Texture,
    ao: [wgpu::Texture; 2],
    scene: wgpu::Texture,
    /// The composited frame, read by the lens and color stage.
    graded: wgpu::Texture,
    dof: wgpu::Texture,
    bloom: [wgpu::Texture; 2],
    emit: wgpu::Texture,
    glow: [wgpu::Texture; 2],
    glare: wgpu::Texture,
    streak: [wgpu::Texture; 2],
    fx: wgpu::Texture,
}

/// What the emission pass draws.
pub(crate) struct EmissionDraw<'a> {
    pub pipeline: &'a wgpu::RenderPipeline,
    pub mesh: &'a gpu_mesh::GpuMesh,
    pub materials: &'a HashMap<u32, crate::GpuMaterial>,
    pub hidden: &'a std::collections::HashSet<u32>,
}

pub(crate) struct PostPass {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    dummy_color: wgpu::TextureView,
    dummy_depth: wgpu::TextureView,
    depth_pipeline: wgpu::RenderPipeline,
    ao_pipeline: wgpu::RenderPipeline,
    ao_blur_pipeline: wgpu::RenderPipeline,
    bright_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    dof_pipeline: wgpu::RenderPipeline,
    hdr_copy_pipeline: wgpu::RenderPipeline,
    hdr_blur_pipeline: wgpu::RenderPipeline,
    tent_pipeline: wgpu::RenderPipeline,
    scale_add_pipeline: wgpu::RenderPipeline,
    glare_src_pipeline: wgpu::RenderPipeline,
    ghost_add_pipeline: wgpu::RenderPipeline,
    grade_pipeline: wgpu::RenderPipeline,
    uniforms: Vec<wgpu::Buffer>,
    targets: Option<Targets>,
}

fn texture(device: &wgpu::Device, label: &str, size: [u32; 2], format: wgpu::TextureFormat, usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

impl PostPass {
    pub(crate) fn new(device: &wgpu::Device, globals_layout: &wgpu::BindGroupLayout, viewport_shader: &wgpu::ShaderModule) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
        });
        let tex = |binding, depth| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: if depth { wgpu::TextureSampleType::Float { filterable: false } } else { wgpu::TextureSampleType::Float { filterable: true } },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(1, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex(3, false),
                tex(4, true),
                tex(5, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let fullscreen_blend = |fs: &str, format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>| {
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
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let fullscreen = |fs: &str, format: wgpu::TextureFormat| fullscreen_blend(fs, format, None);
        let additive = Some(wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::OVER,
        });
        // Depth only: the viewport vertex shader needs just the globals group.
        let depth_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post depth layout"),
            bind_group_layouts: &[Some(globals_layout)],
            immediate_size: 0,
        });
        let depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post depth prepass"),
            layout: Some(&depth_layout),
            vertex: wgpu::VertexState {
                module: viewport_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(gpu_mesh::Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target::DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let dummy_color = view(&texture(device, "post dummy", [1, 1], target::OUTPUT_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING));
        let dummy_depth = view(&texture(device, "post dummy depth", [1, 1], target::DEPTH_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING));
        let uniforms = (0..UNIFORMS)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("post uniform"),
                    size: std::mem::size_of::<PostUniform>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Self {
            ao_pipeline: fullscreen("fs_ao", AO_FORMAT),
            ao_blur_pipeline: fullscreen("fs_blur", AO_FORMAT),
            bright_pipeline: fullscreen("fs_bright", target::OUTPUT_FORMAT),
            blur_pipeline: fullscreen("fs_blur", target::OUTPUT_FORMAT),
            composite_pipeline: fullscreen("fs_composite", target::OUTPUT_FORMAT),
            dof_pipeline: fullscreen("fs_dof", target::OUTPUT_FORMAT),
            hdr_copy_pipeline: fullscreen("fs_copy", HDR_FORMAT),
            hdr_blur_pipeline: fullscreen("fs_blur", HDR_FORMAT),
            tent_pipeline: fullscreen("fs_tent", HDR_FORMAT),
            scale_add_pipeline: fullscreen_blend("fs_scale", HDR_FORMAT, additive),
            glare_src_pipeline: fullscreen("fs_glare_src", HDR_FORMAT),
            ghost_add_pipeline: fullscreen_blend("fs_ghost", HDR_FORMAT, additive),
            grade_pipeline: fullscreen("fs_grade", target::OUTPUT_FORMAT),
            layout,
            sampler,
            dummy_color,
            dummy_depth,
            depth_pipeline,
            uniforms,
            targets: None,
        }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let half = [size[0].div_ceil(2), size[1].div_ceil(2)];
        self.targets = Some(Targets {
            size,
            depth: texture(device, "post depth", size, target::DEPTH_FORMAT, rt),
            ao: [0, 1].map(|_| texture(device, "post ao", size, AO_FORMAT, rt)),
            scene: texture(device, "post scene copy", size, target::OUTPUT_FORMAT, wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING),
            graded: texture(device, "post grade input", size, target::OUTPUT_FORMAT, wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING),
            dof: texture(device, "post depth of field", size, target::OUTPUT_FORMAT, rt),
            bloom: [0, 1].map(|_| texture(device, "post bloom", half, target::OUTPUT_FORMAT, rt)),
            emit: texture(device, "post emission", size, HDR_FORMAT, rt),
            glow: [0, 1].map(|_| texture(device, "post glow", half, HDR_FORMAT, rt)),
            glare: texture(device, "post glare", half, HDR_FORMAT, rt),
            streak: [0, 1].map(|_| texture(device, "post streak", half, HDR_FORMAT, rt)),
            fx: texture(device, "post effects", half, HDR_FORMAT, rt),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn bind(
        &self,
        device: &wgpu::Device,
        uniform: usize,
        src: Option<&wgpu::TextureView>,
        aux: Option<&wgpu::TextureView>,
        depth: Option<&wgpu::TextureView>,
        ao: Option<&wgpu::TextureView>,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms[uniform].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src.unwrap_or(&self.dummy_color)) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(aux.unwrap_or(&self.dummy_color)) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(depth.unwrap_or(&self.dummy_depth)) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(ao.unwrap_or(&self.dummy_color)) },
            ],
        })
    }

    fn fullscreen(encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup, to: &wgpu::TextureView) {
        Self::draw(encoder, pipeline, group, to, false);
    }

    /// Fullscreen triangle into `to`, clearing it first or adding to what is there.
    fn draw(encoder: &mut wgpu::CommandEncoder, pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup, to: &wgpu::TextureView, keep: bool) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: to,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: if keep { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) }, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Encode the extra passes over the finished scene in `output`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        output: &target::RenderTarget,
        globals_bg: &wgpu::BindGroup,
        mesh: Option<&gpu_mesh::GpuMesh>,
        emission: Option<&EmissionDraw>,
        proj: Mat4,
        params: &crate::post::PostParams,
    ) {
        let size = output.size;
        self.ensure_targets(device, size);
        let t = self.targets.as_ref().expect("created above");
        let view = |tex: &wgpu::Texture| tex.create_view(&wgpu::TextureViewDescriptor::default());
        let orthographic = proj.w_axis.w > 0.5;
        let write = |index: usize, size: [u32; 2], params4: [f32; 4], misc: [f32; 4]| {
            let u = PostUniform {
                inv_proj: proj.inverse().to_cols_array_2d(),
                params: params4,
                size: [size[0] as f32, size[1] as f32, 1.0 / size[0].max(1) as f32, 1.0 / size[1].max(1) as f32],
                misc,
                grade: [[0.0; 4]; 4],
                lens: [0.0; 4],
            };
            queue.write_buffer(&self.uniforms[index], 0, bytemuck::bytes_of(&u));
        };
        let focal = proj.y_axis.y;
        let ortho = if orthographic { 1.0 } else { 0.0 };
        let half = [size[0].div_ceil(2), size[1].div_ceil(2)];
        let (ao_a, ao_b, depth) = (view(&t.ao[0]), view(&t.ao[1]), view(&t.depth));
        let ssao = params.ssao > 0.0 && mesh.is_some();
        let dof = params.dof > 0.0 && mesh.is_some();
        if let (true, Some(mesh)) = (ssao || dof, mesh) {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("post depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.depth_pipeline);
                pass.set_bind_group(0, globals_bg, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                for r in &mesh.ranges {
                    pass.draw_indexed(r.first_index..r.first_index + r.index_count, 0, 0..1);
                }
            }
            if ssao {
            write(U_AO, size, [params.ssao_radius.max(1e-4), params.ssao, 0.0, ortho], [0.0, 0.0, 0.0, focal]);
            write(U_AO_H, size, [0.0; 4], [1.0, 0.0, 0.0, focal]);
            write(U_AO_V, size, [0.0; 4], [0.0, 1.0, 0.0, focal]);
            Self::fullscreen(encoder, &self.ao_pipeline, &self.bind(device, U_AO, None, None, Some(&depth), None), &ao_a);
            Self::fullscreen(encoder, &self.ao_blur_pipeline, &self.bind(device, U_AO_H, Some(&ao_a), None, None, None), &ao_b);
            Self::fullscreen(encoder, &self.ao_blur_pipeline, &self.bind(device, U_AO_V, Some(&ao_b), None, None, None), &ao_a);
            }
        }
        encoder.copy_texture_to_texture(
            output.output.as_image_copy(),
            t.scene.as_image_copy(),
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        );
        let scene_sharp = view(&t.scene);
        let dof_view = view(&t.dof);
        if dof {
            let radius = params.dof.clamp(0.0, 1.0) * 0.05 * size[1] as f32;
            write(U_DOF, size, [params.dof_focus.max(1e-4), params.dof_range.max(0.0), radius.max(1.0), ortho], [0.0, 0.0, 0.0, focal]);
            Self::fullscreen(encoder, &self.dof_pipeline, &self.bind(device, U_DOF, Some(&scene_sharp), None, Some(&depth), None), &dof_view);
        }
        // Everything after depth of field (AO, glow) lands on the blurred picture.
        let scene = if dof { dof_view } else { scene_sharp };
        let (bloom_a, bloom_b) = (view(&t.bloom[0]), view(&t.bloom[1]));
        let fx = view(&t.fx);
        let glow_wanted = emission.is_some() && params.emission_glow > 0.0;
        let glare_wanted = emission.is_some() && params.glare > 0.0;
        let flare_wanted = params.flare > 0.0;
        let fx_used = params.bloom > 0.0 || glow_wanted || glare_wanted || flare_wanted;
        if fx_used {
            // The effects buffer starts black; bloom, glow and glare are added into it.
            let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post effects clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &fx,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        if params.bloom > 0.0 {
            write(U_BRIGHT, half, [0.0; 4], [0.0, 0.0, params.bloom_threshold, focal]);
            // Falloff widens the blur: 0.5 is the original two-texel step.
            let step = 1.0 + 2.0 * params.bloom_falloff.clamp(0.0, 1.0);
            write(U_BLOOM_H, half, [0.0; 4], [step, 0.0, 0.0, focal]);
            write(U_BLOOM_V, half, [0.0; 4], [0.0, step, 0.0, focal]);
            Self::fullscreen(encoder, &self.bright_pipeline, &self.bind(device, U_BRIGHT, Some(&scene), None, None, None), &bloom_a);
            // Two blur rounds at half resolution give a soft glow a few percent of the view wide.
            for _ in 0..2 {
                Self::fullscreen(encoder, &self.blur_pipeline, &self.bind(device, U_BLOOM_H, Some(&bloom_a), None, None, None), &bloom_b);
                Self::fullscreen(encoder, &self.blur_pipeline, &self.bind(device, U_BLOOM_V, Some(&bloom_b), None, None, None), &bloom_a);
            }
            write(U_FX_BLOOM, half, [0.0, 0.0, params.bloom, 0.0], [0.0, 0.0, 0.0, focal]);
            Self::draw(encoder, &self.scale_add_pipeline, &self.bind(device, U_FX_BLOOM, Some(&bloom_a), None, None, None), &fx, true);
        }
        if flare_wanted {
            // Lens flare: the bright areas, softened, mirrored through the view centre as ghosts.
            write(U_FLARE_BRIGHT, half, [0.0; 4], [0.0, 0.0, params.flare_threshold, focal]);
            write(U_FLARE_H, half, [0.0; 4], [1.5, 0.0, 0.0, focal]);
            write(U_FLARE_V, half, [0.0; 4], [0.0, 1.5, 0.0, focal]);
            Self::fullscreen(encoder, &self.bright_pipeline, &self.bind(device, U_FLARE_BRIGHT, Some(&scene), None, None, None), &bloom_b);
            Self::fullscreen(encoder, &self.blur_pipeline, &self.bind(device, U_FLARE_H, Some(&bloom_b), None, None, None), &bloom_a);
            Self::fullscreen(encoder, &self.blur_pipeline, &self.bind(device, U_FLARE_V, Some(&bloom_a), None, None, None), &bloom_b);
            write(U_FLARE, half, [params.flare_ghosts.clamp(1, 8) as f32, params.flare_scale.clamp(0.05, 4.0), params.flare, 0.0], [0.0, 0.0, 0.0, focal]);
            Self::draw(encoder, &self.ghost_add_pipeline, &self.bind(device, U_FLARE, Some(&bloom_b), None, None, None), &fx, true);
        }
        if let (true, Some(draw)) = (glow_wanted || glare_wanted, emission) {
            let emit = view(&t.emit);
            {
                // Emission only, depth tested against itself so geometry hides what is behind it.
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("post emission"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &emit,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(draw.pipeline);
                pass.set_bind_group(0, globals_bg, &[]);
                pass.set_vertex_buffer(0, draw.mesh.vertices.slice(..));
                pass.set_index_buffer(draw.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                for r in &draw.mesh.ranges {
                    if draw.hidden.contains(&r.material_slot) { continue; }
                    let Some(bg) = draw.materials.get(&r.material_slot).and_then(|m| m.bind_group.as_ref()) else { continue };
                    pass.set_bind_group(1, bg, &[]);
                    pass.draw_indexed(r.first_index..r.first_index + r.index_count, 0, 0..1);
                }
            }
            let (glow_a, glow_b, glare) = (view(&t.glow[0]), view(&t.glow[1]), view(&t.glare));
            write(U_DOWN, half, [0.0; 4], [0.0, 0.0, 0.0, focal]);
            Self::fullscreen(encoder, &self.hdr_copy_pipeline, &self.bind(device, U_DOWN, Some(&emit), None, None, None), &glow_a);
            if glow_wanted {
                // Rounds and step grow with the radius so a big glow stays smooth.
                let radius = params.glow_radius.clamp(0.25, 8.0);
                let rounds = (2.0 + radius).round() as usize;
                let step = 1.0 + radius * 0.75;
                write(U_GLOW_H, half, [0.0; 4], [step, 0.0, 0.0, focal]);
                write(U_GLOW_V, half, [0.0; 4], [0.0, step, 0.0, focal]);
                // Ping-pong between the two glow buffers (glow_a is scratch, the result ends in glow_b).
                let (current, other) = (&glow_a, &glow_b);
                Self::fullscreen(encoder, &self.hdr_copy_pipeline, &self.bind(device, U_DOWN, Some(&emit), None, None, None), other);
                for _ in 0..rounds {
                    Self::fullscreen(encoder, &self.hdr_blur_pipeline, &self.bind(device, U_GLOW_H, Some(other), None, None, None), current);
                    Self::fullscreen(encoder, &self.hdr_blur_pipeline, &self.bind(device, U_GLOW_V, Some(current), None, None, None), other);
                }
                write(U_FX_GLOW, half, [0.0, 0.0, params.emission_glow, 0.0], [0.0, 0.0, 0.0, focal]);
                Self::draw(encoder, &self.scale_add_pipeline, &self.bind(device, U_FX_GLOW, Some(other), None, None, None), &fx, true);
                // glow_a now holds a scratch blur; refresh the sharp emission for the glare below.
                if glare_wanted {
                    Self::fullscreen(encoder, &self.hdr_copy_pipeline, &self.bind(device, U_DOWN, Some(&emit), None, None, None), &glow_a);
                }
            }
            if glare_wanted {
                // Each direction: a chain of [1 2 1] stages at doubling strides (a tent of half-width
                // 2^stages texels). Longer streaks run more stages; the gain keeps them readable.
                let recipe = glare_recipe(params.glare_mode);
                let length = (params.glare_length.clamp(0.0, 1.0) * recipe.length).clamp(0.0, 1.0);
                let stages = (4.0 + length * 2.0).round() as usize;
                // Threshold and remap shape what throws glare; the defaults keep all the emission.
                if params.glare_threshold > 0.0 || (params.glare_remap - 1.0).abs() > 1e-6 {
                    write(U_GLARE_SRC, half, [params.glare_threshold.max(0.0), params.glare_remap.clamp(0.05, 8.0), 0.0, 0.0], [0.0, 0.0, 0.0, focal]);
                    Self::fullscreen(encoder, &self.glare_src_pipeline, &self.bind(device, U_GLARE_SRC, Some(&emit), None, None, None), &glow_a);
                }
                let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("post glare clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &glare,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                let dirs = recipe.directions;
                let (scratch_a, scratch_b) = (view(&t.streak[0]), view(&t.streak[1]));
                let parts = dirs.len().max(1) as f32;
                let gain = params.glare * 0.5 * 2f32.powf(stages as f32 * 0.5) / parts;
                write(U_FX_GLARE, half, [0.0, 0.0, gain, 0.0], [0.0, 0.0, 0.0, focal]);
                // Spectral shapes spread red further and blue less far along each streak.
                let spread = if recipe.spectral { 0.35 } else { 0.0 };
                for (i, d) in dirs.iter().enumerate() {
                    let mut from: &wgpu::TextureView = &glow_a;
                    for k in 0..stages {
                        let stride = (1u32 << k) as f32;
                        write(U_STREAK + i * STAGES + k, half, [spread, 0.0, 0.0, 0.0], [d[0] * stride, d[1] * stride, 0.0, focal]);
                        let to = if k % 2 == 0 { &scratch_a } else { &scratch_b };
                        Self::fullscreen(encoder, &self.tent_pipeline, &self.bind(device, U_STREAK + i * STAGES + k, Some(from), None, None, None), to);
                        from = to;
                    }
                    Self::draw(encoder, &self.scale_add_pipeline, &self.bind(device, U_FX_GLARE, Some(from), None, None, None), &glare, true);
                }
                if recipe.glow > 0.0 {
                    // A round glow: three Gaussian rounds over the glare source.
                    write(U_GLARE_GLOW_H, half, [0.0; 4], [2.0 + 2.0 * length, 0.0, 0.0, focal]);
                    write(U_GLARE_GLOW_V, half, [0.0; 4], [0.0, 2.0 + 2.0 * length, 0.0, focal]);
                    let mut from: &wgpu::TextureView = &glow_a;
                    for _ in 0..3 {
                        Self::fullscreen(encoder, &self.hdr_blur_pipeline, &self.bind(device, U_GLARE_GLOW_H, Some(from), None, None, None), &scratch_a);
                        Self::fullscreen(encoder, &self.hdr_blur_pipeline, &self.bind(device, U_GLARE_GLOW_V, Some(&scratch_a), None, None, None), &scratch_b);
                        from = &scratch_b;
                    }
                    write(U_GLARE_GLOW_ADD, half, [0.0, 0.0, params.glare * 4.0 * recipe.glow, 0.0], [0.0, 0.0, 0.0, focal]);
                    Self::draw(encoder, &self.scale_add_pipeline, &self.bind(device, U_GLARE_GLOW_ADD, Some(from), None, None, None), &glare, true);
                }
                if recipe.ghosts > 0 {
                    write(U_GLARE_GHOST, half, [recipe.ghosts as f32, 1.0, params.glare, 0.0], [0.0, 0.0, 0.0, focal]);
                    Self::draw(encoder, &self.ghost_add_pipeline, &self.bind(device, U_GLARE_GHOST, Some(&glow_a), None, None, None), &glare, true);
                }
                write(U_FX_GLARE_ADD, half, [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, focal]);
                Self::draw(encoder, &self.scale_add_pipeline, &self.bind(device, U_FX_GLARE_ADD, Some(&glare), None, None, None), &fx, true);
            }
        }
        write(U_COMPOSITE, size, [0.0, if ssao { params.ssao } else { 0.0 }, if fx_used { 1.0 } else { 0.0 }, ortho], [0.0, 0.0, 0.0, focal]);
        let group = self.bind(device, U_COMPOSITE, Some(&scene), fx_used.then_some(&fx), None, ssao.then_some(&ao_a));
        Self::fullscreen(encoder, &self.composite_pipeline, &group, &output.output_view);
        if params.grade.active() {
            // Lens and color stage over the finished picture.
            encoder.copy_texture_to_texture(
                output.output.as_image_copy(),
                t.graded.as_image_copy(),
                wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            );
            let g = &params.grade;
            let wb = if g.color_correction { crate::grade::white_balance(g.temperature) } else { [1.0; 3] };
            let flag = |on: bool| if on { 1.0 } else { 0.0 };
            let u = PostUniform {
                inv_proj: proj.inverse().to_cols_array_2d(),
                params: [g.saturation, g.contrast, g.brightness, g.bias],
                size: [size[0] as f32, size[1] as f32, 1.0 / size[0].max(1) as f32, 1.0 / size[1].max(1) as f32],
                misc: [wb[0], wb[1], wb[2], focal],
                grade: [
                    [g.aberration.max(0.0), g.aberration_radius, g.aberration_softness.max(1e-4), g.aberration_ratio.max(1e-3)],
                    [g.aberration_center[0], g.aberration_center[1], flag(g.aberration_achromatic), g.vignette.clamp(0.0, 1.0)],
                    [g.sharpen.max(0.0), g.sharpen_diameter.max(0.0), g.grain.max(0.0), g.grain_radius.max(1.0)],
                    [flag(g.grain_colored), 1.0 / g.gamma.clamp(0.1, 10.0), g.sepia.clamp(0.0, 1.0), flag(g.color_correction)],
                ],
                lens: [g.lens_power, g.lens_zoom(), g.lens_roundness.clamp(0.0, 1.0), g.lens_smoothness.clamp(0.0, 1.0)],
            };
            queue.write_buffer(&self.uniforms[U_GRADE], 0, bytemuck::bytes_of(&u));
            Self::fullscreen(encoder, &self.grade_pipeline, &self.bind(device, U_GRADE, Some(&view(&t.graded)), None, None, None), &output.output_view);
        }
    }
}

#[cfg(test)]
mod recipe_tests {
    use super::*;

    #[test]
    fn every_glare_shape_has_a_recipe_of_unit_directions() {
        for shape in GlareMode::ALL {
            let r = glare_recipe(shape);
            assert!(!r.directions.is_empty() || r.glow > 0.0, "{shape:?} draws something");
            assert!(r.directions.len() <= 4, "{shape:?}: the streak uniforms hold four directions");
            for d in r.directions {
                assert!(((d[0] * d[0] + d[1] * d[1]).sqrt() - 1.0).abs() < 1e-5, "{shape:?}: {d:?} is a unit step");
            }
        }
        // Painter's filters: cross screen 4 points, snow cross 6, sunny cross 8 (two per direction).
        assert_eq!(glare_recipe(GlareMode::Both).directions.len() * 2, 4);
        assert_eq!(glare_recipe(GlareMode::SnowCross).directions.len() * 2, 6);
        assert_eq!(glare_recipe(GlareMode::Star).directions.len() * 2, 8);
        let snow = glare_recipe(GlareMode::SnowCross).directions;
        let angle = |d: [f32; 2]| d[1].atan2(d[0]).to_degrees();
        assert!((angle(snow[1]) - 60.0).abs() < 1e-3 && (angle(snow[2]) + 60.0).abs() < 1e-3, "arms 60 degrees apart");
        for (plain, spectral) in [(GlareMode::Both, GlareMode::CrossScreenSpectral), (GlareMode::SnowCross, GlareMode::SnowCrossSpectral), (GlareMode::Star, GlareMode::SunnyCrossSpectral)] {
            let (a, b) = (glare_recipe(plain), glare_recipe(spectral));
            assert!(a.directions == b.directions && !a.spectral && b.spectral, "{spectral:?} is {plain:?} with fringes");
        }
        assert!(glare_recipe(GlareMode::Bloom).directions.is_empty() && glare_recipe(GlareMode::LensFlare).ghosts > 0);
    }
}
