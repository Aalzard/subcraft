//! UI-side state shared by all panels (the document itself lives in the `Session`).

use std::collections::HashMap;

use glam::Vec2;
use subcraft_app::{Command, Session};
use subcraft_core::ChannelKind;
use subcraft_paint::PolygonFillMode;
use subcraft_render::{PreviewSource, SHADED_CHANNELS, ViewportRenderer, egui_wgpu};

use crate::orbit::OrbitCamera;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Brush,
    Eraser,
    PolygonFill,
}

pub struct UvView {
    pub zoom: f32,
    pub offset: egui::Vec2,
    pub source: PreviewSource,
    pub texture: Option<egui::TextureId>,
}

pub struct UiState {
    pub session: Session,
    pub camera: OrbitCamera,
    pub render_state: Option<egui_wgpu::RenderState>,
    pub renderer: Option<ViewportRenderer>,
    pub viewport_texture: Option<egui::TextureId>,
    /// GPU copy of the mesh/materials is stale (new session).
    pub gpu_dirty: bool,
    pub tool: Tool,
    pub fill_mode: PolygonFillMode,
    /// Channel whose blend mode/opacity the layer stack shows.
    pub active_channel: ChannelKind,
    pub status: Option<(String, f64)>,
    pub uv: UvView,
    pub painting: bool,
    pub last_stroke_point: Option<Vec2>,
    pub uv_edges: HashMap<u32, Vec<[Vec2; 2]>>,
    pub rename: Option<(subcraft_core::LayerId, String)>,
    pub show_shortcuts: bool,
    pub frame_time_ms: f32,
    /// Viewport height-to-bump strength.
    pub height_scale: f32,
    pub export: crate::dialogs::ExportUi,
    /// Action waiting for the unsaved-changes prompt.
    pub confirm: Option<crate::dialogs::Pending>,
    /// The user already agreed to close without saving.
    pub allow_close: bool,
    pub last_title: String,
    pub bake: crate::dialogs::BakeUi,
    pub uv_wireframe: bool,
    pub library: subcraft_app::assets::Library,
    pub shelf: crate::panels::shelf::ShelfUi,
    pub env: crate::panels::shelf::EnvUi,
    /// Mesh maps last uploaded to the GPU (to re-upload only when a bake replaced them).
    pub uploaded_maps: HashMap<(subcraft_core::TextureSetId, subcraft_core::MeshMapKind), subcraft_image::TiledImage>,
    /// Thumbnails of imported images (resources are immutable, so entries never go stale).
    pub image_thumbs: HashMap<subcraft_core::ResourceId, egui::TextureHandle>,
}

impl UiState {
    pub fn new(mut session: Session, render_state: Option<egui_wgpu::RenderState>) -> Self {
        // A brush color that stands out on the default gray material.
        session.brush.material.set(ChannelKind::BaseColor, subcraft_core::ChannelValue::rgb(0.85, 0.32, 0.06));
        let renderer = render_state.as_ref().map(|rs| ViewportRenderer::new(&rs.device, &rs.queue));
        Self {
            camera: OrbitCamera::framing(&session.scene().mesh.bounds),
            session,
            render_state,
            renderer,
            viewport_texture: None,
            gpu_dirty: true,
            tool: Tool::Brush,
            fill_mode: PolygonFillMode::UvIsland,
            active_channel: ChannelKind::BaseColor,
            status: None,
            uv: UvView { zoom: 1.0, offset: egui::Vec2::ZERO, source: PreviewSource::Channel(ChannelKind::BaseColor), texture: None },
            painting: false,
            last_stroke_point: None,
            uv_edges: HashMap::new(),
            rename: None,
            show_shortcuts: false,
            frame_time_ms: 0.0,
            height_scale: 0.02,
            export: Default::default(),
            confirm: None,
            allow_close: false,
            last_title: String::new(),
            bake: Default::default(),
            uv_wireframe: true,
            library: subcraft_app::assets::Library::builtin(),
            shelf: Default::default(),
            env: Default::default(),
            uploaded_maps: HashMap::new(),
            image_thumbs: HashMap::new(),
        }
    }

    /// Replace the session (new project / opened mesh).
    pub fn set_session(&mut self, mut session: Session) {
        session.brush.material = self.session.brush.material.clone();
        self.camera = OrbitCamera::framing(&session.scene().mesh.bounds);
        self.session = session;
        self.gpu_dirty = true;
        self.uv_edges.clear();
        self.uploaded_maps.clear();
        self.painting = false;
        self.last_stroke_point = None;
    }

    pub fn set_status(&mut self, msg: impl Into<String>, now: f64) {
        self.status = Some((msg.into(), now));
    }

    /// Execute commands, reporting the first error in the status bar.
    pub fn run(&mut self, cmds: Vec<Command>, now: f64) {
        for c in cmds {
            if let Err(e) = self.session.execute(c) {
                self.set_status(e.to_string(), now);
            }
        }
    }

    /// Re-evaluate composites and upload changed tiles to the GPU.
    pub fn sync_gpu(&mut self) {
        let (Some(rs), Some(renderer)) = (&self.render_state, &mut self.renderer) else {
            self.session.update_composites();
            return;
        };
        let full = self.gpu_dirty;
        if full {
            renderer.set_mesh(&rs.device, &self.session.scene().mesh);
            self.gpu_dirty = false;
        }
        let updates = self.session.update_composites();
        let session = &self.session;
        let upload = |renderer: &mut ViewportRenderer, ts: subcraft_core::TextureSetId, slot: u32, tiles: Option<&[usize]>| {
            let Some(c) = session.composite(ts) else { return };
            for k in SHADED_CHANNELS {
                if let Some(img) = c.channel(k) {
                    renderer.upload_channel(&rs.device, &rs.queue, slot, k, img, tiles);
                }
            }
        };
        if full {
            for ts in &session.document().texture_sets {
                upload(renderer, ts.id, ts.material_slot, None);
            }
        } else {
            for u in &updates {
                upload(renderer, u.texture_set, u.material_slot, Some(&u.tiles));
            }
        }
        // Baked maps: re-upload when a bake replaced them. Baked AO also shades the viewport
        // when the texture set has no Ambient Occlusion channel of its own.
        for ts in &session.document().texture_sets {
            let Some(maps) = session.mesh_maps(ts.id) else { continue };
            for (kind, img) in maps.iter() {
                let key = (ts.id, kind);
                if self.uploaded_maps.get(&key).is_some_and(|u| u.same_storage(img)) {
                    continue;
                }
                renderer.upload_mesh_map(&rs.device, &rs.queue, ts.material_slot, kind, img);
                if kind == subcraft_core::MeshMapKind::AmbientOcclusion && !ts.has_channel(ChannelKind::AmbientOcclusion) {
                    renderer.upload_channel(&rs.device, &rs.queue, ts.material_slot, ChannelKind::AmbientOcclusion, img, None);
                }
                self.uploaded_maps.insert(key, img.clone());
            }
        }
    }
}
