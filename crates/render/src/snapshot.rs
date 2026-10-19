//! Offscreen renders of a whole textured model for tools (the MCP server, scripted previews):
//! its own headless device and [`ViewportRenderer`], every channel uploaded per render.
//!
//! Each render rebuilds the mesh buffers and channel textures from scratch, so the image never
//! depends on what an earlier render uploaded (another project's channels can't linger). Tool
//! renders are rare compared to viewport frames, so the extra upload is not worth caching.

use std::collections::BTreeMap;

use egui_wgpu::wgpu;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_image::TiledImage;
use subcraft_mesh::Mesh;

use crate::{HdriUpload, SHADED_CHANNELS, ViewParams, ViewportRenderer, headless};

/// One texture set's images: its composite channels and baked mesh maps (baked AO shades sets
/// without an Ambient Occlusion channel, like the viewport).
pub struct SetImages<'a> {
    pub material_slot: u32,
    pub channels: &'a BTreeMap<ChannelKind, TiledImage>,
    pub mesh_maps: Option<&'a BTreeMap<MeshMapKind, TiledImage>>,
}

pub struct Snapshotter {
    adapter: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: ViewportRenderer,
}

impl Snapshotter {
    /// On any available adapter (hardware or software); `Err` when the machine has none.
    pub fn new() -> Result<Self, String> {
        let (adapter, device, queue) = headless::device().ok_or("no GPU adapter available for rendering (install Vulkan, Metal or DirectX 12 drivers, or a software rasterizer such as lavapipe)")?;
        let info = adapter.get_info();
        let renderer = ViewportRenderer::new(&device, &queue);
        Ok(Self { adapter: format!("{} ({:?})", info.name, info.backend), device, queue, renderer })
    }

    /// Adapter name and backend, for diagnostics.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Environment HDRI for renders with `environment.use_hdri` (`None` = placeholder).
    pub fn set_hdri(&mut self, hdri: Option<HdriUpload>) {
        self.renderer.set_hdri(&self.device, &self.queue, hdri);
    }

    /// Render `mesh` textured with `sets` and return tightly packed sRGB RGBA8 rows.
    pub fn render(&mut self, mesh: &Mesh, sets: &[SetImages], size: [u32; 2], params: &ViewParams) -> Vec<u8> {
        render_scene(&mut self.renderer, &self.device, &self.queue, mesh, sets, size, params)
    }
}

/// Upload `mesh` and every channel of `sets` into `renderer` (a dedicated offscreen one: its
/// textures are replaced), render, and read the image back as sRGB RGBA8 rows.
pub fn render_scene(
    renderer: &mut ViewportRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mesh: &Mesh,
    sets: &[SetImages],
    size: [u32; 2],
    params: &ViewParams,
) -> Vec<u8> {
    renderer.set_mesh(device, mesh);
    for set in sets {
        for kind in SHADED_CHANNELS {
            if let Some(image) = set.channels.get(&kind) {
                renderer.upload_channel(device, queue, set.material_slot, kind, image, None);
            }
        }
        if !set.channels.contains_key(&ChannelKind::AmbientOcclusion)
            && let Some(ao) = set.mesh_maps.and_then(|m| m.get(&MeshMapKind::AmbientOcclusion))
        {
            renderer.upload_channel(device, queue, set.material_slot, ChannelKind::AmbientOcclusion, ao, None);
        }
    }
    let size = [size[0].max(1), size[1].max(1)];
    renderer.render(device, queue, size, params).read_rgba8(device, queue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Mat4, Vec3};
    use subcraft_mesh::primitives;

    fn params(size: u32) -> ViewParams {
        let view = Mat4::look_at_rh(Vec3::new(2.0, 1.5, 3.0), Vec3::ZERO, Vec3::Y);
        let proj = Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 50.0);
        let _ = size;
        ViewParams::new(view, proj)
    }

    #[test]
    fn renders_channel_colors_and_rebuilds_every_time() {
        let Ok(mut snap) = Snapshotter::new() else {
            eprintln!("no wgpu adapter: skipping");
            return;
        };
        let mesh = primitives::cube(2.0);
        let red = BTreeMap::from([(ChannelKind::BaseColor, TiledImage::new(64, 64, 4, [0.8, 0.02, 0.02, 1.0]))]);
        let size = 96;
        let img = snap.render(&mesh, &[SetImages { material_slot: 0, channels: &red, mesh_maps: None }], [size, size], &params(size));
        assert_eq!(img.len(), (size * size * 4) as usize);
        let center = &img[((size / 2 * size + size / 2) * 4) as usize..][..4];
        assert!(center[0] > center[1] + 40 && center[0] > center[2] + 40, "red cube at the center: {center:?}");
        // A later render without the channel shows the default gray, not the earlier red.
        let none = BTreeMap::new();
        let img = snap.render(&mesh, &[SetImages { material_slot: 0, channels: &none, mesh_maps: None }], [size, size], &params(size));
        let center = &img[((size / 2 * size + size / 2) * 4) as usize..][..4];
        assert!(center[0].abs_diff(center[1]) < 20, "neutral without a base color: {center:?}");
    }
}
