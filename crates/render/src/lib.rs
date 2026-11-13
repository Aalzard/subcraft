//! wgpu viewport renderer (ADR-0001/0003): draws the mesh with its composite channel textures
//! into an offscreen target that the UI shows as an image (and tests read back).
//!
//! Textures are updated per dirty tile straight from the composite `TiledImage`s (f16 data is
//! uploaded as-is, no conversion). Lighting: metal/roughness GGX with one key light and a
//! procedural sky environment (analytic irradiance + split-sum approximation); ACES tone map;
//! sRGB output.

pub mod caps;
pub mod gpu_composite;
pub mod gpu_mesh;
pub mod grade;
pub mod grid;
mod bake_preview;
mod display;
pub mod headless;
pub mod material;
mod mipmap;
pub mod offline;
pub mod pace;
pub mod pathtrace;
pub mod post;
pub mod ray_bake;
pub mod preview;
pub mod target;

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use glam::{Mat4, Vec3};
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_image::TiledImage;
use subcraft_mesh::Mesh;
use wgpu::util::DeviceExt;

pub use egui_wgpu;
pub use egui_wgpu::wgpu as wgpu_api;
pub use gpu_mesh::GpuMesh;
pub use grid::{GridParams, grid_step};
pub use subcraft_doc::display::ChannelComponent;
pub use material::{ChannelTexture, GpuMaterial, SHADED_CHANNELS};
pub use pathtrace::{Denoiser, FIREFLY_CLAMP, GroundSettings, PathTraceScene, PathTraceSettings, PathTraceStatus, render_done};
pub use grade::Grade;
pub use post::{GlareMode, PostParams, ToneMap};
pub use preview::{PreviewPass, PreviewTarget};
pub use target::{DISPLAY_FORMAT, OUTPUT_FORMAT, PendingReadback, RenderTarget};

/// What the 2D view displays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PreviewSource {
    Channel(ChannelKind),
    MeshMap(MeshMapKind),
}

/// Environment lighting (procedural sky + studio lights, or an uploaded HDRI).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentParams {
    pub ground: [f32; 3],
    pub horizon: [f32; 3],
    pub zenith: [f32; 3],
    pub irradiance_up: [f32; 3],
    pub irradiance_down: [f32; 3],
    pub panels: f32,
    pub top: f32,
    pub key_softbox: f32,
    /// Use the HDRI uploaded with [`ViewportRenderer::set_hdri`] instead of the procedural sky.
    pub use_hdri: bool,
    pub hdri_intensity: f32,
    /// Rotation of the environment around +Y (radians).
    pub rotation: f32,
    /// How much of the environment is drawn behind the model, over the background color
    /// (0 = none: the frame is exactly the plain-background render).
    pub background_opacity: f32,
    /// Blur of that background, 0 (sharp) to 1 (the fully diffused map).
    pub background_blur: f32,
}

impl Default for EnvironmentParams {
    fn default() -> Self {
        Self {
            ground: [0.10, 0.09, 0.08],
            horizon: [0.42, 0.43, 0.46],
            zenith: [0.20, 0.27, 0.40],
            irradiance_up: [0.32, 0.35, 0.40],
            irradiance_down: [0.13, 0.12, 0.11],
            panels: 14.0,
            top: 10.0,
            key_softbox: 30.0,
            use_hdri: false,
            hdri_intensity: 1.0,
            rotation: 0.0,
            background_opacity: 0.0,
            background_blur: 0.0,
        }
    }
}

/// HDRI data to upload: prefiltered levels (largest first, linear RGBA f32) + SH9 irradiance.
pub struct HdriUpload<'a> {
    pub levels: &'a [(u32, u32, &'a [f32])],
    pub sh: [[f32; 3]; 9],
}

/// How a channel shown alone on the model is displayed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelDisplay {
    /// The channel as it is, without lighting. Off shades it with the key light, so the shape
    /// of the model stays readable.
    pub unlit: bool,
    /// Values are divided by this before they are shown (brings HDR values into view).
    pub hdr_scale: f32,
    /// Grayscale channels: positive values in `positive`, negative ones in `negative`.
    pub signed_colors: bool,
    /// Display (sRGB) colors of the two signs.
    pub positive: [f32; 3],
    pub negative: [f32; 3],
    /// One component as gray, or all of them.
    pub component: ChannelComponent,
}

impl Default for ChannelDisplay {
    fn default() -> Self {
        Self { unlit: true, hdr_scale: 1.0, signed_colors: false, positive: [0.25, 0.85, 0.30], negative: [0.90, 0.25, 0.20], component: ChannelComponent::All }
    }
}

/// Camera and lighting for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewParams {
    pub view: Mat4,
    pub proj: Mat4,
    pub eye: Vec3,
    /// Direction towards the key light (world space).
    pub light_dir: Vec3,
    pub light_intensity: f32,
    pub light_color: [f32; 3],
    pub environment_intensity: f32,
    pub environment: EnvironmentParams,
    pub exposure: f32,
    /// Height-to-bump strength (UV units of slope per height unit; 0 disables bumps).
    pub height_scale: f32,
    /// Linear background color.
    pub background: [f32; 3],
    /// 4x MSAA when true, no anti-aliasing (faster) when false.
    pub antialias: bool,
    /// Full material shading or one channel shown unlit.
    pub view_mode: ViewMode,
    /// Display-only floor/axes. None for thumbnails, exports and callers that need no guides.
    pub grid: Option<GridParams>,
    /// Display-only high/low comparison using meshes supplied by `set_bake_preview`.
    pub bake_overlay: bool,
    pub bake_cage: bool,
    /// Color the low poly in the bake overlay by the miss maps of `set_bake_miss_maps` (red = the
    /// projection ray reaches no high-poly surface).
    pub bake_miss_heatmap: bool,
    /// Placement matrix of the high poly in the overlay, and its opacity (1 = solid).
    pub bake_high_transform: Mat4,
    pub bake_high_opacity: f32,
    pub bake_distances: [f32; 2],
    /// Show a raw baked map on the low-poly UVs without changing material textures.
    pub bake_map: Option<MeshMapKind>,
    pub symmetry_guides: bool,
    /// Draw the polygon wireframe uploaded with `set_wireframe` over the model.
    pub wireframe: bool,
    /// Tone mapping, bloom and ambient occlusion (defaults leave the image as before).
    pub post: PostParams,
    /// Any channel shown alone on the model, uploaded or not (overrides `view_mode`).
    pub solo_channel: Option<ChannelKind>,
    /// How single-channel views are displayed.
    pub channel_display: ChannelDisplay,
    /// Texture level bias of the material textures: 0 (soft) down to -3 (very sharp).
    pub mip_bias: f32,
}

/// What the 3D view shows on the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// Full PBR shading.
    #[default]
    Material,
    BaseColor,
    Normal,
    Roughness,
    Metallic,
    Height,
    AmbientOcclusion,
    /// Full shading, but the normal input is the baked tangent-space normal map instead of the
    /// painted Normal channel (a flat normal until a bake exists).
    BakedNormal,
    /// The baked tangent-space normal map itself, unlit, on the model.
    BakedNormalMap,
}

impl ViewMode {
    pub const ALL: [ViewMode; 9] = [
        ViewMode::Material,
        ViewMode::BaseColor,
        ViewMode::Normal,
        ViewMode::Roughness,
        ViewMode::Metallic,
        ViewMode::Height,
        ViewMode::AmbientOcclusion,
        ViewMode::BakedNormal,
        ViewMode::BakedNormalMap,
    ];

    /// Value of the shader's view-mode switch (0 = full shading). The baked-normal modes are
    /// handled by the renderer: one shades normally with another bind group, the other previews
    /// the baked map through the bake-map shader modes.
    fn shader_mode(self) -> u32 {
        match self {
            ViewMode::BakedNormal | ViewMode::BakedNormalMap => 0,
            m => m as u32,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Material => "Material",
            ViewMode::BaseColor => "Base Color",
            ViewMode::Normal => "Normal",
            ViewMode::Roughness => "Roughness",
            ViewMode::Metallic => "Metallic",
            ViewMode::Height => "Height",
            ViewMode::AmbientOcclusion => "Ambient Occlusion",
            ViewMode::BakedNormal => "Baked Normal (shaded)",
            ViewMode::BakedNormalMap => "Baked Normal Map",
        }
    }
}

impl ViewParams {
    pub fn new(view: Mat4, proj: Mat4) -> Self {
        Self {
            view,
            proj,
            eye: view.inverse().w_axis.truncate(),
            light_dir: Vec3::new(0.65, 0.75, 0.35).normalize(),
            light_intensity: 1.5,
            light_color: [1.0, 0.96, 0.9],
            environment_intensity: 1.0,
            environment: EnvironmentParams::default(),
            exposure: 1.0,
            height_scale: 0.02,
            background: [0.018, 0.019, 0.022],
            antialias: true,
            view_mode: ViewMode::Material,
            grid: None,
            bake_overlay: false, bake_cage: true, bake_miss_heatmap: false, bake_high_transform: Mat4::IDENTITY, bake_high_opacity: 1.0, bake_distances: [0.01; 2], bake_map: None, symmetry_guides: false, wireframe: false,
            post: PostParams::default(),
            solo_channel: None,
            channel_display: ChannelDisplay::default(),
            mip_bias: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    light_dir: [f32; 4],
    params: [f32; 4],
    light_color: [f32; 4],
    ground: [f32; 4],
    horizon: [f32; 4],
    zenith: [f32; 4],
    irr_up: [f32; 4],
    irr_down: [f32; 4],
    lights: [f32; 4],
    env_misc: [f32; 4],
    post: [f32; 4],
    sh: [[f32; 4]; 9],
    inv_view_proj: [[f32; 4]; 4],
    /// rgb: background color (linear), a: environment opacity behind the model.
    background: [f32; 4],
    /// x: background blur 0..1.
    env_view: [f32; 4],
    /// x: 1 / HDR scale, y: 1 = two-color signed display, z: component (0 all, 1 R, 2 G, 3 B, 4 A), w: 1 = lit.
    solo: [f32; 4],
    /// rgb: color of positive values, a: texture level bias.
    solo_positive: [f32; 4],
    solo_negative: [f32; 4],
}

pub const SAMPLE_COUNT: u32 = 4;

pub struct ViewportRenderer {
    /// 4x MSAA pipeline.
    pipeline: wgpu::RenderPipeline,
    /// Single-sample pipeline (anti-aliasing off).
    pipeline_no_aa: wgpu::RenderPipeline,
    /// Alpha-blended materials (4x MSAA, single-sample), drawn after the opaque ones.
    pipeline_blend: [wgpu::RenderPipeline; 2],
    /// Opaque `MaterialParams` for views that ignore opacity (solo channel, baked maps).
    opaque_params: wgpu::Buffer,
    /// Emission-only pass feeding the glow and glare effects.
    pipeline_emissive: wgpu::RenderPipeline,
    /// Environment drawn behind the model (4x MSAA and single-sample).
    pipeline_background: [wgpu::RenderPipeline; 2],
    grid: grid::GridPass,
    symmetry: Option<grid::GridPass>,
    wireframe: Option<grid::GridPass>,
    bake_preview: bake_preview::BakePreviewPass,
    globals: wgpu::Buffer,
    globals_layout: wgpu::BindGroupLayout,
    globals_bg: wgpu::BindGroup,
    env_sampler: wgpu::Sampler,
    env_texture: wgpu::TextureView,
    env_levels: u32,
    env_sh: [[f32; 3]; 9],
    material_layout: wgpu::BindGroupLayout,
    /// Anisotropic filtering samples of `sampler` (1 = off).
    anisotropy: u16,
    sampler: wgpu::Sampler,
    nearest_sampler: wgpu::Sampler,
    mipmaps: mipmap::MipGenerator,
    defaults: HashMap<ChannelKind, ChannelTexture>,
    mesh: Option<GpuMesh>,
    materials: HashMap<u32, GpuMaterial>,
    target: Option<RenderTarget>,
    preview: PreviewPass,
    preview_target: Option<PreviewTarget>,
    post: post::PostPass,
    /// Rendering mode (created on first use), its single-sample target, and why it is unavailable.
    path_tracer: Option<pathtrace::PathTracer>,
    /// Material slots not drawn (hidden texture sets). The path tracer ignores this.
    hidden_slots: std::collections::HashSet<u32>,
    path_target: Option<RenderTarget>,
    path_support: Option<Result<(), String>>,
}

impl ViewportRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport shader"),
            source: wgpu::ShaderSource::Wgsl(concat!(include_str!("tonemap.wgsl"), include_str!("viewport.wgsl")).into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        }];
        for i in 0..SHADED_CHANNELS.len() as u32 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: i + 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 8, visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None,
        });
        // Opacity (9) and the per-material parameters (10), issue #145.
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 9,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 10,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        });
        let material_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("material layout"), entries: &entries });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport layout"),
            bind_group_layouts: &[Some(&globals_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        // `blend`: the alpha-blended pass (straight alpha over what is drawn, target alpha kept,
        // depth tested but not written).
        let make_pipeline = |samples: u32, blend: bool| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if blend { "viewport blend pipeline" } else { "viewport pipeline" }),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(gpu_mesh::Vertex::layout())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target::DEPTH_FORMAT,
                depth_write_enabled: Some(!blend),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OUTPUT_FORMAT,
                    blend: blend.then_some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let pipeline_emissive = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("emission pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[Some(gpu_mesh::Vertex::layout())] },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target::DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_emissive"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: post::HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        // The environment behind the model: one triangle over the whole view, drawn before the
        // mesh without touching depth. It needs the globals only.
        let background_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("background layout"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let make_background = |samples: u32| device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("background pipeline"),
            layout: Some(&background_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_background"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: target::DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_background"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: OUTPUT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let pipeline_background = [make_background(if caps::get().msaa4 { SAMPLE_COUNT } else { 1 }), make_background(1)];
        let pipeline = make_pipeline(if caps::get().msaa4 { SAMPLE_COUNT } else { 1 }, false);
        let pipeline_no_aa = make_pipeline(1, false);
        let pipeline_blend = [make_pipeline(if caps::get().msaa4 { SAMPLE_COUNT } else { 1 }, true), make_pipeline(1, true)];
        let globals = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("globals"),
            contents: bytemuck::bytes_of(&Globals::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let env_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let env_texture = ChannelTexture::constant(device, queue, [0.5, 0.5, 0.5, 1.0], 4, "environment placeholder").view;
        let globals_bg = Self::globals_bind_group(device, &globals_layout, &globals, &env_texture, &env_sampler);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let defaults = SHADED_CHANNELS
            .iter()
            .chain(&[ChannelKind::Opacity])
            .map(|&k| {
                let d = k.default_value().0;
                (k, ChannelTexture::constant(device, queue, [d[0], d[1], d[2], 1.0], composite_comps(k), k.key()))
            })
            .collect();
        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest albedo"), address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge, mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest, lod_max_clamp: 0.0, ..Default::default()
        });
        let post = post::PostPass::new(device, &globals_layout, &shader);
        let opaque_params = material::params_buffer(device, &subcraft_doc::AlphaSettings::default(), &subcraft_doc::ShaderParams::default());
        Self {
            post,
            pipeline_blend,
            opaque_params,
            pipeline_emissive,
            pipeline_background,
            pipeline,
            pipeline_no_aa,
            grid: grid::GridPass::new(device),
            symmetry: None,
            wireframe: None,
            bake_preview: bake_preview::BakePreviewPass::new(device),
            globals,
            globals_layout,
            globals_bg,
            env_sampler,
            env_texture,
            env_levels: 1,
            env_sh: [[0.0; 3]; 9],
            material_layout, anisotropy: 1, sampler, nearest_sampler, mipmaps: mipmap::MipGenerator::new(device), defaults, mesh: None, materials: HashMap::new(), target: None, preview: PreviewPass::new(device), preview_target: None,
            path_tracer: None, hidden_slots: Default::default(), path_target: None, path_support: None }
    }

    fn globals_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        globals: &wgpu::Buffer,
        env: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(env) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    /// Upload a prefiltered HDRI (mip level k = level k of the data). `None` restores the
    /// placeholder; select it per frame with [`EnvironmentParams::use_hdri`].
    pub fn set_hdri(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, hdri: Option<HdriUpload>) {
        match hdri {
            None => {
                self.env_texture = ChannelTexture::constant(device, queue, [0.5, 0.5, 0.5, 1.0], 4, "environment placeholder").view;
                self.env_levels = 1;
                self.env_sh = [[0.0; 3]; 9];
            }
            Some(h) => {
                let (w, hgt, _) = h.levels[0];
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("hdri"),
                    size: wgpu::Extent3d { width: w, height: hgt, depth_or_array_layers: 1 },
                    mip_level_count: h.levels.len() as u32,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                for (k, (lw, lh, data)) in h.levels.iter().enumerate() {
                    let halfs: Vec<half::f16> = data.iter().map(|&v| half::f16::from_f32(v)).collect();
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: k as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                        bytemuck::cast_slice(&halfs),
                        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 8), rows_per_image: Some(*lh) },
                        wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 },
                    );
                }
                self.env_texture = tex.create_view(&wgpu::TextureViewDescriptor::default());
                self.env_levels = h.levels.len() as u32;
                self.env_sh = h.sh;
            }
        }
        self.globals_bg = Self::globals_bind_group(device, &self.globals_layout, &self.globals, &self.env_texture, &self.env_sampler);
        if let Some(pt) = &mut self.path_tracer { pt.environment_changed(); }
    }

    /// Replace the mesh (drops all material textures).
    pub fn set_mesh(&mut self, device: &wgpu::Device, mesh: &Mesh) {
        self.mesh = Some(GpuMesh::new(device, mesh));
        self.materials.clear();
        if let Some(pt) = &mut self.path_tracer { pt.mark_all(); }
    }

    /// Hide the geometry of these material slots in the raster viewport.
    pub fn set_hidden_slots(&mut self, slots: impl IntoIterator<Item = u32>) {
        self.hidden_slots = slots.into_iter().collect();
    }

    pub fn has_mesh(&self) -> bool {
        self.mesh.is_some()
    }

    pub fn set_symmetry_guides(&mut self, device: &wgpu::Device, lines: &[[Vec3; 2]]) {
        if lines.is_empty() { self.symmetry = None; return; }
        self.symmetry.get_or_insert_with(|| grid::GridPass::new_guides(device)).set_lines(device, lines);
    }

    /// Upload the wireframe lines (world space) with their linear color and opacity. Call again
    /// when the lines, color or opacity change; an empty list removes the overlay.
    pub fn set_wireframe(&mut self, device: &wgpu::Device, lines: &[[Vec3; 2]], color: [f32; 3], opacity: f32) {
        if lines.is_empty() { self.wireframe = None; return; }
        let rgba = [color[0], color[1], color[2], opacity.clamp(0.0, 1.0)];
        self.wireframe.get_or_insert_with(|| grid::GridPass::new_guides(device)).set_lines_colored(device, lines, rgba);
    }

    /// Cache display geometry once, independently of the project's mesh and material textures.
    pub fn set_bake_preview(&mut self, device: &wgpu::Device, meshes: Option<(&Mesh, &Mesh)>) {
        self.bake_preview.set_meshes(device, meshes);
    }

    /// Ray-miss maps (1 = miss) per low-poly material slot for the bake overlay's heatmap. Uploading
    /// new overlay meshes drops them; an empty list removes them.
    pub fn set_bake_miss_maps(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, maps: &[(u32, &subcraft_image::TiledImage)]) {
        self.bake_preview.set_miss_maps(device, queue, maps);
    }

    /// Upload `tiles` of a channel's composite (all tiles when `None`, or when the texture had
    /// to be (re)created because size or format changed).
    pub fn upload_channel(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material_slot: u32,
        kind: ChannelKind,
        image: &TiledImage,
        tiles: Option<&[usize]>,
    ) {
        let mat = self.materials.entry(material_slot).or_default();
        let recreate = mat.channels.get(&kind).is_none_or(|t| t.size != image.width() || t.comps != image.comps());
        if recreate {
            mat.channels.insert(kind, ChannelTexture::new_mipmapped(device, image.width(), image.comps(), kind.key(), &self.mipmaps.layout));
            mat.bind_group = None;
        }
        let tex = &mat.channels[&kind];
        match tiles {
            Some(t) if !recreate => tex.upload(queue, image, t),
            _ => tex.upload_all(device, queue, image),
        }
        self.mipmaps.update(device, queue, tex, image, if recreate { None } else { tiles });
        if let Some(pt) = &mut self.path_tracer { pt.mark_slot(material_slot); }
    }

    /// View of an uploaded channel texture (UV view display).
    pub fn channel_view(&self, material_slot: u32, kind: ChannelKind) -> Option<&wgpu::TextureView> {
        self.materials.get(&material_slot)?.channels.get(&kind).map(|t| &t.view)
    }

    /// Upload a baked mesh map (2D view display; replaces any previous one of that kind).
    pub fn upload_mesh_map(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, material_slot: u32, kind: MeshMapKind, image: &TiledImage) {
        let mat = self.materials.entry(material_slot).or_default();
        let tex = ChannelTexture::new(device, image.width(), image.comps(), kind.key());
        tex.upload(queue, image, &(0..image.tile_count()).collect::<Vec<_>>());
        mat.mesh_maps.insert(kind, tex);
        mat.bake_bind_groups.remove(&kind);
    }

    /// Render a display preview (sRGB 8-bit, egui-ready `display_view`) of an uploaded channel or
    /// baked map. Scalar sources are shown as gray.
    pub fn preview(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, material_slot: u32, source: PreviewSource) -> Option<&PreviewTarget> {
        let mat = self.materials.get(&material_slot)?;
        let (tex, is_color) = match source {
            PreviewSource::Channel(k) => (mat.channels.get(&k)?, k.is_color()),
            PreviewSource::MeshMap(k) => (mat.mesh_maps.get(&k)?, false),
        };
        if self.preview_target.as_ref().is_none_or(|t| t.size != tex.size) {
            self.preview_target = Some(PreviewPass::target(device, tex.size));
        }
        let dst = self.preview_target.as_ref().expect("created above");
        self.preview.run(device, queue, &tex.view, is_color, tex.comps == 1, dst);
        Some(dst)
    }

    /// Anisotropic filtering of the material textures: `samples` per pixel (1 = off, up to 16).
    /// Ignored (stays off) where the GPU has no anisotropic filtering.
    pub fn set_texture_filtering(&mut self, device: &wgpu::Device, samples: u16) {
        let samples = if caps::get().anisotropy { samples.clamp(1, 16) } else { 1 };
        if samples == self.anisotropy {
            return;
        }
        self.anisotropy = samples;
        self.sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: samples,
            ..Default::default()
        });
        // Every bind group holds the old sampler.
        for mat in self.materials.values_mut() {
            mat.bind_group = None;
            mat.bake_bind_groups.clear();
        }
    }

    /// Anisotropic filtering samples in use (1 = off).
    pub fn texture_filtering(&self) -> u16 {
        self.anisotropy
    }

    /// Change only Base Color reconstruction; reuse all channel textures and pipelines.
    pub fn set_albedo_filter(&mut self, material_slot: u32, filter: subcraft_image::TextureFilter) {
        let mat = self.materials.entry(material_slot).or_default();
        if mat.albedo_filter != filter { mat.albedo_filter = filter; mat.bind_group = None; }
    }

    /// How the viewport draws a texture set's opacity (issue #145): alpha test, blending or
    /// dithering of Base Color coverage times the Opacity channel. Opaque by default.
    pub fn set_alpha(&mut self, material_slot: u32, alpha: subcraft_doc::AlphaSettings) {
        let mat = self.materials.entry(material_slot).or_default();
        if mat.alpha != alpha {
            mat.alpha = alpha;
            mat.params = None;
            mat.bind_group = None;
        }
    }

    /// A texture set's shader instance parameters (Shader Settings): AO intensity, emissive
    /// intensity and horizon fading.
    pub fn set_shader(&mut self, material_slot: u32, shader: subcraft_doc::ShaderParams) {
        let shader = shader.clamped();
        let mat = self.materials.entry(material_slot).or_default();
        if mat.shader != shader {
            mat.shader = shader;
            mat.params = None;
            mat.bind_group = None;
        }
    }

    fn ensure_bind_groups(&mut self, device: &wgpu::Device) {
        for mat in self.materials.values_mut() {
            if mat.bind_group.is_some() {
                continue;
            }
            let params = mat.params.get_or_insert_with(|| material::params_buffer(device, &mat.alpha, &mat.shader));
            let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) }];
            let albedo_sampler = if mat.albedo_filter == subcraft_image::TextureFilter::Nearest { &self.nearest_sampler } else { &self.sampler };
            entries.push(wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(albedo_sampler) });
            for (i, k) in SHADED_CHANNELS.iter().enumerate() {
                let view = mat.channels.get(k).map_or(&self.defaults[k].view, |t| &t.view);
                entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: wgpu::BindingResource::TextureView(view) });
            }
            let opacity = mat.channels.get(&ChannelKind::Opacity).map_or(&self.defaults[&ChannelKind::Opacity].view, |t| &t.view);
            entries.extend(alpha_entries(opacity, params));
            mat.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material"),
                layout: &self.material_layout,
                entries: &entries,
            }));
        }
    }

    /// Render a frame into the offscreen target (resized as needed, never beyond what the
    /// device can allocate) and return the target.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, size: [u32; 2], params: &ViewParams) -> &RenderTarget {
        let size = fit_target_size(size, device.limits().max_texture_dimension_2d);
        let samples = if params.antialias && caps::get().msaa4 { SAMPLE_COUNT } else { 1 };
        if self.target.as_ref().is_none_or(|t| t.size != size || t.samples != samples) {
            self.target = Some(RenderTarget::new(device, size, samples));
        }
        self.ensure_bind_groups(device);
        // "Baked Normal Map" is the bake-map preview of the Normal map.
        let bake_map = params.bake_map.or((params.view_mode == ViewMode::BakedNormalMap).then_some(MeshMapKind::Normal));
        // "Baked Normal (shaded)": the material's own bind group with the Normal slot swapped.
        let mut baked_normal_groups: std::collections::HashMap<u32, wgpu::BindGroup> = Default::default();
        if params.view_mode == ViewMode::BakedNormal && bake_map.is_none() {
            for (slot, mat) in &self.materials {
                let albedo_sampler = if mat.albedo_filter == subcraft_image::TextureFilter::Nearest { &self.nearest_sampler } else { &self.sampler };
                let mut entries = vec![
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(albedo_sampler) },
                ];
                for (i, k) in SHADED_CHANNELS.iter().enumerate() {
                    let view = if *k == ChannelKind::Normal {
                        mat.mesh_maps.get(&MeshMapKind::Normal).map_or(&self.defaults[k].view, |t| &t.view)
                    } else {
                        mat.channels.get(k).map_or(&self.defaults[k].view, |t| &t.view)
                    };
                    entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: wgpu::BindingResource::TextureView(view) });
                }
                let opacity = mat.channels.get(&ChannelKind::Opacity).map_or(&self.defaults[&ChannelKind::Opacity].view, |t| &t.view);
                entries.extend(alpha_entries(opacity, mat.params.as_ref().unwrap_or(&self.opaque_params)));
                baked_normal_groups.insert(*slot, device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("baked normal material"), layout: &self.material_layout, entries: &entries }));
            }
        }
        // A channel shown alone: its texture in the Base Color slot, read by the unlit modes.
        // Channels the shader does not otherwise use (Opacity, Specular, User...) work the same.
        let solo = params.solo_channel.filter(|_| bake_map.is_none());
        let mut solo_groups: std::collections::HashMap<u32, wgpu::BindGroup> = Default::default();
        if let Some(kind) = solo {
            let d = kind.default_value().0;
            let neutral = ChannelTexture::constant(device, queue, [d[0], d[1], d[2], 1.0], composite_comps(kind), "solo channel default");
            for (slot, mat) in &self.materials {
                let nearest = kind == ChannelKind::BaseColor && mat.albedo_filter == subcraft_image::TextureFilter::Nearest;
                let mut entries = vec![
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(if nearest { &self.nearest_sampler } else { &self.sampler }) },
                ];
                for (i, k) in SHADED_CHANNELS.iter().enumerate() {
                    let view = if i == 0 { mat.channels.get(&kind).map_or(&neutral.view, |t| &t.view) } else { &self.defaults[k].view };
                    entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: wgpu::BindingResource::TextureView(view) });
                }
                entries.extend(alpha_entries(&self.defaults[&ChannelKind::Opacity].view, &self.opaque_params));
                solo_groups.insert(*slot, device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("solo channel"), layout: &self.material_layout, entries: &entries }));
            }
        }
        // Shader mode of the solo channel: a color (1), three stored components (7) or one (8).
        let solo_mode = solo.map(|k| if k.is_color() { 1.0 } else if k.components() == 3 { 7.0 } else { 8.0 });
        if let Some(kind) = bake_map {
            for mat in self.materials.values_mut() {
                if mat.bake_bind_groups.contains_key(&kind) { continue; }
                let neutral = ChannelTexture::constant(device, queue, {
                    let n = kind.neutral_value(); [n[0], n[1], n[2], 1.]
                }, if kind.components() == 1 { 1 } else { 4 }, "pending bake neutral");
                let tex = mat.mesh_maps.get(&kind).unwrap_or(&neutral);
                let mut entries = vec![
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ];
                for (i, k) in SHADED_CHANNELS.iter().enumerate() {
                    let view = if i == 0 { &tex.view } else { &self.defaults[k].view };
                    entries.push(wgpu::BindGroupEntry { binding: i as u32 + 1, resource: wgpu::BindingResource::TextureView(view) });
                }
                entries.extend(alpha_entries(&self.defaults[&ChannelKind::Opacity].view, &self.opaque_params));
                mat.bake_bind_groups.insert(kind, device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bake map preview"), layout: &self.material_layout, entries: &entries,
                }));
            }
        }
        let e = &params.environment;
        let v4 = |c: [f32; 3]| [c[0], c[1], c[2], 0.0];
        let mut sh = [[0.0f32; 4]; 9];
        for (dst, src) in sh.iter_mut().zip(&self.env_sh) {
            *dst = v4(*src);
        }
        let globals = Globals {
            view_proj: (params.proj * params.view).to_cols_array_2d(),
            eye: params.eye.extend(1.0).to_array(),
            light_dir: params.light_dir.normalize_or(Vec3::Y).extend(0.0).to_array(),
            params: [params.exposure, params.environment_intensity, params.light_intensity, params.height_scale],
            light_color: v4(params.light_color),
            ground: v4(e.ground),
            horizon: v4(e.horizon),
            zenith: v4(e.zenith),
            irr_up: v4(e.irradiance_up),
            irr_down: v4(e.irradiance_down),
            lights: [e.panels, e.top, e.key_softbox, if e.use_hdri && self.env_levels > 1 { 1.0 } else { 0.0 }],
            env_misc: [e.rotation, e.hdri_intensity, (self.env_levels - 1) as f32,
                bake_map.map_or(solo_mode.unwrap_or(params.view_mode.shader_mode() as f32), |k| if k.components() == 1 { 8. } else { 7. })],
            post: [grade::tone_map_id(params.post.tone_map), params.post.mapping_factor, 0.0, 0.0],
            sh,
            inv_view_proj: (params.proj * params.view).inverse().to_cols_array_2d(),
            background: [params.background[0], params.background[1], params.background[2], e.background_opacity.clamp(0.0, 1.0)],
            env_view: [e.background_blur.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
            solo: [
                1.0 / params.channel_display.hdr_scale.max(1e-6),
                if params.channel_display.signed_colors { 1.0 } else { 0.0 },
                params.channel_display.component as u32 as f32,
                if params.channel_display.unlit { 0.0 } else { 1.0 },
            ],
            solo_positive: { let c = params.channel_display.positive; [c[0], c[1], c[2], params.mip_bias.clamp(-8.0, 8.0)] },
            solo_negative: v4(params.channel_display.negative),
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        if params.symmetry_guides && let Some(guides) = &self.symmetry { guides.prepare_lines(queue, params.proj * params.view); }
        if params.wireframe && let Some(wire) = &self.wireframe { wire.prepare_lines(queue, params.proj * params.view); }
        if params.bake_overlay {
            self.bake_preview.prepare(queue, params.proj * params.view, params.bake_distances, params.bake_high_transform, params.bake_high_opacity, params.bake_miss_heatmap);
        }
        if let Some(grid) = params.grid {
            self.grid.prepare(device, queue, params.proj * params.view, params.eye, grid);
        }
        let target = self.target.as_ref().expect("created above");
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport") });
        {
            let bg = params.background;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport pass"),
                // Multisampled: draw into the MSAA texture and resolve; single-sample: draw directly.
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.msaa_view.as_ref().unwrap_or(&target.output_view),
                    depth_slice: None,
                    resolve_target: target.msaa_view.as_ref().map(|_| &target.output_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: 1.0 }),
                        // The MSAA samples are only needed until resolved; a direct draw is the image.
                        store: if target.msaa_view.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if e.background_opacity > 0.0 {
                pass.set_pipeline(&self.pipeline_background[if samples > 1 { 0 } else { 1 }]);
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.draw(0..3, 0..1);
            }
            if params.bake_overlay {
                self.bake_preview.draw(&mut pass, samples > 1, params.bake_cage);
            } else if let Some(mesh) = &self.mesh {
                pass.set_bind_group(0, &self.globals_bg, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                // Opaque (and alpha-tested / dithered) sets first, then alpha-blended ones over
                // them without writing depth. Single-channel and baked-map views are opaque.
                let channel_view = bake_map.is_some() || solo.is_some();
                for blended in [false, true] {
                    let pipeline = match (blended, samples > 1) {
                        (false, true) => &self.pipeline,
                        (false, false) => &self.pipeline_no_aa,
                        (true, true) => &self.pipeline_blend[0],
                        (true, false) => &self.pipeline_blend[1],
                    };
                    pass.set_pipeline(pipeline);
                    for r in &mesh.ranges {
                        if self.hidden_slots.contains(&r.material_slot) { continue; }
                        let Some(m) = self.materials.get(&r.material_slot) else { continue };
                        if (m.blends() && !channel_view) != blended { continue; }
                        let Some(bg) = (match bake_map {
                            Some(k) => m.bake_bind_groups.get(&k),
                            None => solo_groups.get(&r.material_slot).or(baked_normal_groups.get(&r.material_slot)).or(m.bind_group.as_ref()),
                        }) else { continue };
                        pass.set_bind_group(1, bg, &[]);
                        pass.draw_indexed(r.first_index..r.first_index + r.index_count, 0, 0..1);
                    }
                }
            }
            if params.grid.is_some() {
                // The mesh depth rejects guides behind it; guides never modify depth or textures.
                self.grid.draw(&mut pass, samples > 1);
            }
            if params.symmetry_guides && let Some(guides) = &self.symmetry { guides.draw(&mut pass, samples > 1); }
            if params.wireframe && let Some(wire) = &self.wireframe { wire.draw(&mut pass, samples > 1); }
        }
        // Glow and glare only exist where the model has an Emissive channel to draw.
        let has_emission = self.materials.values().any(|m| m.channels.contains_key(&ChannelKind::Emissive));
        let emission_on = has_emission && params.post.wants_emission() && params.bake_map.is_none();
        if (params.post.active() || emission_on) && !params.bake_overlay {
            let draw = self.mesh.as_ref().filter(|_| emission_on).map(|mesh| post::EmissionDraw { pipeline: &self.pipeline_emissive, mesh, materials: &self.materials, hidden: &self.hidden_slots });
            self.post.run(device, queue, &mut encoder, target, &self.globals_bg, self.mesh.as_ref(), draw.as_ref(), params.proj, &params.post);
        }
        target.encode_display(&mut encoder);
        queue.submit([encoder.finish()]);
        target
    }

    pub fn target(&self) -> Option<&RenderTarget> {
        self.target.as_ref()
    }

    /// Rendering mode: progressively path trace the model (see [`pathtrace`]) into a
    /// single-sample target of `size`. `Err` carries a message for the user when this adapter
    /// or model cannot be path traced; the raster [`Self::render`] is unaffected either way.
    pub fn render_path_traced(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        params: &ViewParams,
        scene: &PathTraceScene,
        settings: &PathTraceSettings,
    ) -> Result<(&RenderTarget, PathTraceStatus), String> {
        self.path_support.get_or_insert_with(|| pathtrace::check_support(device)).clone()?;
        let limits = device.limits();
        let max_buffer = (limits.max_storage_buffer_binding_size as u64).min(limits.max_buffer_size);
        let size = pathtrace::fit_trace_size(fit_target_size(size, limits.max_texture_dimension_2d), max_buffer);
        if self.path_target.as_ref().is_none_or(|t| t.size != size) {
            self.path_target = Some(RenderTarget::new(device, size, 1));
        }
        if self.path_tracer.is_none() {
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let pt = pathtrace::PathTracer::new(device);
            if let Some(error) = pollster::block_on(scope.pop()) {
                let message = format!("Path tracing is unavailable on this GPU: {error}");
                self.path_support = Some(Err(message.clone()));
                return Err(message);
            }
            self.path_tracer = Some(pt);
        }
        let hdri = params.environment.use_hdri && self.env_levels > 1;
        let target = self.path_target.as_ref().expect("created above");
        let pt = self.path_tracer.as_mut().expect("created above");
        // Any GPU error (a buffer the adapter cannot allocate, a lost resource) ends the mode
        // with a message instead of the default fatal error handler.
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let result = pt.render(device, queue, target, params, scene, settings, &self.materials, &self.defaults, (&self.env_texture, &self.env_sampler, hdri));
        let errors = [pollster::block_on(validation.pop()), pollster::block_on(memory.pop())];
        if let Some(error) = errors.into_iter().flatten().next() {
            self.path_tracer = None;
            return Err(format!("Path tracing stopped: {error}"));
        }
        Ok((target, result?))
    }

    /// The Rendering mode's last image (after [`Self::render_path_traced`] succeeded).
    pub fn path_target(&self) -> Option<&RenderTarget> {
        self.path_target.as_ref()
    }

    /// Linear mean of the path-traced samples so far (diagnostics and tests).
    pub fn path_traced_linear(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<[f32; 4]>> {
        self.path_tracer.as_ref()?.read_mean(device, queue)
    }

    /// Linear denoised path-traced image of the last frame that denoised.
    pub fn path_traced_denoised(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<[f32; 4]>> {
        self.path_tracer.as_ref()?.read_denoised(device, queue)
    }

    /// Drop the path tracer's GPU memory (Rendering mode switched off).
    pub fn release_path_tracer(&mut self) {
        self.path_tracer = None;
        self.path_target = None;
    }
}

/// Stored components of a composite channel image (mirrors `subcraft_compositor`).
/// Bindings 9 (Opacity texture) and 10 (`MaterialParams`) of a material bind group.
fn alpha_entries<'a>(opacity: &'a wgpu::TextureView, params: &'a wgpu::Buffer) -> [wgpu::BindGroupEntry<'a>; 2] {
    [
        wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(opacity) },
        wgpu::BindGroupEntry { binding: 10, resource: params.as_entire_binding() },
    ]
}

pub fn composite_comps(kind: ChannelKind) -> usize {
    if kind.components() == 3 { 4 } else { 1 }
}

/// A render-target size the device can allocate: at least 1 px, at most `max_dimension` per axis.
/// A window spanning several monitors (or an 8K display) can ask for more than the 8192 px
/// that the default device limits allow; the image is then rendered smaller and stretched.
pub fn fit_target_size(size: [u32; 2], max_dimension: u32) -> [u32; 2] {
    size.map(|s| s.clamp(1, max_dimension.max(1)))
}

#[cfg(test)]
mod size_tests {
    use super::fit_target_size;
    #[test]
    fn target_size_is_clamped_to_the_device_limit_and_never_zero() {
        assert_eq!(fit_target_size([10147, 1009], 8192), [8192, 1009]);
        assert_eq!(fit_target_size([0, 5], 8192), [1, 5]);
        assert_eq!(fit_target_size([100, 100], 8192), [100, 100]);
        assert_eq!(fit_target_size([u32::MAX, 3], 0), [1, 1]);
    }
}
