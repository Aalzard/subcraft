//! UI-side state shared by all panels (the document itself lives in the `Session`).

use std::collections::HashMap;

use glam::Vec2;
use subcraft_app::assets::{UserAssetKind, UserLibrary};
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
    Eyedropper,
    Pen,
    /// Paints what lies at the clone source (Ctrl+click sets it).
    Clone,
    /// Drags the paint under the brush along the stroke.
    Smudge,
}

/// Viewport acknowledgement of the brush chosen on the Shelf. It holds, then fades out.
#[derive(Clone, Debug, PartialEq)]
pub struct BrushNotice {
    pub name: String,
    /// Tip in use, for the small thumbnail.
    pub tip: String,
    pub since: f64,
}

impl BrushNotice {
    pub const HOLD: f64 = 1.4;
    pub const FADE: f64 = 0.6;

    /// Opacity at `now`: 1 while holding, then fading to 0; `None` once gone.
    pub fn opacity(&self, now: f64) -> Option<f32> {
        let age = (now - self.since).max(0.0);
        if age >= Self::HOLD + Self::FADE { return None; }
        Some(if age <= Self::HOLD { 1.0 } else { (1.0 - (age - Self::HOLD) / Self::FADE) as f32 })
    }
}

pub struct UvView {
    pub zoom: f32,
    pub offset: egui::Vec2,
    pub source: PreviewSource,
    pub texture: Option<egui::TextureId>,
}

/// Compositing time spent per frame before the remaining dirty tiles wait for the next frame.
pub const COMPOSITE_FRAME_BUDGET: std::time::Duration = std::time::Duration::from_millis(10);

pub struct UiState {
    /// End of this frame's compositing time (shared by every `sync_gpu` call in the frame).
    pub composite_deadline: Option<web_time::Instant>,
    pub layer_previews: crate::layer_previews::PreviewUi,
    pub graphics: crate::graphics::GraphicsUi,
    pub asset_preview: crate::asset_preview::AssetPreviewUi,
    pub welcome: crate::welcome::WelcomeUi,

    pub updater: Option<subcraft_app::updater::Updater>,
    /// The startup update check is running or found a release: show the callout at the update
    /// button until the user answers it.
    pub update_notice: bool,
    /// Where the update button was drawn last frame (the callout points at it).
    pub update_button: Option<egui::Rect>,
    pub parts_kind: subcraft_core::PartKind,
    pub context_part: Option<subcraft_app::SurfaceTarget>,
    pub uploaded_view_revision: u64,
    uploaded_composite_generation: u64,
    pub session: Session,
    pub camera: OrbitCamera,
    pub render_state: Option<egui_wgpu::RenderState>,
    pub renderer: Option<ViewportRenderer>,
    pub viewport_texture: Option<egui::TextureId>,
    /// GPU copy of the mesh/materials is stale (new session).
    pub gpu_dirty: bool,
    pub tool: Tool,
    pub pen: subcraft_app::pen::PenEditor,
    pub pen_after: Option<crate::pen::After>,
    pub fill_mode: PolygonFillMode,
    /// Channel whose blend mode/opacity the layer stack shows.
    pub active_channel: ChannelKind,
    pub status: Option<(String, f64)>,
    /// Brief acknowledgement in the 3D view's top-left corner after a Shelf brush click.
    pub brush_notice: Option<BrushNotice>,
    /// Shelf double-click: bring the Brush settings tab forward on the next frame.
    pub open_brush_editor: bool,
    /// Scripted capture only (`--select-brush`): Shelf brush to select before the screenshot.
    pub capture_select_brush: Option<String>,
    /// Scripted Clone/Smudge demonstration for native captures (`--capture-tool`).
    pub capture_tool: Option<String>,
    /// "Render quality" window open, its export settings, and a pending export request.
    pub render_window: bool,
    /// Scripted export waiting for the first finished frame.
    pub pending_render_job: Option<crate::render_export::Job>,
    pub render_export: crate::render_export::ExportSettings,
    pub render_request: Option<crate::render_export::RenderRequest>,
    /// Pixel size of the 3D view at its last draw (export keeps the aspect ratio).
    pub last_view_size: [u32; 2],
    pub uv: UvView,
    pub painting: bool,
    /// Clicks in the 3D view show / hide the selected layer on mesh parts (Properties → Parts).
    pub picking_parts: bool,
    pub last_stroke_point: Option<Vec2>,
    pub uv_edges: HashMap<u32, Vec<[Vec2; 2]>>,
    pub rename: Option<(subcraft_core::LayerId, String)>,
    pub show_shortcuts: bool,
    pub frame_time_ms: f32,
    /// Viewport height-to-bump strength.
    pub height_scale: f32,
    pub export: crate::dialogs::ExportUi,
    pub new_project: crate::new_project::NewProjectUi,
    /// Action waiting for the unsaved-changes prompt.
    pub confirm: Option<crate::dialogs::Pending>,
    /// The user already agreed to close without saving.
    pub allow_close: bool,
    pub last_title: String,
    pub bake: crate::dialogs::BakeUi,
    pub symmetry_uploaded: Option<(usize, [bool; 3])>,
    pub uv_wireframe: bool,
    pub library: subcraft_app::assets::Library,
    pub shelf: crate::panels::shelf::ShelfUi,
    pub env: crate::panels::shelf::EnvUi,
    /// Mesh maps last uploaded to the GPU (to re-upload only when a bake replaced them).
    pub uploaded_maps: HashMap<(subcraft_core::TextureSetId, subcraft_core::MeshMapKind), subcraft_image::TiledImage>,
    /// Channel lists of the texture sets last uploaded. A change (an import adding Ambient
    /// Occlusion, or undoing it) re-uploads everything so no removed channel stays on the GPU.
    pub uploaded_channels: Vec<(subcraft_core::TextureSetId, Vec<ChannelKind>)>,
    /// Thumbnails of imported images (resources are immutable, so entries never go stale).
    pub image_thumbs: HashMap<subcraft_core::ResourceId, egui::TextureHandle>,
    /// The user's asset library folder (None if no Documents folder could be found).
    pub user_library: Option<UserLibrary>,
    /// Library HDRIs still being prefiltered on a background thread.
    pub env_loader: Option<std::sync::mpsc::Receiver<Result<subcraft_app::assets::Environment, String>>>,
    /// Brush presets from the user library.
    pub brush_presets: Vec<subcraft_app::brush_preset::BrushPreset>,
    /// Name typed for the next "Save preset".
    pub preset_name: String,
    /// Overlay texture of the current stencil image (keyed by its bitmap pointer).
    pub stencil_tex: Option<(usize, egui::TextureHandle)>,
    /// Image textures shown in Shelf → Textures (the library's `textures/` folder).
    pub textures: crate::panels::textures::TextureLibrary,
    /// Substance archives in Shelf → Materials (`Library/sbsar/`).
    pub sbsar_materials: crate::panels::sbsar_materials::SbsarMaterialLibrary,
    /// Output size when applying an SBSAR to the project (power of two, up to 8192).
    pub sbsar_bake_size: u32,
    /// Autosave, crash recovery and preferences.
    pub recovery: crate::recovery::RecoveryUi,
    /// Help → Report a bug and crash reports of earlier runs.
    pub bug_report: crate::bug_report::BugReportUi,
}

impl UiState {
    pub fn new(mut session: Session, render_state: Option<egui_wgpu::RenderState>) -> Self {
        let tool = if session.selected_layer().is_some_and(|l| l.kind() == subcraft_doc::LayerKind::Path) { Tool::Pen } else { Tool::Brush };
        // A brush color that stands out on the default gray material.
        session.brush.material.set(ChannelKind::BaseColor, subcraft_core::ChannelValue::rgb(0.85, 0.32, 0.06));
        let renderer = render_state.as_ref().map(|rs| ViewportRenderer::new(&rs.device, &rs.queue));
        let mut graphics = crate::graphics::GraphicsUi::default();
        if let Some(rs) = &render_state { graphics.record_active(rs.adapter.get_info().backend); }
        Self {
            composite_deadline: None,
            layer_previews: Default::default(),
            graphics,
            asset_preview: Default::default(),
            welcome: Default::default(),

            updater: subcraft_app::updater::Updater::new().ok(),
            update_notice: false,
            update_button: None,
            parts_kind: subcraft_core::PartKind::Connected,
            context_part: None,
            uploaded_view_revision: u64::MAX,
            uploaded_composite_generation: 0,
            camera: OrbitCamera::framing(&session.scene().mesh.bounds),
            session,
            render_state,
            renderer,
            viewport_texture: None,
            gpu_dirty: true,
            tool,
            pen: Default::default(),
            pen_after: None,
            fill_mode: PolygonFillMode::UvIsland,
            active_channel: ChannelKind::BaseColor,
            status: None,
            brush_notice: None,
            open_brush_editor: false,
            capture_select_brush: None,
            capture_tool: None,
            render_window: false,
            pending_render_job: None,
            render_export: Default::default(),
            new_project: Default::default(),
            render_request: None,
            last_view_size: [1280, 720],
            uv: UvView { zoom: 1.0, offset: egui::Vec2::ZERO, source: PreviewSource::Channel(ChannelKind::BaseColor), texture: None },
            painting: false,
            picking_parts: false,
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
            symmetry_uploaded: None,
            uv_wireframe: true,
            library: subcraft_app::assets::Library::builtin(),
            shelf: Default::default(),
            env: Default::default(),
            uploaded_maps: HashMap::new(),
            uploaded_channels: Vec::new(),
            image_thumbs: HashMap::new(),
            user_library: None,
            env_loader: None,
            brush_presets: Vec::new(),
            preset_name: String::new(),
            stencil_tex: None,
            textures: Default::default(),
            sbsar_materials: Default::default(),
            sbsar_bake_size: subcraft_app::DEFAULT_BAKE_SIZE,
            recovery: crate::recovery::RecoveryUi::disabled(),
            bug_report: Default::default(),
        }
    }

    /// Add the library's alphas now and start loading its HDRIs in the background.
    pub fn with_user_library(mut self, lib: Option<UserLibrary>) -> Self {
        let Some(lib) = lib else { return self };
        let (alphas, errors) = lib.load_alphas();
        for e in errors {
            log::warn!("library alpha skipped: {e}");
        }
        self.library.alphas.extend(alphas);
        let (materials, errors) = subcraft_app::library::load_smart_materials(&lib);
        for e in errors {
            log::warn!("library smart material skipped: {e}");
        }
        self.library.smart_materials.extend(materials);
        let (masks, errors) = subcraft_app::library::load_smart_masks(&lib);
        for e in errors {
            log::warn!("library smart mask skipped: {e}");
        }
        self.library.smart_masks.extend(masks);
        let (presets, errors) = subcraft_app::brush_preset::load_brush_presets(&lib);
        for e in errors {
            log::warn!("library brush preset skipped: {e}");
        }
        self.brush_presets = presets;
        self.textures = crate::panels::textures::TextureLibrary::load(&lib);
        self.sbsar_materials = crate::panels::sbsar_materials::SbsarMaterialLibrary::load(&lib);
        if !self.textures.items.is_empty() {
            // Open the shelf on the user's own textures once there are some.
            self.shelf.tab = crate::panels::shelf::ShelfTab::Textures;
        } else if !self.sbsar_materials.items.is_empty() {
            self.shelf.tab = crate::panels::shelf::ShelfTab::Materials;
        }
        let files = lib.list(UserAssetKind::Environment);
        if !files.is_empty() {
            let (tx, rx) = std::sync::mpsc::channel();
            subcraft_core::task::spawn(move || {
                for f in files {
                    if tx.send(subcraft_app::assets::user_library::load_environment(&f)).is_err() {
                        break;
                    }
                }
            });
            self.env_loader = Some(rx);
        }
        log::info!("asset library: {}", lib.root().display());
        self.user_library = Some(lib);
        self
    }

    /// Remember image files the user imported: copied to the library's textures (when there is
    /// a library) and listed in Shelf → Textures.
    pub fn remember_textures(&mut self, paths: &[std::path::PathBuf]) {
        let lib = self.user_library.clone();
        for e in self.textures.add_files(lib.as_ref(), paths) {
            log::warn!("texture not added to the library: {e}");
        }
    }

    /// Copy `.sbsar` files into the library and list them under Shelf → Materials.
    pub fn remember_sbsar(&mut self, paths: &[std::path::PathBuf]) {
        let lib = self.user_library.clone();
        for e in self.sbsar_materials.add_files(lib.as_ref(), paths) {
            log::warn!("SBSAR not added to the library: {e}");
        }
        if !paths.is_empty() {
            self.shelf.tab = crate::panels::shelf::ShelfTab::Materials;
        }
    }

    /// Pick up HDRIs finished by the background loader (call once per frame).
    pub fn poll_library(&mut self) {
        let Some(rx) = &self.env_loader else { return };
        loop {
            match rx.try_recv() {
                Ok(Ok(env)) => self.library.environments.push(env),
                Ok(Err(e)) => log::warn!("library environment skipped: {e}"),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.env_loader = None;
                    break;
                }
            }
        }
    }

    /// Replace the session (new project / opened mesh).
    pub fn set_session(&mut self, mut session: Session) {
        self.layer_previews = Default::default();
        self.asset_preview.cancel_apply();
        self.pen = Default::default();
        self.pen_after = None;
        if session.selected_layer().is_some_and(|l| l.kind() == subcraft_doc::LayerKind::Path) { self.tool = Tool::Pen; }
        session.brush.material = self.session.brush.material.clone();
        self.camera = OrbitCamera::framing(&session.scene().mesh.bounds);
        self.session = session;
        self.welcome.open = false;
        self.context_part = None;
        self.uploaded_view_revision = u64::MAX;
        self.gpu_dirty = true;
        self.uv_edges.clear();
        self.uploaded_maps.clear();
        self.painting = false;
        self.last_stroke_point = None;
        self.recovery.autosaver.session_replaced();
    }

    /// One Shelf click makes the brush usable at once: the app applies it, and Fill, Eyedropper
    /// and Pen switch to the Brush (the Eraser keeps erasing with the new tip). The layer, mask
    /// or Paint-effect destination is untouched. Only a brush that actually became active is
    /// acknowledged in the viewport; a failed preset reports why in the status bar instead.
    pub fn select_shelf_brush(&mut self, brush: subcraft_app::brush_preset::ShelfBrush, now: f64) {
        match self.session.select_shelf_brush(brush, &self.library.alphas) {
            Ok(selected) => {
                if !matches!(self.tool, Tool::Brush | Tool::Eraser) { self.tool = Tool::Brush; }
                self.picking_parts = false;
                if let Some(warning) = &selected.warning { self.set_status(warning.clone(), now); }
                self.brush_notice = Some(BrushNotice { name: selected.name, tip: selected.tip, since: now });
            }
            Err(e) => self.set_status(e, now),
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>, now: f64) {
        let msg = msg.into();
        log::info!("status: {msg}");
        self.status = Some((msg, now));
    }

    /// Execute commands, reporting the first error in the status bar.
    pub fn run(&mut self, cmds: Vec<Command>, now: f64) {
        for c in cmds {
            if let Err(e) = self.session.execute(c) {
                self.set_status(e.to_string(), now);
            }
        }
    }

    /// Start a frame: the compositing budget for this frame's `sync_gpu` calls.
    pub fn begin_frame(&mut self) {
        self.composite_deadline = Some(web_time::Instant::now() + COMPOSITE_FRAME_BUDGET);
    }

    /// Re-evaluate composites (within the frame budget) and upload changed tiles to the GPU.
    pub fn sync_gpu(&mut self) {
        let (Some(rs), Some(renderer)) = (&self.render_state, &mut self.renderer) else {
            self.session.update_composites();
            return;
        };
        let view_changed = self.uploaded_view_revision != self.session.view_revision();
        let channels: Vec<_> = self.session.document().texture_sets.iter().map(|ts| (ts.id, ts.channel_kinds().collect())).collect();
        let channels_changed = self.uploaded_channels != channels;
        // Export/sampling may synchronously install the final composite and consume its tile
        // notifications between frames. Refresh it instead of leaving a partial GPU result.
        let external_composite = self.uploaded_composite_generation != self.session.composite_generation();
        let full = self.gpu_dirty || view_changed || channels_changed || external_composite;
        if full {
            renderer.set_mesh(&rs.device, &self.session.viewport_scene().mesh);
            self.uploaded_view_revision = self.session.view_revision();
            self.uploaded_channels = channels;
            self.gpu_dirty = false;
        }
        // Bounded per frame so editing never stalls the UI; the rest follows on the next frames.
        // Several panels sync in one frame: they share one budget (see `begin_frame`).
        let budget = self.composite_deadline.map_or(COMPOSITE_FRAME_BUDGET, |d| d.saturating_duration_since(web_time::Instant::now()));
        let updates = crate::edit_profile::time_tab("  sync: composite", || self.session.update_composites_within(Some(budget)));
        let upload_start = web_time::Instant::now();
        let session = &self.session;
        for ts in &session.document().texture_sets { renderer.set_albedo_filter(ts.material_slot, ts.albedo_filter); }
        let upload = |renderer: &mut ViewportRenderer, ts: subcraft_core::TextureSetId, slot: u32, changed: Option<&subcraft_app::ChangedTiles>| {
            let Some(c) = session.composite(ts) else { return };
            for k in SHADED_CHANNELS {
                let Some(img) = c.channel(k) else { continue };
                match changed {
                    None => renderer.upload_channel(&rs.device, &rs.queue, slot, k, img, None),
                    // Only tiles whose texels changed; unchanged channels upload nothing.
                    Some(changed) => renderer.upload_channel(&rs.device, &rs.queue, slot, k, img, Some(changed.get(&k).map_or(&[], Vec::as_slice))),
                }
            }
        };
        if full {
            for ts in &session.document().texture_sets {
                upload(renderer, ts.id, ts.material_slot, None);
            }
        } else {
            for u in &updates {
                upload(renderer, u.texture_set, u.material_slot, Some(&u.changed));
            }
        }
        crate::edit_profile::record("  sync: upload", upload_start.elapsed());
        self.uploaded_composite_generation = session.composite_generation();
        // Baked maps: re-upload when a bake replaced them. Baked AO also shades the viewport
        // when the texture set has no Ambient Occlusion channel of its own.
        for ts in &session.document().texture_sets {
            let Some(maps) = session.mesh_maps(ts.id) else { continue };
            for (kind, img) in maps.iter() {
                let key = (ts.id, kind);
                if !full && self.uploaded_maps.get(&key).is_some_and(|u| u.same_storage(img)) {
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
