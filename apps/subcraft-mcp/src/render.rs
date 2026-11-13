//! Pictures for the agent: shaded 3D previews through the app's own viewport renderer on a
//! window-less GPU device, and flat 2D images of a texture set's channels or baked maps (CPU).

use glam::Vec3;
use subcraft_app::Session;
use subcraft_app::assets::Library;
use subcraft_core::{AssetId, ChannelKind, MeshMapKind};
use subcraft_image::TiledImage;
use subcraft_render::wgpu_api as wgpu;
use subcraft_render::{SHADED_CHANNELS, ViewMode, ViewportRenderer};
use subcraft_ui::orbit::{CameraView, OrbitCamera};

pub struct Gpu {
    pub adapter: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: ViewportRenderer,
    uploaded_env: Option<AssetId>,
}

impl Gpu {
    pub fn new() -> Result<Self, String> {
        let (adapter, device, queue) = subcraft_render::headless::device().ok_or("no graphics adapter is available for rendering (install a Vulkan driver, e.g. mesa-vulkan-drivers)")?;
        let info = adapter.get_info();
        let renderer = ViewportRenderer::new(&device, &queue);
        Ok(Self { adapter: format!("{} ({:?})", info.name, info.backend), device, queue, renderer, uploaded_env: None })
    }

    pub fn max_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }
}

pub struct Shot {
    pub camera: CameraView,
    pub size: [u32; 2],
    pub view_mode: ViewMode,
    pub environment: usize,
    pub environment_rotation_deg: f32,
    pub environment_intensity: f32,
    pub exposure: f32,
    pub background: Option<[f32; 3]>,
    pub wireframe: bool,
}

/// The orbit camera a shot (and a 3D stroke with the same `camera` / `size`) uses.
pub fn orbit(session: &Session, view: &CameraView) -> OrbitCamera {
    OrbitCamera::framing_with(&session.scene().mesh.bounds, Some(view))
}

pub fn default_view() -> CameraView {
    CameraView { yaw_deg: 35.0, pitch_deg: 22.0, zoom: 1.0, offset: Vec3::ZERO }
}

/// Render the model as the 3D viewport shows it. Returns sRGB RGBA8 pixels.
pub fn shaded(gpu: &mut Gpu, session: &mut Session, library: &Library, shot: &Shot) -> Result<Vec<u8>, String> {
    let limit = gpu.max_size();
    if shot.size.iter().any(|&s| s == 0 || s > limit) {
        return Err(format!("size must be 1..{limit} pixels per side"));
    }
    session.update_composites();
    let Gpu { device, queue, renderer, uploaded_env, .. } = gpu;
    renderer.set_mesh(device, &session.viewport_scene().mesh);
    if shot.wireframe {
        let mesh = &session.viewport_scene().mesh;
        let mut lines = Vec::with_capacity(mesh.indices.len());
        for t in mesh.indices.as_chunks::<3>().0 {
            let p = |i: u32| mesh.positions[i as usize];
            lines.push([p(t[0]), p(t[1])]);
            lines.push([p(t[1]), p(t[2])]);
            lines.push([p(t[2]), p(t[0])]);
        }
        renderer.set_wireframe(device, &lines, [0.05, 0.05, 0.05], 0.6);
    }
    for ts in &session.document().texture_sets {
        renderer.set_albedo_filter(ts.material_slot, ts.albedo_filter);
    }
    renderer.set_hidden_slots(session.document().texture_sets.iter().filter(|ts| session.is_set_hidden(ts.id)).map(|ts| ts.material_slot));
    for ts in &session.document().texture_sets {
        if let Some(c) = session.composite(ts.id) {
            for k in SHADED_CHANNELS {
                if let Some(img) = c.channel(k) {
                    renderer.upload_channel(device, queue, ts.material_slot, k, img, None);
                }
            }
        }
        if let Some(maps) = session.mesh_maps(ts.id) {
            for (kind, img) in maps.iter() {
                renderer.upload_mesh_map(device, queue, ts.material_slot, kind, img);
                if kind == MeshMapKind::AmbientOcclusion && !ts.has_channel(ChannelKind::AmbientOcclusion) {
                    renderer.upload_channel(device, queue, ts.material_slot, ChannelKind::AmbientOcclusion, img, None);
                }
            }
        }
    }
    let camera = orbit(session, &shot.camera);
    let mut params = camera.view_params(shot.size);
    params.antialias = true;
    params.view_mode = shot.view_mode;
    params.wireframe = shot.wireframe;
    let prefs = subcraft_ui::viewport_bar::ViewportPrefs::default();
    params.exposure = shot.exposure;
    params.post = prefs.post_params(session.scene().mesh.bounds.diagonal());
    subcraft_ui::panels::shelf::apply_environment(library, shot.environment, shot.environment_rotation_deg, shot.environment_intensity, &mut params, renderer, uploaded_env, device, queue);
    if let Some(bg) = shot.background {
        params.background = bg;
    }
    let mut pixels = renderer.render(device, queue, shot.size, &params).read_rgba8(device, queue);
    for a in pixels.iter_mut().skip(3).step_by(4) {
        *a = 255;
    }
    if shot.wireframe {
        renderer.set_wireframe(device, &[], [0.0; 3], 0.0);
    }
    Ok(pixels)
}

/// A flat image of a channel or baked map (UV space, top-left origin), at most `max` pixels
/// per side. Color channels are shown in sRGB; data channels as their raw values.
pub fn flat(img: &TiledImage, kind: FlatKind, max: u32) -> (Vec<u8>, [u32; 2]) {
    let (w, h) = (img.width(), img.height());
    let scale = (w.max(h) as f32 / max.max(1) as f32).max(1.0);
    let (ow, oh) = (((w as f32 / scale).round() as u32).max(1), ((h as f32 / scale).round() as u32).max(1));
    let comps = img.comps();
    let mut out = Vec::with_capacity((ow * oh * 4) as usize);
    for y in 0..oh {
        for x in 0..ow {
            let sx = (((x as f32 + 0.5) * scale) as u32).min(w - 1);
            let sy = (((y as f32 + 0.5) * scale) as u32).min(h - 1);
            let p = img.get(sx, sy);
            let rgb = match (kind, comps) {
                (FlatKind::Color, _) => subcraft_core::color::linear3_to_srgb([p[0], p[1], p[2]]),
                (FlatKind::Signed, _) => { let v = p[0] * 0.5 + 0.5; [v, v, v] }
                (_, 1 | 2) => [p[0]; 3],
                _ => [p[0], p[1], p[2]],
            };
            let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            out.extend_from_slice(&[b(rgb[0]), b(rgb[1]), b(rgb[2]), 255]);
        }
    }
    (out, [ow, oh])
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FlatKind { Color, Data, Signed }

impl FlatKind {
    pub fn for_channel(k: ChannelKind) -> Self {
        if k.is_color() { Self::Color } else if k.is_signed() { Self::Signed } else { Self::Data }
    }
}

pub fn png(pixels: &[u8], size: [u32; 2]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(pixels, size[0], size[1], image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("PNG encoding failed: {e}"))?;
    Ok(out)
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foobar"), "Zm9vYmFy");
    }
}
