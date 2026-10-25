//! The eframe application: menu bar, dockable panels, status bar and keyboard shortcuts.

use std::path::PathBuf;

use std::collections::HashSet;

use egui::{RichText, Ui};
use egui_dock::{DockArea, DockState, NodeIndex};
use serde::{Deserialize, Serialize};
use subcraft_app::{ACTIONS, Action, Command, Key, NewLayer, Session, Shortcut, action_for};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, LayerContent, MaskEffectKind, ProceduralKind, ProceduralParams};

use crate::dialogs::{self, Pending};
use crate::orbit::OrbitCamera;
use crate::state::{Tool, UiState};
use crate::widgets::{icon_button, primary_button, secondary_button, tool_button};
use crate::{icons, panels, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Viewport,
    UvView,
    Layers,
    Properties,
    Brush,
    TextureSets,
    Parts,
    History,
    Shelf,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Viewport => "3D View",
            Tab::UvView => "2D View",
            Tab::Layers => "Layers",
            Tab::Properties => "Properties",
            Tab::Brush => "Brush",
            Tab::TextureSets => "Texture Sets",
            Tab::Parts => "Parts",
            Tab::History => "History",
            Tab::Shelf => "Shelf",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tab::Viewport => icons::CUBE,
            Tab::UvView => icons::GRID,
            Tab::Layers => icons::STACK,
            Tab::Properties => icons::SLIDERS,
            Tab::Brush => icons::PAINT_BRUSH,
            Tab::TextureSets => icons::CUBE_WIRE,
            Tab::Parts => icons::CUBE_WIRE,
            Tab::History => icons::HISTORY,
            Tab::Shelf => icons::SQUARES,
        }
    }
}

/// Command-line options.
#[derive(Clone, Debug, Default)]
pub struct LaunchOptions {
    /// Open Preferences (also useful for isolated renderer smoke tests).
    pub preferences: bool,
    pub graphics_startup: Option<crate::graphics::GraphicsUi>,
    /// Open the welcome hub even when taking a screenshot.
    pub welcome: bool,
    /// Open a step of the in-app guide (zero-based).
    pub guide_step: Option<usize>,

    /// Open a workspace snapshot preserved during an in-app update.
    pub resume_update: Option<PathBuf>,
    /// Check releases on startup (scripted verification / Help).
    pub check_updates: bool,
    #[cfg(feature = "update-fixture")]
    pub auto_update_fixture: bool,
    /// Project (`.subcraft`) or mesh to open instead of the sample crate.
    pub mesh: Option<PathBuf>,
    /// Build a small demo layer stack (fills, masks and painted strokes) on start.
    pub demo: bool,
    /// Save a screenshot of the window to this path after a few frames, then quit.
    pub screenshot: Option<PathBuf>,
    /// Start with the 2D (UV) view in front.
    pub view_2d: bool,
    /// Start with Brush settings in front (also useful for native screenshot verification).
    pub brush_panel: bool,
    /// Start with a narrow vertical texture shelf (native layout validation).
    pub shelf_panel: bool,
    /// Open the selected layer's first mask generator for native validation.
    pub generator_panel: bool,
    /// Select this Shelf brush (tip or preset name) shortly before a scripted screenshot, the
    /// same way a Shelf click does, to capture the viewport acknowledgement natively.
    pub select_brush: Option<String>,
    /// Run a scripted Clone or Smudge demonstration ("clone" / "smudge") for native captures.
    pub capture_tool: Option<String>,
    /// Open this Shelf tab (textures, materials, smart-materials, smart-masks, brushes, environments).
    pub shelf_tab: Option<String>,
    /// Open the Export Textures window (scripted captures).
    pub export_window: bool,
    /// Open the New Project window with this model loaded (scripted captures).
    pub new_project_model: Option<PathBuf>,
    /// High-poly model for the same window.
    pub new_project_high: Option<PathBuf>,
    /// Give Properties the whole right column (scripted captures of its long blocks).
    pub properties_panel: bool,
    /// Export template to select (index into the built-in list) when the window opens.
    pub export_template: Option<usize>,
    /// Open the Render quality window (scripted captures).
    pub render_window: bool,
    /// Render a still or a turntable from the loaded project, then quit (scripted export).
    pub render_job: Option<crate::render_export::Job>,
    /// Look overrides for scripted renders, e.g. `tone=filmic,bloom=0.8,ao=0.7,exposure=1.2`.
    pub quality: Option<String>,
    pub effect_panel: bool,
    pub symmetry_axes: Option<[bool; 3]>,
    /// Start a real high-to-low bake and show its dialog (also usable with --screenshot).
    pub bake_high_poly: Option<PathBuf>,
    pub bake_size: Option<u32>,
    /// Inspect the real high/low geometry before baking, without starting the bake.
    pub bake_preview_only: bool,
    /// Pin a completed map for reproducible native screenshot validation.
    pub bake_preview_map: Option<subcraft_core::MeshMapKind>,
    /// Recover the newest crashed session without asking.
    pub recover: bool,
    /// Autosave interval instead of the preference (testing).
    pub autosave_interval: Option<std::time::Duration>,
    /// Panic a moment after start (tests the crash report).
    pub test_crash: bool,
    /// Measure frames while a generator value changes every frame, write stats, quit.
    pub profile_edits: Option<(PathBuf, Option<u32>)>,
}

impl LaunchOptions {
    fn shows_welcome(&self, enabled: bool, recovered: bool) -> bool {
        self.resume_update.is_none() && (self.welcome || (enabled && self.mesh.is_none()
            && !self.demo && self.screenshot.is_none() && !recovered))
    }
}

pub fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport, Tab::UvView]);
    let surface = dock.main_surface_mut();
    let [center, right] = surface.split_right(NodeIndex::root(), 0.77, vec![Tab::Layers]);
    surface.split_below(right, 0.48, vec![Tab::Properties, Tab::Brush]);
    let [center, _] = surface.split_left(center, 0.28, vec![Tab::Shelf]);
    surface.split_below(center, 0.82, vec![Tab::Parts, Tab::TextureSets, Tab::History]);
    dock
}

// Apply the requested shelf-left / utility-tabs-bottom arrangement once, then retain edits.
const DOCK_KEY: &str = "subcraft_dock_v5";

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn shelf_is_separate_and_utility_tabs_share_the_bottom_center_node() {
        let dock = default_layout();
        let parts = dock.find_tab(&Tab::Parts).unwrap();
        for tab in [Tab::TextureSets,Tab::History] {
            let path = dock.find_tab(&tab).unwrap(); assert_eq!((path.surface,path.node),(parts.surface,parts.node));
        }
        for tab in [Tab::Shelf,Tab::Viewport,Tab::Layers] {assert_ne!(dock.find_tab(&tab).unwrap().node,parts.node);}
        assert_ne!(dock.find_tab(&Tab::Shelf).unwrap().node,dock.find_tab(&Tab::Viewport).unwrap().node);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct InspectorSelection(subcraft_app::Selection, Option<subcraft_doc::LayerKind>, subcraft_app::LayerTarget, Option<subcraft_core::EffectId>, bool);

impl InspectorSelection {
    fn current(session: &Session) -> Self {
        let paints_effect = matches!(session.brush.target, subcraft_paint::PaintTarget::ContentEffect(_));
        Self(session.selection(), session.selected_layer().map(|layer| layer.kind()), session.layer_target(), session.selected_effect(), paints_effect)
    }
}

#[cfg(test)]
#[path = "inspector_tests.rs"]
mod inspector_tests;

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn restored_updates_and_existing_work_open_directly_in_the_workspace() {
        let mut options = LaunchOptions::default();
        assert!(options.shows_welcome(true, false)); assert!(!options.shows_welcome(false, false));
        assert!(!options.shows_welcome(true, true));
        options.mesh = Some("project.subcraft".into()); assert!(!options.shows_welcome(true, false));
        options.mesh = None; options.demo = true; assert!(!options.shows_welcome(true, false));
        options.demo = false; options.screenshot = Some("capture.png".into()); assert!(!options.shows_welcome(true, false));
        options.welcome = true; assert!(options.shows_welcome(false, false));
        options.resume_update = Some("job.json".into());
        assert!(!options.shows_welcome(true, false)); assert!(!options.shows_welcome(true, true));
    }

    #[test]
    fn recent_open_waits_for_unsaved_changes_before_replacing_a_session() {
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 128), None),
            dock: default_layout(), frames: 0, screenshot: None, screenshot_requested: false,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None,
        };
        app.request(Pending::OpenRecent, 0.0);
        assert_eq!(app.pending_now.take(), Some(Pending::OpenRecent));
        assert_eq!(app.state.confirm, None);
        app.state.session.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
        let doc = app.state.session.document().clone();
        app.request(Pending::OpenRecent, 0.0);
        assert_eq!(app.pending_now, None);
        assert_eq!(app.state.confirm, Some(Pending::OpenRecent));
        assert_eq!(app.state.session.document(), &doc);
    }

    #[test]
    fn shelf_double_click_request_brings_the_brush_settings_forward_once() {
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None),
            dock: default_layout(), frames: 0, screenshot: None, screenshot_requested: false,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None,
        };
        let brush = app.dock.find_tab(&Tab::Brush).unwrap();
        let properties = app.dock.find_tab(&Tab::Properties).unwrap();
        app.dock.set_active_tab(properties).unwrap();
        let active = |app: &mut SubcraftApp| app.dock.find_active_focused().map(|(_, tab)| *tab);
        app.open_requested_brush_editor();
        assert_ne!(active(&mut app), Some(Tab::Brush), "no request, no change");
        app.state.open_brush_editor = true;
        app.open_requested_brush_editor();
        assert_eq!(active(&mut app), Some(Tab::Brush));
        assert!(!app.state.open_brush_editor, "consumed");
        assert_eq!(Some(brush.node_path()), app.dock.focused_leaf());
    }

    #[test]
    fn file_menu_opens_preferences_without_project_edits() {
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None),
            dock: default_layout(), frames: 0, screenshot: None, screenshot_requested: false,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None,
        };
        let doc = app.state.session.document().clone();
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let draw = |app: &mut SubcraftApp, events| {
            let mut out = ctx.run_ui(egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))), ..Default::default() }, |ui| {
                egui::Panel::top("file-menu-test").exact_size(44.0).show(ui, |ui| app.header_bar(ui));
            });
            out.textures_delta.clear(); out
        };
        let find = |out: &egui::FullOutput, text: &str| out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text().ends_with(text) => Some(t.pos + t.galley.size() * 0.5), _ => None,
        }).unwrap_or_else(|| panic!("missing {text}"));
        for label in ["File", "Preferences…"] {
            for _ in 0..3 { draw(&mut app, vec![]); }
            let out = draw(&mut app, vec![]); let p = find(&out, label);
            for pressed in [true, false] {
                draw(&mut app, vec![egui::Event::PointerMoved(p), egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]);
            }
        }
        assert!(app.state.recovery.show_prefs);
        assert_eq!(app.state.session.document(), &doc);
    }

    #[test]
    fn long_header_titles_cannot_paint_over_menus_or_actions() {
        let mut mesh = subcraft_mesh::primitives::cube(2.0);
        mesh.name = "A very long multi-part model with a name that exceeds the header gap".into();
        let mut app = SubcraftApp {
            state: UiState::new(Session::new(mesh, 128), None),
            dock: default_layout(), frames: 0, screenshot: None, screenshot_requested: false,
            pending_now: None, active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None,
        };
        let title = dialogs::title(&app.state).trim_start_matches("SubCraft — ").to_string();
        for width in [460.0, 1200.0] {
            let ctx = egui::Context::default(); theme::apply(&ctx);
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0))), ..Default::default() }, |ui| {
                egui::Panel::top("test-header").exact_size(44.0).show(ui, |ui| app.header_bar(ui));
            });
            output.textures_delta.clear();
            let texts: Vec<_> = output.shapes.iter().filter_map(|s| {
                if let egui::epaint::Shape::Text(t) = &s.shape { Some((s.clip_rect, t)) } else { None }
            }).collect();
            let project = texts.iter().find(|(_, t)| t.galley.job.text == title);
            if width == 460.0 { assert!(project.is_none(), "A narrow header must leave room for its controls"); }
            else {
                let (clip, _) = project.expect("A wide header should show the project name");
                let (_, help) = texts.iter().find(|(_, t)| t.galley.job.text == "Help").unwrap();
                let (_, export) = texts.iter().find(|(_, t)| t.galley.job.text.contains("Export")).unwrap();
                assert!(clip.left() > help.pos.x + help.galley.size().x);
                assert!(clip.right() < export.pos.x);
            }
        }
    }

    #[test]
    fn save_and_open_wait_for_pen_jobs_and_then_use_the_finished_document() {
        let file = std::env::temp_dir().join(format!("subcraft-pen-save-{}.subcraft", subcraft_core::ObjectId::new()));
        let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 128); session.save(Some(&file)).unwrap();
        let mut app = SubcraftApp {
            state: UiState::new(session, None), dock: default_layout(), frames: 0,
            screenshot: None, screenshot_requested: false, pending_now: None,
            active_tabs: HashSet::new(), inspector_selection: None, test_crash: false, profile: None,
        };
        app.state.pen.begin(&app.state.session, glam::Vec2::splat(0.4), None).unwrap();
        app.perform(Action::Save, 0.0); assert!(app.state.pen_after.is_some());
        assert_eq!(Session::open_project(&file).unwrap().selected_layer().unwrap().kind(), subcraft_doc::LayerKind::Paint);
        while app.state.pen.poll(&mut app.state.session, 16384).unwrap() {}
        app.flush_pen_after(0.1);
        assert_eq!(Session::open_project(&file).unwrap().document(), app.state.session.document());
        assert!(!app.state.session.is_modified());
        app.state.pen.begin(&app.state.session, glam::Vec2::splat(0.6), None).unwrap();
        app.request(Pending::OpenProject, 0.2); assert_eq!(app.state.confirm, None); assert!(app.pending_now.is_none());
        while app.state.pen.poll(&mut app.state.session, 16384).unwrap() {}
        app.flush_pen_after(0.3); assert_eq!(app.state.confirm, Some(Pending::OpenProject));
        std::fs::remove_file(file).unwrap();
    }
}

pub struct SubcraftApp {
    state: UiState,
    dock: DockState<Tab>,
    frames: u64,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    /// Action approved this frame, run after the UI pass.
    pending_now: Option<Pending>,
    /// Tabs whose body was shown last frame (drawn with the accent marker).
    active_tabs: HashSet<Tab>,
    /// Follow selection changes once, leaving explicit tab choices alone between selections.
    inspector_selection: Option<InspectorSelection>,
    test_crash: bool,
    profile: Option<crate::edit_profile::EditProfile>,
    /// Background Substance Painter `.spp` open (experimental).
    spp_open: Option<subcraft_app::SppOpenJob>,
}

impl SubcraftApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: LaunchOptions) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut status = None;
        let is_project = |p: &std::path::Path| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(subcraft_app::PROJECT_EXTENSION));
        let is_spp = |p: &std::path::Path| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("spp"));
        let mut spp_open = None;
        let resumed = options.resume_update.as_ref().and_then(|path| match subcraft_app::updater::resume_session(path) {
            Ok(session) => Some(session), Err(e) => { status = Some(format!("Could not restore update snapshot: {e}")); None }
        });
        let session = if let Some(session) = resumed { session } else { match &options.mesh {
            Some(path) if is_project(path) => match Session::open_project(path) {
                Ok(s) => s,
                Err(e) => {
                    status = Some(format!("Could not open {}: {e}. Showing the sample.", path.display()));
                    Session::sample()
                }
            },
            Some(path) if is_spp(path) => {
                spp_open = Some(crate::spp_dialogs::start_spp_open(path.clone()));
                status = Some(format!("Opening Substance project {}…", path.display()));
                Session::sample()
            }
            Some(path) => match Session::open_mesh(path, subcraft_app::DEFAULT_RESOLUTION) {
                Ok((s, warnings)) => {
                    if !warnings.is_empty() {
                        status = Some(format!("Imported with {} warning(s): {}", warnings.len(), warnings[0]));
                    }
                    s
                }
                Err(e) => {
                    status = Some(format!("Could not open {}: {e}. Showing the sample.", path.display()));
                    Session::sample()
                }
            },
            None => Session::sample(),
        }};
        let restore_failed = status.is_some();
        let mut state = UiState::new(session, cc.wgpu_render_state.clone())
            .with_user_library(subcraft_app::assets::UserLibrary::default_root().map(subcraft_app::assets::UserLibrary::new));
        let opened_from_arg = !status.as_ref().is_some_and(|s| s.starts_with("Could not open"));
        if let Some(s) = status {
            state.set_status(s, 0.0);
        }
        {
            if !restore_failed && let Some(path) = &options.resume_update {
                let result = std::fs::read_to_string(path.with_file_name("result.txt")).unwrap_or_else(|_| "Your workspace was restored after an update.".into());
                state.set_status(result.clone(), 0.0);
                if result.starts_with("Update failed") && let Some(updater) = &mut state.updater { updater.open = true; updater.phase = subcraft_app::updater::Phase::Error(result); }
            }
        }
        crate::bug_report::record_gpu(cc.wgpu_render_state.as_ref());
        state.bug_report = crate::bug_report::BugReportUi::load();
        if let Some(graphics) = options.graphics_startup.clone() {
            let active = state.graphics.active.clone(); state.graphics = graphics; state.graphics.active = active;
        }
        let prefs = cc.storage.and_then(|s| s.get_string(crate::recovery::PREFS_KEY)).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        state.recovery = crate::recovery::RecoveryUi::start(&state.session, prefs, options.autosave_interval);
        state.recovery.prefs.apply_ui_size(&cc.egui_ctx);
        state.recovery.show_prefs = options.preferences;
        if let Some(high) = &options.bake_high_poly {
            state.bake.open = true;
            state.bake.high_poly = high.display().to_string();
            if let Some(size) = options.bake_size { state.bake.settings.size = size; }
            let texture_sets = state.session.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
            if !options.bake_preview_only { state.bake.job = Some(state.session.start_bake(subcraft_app::BakeRequest {
                settings: state.bake.settings.clone(), high_poly: Some(high.clone()), texture_sets,
            })); }
            if let Some(kind) = options.bake_preview_map { state.bake.displayed_map = Some(kind); state.bake.follow_maps = false; }
        }
        {
            // Background release check; a newer version gets a callout at the update button.
            // Not for scripted screenshots or right after an update restart.
            let automatic = state.recovery.prefs.check_updates_on_start && options.screenshot.is_none() && options.resume_update.is_none();
            if (options.check_updates || automatic) && let Some(updater) = &mut state.updater {
                updater.check();
                // The automatic check is silent: no window, only the callout if there's an update.
                updater.open = options.check_updates;
                state.update_notice = true;
            }
        }
        {
            state.welcome.recent = cc.storage.and_then(|s| s.get_string(crate::welcome::RECENTS_KEY))
                .and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
            state.welcome.refresh();
            if opened_from_arg && let Some(path) = &options.mesh { state.welcome.remember(path); }
        }
        if options.recover && !state.recovery.crashed.is_empty() {
            let c = state.recovery.crashed.remove(0);
            crate::recovery::recover(&mut state, c, 0.0);
        }
        if options.demo || options.profile_edits.is_some() {
            build_demo(&mut state);
        }
        if let Some(resolution) = options.profile_edits.as_ref().and_then(|p| p.1) {
            let _ = state.session.execute(Command::SetResolution { texture_set: None, resolution });
        }
        state.welcome.open = options.shows_welcome(state.recovery.prefs.show_welcome, state.session.is_recovered());
        if let Some(symmetry) = options.symmetry_axes {
            let stroke = subcraft_paint::StrokeOptions { symmetry, ..state.session.brush.options };
            let _ = state.session.execute(Command::SetStrokeOptions(stroke));
        }
        if options.effect_panel && let Some(layer) = state.session.selected_layer() {
            let target = layer.content_effects.last().map(|e| (layer.id, subcraft_app::LayerTarget::Content, e.id))
                .or_else(|| layer.mask.as_ref().and_then(|m| m.effects.last()).map(|e| (layer.id, subcraft_app::LayerTarget::Mask, e.id)));
            if let Some((layer, target, effect)) = target { let _ = state.session.execute(Command::SelectEffect { layer, target, effect: Some(effect) }); }
        }
        if options.generator_panel && let Some(layer) = state.session.selected_layer() {
            let target = layer.mask.as_ref().and_then(|m| m.effects.iter().find(|e| matches!(e.kind, subcraft_doc::MaskEffectKind::Generator(_)))).map(|e| (layer.id, e.id));
            if let Some((layer, effect)) = target { let _ = state.session.execute(Command::SelectEffect { layer, target: subcraft_app::LayerTarget::Mask, effect: Some(effect) }); }
        }
        state.capture_select_brush = options.select_brush.clone();
        state.capture_tool = options.capture_tool.clone();
        if let Some(spec) = options.quality.as_deref() {
            let prefs = &mut state.recovery.prefs.viewport;
            for part in spec.split(',') {
                let Some((key, value)) = part.split_once('=') else { continue };
                let number = value.parse::<f32>().ok();
                match (key.trim(), number) {
                    ("bloom", Some(v)) => prefs.bloom = v,
                    ("threshold", Some(v)) => prefs.bloom_threshold = v,
                    ("ao", Some(v)) => prefs.ssao = v,
                    ("ao-radius", Some(v)) => prefs.ssao_radius = v,
                    ("exposure", Some(v)) => prefs.exposure = v,
                    ("glow", Some(v)) => prefs.emission_glow = v,
                    ("glow-radius", Some(v)) => prefs.glow_radius = v,
                    ("glare", Some(v)) => prefs.glare = v,
                    ("glare-length", Some(v)) => prefs.glare_length = v,
                    ("glare-mode", _) => {
                        let wanted = value.trim().to_ascii_lowercase();
                        if let Some(i) = subcraft_render::GlareMode::ALL.iter().position(|m| m.label().to_ascii_lowercase().starts_with(&wanted)) { prefs.glare_mode = i; }
                    }
                    ("tone", _) => {
                        let wanted = value.trim().to_ascii_lowercase();
                        if let Some(i) = subcraft_render::ToneMap::ALL.iter().position(|t| t.label().to_ascii_lowercase().starts_with(&wanted)) { prefs.tone_map = i; }
                    }
                    _ => {}
                }
            }
        }
        state.pending_render_job = options.render_job.clone();
        if let Some(model) = &options.new_project_model {
            state.new_project.show_window(&state.recovery.prefs.new_project.clone());
            state.new_project.load_model(model);
            if let Some(high) = &options.new_project_high { state.new_project.load_high_poly(high); }
            state.welcome.open = false;
        }
        state.export.open = options.export_window || options.export_template.is_some();
        if let Some(i) = options.export_template { state.export.template = i.min(subcraft_export::ExportTemplate::builtin().len() - 1); }
        state.render_window = options.render_window;
        if let Some(tab) = options.shelf_tab.as_deref() {
            use crate::panels::shelf::ShelfTab;
            state.shelf.tab = match tab {
                "materials" => ShelfTab::Materials,
                "smart-materials" => ShelfTab::SmartMaterials,
                "smart-masks" => ShelfTab::SmartMasks,
                "brushes" => ShelfTab::Brushes,
                "environments" => ShelfTab::Environments,
                _ => ShelfTab::Textures,
            };
        }
        state.welcome.guide = options.guide_step.map(|s| s.min(subcraft_app::launch::QUICK_START.len() - 1));
        if state.welcome.guide.is_some() { state.welcome.open = false; }

        #[cfg(feature = "update-fixture")]
        if options.auto_update_fixture && let Some(updater) = &mut state.updater { updater.request_update(); }
        let dock = cc
            .storage
            .and_then(|s| s.get_string(DOCK_KEY))
            .and_then(|j| serde_json::from_str(&j).ok())
            .filter(|_| options.screenshot.is_none())
            .unwrap_or_else(default_layout);
        let mut dock = dock;
        if options.properties_panel && let Some(path) = dock.find_tab(&Tab::Layers) { let _ = dock.remove_tab(path); }
        if options.shelf_panel {
            dock = default_layout();
            state.shelf.tab = crate::panels::shelf::ShelfTab::Textures;
            if let Some(path) = dock.find_tab(&Tab::TextureSets) { let _ = dock.set_active_tab(path); }
        }
        if options.view_2d
            && let Some(path) = dock.find_tab(&Tab::UvView)
        {
            let _ = dock.set_active_tab(path);
        }
        if options.brush_panel && let Some(path) = dock.find_tab(&Tab::Brush) {
            let _ = dock.set_active_tab(path);
        } else if state.session.selected_layer().is_some_and(|layer| layer.kind() != subcraft_doc::LayerKind::Paint)
            && let Some(path) = dock.find_tab(&Tab::Properties)
        {
            let _ = dock.set_active_tab(path);
        }
        Self {
            inspector_selection: Some(InspectorSelection::current(&state.session)),
            state,
            dock,
            frames: 0,
            screenshot: options.screenshot,
            screenshot_requested: false,
            pending_now: None,
            active_tabs: HashSet::new(),
            test_crash: options.test_crash,
            profile: options.profile_edits.map(|(out, res)| crate::edit_profile::EditProfile::new(out, res)),
            spp_open,
        }
    }

    fn follow_layer_selection(&mut self) -> bool {
        let selection = InspectorSelection::current(&self.state.session);
        if self.inspector_selection.replace(selection) == Some(selection) {
            return false;
        }
        // Paint layers and content Paint effects open the brush; everything else its Properties.
        let paint_content = selection.1 == Some(subcraft_doc::LayerKind::Paint) && selection.2 == subcraft_app::LayerTarget::Content && selection.3.is_none();
        let tab = if paint_content || selection.4 { Tab::Brush } else { Tab::Properties };
        self.dock.find_tab(&tab).is_some_and(|path| self.dock.set_active_tab(path).is_ok())
    }

    /// A Shelf double-click asks for the Brush settings: bring that tab forward and focus it.
    fn open_requested_brush_editor(&mut self) {
        if std::mem::take(&mut self.state.open_brush_editor) && let Some(path) = self.dock.find_tab(&Tab::Brush) {
            let _ = self.dock.set_active_tab(path);
            self.dock.set_focused_node_and_surface(path.node_path());
        }
    }

    /// Resolve selection before drawing (shortcuts/imports) and after drawing (layer clicks).
    fn workspace(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.follow_layer_selection();
        self.open_requested_brush_editor();
        let style = theme::dock_style(ui.style());
        let mut viewer = Viewer { state: &mut self.state, was_active: &self.active_tabs, active: HashSet::new() };
        egui::Frame::new().fill(theme::BG_0).inner_margin(egui::Margin { left: 3, right: 0, top: 3, bottom: 0 }).show(ui, |ui| {
            DockArea::new(&mut self.dock).style(style).show_close_buttons(false).show_leaf_close_all_buttons(false).show_leaf_collapse_buttons(false).show_inside(ui, &mut viewer);
        });
        let active = std::mem::take(&mut viewer.active);
        if active != self.active_tabs {
            self.active_tabs = active;
            ctx.request_repaint();
        }
        if self.follow_layer_selection() {
            ctx.request_repaint();
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed: Vec<Shortcut> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => map_key(*key).map(|k| Shortcut {
                        ctrl: modifiers.command,
                        shift: modifiers.shift,
                        alt: modifiers.alt,
                        key: k,
                    }),
                    _ => None,
                })
                .collect()
        });
        for s in pressed {
            if let Some(a) = action_for(s) {
                self.perform(a, ctx.input(|i| i.time));
            }
        }
    }

    fn perform(&mut self, action: Action, now: f64) {
        if self.state.pen.is_editing() || self.state.pen.is_pending() || self.state.asset_preview.is_applying() {
            if action == Action::Undo { if self.state.asset_preview.is_applying() {self.state.asset_preview.cancel_apply();} else {self.state.pen.undo_pending(&self.state.session);} return; }
            if matches!(action, Action::Save | Action::SaveAs | Action::Export) {
                if let Err(e) = self.state.pen.release(&self.state.session) { self.state.set_status(e.to_string(), now); }
                if self.state.pen.is_pending() || self.state.asset_preview.is_applying() {
                    self.state.pen_after = Some(crate::pen::After::Action(action)); return;
                }
            }
        }
        match action {
            Action::ResetCamera => self.state.camera = OrbitCamera::framing(&self.state.session.scene().mesh.bounds),
            Action::BrushTool => self.state.tool = Tool::Brush,
            Action::EraserTool => self.state.tool = Tool::Eraser,
            Action::PenTool => { self.state.tool = Tool::Pen; self.state.picking_parts = false; }
            Action::Save => {
                dialogs::save(&mut self.state, false, now);
            }
            Action::SaveAs => {
                dialogs::save(&mut self.state, true, now);
            }
            Action::OpenProject => self.request(Pending::OpenProject, now),
            Action::Bake => self.state.bake.open = true,
            Action::Export => {
                self.state.export.open = true;
                self.state.export.settings.height_to_normal = self.state.height_scale;
            }
            _ => {
                if let Err(e) = self.state.session.perform(action) {
                    self.state.set_status(e.to_string(), now);
                }
            }
        }
    }

    /// Top bar: brand, menus, project name, then undo/redo and the main actions on the right.
    fn header_bar(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let bar = ui.max_rect();
        // Right side first so the left side can't push it out of the bar.
        let right_edge = ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            if primary_button(ui, format!("{}  Export", icons::EXPORT)).on_hover_text("Export textures (Ctrl+Shift+E)").clicked() {
                self.perform(Action::Export, now);
            }
            if secondary_button(ui, format!("{}  Bake", icons::MAGIC_WAND)).on_hover_text("Bake mesh maps (Ctrl+B)").clicked() {
                self.perform(Action::Bake, now);
            }
            crate::updates::button(&mut self.state, ui);
            if icon_button(ui, icons::SAVE, "Save (Ctrl+S)").clicked() {
                self.perform(Action::Save, now);
            }
            ui.add_space(6.0);
            let h = self.state.session.history();
            let (can_undo, can_redo) = (h.can_undo(), h.can_redo());
            if ui.add_enabled_ui(can_redo, |ui| icon_button(ui, icons::REDO, "Redo (Ctrl+Y)")).inner.clicked() {
                self.perform(Action::Redo, now);
            }
            if ui.add_enabled_ui(can_undo, |ui| icon_button(ui, icons::UNDO, "Undo (Ctrl+Z)")).inner.clicked() {
                self.perform(Action::Undo, now);
            }
            ui.cursor().right()
        }).inner;
        let left_edge = ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            // Brand mark.
            let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 0.0, theme::ACCENT);
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(5.0, 5.0), egui::vec2(8.0, 8.0)), 0.0, theme::BG_2);
            ui.label(RichText::new("SubCraft").font(theme::font_strong(14.0)).color(theme::TEXT));
            ui.add_space(10.0);
            self.menu(ui)
        }).inner;
        // Keep long project names out of the menus/actions.
        let title_rect = egui::Rect::from_min_max(egui::pos2(left_edge + 8.0, bar.top()), egui::pos2(right_edge - 8.0, bar.bottom()));
        if title_rect.width() >= 80.0 {
            let title = dialogs::title(&self.state);
            let name = title.trim_start_matches("SubCraft — ");
            let center = egui::pos2(bar.center().x.clamp(title_rect.left(), title_rect.right()), bar.center().y);
            ui.painter().with_clip_rect(title_rect.intersect(ui.clip_rect())).text(center, egui::Align2::CENTER_CENTER, name, theme::font(12.5), theme::TEXT_FAINT);
        }
    }

    /// Vertical tool strip left of the dock area.
    fn tool_rail(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            let tool = &mut self.state.tool;
            for (t, icon, tip) in [
                (Tool::Brush, icons::PAINT_BRUSH, "Brush (B)"),
                (Tool::Eraser, icons::ERASER, "Eraser (E)"),
                (Tool::PolygonFill, icons::PAINT_BUCKET, "Polygon fill"),
                (Tool::Clone, icons::COPY, "Clone: Ctrl+click the model to set the source, then paint to copy it"),
                (Tool::Smudge, icons::DROP, "Smudge: drag the paint under the brush along the stroke"),
                (Tool::Eyedropper, icons::CURSOR, "Eyedropper (Shift+click picks material)"),
                (Tool::Pen, icons::CURVE, "Pen paths (P): click points, drag for curves; edit anchors and handles in 2D View"),
            ] {
                if tool_button(ui, icon, *tool == t, tip).clicked() {
                    *tool = t;
                    if t == Tool::Pen { self.state.picking_parts = false; }
                }
            }
            ui.add_space(6.0);
            rail_divider(ui);
            ui.add_space(6.0);
            let target = &mut self.state.session.brush.target;
            let on_mask = target.is_mask();
            if ui.add_enabled_ui(*tool != Tool::Pen, |ui| tool_button(ui, icons::MASK, on_mask, "Paint the layer's mask instead of its content (Brush tool)")).inner.clicked() {
                *target = if on_mask { subcraft_paint::PaintTarget::Content } else { subcraft_paint::PaintTarget::MaskBase };
            }
            ui.add_space(6.0);
            rail_divider(ui);
            ui.add_space(6.0);
            if tool_button(ui, icons::CAMERA, false, "Reset camera (Home)").clicked() {
                self.perform(Action::ResetCamera, ui.input(|i| i.time));
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if tool_button(ui, icons::KEYBOARD, self.state.show_shortcuts, "Keyboard shortcuts").clicked() {
                    self.state.show_shortcuts = !self.state.show_shortcuts;
                }
            });
        });
    }

    fn menu(&mut self, ui: &mut Ui) -> f32 {
        let now = ui.input(|i| i.time);
        egui::MenuBar::new().ui(ui, |ui| {
            let item = |ui: &mut Ui, icon: &str, label: &str, sc: &str| ui.add(egui::Button::new(format!("{icon}   {label}")).shortcut_text(sc)).clicked();
            ui.menu_button("File", |ui| {
                ui.set_min_width(240.0);
                if item(ui, icons::FILE_PLUS, "New (sample crate)", "") {
                    self.request(Pending::NewSample, now);
                    ui.close();
                }
                if item(ui, icons::CUBE, "New project from a model…", "") {
                    ui.close();
                    self.request(Pending::OpenMesh, now);
                }
                if item(ui, icons::FOLDER_OPEN, "Open project…", "Ctrl+O") {
                    ui.close();
                    self.request(Pending::OpenProject, now);
                }
                if item(ui, icons::FOLDER_OPEN, "Open Substance (.spp)…", "") {
                    ui.close();
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Open Substance Painter project")
                        .add_filter("Substance Painter", &["spp"])
                        .pick_file()
                    {
                        self.spp_open = Some(crate::spp_dialogs::start_spp_open(path));
                    }
                }
                ui.separator();
                if item(ui, icons::SAVE, "Save", "Ctrl+S") {
                    ui.close();
                    self.perform(Action::Save, now);
                }
                if item(ui, icons::SAVE, "Save as…", "Ctrl+Shift+S") {
                    ui.close();
                    self.perform(Action::SaveAs, now);
                }
                ui.separator();
                if item(ui, icons::EXPORT, "Export textures…", "Ctrl+Shift+E") {
                    ui.close();
                    self.perform(Action::Export, now);
                }
                if item(ui, icons::CAMERA, "Render image / turntable…", "") {
                    ui.close();
                    self.state.render_window = true;
                }
                if item(ui, icons::MAGIC_WAND, "Bake mesh maps…", "Ctrl+B") {
                    ui.close();
                    self.perform(Action::Bake, now);
                }
                ui.separator();
                if item(ui, icons::SLIDERS, "Preferences…", "") {
                    self.state.recovery.show_prefs = true;
                    ui.close();
                }
                ui.separator();
                if item(ui, icons::X, "Quit", "") {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                ui.set_min_width(240.0);
                let h = self.state.session.history();
                let undo = h.undo_label().map(|l| format!("Undo {l}")).unwrap_or("Undo".into());
                let redo = h.redo_label().map(|l| format!("Redo {l}")).unwrap_or("Redo".into());
                let (can_undo, can_redo) = (h.can_undo(), h.can_redo());
                if ui.add_enabled(can_undo, egui::Button::new(format!("{}   {undo}", icons::UNDO)).shortcut_text("Ctrl+Z")).clicked() {
                    self.perform(Action::Undo, now);
                }
                if ui.add_enabled(can_redo, egui::Button::new(format!("{}   {redo}", icons::REDO)).shortcut_text("Ctrl+Y")).clicked() {
                    self.perform(Action::Redo, now);
                }
                ui.separator();
                if item(ui, icons::SLIDERS, "Preferences…", "") {
                    self.state.recovery.show_prefs = true;
                    ui.close();
                }
            });
            ui.menu_button("Layer", |ui| {
                ui.set_min_width(240.0);
                for (a, icon) in [
                    (Action::NewPaintLayer, icons::PAINT_BRUSH),
                    (Action::NewFillLayer, icons::PAINT_BUCKET),
                    (Action::NewFolder, icons::FOLDER),
                    (Action::DuplicateLayer, icons::COPY),
                    (Action::DeleteLayer, icons::TRASH),
                ] {
                    let info = subcraft_app::actions::info(a);
                    let sc = info.shortcuts.first().map(|s| s.label()).unwrap_or_default();
                    if item(ui, icon, info.label, &sc) {
                        self.perform(a, now);
                        ui.close();
                    }
                }
                ui.separator();
                if item(ui, icons::IMAGE, "Import texture set…", "") {
                    ui.close();
                    crate::panels::layers::import_texture_set_dialog(ui.ctx());
                }
                if item(ui, icons::SPARKLE, "Import SBSAR…", "") {
                    ui.close();
                    crate::panels::layers::import_sbsar_to_library_dialog(&mut self.state, now);
                }
                if item(ui, icons::SPARKLE, "Import Smart Material (.spsm)…", "") {
                    ui.close();
                    crate::panels::layers::import_smart_material_dialog(&mut self.state, now);
                }
                if item(ui, icons::MASK, "Import Smart Mask (.spmsk)…", "") {
                    ui.close();
                    crate::panels::layers::import_smart_mask_dialog(&mut self.state, now);
                }
            });
            ui.menu_button("View", |ui| {
                ui.set_min_width(240.0);
                if item(ui, icons::CAMERA, "Reset camera", "Home") {
                    self.perform(Action::ResetCamera, now);
                    ui.close();
                }
                if item(ui, icons::SQUARES, "Reset layout", "") {
                    self.dock = default_layout();
                    ui.close();
                }
                if item(ui, icons::SUN, "Render quality…", "") {
                    self.state.render_window = true;
                    ui.close();
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Height bump");
                    ui.add(egui::Slider::new(&mut self.state.height_scale, 0.0..=0.1));
                });
            });
            ui.menu_button("Help", |ui| {
                ui.set_min_width(240.0);
                if item(ui, icons::CUBE, "Welcome screen", "") { self.state.welcome.open = true; ui.close(); }
                if item(ui, icons::CURSOR, "Quick start", "") { self.state.welcome.guide = Some(0); self.state.welcome.open = false; ui.close(); }
                if item(ui, icons::INFO, "What's new", "") { self.state.welcome.whats_new = true; ui.close(); }
                if item(ui, icons::INFO, "About SubCraft…", "") { self.state.welcome.about = true; ui.close(); }
                ui.separator();

                if item(ui, icons::DOWNLOAD, "Check for updates…", "") { if let Some(updater) = &mut self.state.updater { updater.check(); } ui.close(); }
                if item(ui, icons::KEYBOARD, "Keyboard shortcuts", "") {
                    self.state.show_shortcuts = true;
                    ui.close();
                }
                ui.separator();
                if item(ui, icons::INFO, "Report a bug…", "") {
                    self.state.bug_report.open = true;
                    ui.close();
                }
                if let Some(log) = subcraft_app::diagnostics::log_file()
                    && item(ui, icons::FOLDER_OPEN, "Show log files", "")
                {
                    crate::panels::shelf::show_in_folder(&log);
                    ui.close();
                }
            });
            ui.cursor().left()
        }).inner
    }

    /// Run `action` now, or after the unsaved-changes prompt.
    fn request(&mut self, action: Pending, now: f64) {
        if self.state.pen.is_editing() || self.state.pen.is_pending() || self.state.asset_preview.is_applying() {
            if let Err(e) = self.state.pen.release(&self.state.session) { self.state.set_status(e.to_string(), now); }
            if self.state.pen.is_pending() || self.state.asset_preview.is_applying() {
                self.state.pen_after = Some(crate::pen::After::Request(action)); return;
            }
        }
        if self.state.session.is_modified() && self.screenshot.is_none() {
            self.state.confirm = Some(action);
        } else {
            self.pending_now = Some(action);
        }
        let _ = now;
    }

    fn flush_pen_after(&mut self, now: f64) {
        if !self.state.pen.is_editing() && !self.state.pen.is_pending() && !self.state.asset_preview.is_applying()
            && let Some(after) = self.state.pen_after.take()
        {
            match after {
                crate::pen::After::Action(action) => self.perform(action, now),
                crate::pen::After::Request(action) => self.request(action, now),
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let (icon, tool) = match self.state.tool {
                Tool::Brush => (icons::PAINT_BRUSH, "Brush"),
                Tool::Eraser => (icons::ERASER, "Eraser"),
                Tool::PolygonFill => (icons::PAINT_BUCKET, "Polygon fill"),
                Tool::Clone => (icons::COPY, "Clone"),
                Tool::Smudge => (icons::DROP, "Smudge"),
                Tool::Eyedropper => (icons::CURSOR, "Eyedropper"),
                Tool::Pen => (icons::CURVE, "Pen paths"),
            };
            let small = |t: String, c| RichText::new(t).font(theme::font(11.5)).color(c);
            ui.label(small(format!("{icon}  {tool}"), theme::ACCENT));
            let size = if self.state.tool == Tool::Pen {
                self.state.pen.preview(&self.state.session).map_or_else(|| "Click to start a path".into(), |p| format!("Width {:.2}% UV", p.radius * 200.0))
            } else { format!("Size {:.1}%", self.state.session.brush.settings.size) };
            ui.label(small(size, theme::TEXT_DIM));
            if let Some(l) = self.state.session.selected_layer() {
                ui.label(small(format!("{}  {} / {}", icons::STACK, self.state.session.selected_texture_set().name, l.name), theme::TEXT_DIM));
            }
            if let Some((msg, t)) = &self.state.status
                && now - t < 6.0
            {
                ui.label(small(format!("{}  {msg}", icons::INFO), theme::WARNING));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(small(format!("{:.1} ms", self.state.frame_time_ms), theme::TEXT_FAINT));
                let rec = &self.state.recovery;
                if rec.autosaver.is_saving() {
                    ui.label(small(format!("{}  Autosaving…", icons::SAVE), theme::TEXT_FAINT));
                } else if let Some(t) = rec.last_saved {
                    let mins = t.elapsed().map_or(0, |d| d.as_secs() / 60);
                    let when = if mins == 0 { "just now".to_string() } else { format!("{mins} min ago") };
                    ui.label(small(format!("{}  Autosaved {when}", icons::SAVE), theme::TEXT_FAINT))
                        .on_hover_text(rec.autosaver.live().map(|l| format!("Recovery copies: {}", l.dir().display())).unwrap_or_default());
                }
                let mesh = &self.state.session.scene().mesh;
                ui.label(small(format!("{} tris", mesh.triangle_count()), theme::TEXT_FAINT));
            });
        });
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let mut open = self.state.show_shortcuts;
        egui::Window::new("Keyboard shortcuts").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("shortcuts").num_columns(2).spacing([32.0, 8.0]).show(ui, |ui| {
                let key = |t: String| RichText::new(t).monospace().color(theme::ACCENT);
                for a in ACTIONS {
                    ui.label(RichText::new(a.label).color(theme::TEXT_DIM));
                    ui.label(key(a.shortcuts.iter().map(|s| s.label()).collect::<Vec<_>>().join(" / ")));
                    ui.end_row();
                }
                for (what, how) in [("Orbit", "Alt+LMB or RMB drag"), ("Pan", "MMB drag"), ("Zoom", "Wheel or Alt+RMB drag"), ("Straight line", "Shift+click"), ("Part actions", "RMB click"), ("Isolate / hide part", "I / H in the 3D view"), ("Restore all parts", "Esc in the 3D view"), ("Pick material", "Shift+click with eyedropper")] {
                    ui.label(RichText::new(what).color(theme::TEXT_DIM));
                    ui.label(key(how.to_string()));
                    ui.end_row();
                }
            });
        });
        self.state.show_shortcuts = open;
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        ctx.request_repaint();
        // A pinned map can be captured during a real bake, after its GPU upload. This follows
        // actual worker completion, never a simulated timer or a placeholder image.
        let pinned_ready = !self.state.bake.follow_maps && self.state.bake.displayed_map.is_some_and(|kind| {
            let ts = self.state.session.selection().texture_set;
            self.state.session.mesh_maps(ts).and_then(|m| m.get(kind)).is_some_and(|image|
                self.state.uploaded_maps.get(&(ts, kind)).is_some_and(|gpu| gpu.same_storage(image)))
        });
        if !self.screenshot_requested && self.frames >= 20 && (pinned_ready || self.state.bake.job.is_none() && !self.state.session.composites_pending())
            && !self.state.bake.preview_pending() && (self.state.bake.preview_mesh.is_none() || self.state.bake.preview_uploaded)
        {
            if let Some(job) = self.state.pending_render_job.take() {
                match crate::render_export::run(&mut self.state, &job) {
                    Ok(message) => log::info!("{message}"),
                    Err(e) => log::error!("scripted render failed: {e}"),
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.screenshot_requested = true;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let data: Vec<f32> = img.pixels.iter().flat_map(|c| c.to_array()).map(|b| b as f32 / 255.0).collect();
            match subcraft_image::save_image(&path, w as u32, h as u32, 4, &data, subcraft_image::FileFormat::Png8) {
                Ok(()) => log::info!("screenshot saved to {}", path.display()),
                Err(e) => log::error!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn rail_divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 1.0), egui::Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, theme::BORDER_STRONG));
}

fn map_key(k: egui::Key) -> Option<Key> {
    Some(match k {
        egui::Key::B => Key::B,
        egui::Key::D => Key::D,
        egui::Key::E => Key::E,
        egui::Key::F => Key::F,
        egui::Key::G => Key::G,
        egui::Key::N => Key::N,
        egui::Key::O => Key::O,
        egui::Key::P => Key::P,
        egui::Key::S => Key::S,
        egui::Key::X => Key::X,
        egui::Key::Y => Key::Y,
        egui::Key::Z => Key::Z,
        egui::Key::Delete => Key::Delete,
        egui::Key::OpenBracket => Key::OpenBracket,
        egui::Key::CloseBracket => Key::CloseBracket,
        egui::Key::Home => Key::Home,
        _ => return None,
    })
}

struct Viewer<'a> {
    state: &'a mut UiState,
    /// Tabs shown last frame (marked) and the set being collected this frame.
    was_active: &'a HashSet<Tab>,
    active: HashSet<Tab>,
}

impl egui_dock::TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        RichText::new(format!("{}   {}", tab.icon(), tab.title())).font(theme::font_strong(12.0)).into()
    }

    fn on_tab_button(&mut self, tab: &mut Tab, response: &egui::Response) {
        if self.was_active.contains(tab) {
            let r = response.rect;
            let painter = response.ctx.layer_painter(response.layer_id);
            painter.rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width(), 2.0)), 0.0, theme::ACCENT);
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        self.active.insert(*tab);
        match tab {
            Tab::Viewport => crate::edit_profile::time_tab("  tab 3D", || crate::viewport::show(self.state, ui)),
            Tab::UvView => crate::edit_profile::time_tab("  tab 2D", || panels::uv_view::show(self.state, ui)),
            Tab::Layers => crate::edit_profile::time_tab("  tab Layers", || panels::layers::show(self.state, ui)),
            Tab::Properties => crate::edit_profile::time_tab("  tab Properties", || panels::properties::show(self.state, ui)),
            Tab::Brush => crate::edit_profile::time_tab("  tab Brush", || panels::brush::show(self.state, ui)),
            Tab::TextureSets => crate::edit_profile::time_tab("  tab Texture sets", || panels::texture_sets::show(self.state, ui)),
            Tab::Parts => crate::edit_profile::time_tab("  tab Parts", || panels::parts::show(self.state, ui)),
            Tab::History => crate::edit_profile::time_tab("  tab History", || panels::history::show(self.state, ui)),
            Tab::Shelf => crate::edit_profile::time_tab("  tab Shelf", || panels::shelf::show(self.state, ui)),
        }
    }

    fn is_closeable(&self, _tab: &Tab) -> bool {
        false
    }

    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        // These panels own bounded vertical lists and pinned preview-size footers. The dock's
        // extra scroll area would let content widen itself and scroll the footer offscreen.
        if matches!(tab, Tab::Shelf | Tab::Layers | Tab::TextureSets) { [false, false] } else { [true, true] }
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Viewport | Tab::UvView)
    }
}

impl eframe::App for SubcraftApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(p) = &mut self.profile {
            p.ui_begin();
        }
        self.state.recovery.prefs.apply_ui_size(&ctx);
        self.frames += 1;
        if self.frames == 15 && let Some(name) = self.state.capture_select_brush.take() {
            let now = ctx.input(|i| i.time);
            let preset = self.state.brush_presets.iter().find(|p| p.name == name).cloned();
            let tip = self.state.library.alphas.iter().find(|a| a.name == name).cloned();
            match (preset, tip) {
                (Some(p), _) => self.state.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Preset(&p), now),
                (None, Some(a)) => self.state.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Tip(&a), now),
                (None, None) => self.state.set_status(format!("No Shelf brush named {name}"), now),
            }
        }
        if self.frames == 16 && let Some(which) = self.state.capture_tool.take() {
            scripted_tool_capture(&mut self.state, &which, ctx.input(|i| i.time));
        }
        crate::pen::keyboard(&mut self.state, &ctx, ctx.input(|i| i.time));
        self.shortcuts(&ctx);
        if ctx.input(|i| i.pointer.any_released()) {
            let _ = self.state.session.execute(Command::Seal);
        }
        if let Some(p) = &mut self.profile { p.mark("input+shortcuts"); }
        if let Some(p) = &mut self.profile {
            p.before_sync(&mut self.state);
            ctx.request_repaint();
        }
        self.state.begin_frame();
        self.state.sync_gpu();
        if self.state.session.composites_pending() {
            ctx.request_repaint();
        }
        if self.profile.as_mut().is_some_and(|p| p.after_sync(&self.state)) {
            self.profile = None;
            self.state.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.state.poll_library();
        self.state.sbsar_materials.poll(&ctx);
        {
            let now = ctx.input(|i| i.time);
            crate::spp_dialogs::poll_spp_open(&mut self.state, &mut self.spp_open, now);
            if self.spp_open.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        self.state.layer_previews.poll(&ctx);
        self.state.textures.poll(&ctx);
        if let Some(p) = &mut self.profile { p.mark("sync+polls"); }
        if self.state.env_loader.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        let bar = |fill| egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(12, 0));
        egui::Panel::top("header").exact_size(44.0).show_separator_line(false).frame(bar(theme::BG_2)).show(ui, |ui| self.header_bar(ui));
        egui::Panel::bottom("status").exact_size(26.0).show_separator_line(false).frame(bar(theme::BG_0)).show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("tools")
            .exact_size(48.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(theme::BG_2).inner_margin(egui::Margin::symmetric(4, 0)))
            .show(ui, |ui| self.tool_rail(ui));
        if let Some(p) = &mut self.profile { p.mark("bars"); }
        self.workspace(ui);
        if let Some(p) = &mut self.profile { p.mark("workspace (docked panels)"); }
        if let Some(layer) = ctx.data_mut(|data| data.remove_temp::<subcraft_core::LayerId>(egui::Id::new("inspect_mask"))) {
            self.state.layer_previews.inspect = Some(layer);
        }
        crate::layer_previews::inspection_window(&mut self.state, &ctx);
        self.shortcuts_window(&ctx);
        let now = ctx.input(|i| i.time);
        crate::asset_preview::show(&mut self.state,&ctx);
        crate::render_export::window(&mut self.state, &ctx);
        crate::new_project::window(&mut self.state, &ctx);
        crate::render_export::handle_request(&mut self.state, ctx.input(|i| i.time));
        crate::pen::finish_frame(&mut self.state, &ctx, now);
        self.flush_pen_after(now);
        if let Some(p) = &mut self.profile { p.mark("windows+pen"); }
        crate::recovery::tick(&mut self.state, &ctx, now);
        if let Some(p) = &mut self.profile { p.mark("recovery tick"); }
        crate::recovery::preferences_window(&mut self.state, &ctx);
        crate::recovery::prompt(&mut self.state, &ctx, now);
        crate::bug_report::report_window(&mut self.state, &ctx);
        crate::bug_report::crash_window(&mut self.state, &ctx);
        if let Some(action) = crate::welcome::show(&mut self.state, &ctx) {
            match action {
                crate::welcome::WelcomeAction::Pending(pending) => self.request(pending, now),
                crate::welcome::WelcomeAction::Guide(action) => match action {
                    subcraft_app::launch::GuideAction::OpenModel => self.request(Pending::OpenMesh, now),
                    subcraft_app::launch::GuideAction::Bake => self.state.bake.open = true,
                    subcraft_app::launch::GuideAction::SmartMaterial => {
                        self.state.shelf.tab = panels::shelf::ShelfTab::SmartMaterials;
                        if let Some(path) = self.dock.find_tab(&Tab::Shelf) { let _ = self.dock.set_active_tab(path); }
                    }
                    subcraft_app::launch::GuideAction::PaintParts => {
                        if let Some(path) = self.dock.find_tab(&Tab::Parts) { let _ = self.dock.set_active_tab(path); }
                        self.state.tool = Tool::Brush;
                    }
                    subcraft_app::launch::GuideAction::Save => self.perform(Action::Save, now),
                    subcraft_app::launch::GuideAction::Export => self.state.export.open = true,
                },
            }
            ctx.request_repaint();
        }

        crate::updates::tick(&mut self.state, &ctx);
        if let Some(p) = &mut self.profile { p.mark("dialogs+welcome"); }
        if self.test_crash && self.frames > 30 {
            panic!("test crash requested with --test-crash");
        }
        dialogs::export_window(&mut self.state, &ctx, now);
        dialogs::bake_window(&mut self.state, &ctx, now);
        dialogs::poll_bake(&mut self.state, &ctx, now);
        crate::files::poll(&mut self.state, now);
        if let Some(action) = dialogs::unsaved_prompt(&mut self.state, &ctx, now) {
            self.pending_now = Some(action);
        }
        // Closing with unsaved work asks first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.state.allow_close && self.screenshot.is_none()
            && (self.state.session.is_modified() || self.state.pen.is_editing() || self.state.pen.is_pending() || self.state.asset_preview.is_applying()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Pending::Close, now);
        }
        if let Some(action) = self.pending_now.take() {
            if action == Pending::Close {
                self.state.allow_close = true;
            }
            dialogs::run_pending(&mut self.state, action, &ctx, now);
        }
        let title = dialogs::title(&self.state);
        if title != self.state.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.state.last_title = title;
        }
        self.handle_screenshot(&ctx);
        if let Some(p) = &mut self.profile {
            p.mark("rest (title, close)");
            p.ui_end();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.screenshot.is_some() {
            return; // scripted runs never overwrite the user's layout
        }
        if let Ok(j) = serde_json::to_string(&self.dock) {
            storage.set_string(DOCK_KEY, j);
        }
        if let Ok(j) = serde_json::to_string(&self.state.recovery.prefs) {
            storage.set_string(crate::recovery::PREFS_KEY, j);
        }
        if let Ok(j) = serde_json::to_string(&self.state.welcome.recent) { storage.set_string(crate::welcome::RECENTS_KEY, j); }
    }

    /// Clean exit: the recovery folder of this session is no longer needed.
    fn on_exit(&mut self) {
        self.state.recovery.autosaver.finish_clean();
    }
}

/// Demo stack on the sample crate: procedural base, an upward-facing "moss" fill masked by a
/// directional generator, and a few painted strokes.
/// Native-capture helper: the same session calls the viewport makes, driven by a script instead of
/// the mouse (no input is sent to the desktop).
fn scripted_tool_capture(state: &mut UiState, which: &str, now: f64) {
    use glam::Vec2;
    use subcraft_paint::StrokeMode;
    let cam = state.camera.paint_camera([900, 650]);
    let s = &mut state.session;
    s.brush.settings.size = 7.0;
    s.brush.settings.hardness = 0.8;
    let stroke = |s: &mut Session, mode: StrokeMode, pts: &[(f32, f32)]| {
        s.brush.mode = mode;
        if s.begin_stroke().is_err() { return; }
        for &(x, y) in pts { let _ = s.stroke_to(&cam, Vec2::new(x, y), false); }
        s.end_stroke();
    };
    let line = |x0: f32, y0: f32, x1: f32, y1: f32| (0..=24).map(|i| { let t = i as f32 / 24.0; (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t) }).collect::<Vec<_>>();
    let set = |s: &mut Session, c: [f32; 3]| s.brush.material = subcraft_paint::BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(c[0], c[1], c[2]))]);
    match which {
        "smudge" => {
            state.tool = Tool::Smudge;
            for (i, c) in [[0.85, 0.1, 0.05], [0.1, 0.25, 0.9], [0.95, 0.8, 0.1]].into_iter().enumerate() {
                set(s, c);
                stroke(s, StrokeMode::Paint, &line(300.0, 250.0 + 45.0 * i as f32, 560.0, 250.0 + 45.0 * i as f32));
            }
            stroke(s, StrokeMode::Smudge, &line(380.0, 215.0, 410.0, 420.0));
            stroke(s, StrokeMode::Smudge, &line(470.0, 215.0, 440.0, 420.0));
        }
        "clone" => {
            state.tool = Tool::Clone;
            set(s, [0.85, 0.1, 0.05]);
            stroke(s, StrokeMode::Paint, &line(300.0, 270.0, 380.0, 270.0));
            set(s, [0.1, 0.25, 0.9]);
            stroke(s, StrokeMode::Paint, &line(300.0, 310.0, 380.0, 340.0));
            set(s, [0.95, 0.8, 0.1]);
            stroke(s, StrokeMode::Paint, &line(300.0, 300.0, 380.0, 300.0));
            if s.set_clone_source(&cam, Vec2::new(340.0, 305.0)) {
                stroke(s, StrokeMode::Clone, &line(470.0, 260.0, 560.0, 260.0));
                stroke(s, StrokeMode::Clone, &line(470.0, 330.0, 560.0, 350.0));
            }
        }
        "projection" => {
            // A colourful 8 x 8 checker cast from the view onto whatever the strokes cross.
            let px: Vec<u8> = (0..64).flat_map(|i| { let (x, y) = (i % 8, i / 8); if (x + y) % 2 == 0 { [235, 70 + 20 * y as u8, 60] } else { [40 + 25 * x as u8, 120, 230] } }).collect();
            if let Err(e) = s.set_brush_channel_bitmap(ChannelKind::BaseColor, "projection checker", subcraft_image::Bitmap::new_u8(8, 8, 3, px)) { state.set_status(e, now); return; }
            s.brush.material.camera_projection = true;
            s.brush.material.texture_tiling = false;
            s.brush.material.projection.scale = [1.4, 1.4];
            s.brush.settings.size = 22.0;
            for y in [250.0, 300.0, 350.0, 400.0] { stroke(s, StrokeMode::Paint, &line(260.0, y, 640.0, y)); }
        }
        "emission" => {
            // A new project: bake curvature, add emission through the same command as the "+ emiss"
            // chip, keep only that channel on a white fill and mask it with the Edges generator.
            use subcraft_core::{ChannelKind, MeshMapKind};
            let mesh = s.scene().mesh.clone();
            let bvh = subcraft_mesh::Bvh::build(&mesh);
            let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot, t.resolution)).collect();
            for (ts, slot, size) in sets {
                let _ = s.execute(Command::SelectTextureSet(ts));
                let input = subcraft_bake::BakeInput { low: subcraft_bake::Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot };
                let settings = subcraft_bake::BakeSettings { size, maps: vec![MeshMapKind::Curvature], ..Default::default() };
                if let Ok(maps) = subcraft_bake::bake(&input, &settings, &subcraft_bake::Progress::new()) { s.set_mesh_maps(ts, maps); }
                let _ = s.execute(Command::AddLayer(NewLayer::Fill));
                let Some(layer) = s.selection().layer else { continue };
                let _ = s.execute(Command::AddFillChannel { layer, channel: ChannelKind::Emissive });
                for kind in s.channels() {
                    if kind != ChannelKind::Emissive {
                        let _ = s.execute(Command::ToggleFillChannel { layer, effect: None, channel: kind, solo: false });
                    }
                }
                let _ = s.execute(Command::AddMask { layer, white: false });
                let mut g = subcraft_doc::Generator::new(subcraft_doc::GeneratorKind::Edges);
                g.level = 0.55;
                let _ = s.execute(Command::AddMaskEffect { layer, kind: subcraft_doc::MaskEffectKind::Generator(g) });
                // Show the layer's own Properties (Channels), not the mask effect just added.
                let _ = s.execute(Command::SelectEffect { layer, target: subcraft_app::LayerTarget::Content, effect: None });
            }
        }
        "cards" => {
            // A fill layer with every channel on, Properties showing its channel blocks.
            use subcraft_core::ChannelKind;
            let _ = s.execute(Command::AddLayer(NewLayer::Fill));
            if let Some(layer) = s.selection().layer {
                let _ = s.execute(Command::AddFillChannel { layer, channel: ChannelKind::Emissive });
                let _ = s.execute(Command::SelectEffect { layer, target: subcraft_app::LayerTarget::Content, effect: None });
            }
        }
        _ => state.set_status(format!("--capture-tool expects clone, smudge, projection, emission or cards, not {which}"), now),
    }
}

fn build_demo(state: &mut UiState) {
    let s = &mut state.session;
    let run = |s: &mut Session, c: Command| {
        if let Err(e) = s.execute(c) {
            log::warn!("demo: {e}");
        }
    };
    let paint = s.selection().layer.expect("new sessions select a paint layer");
    // Base material (below the paint layer).
    run(s, Command::AddLayer(NewLayer::Fill));
    let base = s.selection().layer.unwrap();
    run(s, Command::MoveDown(base));
    let wood = FillSource::Procedural {
        params: ProceduralParams { scale: 3.0, ..ProceduralParams::new(ProceduralKind::WoodRings) },
        low: ChannelValue::rgb(0.18, 0.09, 0.04),
        high: ChannelValue::rgb(0.45, 0.25, 0.11),
    };
    run(s, Command::SetFillChannel { layer: base, channel: ChannelKind::BaseColor, fill: FillChannel::new(wood) });
    let rough = FillSource::Procedural {
        params: ProceduralParams { scale: 8.0, ..ProceduralParams::new(ProceduralKind::Grunge) },
        low: ChannelValue::scalar(0.45),
        high: ChannelValue::scalar(0.8),
    };
    run(s, Command::SetFillChannel { layer: base, channel: ChannelKind::Roughness, fill: FillChannel::new(rough) });
    run(s, Command::Rename { layer: base, name: "Wood".into() });
    // Moss on top-facing surfaces.
    run(s, Command::SelectLayer(Some(paint)));
    run(s, Command::AddLayer(NewLayer::Fill));
    let moss = s.selection().layer.unwrap();
    run(s, Command::Rename { layer: moss, name: "Moss".into() });
    let moss_color = FillSource::Procedural {
        params: ProceduralParams { scale: 6.0, ..ProceduralParams::new(ProceduralKind::Clouds) },
        low: ChannelValue::rgb(0.06, 0.12, 0.03),
        high: ChannelValue::rgb(0.2, 0.35, 0.08),
    };
    run(s, Command::SetFillChannel { layer: moss, channel: ChannelKind::BaseColor, fill: FillChannel::new(moss_color) });
    run(s, Command::SetFillChannel { layer: moss, channel: ChannelKind::Roughness, fill: FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.9))) });
    if let Some(LayerContent::Fill(f)) = s.selected_layer().map(|l| l.content.clone()) {
        for (k, ch) in f.channels {
            if !matches!(k, ChannelKind::BaseColor | ChannelKind::Roughness) && ch.enabled {
                run(s, Command::SetFillChannel { layer: moss, channel: k, fill: FillChannel { enabled: false, ..ch } });
            }
        }
    }
    run(s, Command::AddMask { layer: moss, white: false });
    let mut g = Generator::new(GeneratorKind::Directional);
    g.level = 0.85;
    g.softness = 0.2;
    g.noise_amount = 0.35;
    run(s, Command::AddMaskEffect { layer: moss, kind: MaskEffectKind::Generator(g) });
    // Painted strokes on the paint layer (now topmost).
    run(s, Command::SelectLayer(Some(paint)));
    run(s, Command::MoveUp(paint));
    s.brush.settings.size = 4.0;
    s.brush.material = subcraft_paint::BrushMaterial::with(&[
        (ChannelKind::BaseColor, ChannelValue::rgb(0.75, 0.12, 0.03)),
        (ChannelKind::Roughness, ChannelValue::scalar(0.35)),
        (ChannelKind::Metallic, ChannelValue::scalar(0.0)),
    ]);
    let cam = state.camera.paint_camera([1280, 800]);
    let center = state.session.scene().mesh.bounds.center();
    let anchor = cam.project(center).map(|p| glam::Vec2::new(p.x, p.y)).unwrap_or(glam::Vec2::new(640.0, 400.0));
    let s = &mut state.session;
    for row in 0..2 {
        if s.begin_stroke().is_err() {
            break;
        }
        for k in 0..60 {
            let t = k as f32 / 59.0;
            let p = anchor + glam::Vec2::new(-260.0 + 420.0 * t, 40.0 + row as f32 * 60.0 + (t * 12.0).sin() * 18.0);
            let _ = s.stroke_to(&cam, p, false);
        }
        s.end_stroke();
    }
}
    #[test]
    fn save_waits_for_pending_shelf_image_and_embeds_the_result() {
        let file=std::env::temp_dir().join(format!("subcraft-image-save-{}.subcraft",subcraft_core::ObjectId::new()));
        let mut session=Session::new(subcraft_mesh::primitives::cube(2.0),128);session.save(Some(&file)).unwrap();
        let mut app=SubcraftApp {state:UiState::new(session,None),dock:default_layout(),frames:0,screenshot:None,screenshot_requested:false,pending_now:None,active_tabs:HashSet::new(),inspector_selection:None,test_crash:false,profile:None};
        app.state.asset_preview.queue(&app.state.session,0,None);app.perform(Action::Save,0.0);assert!(app.state.pen_after.is_some());
        let ctx=egui::Context::default();
        // The worker needs real scheduling time under parallel workspace tests; a fixed
        // number of fast headless frames can finish before image decoding gets a turn.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let mut out=ctx.run_ui(egui::RawInput::default(),|ui|crate::asset_preview::show(&mut app.state,ui.ctx()));out.textures_delta.clear();
            app.flush_pen_after(0.1);if !app.state.asset_preview.is_applying(){break;}
            assert!(std::time::Instant::now() < deadline, "pending shelf image did not finish within 10 seconds");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!app.state.asset_preview.is_applying());assert!(app.state.pen_after.is_none());assert!(!app.state.session.is_modified());
        let reopened=Session::open_project(&file).unwrap();assert_eq!(reopened.document(),app.state.session.document());assert!(reopened.resources().iter().any(|r|r.name.starts_with("Starter: ")));std::fs::remove_file(file).unwrap();
    }
