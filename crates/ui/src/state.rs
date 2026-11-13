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

impl Tool {
    /// The tool as stored in a tool preset (`None` for tools that take no brush preset).
    pub fn preset_tool(self) -> Option<subcraft_app::brush_preset::PresetTool> {
        use subcraft_app::brush_preset::PresetTool as P;
        match self {
            Tool::Brush => Some(P::Paint),
            Tool::Eraser => Some(P::Eraser),
            Tool::Smudge => Some(P::Smudge),
            Tool::Clone => Some(P::Clone),
            Tool::PolygonFill => Some(P::PolygonFill),
            Tool::Eyedropper | Tool::Pen => None,
        }
    }

    pub fn from_preset(tool: subcraft_app::brush_preset::PresetTool) -> Tool {
        use subcraft_app::brush_preset::PresetTool as P;
        match tool {
            P::Paint => Tool::Brush,
            P::Eraser => Tool::Eraser,
            P::Smudge => Tool::Smudge,
            P::Clone => Tool::Clone,
            P::PolygonFill => Tool::PolygonFill,
        }
    }
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
    /// A stroke started in the 2D view is in progress.
    pub painting: bool,
    /// UV of the last 2D stroke point (Shift+click draws a line from it).
    pub last_point: Option<Vec2>,
    /// Polygon fill drag started in the 2D view (screen points), as [`UiState::fill_box`].
    pub fill_box: Option<egui::Pos2>,
    /// Geometry mask box started in the 2D view while picking parts (screen point).
    pub parts_box: Option<egui::Pos2>,
    /// Frame this UV rectangle (centre, size) next time the 2D view is drawn (F on a path).
    pub frame: Option<(Vec2, Vec2)>,
}

/// Compositing time spent per frame before the remaining dirty tiles wait for the next frame.
pub const COMPOSITE_FRAME_BUDGET: std::time::Duration = std::time::Duration::from_millis(10);

pub struct UiState {
    /// End of this frame's compositing time (shared by every `sync_gpu` call in the frame).
    pub composite_deadline: Option<web_time::Instant>,
    /// Most tiles one texture set received in the last [`UiState::sync_gpu`] (a quick preview
    /// lists every stale tile; `--profile-edits` uses it to tell a visible update from a slice).
    pub last_sync_tiles: usize,
    /// A [`UiState::sync_gpu`] of this frame sent the renderer anything (mesh, textures,
    /// materials). Several panels sync in one frame: each adds to it, and the frame starts it over.
    pub gpu_changed: bool,
    /// Temporal anti-aliasing progress of the 3D view.
    pub taa: crate::viewport::TaaState,
    pub layer_previews: crate::layer_previews::PreviewUi,
    /// A layer's mask shown alone in the 3D view (Alt+click on its mask thumbnail).
    pub mask_view: Option<crate::mask_view::MaskView>,
    /// Layers waiting in the "Export flattened" window, and the file name pattern (kept for
    /// the next export in this session).
    pub flatten_export: Option<Vec<subcraft_core::LayerId>>,
    /// The open "Instantiate across texture sets" window.
    pub instantiate: Option<crate::panels::layers::InstantiateDialog>,
    pub flatten_pattern: String,
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
    /// Imported camera looked through (`None` = the Default camera), whether it is locked, the
    /// orbit camera as it was set from it, and the Default camera's view to come back to.
    pub active_camera: Option<usize>,
    pub camera_locked: bool,
    pub camera_applied: Option<OrbitCamera>,
    pub default_camera: Option<OrbitCamera>,
    /// Navigation drag in progress (mode + rebased origin; see `nav`).
    pub nav_drag: Option<crate::nav::NavDrag>,
    /// The point the 3D view orbits around: the surface under the cursor when the last orbit
    /// started over the model (issue #115). `None` orbits around the camera target.
    pub orbit_pivot: Option<glam::Vec3>,
    /// Unsnapped yaw / pitch of a snapped orbit drag (Alt+Shift+LMB).
    pub orbit_free: Option<(f32, f32)>,
    /// Panels hidden so the views fill the window (Tab).
    pub hide_ui: bool,
    /// A plain Tab press taken from this frame's input (see `app::take_tab_press`).
    pub tab_pressed: bool,
    /// Scripted view used whenever a model is framed (`--camera`).
    pub camera_view: Option<crate::orbit::CameraView>,
    pub render_state: Option<egui_wgpu::RenderState>,
    pub renderer: Option<ViewportRenderer>,
    pub viewport_texture: Option<egui::TextureId>,
    /// GPU copy of the mesh/materials is stale (new session).
    pub gpu_dirty: bool,
    pub tool: Tool,
    pub pen: subcraft_app::pen::PenEditor,
    pub text: crate::text_panel::TextUi,
    pub pen_after: Option<crate::pen::After>,
    pub fill_mode: PolygonFillMode,
    /// Polygon fill boxes in the 3D view pick only what the camera sees (no back faces, nothing
    /// hidden behind the model). Off: every triangle under the box, x-ray style.
    pub fill_visible_only: bool,
    /// Polygon fill drag in progress: where the primary button went down (screen points). A
    /// release close by is a click fill there, a longer drag a box fill.
    pub fill_box: Option<egui::Pos2>,
    /// The floating Properties popup a right-click in a view opened.
    pub view_props: Option<crate::view_props::ViewProps>,
    /// This frame's primary press closed that popup: the views don't paint or fill with it.
    pub view_props_swallow: bool,
    /// Channel whose blend mode/opacity the layer stack shows.
    pub active_channel: ChannelKind,
    pub status: Option<(String, f64)>,
    /// Problems and notes of the last smart-material import (shown in a window until closed).
    pub import_report: Vec<String>,
    /// Brief acknowledgement in the 3D view's top-left corner after a Shelf brush click.
    pub brush_notice: Option<BrushNotice>,
    /// Shelf double-click: bring the Brush settings tab forward on the next frame.
    pub open_brush_editor: bool,
    /// Scroll the brush settings back to the top (the selected brush's preview) next frame.
    pub brush_scroll_top: bool,
    /// The Shelf brush (preset or tip name) last chosen and the tip it set: highlights that
    /// Shelf tile while the brush still uses the tip.
    pub selected_brush: Option<(String, String)>,
    /// Properties shows the brush settings regardless of the selected layer (View menu, Shelf
    /// double-click) until the selection changes.
    pub brush_pinned: bool,
    /// No separate Brush tab is docked, so the Properties panel hosts the brush settings.
    pub properties_hosts_brush: bool,
    /// Scripted capture only (`--select-brush`): Shelf brush to select before the screenshot.
    pub capture_select_brush: Option<String>,
    /// Scripted Clone/Smudge demonstration for native captures (`--capture-tool`).
    pub capture_tool: Option<String>,
    /// Scripted capture: take the screenshot while a Shelf image is still loading (shows the
    /// thumbnail brush and the "Loading full resolution" row) instead of waiting for it.
    pub capture_during_load: bool,
    /// "Render quality" window open, its export settings, and a pending export request.
    pub render_window: bool,
    /// Display Settings panel: the group a jump button asked to scroll to.
    pub display_jump: Option<crate::panels::display_settings::Section>,
    /// A shader menu asked for the Shader Settings panel (opened by the app next frame).
    pub open_shader_settings: bool,
    /// Whether the session's saved display settings were taken over yet, and the values last
    /// seen in step with the project (see `panels::display_settings::sync_project`).
    pub display_loaded: bool,
    pub display_baseline: subcraft_doc::DisplaySettings,
    /// Scripted export waiting for the first finished frame.
    pub pending_render_job: Option<crate::render_export::Job>,
    pub render_export: crate::render_export::ExportSettings,
    pub render_request: Option<crate::render_export::RenderRequest>,
    /// Pixel size of the 3D view at its last draw (export keeps the aspect ratio).
    pub last_view_size: [u32; 2],
    /// Screen rectangle (points) of the 3D view at its last draw (scripted input).
    pub last_view_rect: egui::Rect,
    pub uv: UvView,
    pub painting: bool,
    /// Pen pressure of the current contact (1.0 for a mouse).
    pub pen_pressure: f32,
    /// Pen / touch contact to primary-button mapping applied to raw input.
    pub pen_input: crate::pen_input::PenInputFixer,
    /// Clicks in the 3D and 2D views show / hide the selected layer on mesh parts (its geometry
    /// mask, Properties → Parts); a drag boxes several (Ctrl+drag removes them).
    pub picking_parts: bool,
    /// Warp projection grid editing (mode, selected points, display settings).
    pub warp: crate::warp_view::WarpEdit,
    /// Geometry mask box started in a view (screen point), while picking parts.
    pub parts_box: Option<egui::Pos2>,
    /// Geometry mask copied from a layer row's menu.
    pub parts_clipboard: Option<subcraft_doc::PartMask>,
    /// Geometry mask overlay of the 2D view (excluded islands, hovered part).
    pub uv_parts: crate::panels::uv_view::PartsOverlay,
    /// Quick mask overlay of the 2D view: (quick mask revision, texture).
    pub quick_mask_tex: Option<(u64, egui::TextureHandle)>,
    /// Texture Set List > Settings: which description lines the rows show.
    pub set_descriptions: crate::panels::texture_sets::DescriptionDisplay,
    /// Depth of field: the next click on the model sets the focus distance.
    pub picking_focus: bool,
    /// Clicks in the 3D view add the ID map color under the cursor to this ID Color Mask effect.
    pub picking_id_color: Option<(subcraft_core::LayerId, subcraft_core::EffectId)>,
    /// ID color editor eyedropper: clicks in the 3D or 2D view pick an ID map color, select it in
    /// the editor and feed it to the selected layer's ID Color mask.
    pub picking_id_palette: bool,
    /// Color selected in the ID color editor (as shown in the ID map).
    pub id_palette_selected: Option<[f32; 3]>,
    /// The editor's palette, keyed by (texture set, maps generation, document revision).
    pub id_palette_cache: Option<((subcraft_core::TextureSetId, u64, u64), Vec<subcraft_app::id_color::IdPaletteEntry>)>,
    /// The color being typed / picked as the replacement of `id_palette_selected`.
    pub id_palette_new: [f32; 3],
    /// "Import mesh maps…" from Texture Set Settings.
    pub mesh_map_import: crate::mesh_map_import::MeshMapImportUi,
    pub last_stroke_point: Option<Vec2>,
    /// The view `last_stroke_point` was painted through. It is a screen position: once the camera
    /// moves it no longer lies on the stroke's end.
    pub last_stroke_camera: Option<subcraft_app::Camera>,
    pub uv_edges: HashMap<u32, Vec<[Vec2; 2]>>,
    /// UV units per object unit of each material slot (2D brush cursor size).
    pub uv_density: HashMap<u32, f32>,
    pub rename: Option<(subcraft_core::LayerId, String)>,
    pub show_shortcuts: bool,
    pub frame_time_ms: f32,
    /// Rendering mode: sampling paused, last frame's progress, and whether the path tracer
    /// was used last frame (its GPU memory is released when the mode turns off).
    pub pt_paused: bool,
    pub pt_status: Option<subcraft_render::PathTraceStatus>,
    pub pt_active: bool,
    /// `--path-trace N`: samples a scripted screenshot waits for.
    pub pt_capture_samples: Option<u32>,
    /// Viewport height-to-bump strength.
    pub height_scale: f32,
    pub export: crate::dialogs::ExportUi,
    pub new_project: crate::new_project::NewProjectUi,
    pub mesh_reload: crate::mesh_reload::MeshReloadUi,
    pub auto_uv: crate::auto_uv::AutoUvUi,
    /// Layers selected together (Ctrl / Shift click) in the Layers panel.
    pub multi: subcraft_app::multi_select::MultiSelect,
    /// Texture set the multi-selection belongs to (cleared when the selected set changes).
    multi_texture_set: Option<subcraft_core::TextureSetId>,
    /// Ctrl / Shift click extras around the session's selected effect (one stack at a time).
    pub effect_multi: subcraft_app::effect_stack::EffectSelection,
    /// In-app clipboard for layers and effects; outlives the session.
    pub clips: crate::clipboard::Clips,
    /// Action waiting for the unsaved-changes prompt.
    pub confirm: Option<crate::dialogs::Pending>,
    /// A model dropped on the window: preloaded into the New Project window once it opens.
    pub dropped_model: Option<std::path::PathBuf>,
    /// Key light rotation around the model (radians), set with Shift+MMB drag (Painter navigation).
    pub light_yaw: f32,
    /// Tilt of the key light (radians added to its elevation; below the horizon lights the model
    /// from underneath).
    pub light_pitch: f32,
    /// Which view the pointer was over last frame (F frames that one, or both when neither).
    pub hovered_3d: bool,
    pub hovered_2d: bool,
    /// Material slot to draw highlighted next frame (the texture set under the cursor with
    /// Ctrl+Alt, or hovered in the Texture Set List); taken by the 3D view each frame.
    pub highlight_slot: Option<u32>,
    /// The Shelf took this frame's dropped files (the window-wide drop handler skips them).
    pub drops_taken: bool,
    /// The user already agreed to close without saving.
    pub allow_close: bool,
    pub last_title: String,
    pub bake: crate::dialogs::BakeUi,
    /// A Painter project being opened in the background.
    pub spp_import: Option<crate::spp_import::SppImportUi>,
    /// The reconstruction notice after a Painter import finished: the file name.
    pub spp_notice: Option<String>,
    /// Running Edit in PhotoCraft exchange (ADR-0040).
    pub photocraft: Option<subcraft_app::photocraft::Exchange>,
    /// Key of the polygon wireframe currently uploaded to the renderer.
    pub wire_uploaded: Option<crate::view_extras::WireKey>,
    pub symmetry_uploaded: Option<crate::symmetry_view::GuideKey>,
    pub uv_wireframe: bool,
    pub library: subcraft_app::assets::Library,
    pub shelf: crate::panels::shelf::ShelfUi,
    pub env: crate::panels::shelf::EnvUi,
    /// Mesh maps last uploaded to the GPU (to re-upload only when a bake replaced them).
    pub uploaded_maps: HashMap<(subcraft_core::TextureSetId, subcraft_core::MeshMapKind), subcraft_image::TiledImage>,
    /// Channel lists of the texture sets last uploaded. A change (an import adding Ambient
    /// Occlusion, or undoing it) re-uploads everything so no removed channel stays on the GPU.
    pub uploaded_channels: Vec<(subcraft_core::TextureSetId, Vec<ChannelKind>)>,
    /// Channel shown alone that the shader does not otherwise read, as last uploaded.
    pub uploaded_solo: Option<ChannelKind>,
    /// Base Color tinted red where the quick mask protects the surface, while it is edited.
    pub quick_mask_tint: subcraft_app::quick_mask_tint::TintCache,
    /// While a material is dragged with Ctrl over the 3D view: the part it would land on, the
    /// camera the outline was computed for, and the part's visible border edges (object space).
    pub drop_outline: Option<(subcraft_app::PartKey, subcraft_app::Camera, Vec<[glam::Vec3; 2]>)>,
    /// Thumbnails of imported images (resources are immutable, so entries never go stale).
    pub image_thumbs: HashMap<subcraft_core::ResourceId, egui::TextureHandle>,
    /// The library new presets and imports go to (Settings > Libraries default; your_assets
    /// unless changed; None if no Documents folder could be found).
    pub user_library: Option<UserLibrary>,
    /// Every library on disk the Assets panel reads: your_assets first, then the extra ones.
    pub libraries: Vec<crate::libraries::NamedLibrary>,
    /// Files imported for the current session only (not copied to a library).
    pub session_files: std::collections::HashSet<std::path::PathBuf>,
    /// The Import resources window.
    pub import: crate::import_resources::ImportWindow,
    /// Name / folder typed in Settings > Libraries.
    pub libraries_form: crate::libraries::LibrariesUi,
    /// Library HDRIs still being prefiltered on a background thread.
    pub env_loader: Option<std::sync::mpsc::Receiver<Result<subcraft_app::assets::Environment, String>>>,
    /// Brush presets from the user library.
    pub brush_presets: Vec<subcraft_app::brush_preset::BrushPreset>,
    /// Favorite assets of the user library (shown first in the Shelf).
    pub favorites: subcraft_app::brush_preset::Favorites,
    /// Name typed for the next "Save preset".
    pub preset_name: String,
    /// Live preview of the normal-map brush panel.
    pub stamp_preview: crate::panels::stamp_brush::PreviewCache,
    /// Rendered preview of the active (non-stamp) brush tip.
    pub tip_preview: crate::panels::brush_header::PreviewCache,
    /// Overlay texture of the current stencil image (keyed by its bitmap pointer).
    pub stencil_tex: Option<(usize, egui::TextureHandle)>,
    /// How the stencil overlay is shown (display only; painting through it is unaffected).
    pub stencil_display: StencilDisplay,
    /// Image textures shown in Shelf → Textures (the library's `textures/` folder).
    pub textures: crate::panels::textures::TextureLibrary,
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
        if let Some(rs) = &render_state {
            // Probe optional capabilities before any pipeline is built, and keep wgpu errors from panicking.
            let caps = subcraft_render::caps::GpuCaps::from_adapter(&rs.adapter);
            subcraft_app::startup_probe::step(&format!("adapter caps: {}", caps.report().replace('\n', " | ")));
            for problem in [caps.msaa_problem(), caps.compute_problem()].into_iter().flatten() { log::warn!("{problem}"); subcraft_app::startup_probe::step(&problem); }
            // Lighter compute budgets on integrated and software adapters (issue #119): logged
            // so a report shows what was chosen.
            let budget = subcraft_render::pace::GpuBudget::for_device_type(&caps.device_type);
            let memory = subcraft_render::pace::MemoryBudget::for_adapter(&caps.device_type, &caps.backend);
            subcraft_render::caps::set_low_memory(memory.is_tight());
            let chosen = format!("GPU budget for {} ({}): {}; {}; 4x anti-aliasing {}", caps.name, caps.device_type, budget.describe(), memory.describe(),
                if subcraft_render::caps::safe_mode() { "off (safe mode)" } else if !memory.msaa { "off (low video memory)" } else if caps.msaa4 { "on" } else { "off (unsupported)" });
            log::info!("{chosen}");
            subcraft_app::startup_probe::step(&chosen);
            subcraft_render::caps::set(caps);
            subcraft_render::caps::install_error_handlers(&rs.device);
        }
        let renderer = render_state.as_ref().map(|rs| ViewportRenderer::new(&rs.device, &rs.queue));
        let mut graphics = crate::graphics::GraphicsUi::default();
        if let Some(rs) = &render_state { graphics.record_active(rs.adapter.get_info().backend); }
        subcraft_app::startup_probe::step(&format!("renderer created (adapter: {})", render_state.as_ref().map(|rs| rs.adapter.get_info().name).unwrap_or_else(|| "none".into())));
        let mut state = Self {
            composite_deadline: None,
            last_sync_tiles: 0,
            gpu_changed: false,
            taa: Default::default(),
            layer_previews: Default::default(),
            mask_view: None,
            flatten_export: None,
            instantiate: None,
            flatten_pattern: subcraft_app::flatten::DEFAULT_PATTERN.to_string(),
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
            active_camera: None,
            camera_locked: false,
            camera_applied: None,
            default_camera: None,
            nav_drag: None,
            orbit_pivot: None,
            orbit_free: None,
            hide_ui: false,
            tab_pressed: false,
            camera_view: None,
            session,
            render_state,
            renderer,
            viewport_texture: None,
            gpu_dirty: true,
            tool,
            pen: Default::default(),
            text: Default::default(),
            pen_after: None,
            fill_mode: PolygonFillMode::UvIsland,
            fill_visible_only: true,
            fill_box: None,
            view_props: None,
            view_props_swallow: false,
            active_channel: ChannelKind::BaseColor,
            status: None,
            import_report: Vec::new(),
            brush_notice: None,
            open_brush_editor: false,
            brush_scroll_top: false,
            selected_brush: None,
            brush_pinned: false,
            properties_hosts_brush: true,
            capture_select_brush: None,
            capture_tool: None,
            capture_during_load: false,
            render_window: false,
            display_jump: None,
            open_shader_settings: false,
            display_loaded: false,
            display_baseline: Default::default(),
            pending_render_job: None,
            render_export: Default::default(),
            new_project: Default::default(),
            mesh_reload: Default::default(),
            auto_uv: Default::default(),
            render_request: None,
            last_view_size: [1280, 720],
            last_view_rect: egui::Rect::NOTHING,
            uv: UvView { zoom: 1.0, offset: egui::Vec2::ZERO, source: PreviewSource::Channel(ChannelKind::BaseColor), texture: None, painting: false, last_point: None, fill_box: None, parts_box: None, frame: None },
            painting: false,
            picking_parts: false,
            warp: Default::default(),
            parts_box: None,
            parts_clipboard: None,
            uv_parts: Default::default(),
            quick_mask_tex: None,
            set_descriptions: Default::default(),
            picking_focus: false,
            picking_id_color: None,
            picking_id_palette: false,
            id_palette_selected: None,
            id_palette_cache: None,
            id_palette_new: [1.0, 1.0, 1.0],
            mesh_map_import: Default::default(),
            last_stroke_point: None,
            last_stroke_camera: None,
            pen_pressure: 1.0,
            pen_input: Default::default(),
            uv_edges: HashMap::new(),
            uv_density: HashMap::new(),
            rename: None,
            show_shortcuts: false,
            frame_time_ms: 0.0,
            pt_paused: false,
            pt_status: None,
            pt_active: false,
            pt_capture_samples: None,
            height_scale: 0.02,
            export: Default::default(),
            confirm: None,
            dropped_model: None,
            light_yaw: 0.0,
            light_pitch: 0.0,
            hovered_3d: false,
            hovered_2d: false,
            highlight_slot: None,
            drops_taken: false,
            multi: Default::default(),
            multi_texture_set: None,
            clips: Default::default(),
            effect_multi: Default::default(),
            allow_close: false,
            last_title: String::new(),
            bake: Default::default(),
            spp_import: None,
            spp_notice: None,
            photocraft: None,
            wire_uploaded: None,
            symmetry_uploaded: None,
            uv_wireframe: true,
            library: subcraft_app::assets::Library::builtin(),
            shelf: Default::default(),
            env: Default::default(),
            uploaded_maps: HashMap::new(),
            uploaded_channels: Vec::new(),
            uploaded_solo: None,
            quick_mask_tint: Default::default(),
            drop_outline: None,
            image_thumbs: HashMap::new(),
            user_library: None,
            libraries: Vec::new(),
            session_files: Default::default(),
            import: Default::default(),
            libraries_form: Default::default(),
            env_loader: None,
            brush_presets: builtin_brush_presets(),
            favorites: Default::default(),
            preset_name: String::new(),
            stamp_preview: Default::default(),
            tip_preview: Default::default(),
            stencil_tex: None,
            stencil_display: StencilDisplay::default(),
            textures: Default::default(),
            recovery: crate::recovery::RecoveryUi::disabled(),
            bug_report: Default::default(),
        };
        state.env = crate::panels::shelf::EnvUi::startup(&state.library);
        state
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
        self.brush_presets = builtin_brush_presets();
        self.brush_presets.extend(presets);
        self.favorites = subcraft_app::brush_preset::Favorites::load(&lib);
        // The shelf keeps opening on Smart Materials (Painter's default), even with user textures.
        self.textures = crate::panels::textures::TextureLibrary::load(&lib);
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
        self.libraries = vec![crate::libraries::NamedLibrary { id: crate::panels::shelf_filter::YOURS.to_string(), lib: lib.clone() }];
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

    /// Edit in PhotoCraft: write the selected texture set's layers as PSDs and open them.
    pub fn start_photocraft(&mut self, now: f64) {
        let dir = subcraft_app::photocraft::exchange_dir(self.session.selection().texture_set);
        match subcraft_app::photocraft::Exchange::start(&self.session, &dir, subcraft_app::photocraft::launch) {
            Ok(ex) => {
                self.set_status(format!("Editing {} in PhotoCraft: saves there come back here automatically", ex.set_name()), now);
                self.photocraft = Some(ex);
            }
            Err(e) => self.set_status(format!("Edit in PhotoCraft: {e}"), now),
        }
    }

    /// Stop bringing PhotoCraft saves back (PhotoCraft itself stays open).
    pub fn stop_photocraft(&mut self, now: f64) {
        if let Some(ex) = self.photocraft.take() {
            self.set_status(format!("Stopped editing {} in PhotoCraft", ex.set_name()), now);
        }
    }

    /// Apply settled PhotoCraft saves (call once per frame; checks the disk twice a second).
    pub fn poll_photocraft(&mut self, now: f64) {
        let Some(ex) = &mut self.photocraft else { return };
        for event in ex.poll(&mut self.session, now) {
            use subcraft_app::photocraft::PollEvent;
            match event {
                PollEvent::Applied(text) | PollEvent::Failed(text) => self.set_status(text, now),
                PollEvent::Ended(text) => {
                    self.photocraft = None;
                    self.set_status(text, now);
                    return;
                }
            }
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
        // A Painter import still running would hand its own session over when it finishes and
        // discard whatever was opened and edited in the meantime (`spp_import::poll` takes the
        // import out before handing over its result).
        self.spp_import = None;
        self.dropped_model = None;
        // The mask being shown belongs to the old project: left on, the new model was drawn all
        // black as "Mask of" a layer that does not exist, until the next edit.
        self.mask_view = None;
        self.multi.clear();
        self.layer_previews = Default::default();
        self.asset_preview.cancel_apply();
        self.pen = Default::default();
        self.pen_after = None;
        // Another project: its own cameras, starting from the Default camera.
        self.active_camera = None;
        self.camera_locked = false;
        self.camera_applied = None;
        self.default_camera = None;
        if session.selected_layer().is_some_and(|l| l.kind() == subcraft_doc::LayerKind::Path) { self.tool = Tool::Pen; }
        session.brush.material = self.session.brush.material.clone();
        self.camera = OrbitCamera::framing_with(&session.scene().mesh.bounds, self.camera_view.as_ref());
        self.session = session;
        self.display_loaded = false;
        self.welcome.open = false;
        self.context_part = None;
        self.uploaded_view_revision = u64::MAX;
        self.gpu_dirty = true;
        self.uv_edges.clear();
        self.uv_density.clear();
        self.uv.painting = false;
        self.uv.last_point = None;
        self.uploaded_maps.clear();
        self.painting = false;
        self.last_stroke_point = None;
        self.recovery.autosaver.session_replaced();
        // A pivot on the old model swung the next orbit around empty space.
        self.orbit_pivot = None;
    }

    /// The session's mesh was replaced in place (File > Reload mesh): drop everything derived
    /// from the old geometry and upload the new one. The camera stays put for comparison.
    pub fn mesh_reloaded(&mut self) {
        self.layer_previews = Default::default();
        self.asset_preview.cancel_apply();
        self.pen = Default::default();
        self.pen_after = None;
        self.context_part = None;
        self.uploaded_view_revision = u64::MAX;
        self.gpu_dirty = true;
        self.uv_edges.clear();
        self.uv_density.clear();
        self.uploaded_maps.clear();
        self.painting = false;
        self.last_stroke_point = None;
        self.orbit_pivot = None;
        // The reloaded file brings its own cameras: an index into the old list would show the new
        // camera's name over the old camera's view.
        self.active_camera = None;
    }

    /// One Shelf click makes the brush usable at once: the app applies it, and Fill, Eyedropper
    /// and Pen switch to the Brush (the Eraser keeps erasing with the new tip). The layer, mask
    /// or Paint-effect destination is untouched. Only a brush that actually became active is
    /// acknowledged in the viewport; a failed preset reports why in the status bar instead.
    pub fn select_shelf_brush(&mut self, brush: subcraft_app::brush_preset::ShelfBrush, now: f64) {
        match self.session.select_shelf_brush(brush, &self.library.alphas) {
            Ok(selected) => {
                use subcraft_app::brush_preset::PresetKind;
                match (selected.tool, selected.kind) {
                    // A tool preset brings its tool.
                    (Some(tool), _) => self.tool = Tool::from_preset(tool),
                    // A material preset serves whichever painting tool is in use.
                    (None, Some(PresetKind::Material)) if self.tool.preset_tool().is_some() => {}
                    _ => if !matches!(self.tool, Tool::Brush | Tool::Eraser) { self.tool = Tool::Brush; },
                }
                self.picking_parts = false;
                if let Some(warning) = &selected.warning { self.set_status(warning.clone(), now); }
                // A full image still loading for the previous brush choice is no longer wanted.
                if self.asset_preview.pending_brush().is_some() { self.asset_preview.cancel_apply(); }
                self.show_selected_brush(Some((selected.name.clone(), selected.tip.clone())));
                self.brush_notice = Some(BrushNotice { name: selected.name, tip: selected.tip, since: now });
            }
            Err(e) => self.set_status(e, now),
        }
    }

    /// A Shelf click chose a brush: bring the brush settings forward, scrolled to the top where
    /// the selected brush's name and preview are shown (like Substance Painter's brush
    /// properties). `preset` is the Shelf name and tip of a preset / tip; `None` for stamps and
    /// Starter pack images, which the brush state names itself.
    pub fn show_selected_brush(&mut self, preset: Option<(String, String)>) {
        self.open_brush_editor = true;
        self.brush_scroll_top = true;
        self.selected_brush = preset;
    }

    /// The Shelf tile of a preset / tip is the active brush (no stamp, tip unchanged since).
    pub fn is_selected_shelf_brush(&self, name: &str) -> bool {
        let b = &self.session.brush;
        if b.material.stamp.is_some() {
            return false;
        }
        match &self.selected_brush {
            Some((n, tip)) => n == name && *tip == b.alpha_name,
            None => b.alpha_name == name,
        }
    }

    /// A modal window covers the app ("Unsaved changes", "didn't close properly", the update
    /// restart, New Project): document shortcuts must not act behind it. Delete or Ctrl+Z used to
    /// change the document behind the unsaved-changes prompt, which then saved that state.
    pub fn modal_open(&self) -> bool {
        self.confirm.is_some()
            || !self.recovery.crashed.is_empty()
            || self.new_project.open
            || self.updater.as_ref().is_some_and(|u| matches!(u.phase, subcraft_app::updater::Phase::Preparing | subcraft_app::updater::Phase::Exiting))
    }

    /// The start of a Shift + click straight line in the 3D view: the end of the last stroke, as
    /// long as the view has not changed since. After orbiting, zooming or resizing, the remembered
    /// screen position lies over some other part of the model, and the line started there.
    pub fn line_start(&mut self, camera: &subcraft_app::Camera) -> Option<Vec2> {
        if self.last_stroke_camera.as_ref() != Some(camera) {
            self.last_stroke_point = None;
            self.last_stroke_camera = None;
        }
        self.last_stroke_point
    }

    /// Remember where a stroke ended, and through which view.
    pub fn set_line_start(&mut self, point: Vec2, camera: &subcraft_app::Camera) {
        self.last_stroke_point = Some(point);
        self.last_stroke_camera = Some(*camera);
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
        // The views set these while they are drawn (after the shortcuts read them). A view that
        // is no longer drawn (F3 / F2 layouts, a closed tab) kept its last value, and F framed
        // that hidden view instead of the visible one (QA 2026-11).
        self.hovered_3d = false;
        self.hovered_2d = false;
        self.gpu_changed = false;
        // The multi-selection holds layer ids; Delete / Duplicate act on them wherever they live,
        // so ids kept from another texture set would edit layers the user is no longer looking at.
        let ts = self.session.selection().texture_set;
        if self.multi_texture_set != Some(ts) {
            if self.multi_texture_set.is_some() { self.multi.clear(); }
            self.multi_texture_set = Some(ts);
        }
    }

    /// Re-evaluate composites (within the frame budget) and upload changed tiles to the GPU.
    pub fn sync_gpu(&mut self) {
        crate::mask_view::sync(self);
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
        self.last_sync_tiles = updates.iter().map(|u| u.tiles.len()).max().unwrap_or(0);
        // QA 2026-11: overwritten by every call, the 3D view's own (usually last, with nothing
        // left to upload) hid the frame's uploads from TAA, which kept a stale image.
        self.gpu_changed |= full || self.last_sync_tiles > 0;
        let upload_start = web_time::Instant::now();
        let session = &self.session;
        for ts in &session.document().texture_sets { renderer.set_albedo_filter(ts.material_slot, ts.albedo_filter); renderer.set_alpha(ts.material_slot, ts.alpha); renderer.set_shader(ts.material_slot, session.shader_params_of(ts.id)); }
        renderer.set_hidden_slots(session.document().texture_sets.iter().filter(|ts| session.is_set_hidden(ts.id)).map(|ts| ts.material_slot));
        let upload = |renderer: &mut ViewportRenderer, ts: subcraft_core::TextureSetId, slot: u32, changed: Option<&subcraft_app::ChangedTiles>| {
            let Some(c) = session.composite(ts) else { return };
            // Opacity is not shaded but drives the alpha modes (issue #145).
            // Opacity drives the alpha modes (issue #145), Specular the reflectance of non-metals.
            for k in SHADED_CHANNELS.into_iter().chain([ChannelKind::Opacity, ChannelKind::Specular]) {
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
        // A channel shown alone that shading never reads (Opacity, Specular, a user channel):
        // uploaded whole when it is first shown, then tile by tile like the others.
        let solo = self.recovery.prefs.viewport.solo.filter(|k| !SHADED_CHANNELS.contains(k) && *k != ChannelKind::Opacity);
        if let Some(k) = solo {
            let first = full || self.uploaded_solo != solo;
            for ts in &session.document().texture_sets {
                let Some(img) = session.composite(ts.id).and_then(|c| c.channel(k)) else { continue };
                if first {
                    renderer.upload_channel(&rs.device, &rs.queue, ts.material_slot, k, img, None);
                } else if let Some(u) = updates.iter().find(|u| u.texture_set == ts.id) {
                    renderer.upload_channel(&rs.device, &rs.queue, ts.material_slot, k, img, Some(u.changed.get(&k).map_or(&[], Vec::as_slice)));
                }
            }
        }
        self.uploaded_solo = solo;
        // Quick mask edition: Base Color tinted red where the mask protects (3D and 2D views).
        let quick = session.quick_mask().filter(|q| q.editing).map(|q| (q.texture_set, &q.image));
        for ts in &session.document().texture_sets {
            let Some(base) = session.composite(ts.id).and_then(|c| c.channel(ChannelKind::BaseColor)) else { continue };
            let mask = quick.filter(|(id, _)| *id == ts.id).map(|(_, image)| image);
            match self.quick_mask_tint.update(ts.id, base, mask, full) {
                subcraft_app::quick_mask_tint::TintUpload::None => {}
                subcraft_app::quick_mask_tint::TintUpload::Tinted(img, tiles) => {
                    renderer.upload_channel(&rs.device, &rs.queue, ts.material_slot, ChannelKind::BaseColor, img, tiles.as_deref());
                }
                subcraft_app::quick_mask_tint::TintUpload::Restore => renderer.upload_channel(&rs.device, &rs.queue, ts.material_slot, ChannelKind::BaseColor, base, None),
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

/// Built-in brush presets: ours first, then the open-pack channel presets whose names are not
/// already taken.
fn builtin_brush_presets() -> Vec<subcraft_app::brush_preset::BrushPreset> {
    let mut all = subcraft_app::brush_builtin::builtin_brush_presets();
    for p in subcraft_app::brush_preset::builtin() {
        if !all.iter().any(|q| q.name == p.name) {
            all.push(p);
        }
    }
    all
}

/// Display options of the stencil overlay in the 3D view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StencilDisplay {
    /// Opacity of the overlay (0 = invisible, the stencil still masks paint).
    pub opacity: f32,
    /// Hide the overlay while a stroke is being painted.
    pub hide_when_painting: bool,
}

impl Default for StencilDisplay {
    fn default() -> Self {
        Self { opacity: 110.0 / 255.0, hide_when_painting: false }
    }
}

#[cfg(test)]
#[path = "state_sync_tests.rs"]
mod state_sync_tests;

#[cfg(test)]
mod modal_tests {
    use super::*;

    /// QA 2026-11: document shortcuts (Delete, Ctrl+Z) acted behind the unsaved-changes prompt.
    #[test]
    fn any_modal_window_blocks_document_shortcuts() {
        let mut state = UiState::new(Session::sample(), None);
        assert!(!state.modal_open());
        state.confirm = Some(crate::dialogs::Pending::Close);
        assert!(state.modal_open());
        state.confirm = None;
        state.new_project.open = true;
        assert!(state.modal_open());
        state.new_project.open = false;
        assert!(!state.modal_open());
    }
}
