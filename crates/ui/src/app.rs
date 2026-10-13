//! The eframe application: menu bar, dockable panels, status bar and keyboard shortcuts.

use std::path::PathBuf;

use egui::{RichText, Ui};
use egui_dock::{DockArea, DockState, NodeIndex, Style};
use serde::{Deserialize, Serialize};
use subcraft_app::{ACTIONS, Action, Command, Key, NewLayer, Session, Shortcut, action_for};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, LayerContent, MaskEffectKind, ProceduralKind, ProceduralParams};

use crate::dialogs::{self, Pending};
use crate::orbit::OrbitCamera;
use crate::panels;
use crate::state::{Tool, UiState};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Viewport,
    UvView,
    Layers,
    Properties,
    Brush,
    TextureSets,
    History,
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
            Tab::History => "History",
        }
    }
}

/// Command-line options.
#[derive(Clone, Debug, Default)]
pub struct LaunchOptions {
    /// Project (`.subcraft`) or mesh to open instead of the sample crate.
    pub mesh: Option<PathBuf>,
    /// Build a small demo layer stack (fills, masks and painted strokes) on start.
    pub demo: bool,
    /// Save a screenshot of the window to this path after a few frames, then quit.
    pub screenshot: Option<PathBuf>,
    /// Start with the 2D (UV) view in front.
    pub view_2d: bool,
}

pub fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport, Tab::UvView]);
    let surface = dock.main_surface_mut();
    let [center, right] = surface.split_right(NodeIndex::root(), 0.73, vec![Tab::Layers]);
    surface.split_below(right, 0.5, vec![Tab::Properties, Tab::Brush]);
    let [_, left] = surface.split_left(center, 0.2, vec![Tab::TextureSets]);
    surface.split_below(left, 0.4, vec![Tab::History]);
    dock
}

const DOCK_KEY: &str = "subcraft_dock_v1";

pub struct SubcraftApp {
    state: UiState,
    dock: DockState<Tab>,
    frames: u64,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    /// Action approved this frame, run after the UI pass.
    pending_now: Option<Pending>,
}

impl SubcraftApp {
    pub fn new(cc: &eframe::CreationContext<'_>, options: LaunchOptions) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut status = None;
        let is_project = |p: &std::path::Path| p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(subcraft_app::PROJECT_EXTENSION));
        let session = match &options.mesh {
            Some(path) if is_project(path) => match Session::open_project(path) {
                Ok(s) => s,
                Err(e) => {
                    status = Some(format!("Could not open {}: {e}. Showing the sample.", path.display()));
                    Session::sample()
                }
            },
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
        };
        let mut state = UiState::new(session, cc.wgpu_render_state.clone());
        if let Some(s) = status {
            state.set_status(s, 0.0);
        }
        if options.demo {
            build_demo(&mut state);
        }
        let dock = cc
            .storage
            .and_then(|s| s.get_string(DOCK_KEY))
            .and_then(|j| serde_json::from_str(&j).ok())
            .filter(|_| options.screenshot.is_none())
            .unwrap_or_else(default_layout);
        let mut dock = dock;
        if options.view_2d
            && let Some(path) = dock.find_tab(&Tab::UvView)
        {
            let _ = dock.set_active_tab(path);
        }
        Self { state, dock, frames: 0, screenshot: options.screenshot, screenshot_requested: false, pending_now: None }
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
        match action {
            Action::ResetCamera => self.state.camera = OrbitCamera::framing(&self.state.session.scene().mesh.bounds),
            Action::BrushTool => self.state.tool = Tool::Brush,
            Action::EraserTool => self.state.tool = Tool::Eraser,
            Action::Save => {
                dialogs::save(&mut self.state, false, now);
            }
            Action::SaveAs => {
                dialogs::save(&mut self.state, true, now);
            }
            Action::OpenProject => self.request(Pending::OpenProject, now),
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

    fn menu(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                let item = |ui: &mut Ui, label: &str, sc: &str| ui.add(egui::Button::new(label).shortcut_text(sc)).clicked();
                if ui.button("New (sample crate)").clicked() {
                    self.request(Pending::NewSample, now);
                    ui.close();
                }
                if ui.button("New from mesh…").clicked() {
                    ui.close();
                    self.request(Pending::OpenMesh, now);
                }
                if item(ui, "Open project…", "Ctrl+O") {
                    ui.close();
                    self.request(Pending::OpenProject, now);
                }
                ui.separator();
                if item(ui, "Save", "Ctrl+S") {
                    ui.close();
                    self.perform(Action::Save, now);
                }
                if item(ui, "Save as…", "Ctrl+Shift+S") {
                    ui.close();
                    self.perform(Action::SaveAs, now);
                }
                ui.separator();
                if item(ui, "Export textures…", "Ctrl+Shift+E") {
                    ui.close();
                    self.perform(Action::Export, now);
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let h = self.state.session.history();
                let undo = h.undo_label().map(|l| format!("Undo {l}")).unwrap_or("Undo".into());
                let redo = h.redo_label().map(|l| format!("Redo {l}")).unwrap_or("Redo".into());
                let (can_undo, can_redo) = (h.can_undo(), h.can_redo());
                if ui.add_enabled(can_undo, egui::Button::new(undo).shortcut_text("Ctrl+Z")).clicked() {
                    self.perform(Action::Undo, now);
                }
                if ui.add_enabled(can_redo, egui::Button::new(redo).shortcut_text("Ctrl+Y")).clicked() {
                    self.perform(Action::Redo, now);
                }
            });
            ui.menu_button("Layer", |ui| {
                for a in [Action::NewPaintLayer, Action::NewFillLayer, Action::NewFolder, Action::DuplicateLayer, Action::DeleteLayer] {
                    let info = subcraft_app::actions::info(a);
                    let sc = info.shortcuts.first().map(|s| s.label()).unwrap_or_default();
                    if ui.add(egui::Button::new(info.label).shortcut_text(sc)).clicked() {
                        self.perform(a, now);
                        ui.close();
                    }
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Reset camera").clicked() {
                    self.perform(Action::ResetCamera, now);
                    ui.close();
                }
                if ui.button("Reset layout").clicked() {
                    self.dock = default_layout();
                    ui.close();
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Height bump");
                    ui.add(egui::Slider::new(&mut self.state.height_scale, 0.0..=0.1));
                });
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard shortcuts").clicked() {
                    self.state.show_shortcuts = true;
                    ui.close();
                }
            });
        });
    }

    /// Run `action` now, or after the unsaved-changes prompt.
    fn request(&mut self, action: Pending, now: f64) {
        if self.state.session.is_modified() && self.screenshot.is_none() {
            self.state.confirm = Some(action);
        } else {
            self.pending_now = Some(action);
        }
        let _ = now;
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        ui.horizontal(|ui| {
            let tool = match self.state.tool {
                Tool::Brush => "Brush",
                Tool::Eraser => "Eraser",
                Tool::PolygonFill => "Polygon fill",
            };
            ui.label(RichText::new(tool).color(theme::ACCENT));
            ui.separator();
            ui.label(format!("Size {:.1}%", self.state.session.brush.settings.size));
            ui.separator();
            if let Some(l) = self.state.session.selected_layer() {
                ui.label(format!("{} / {}", self.state.session.selected_texture_set().name, l.name));
            }
            if let Some((msg, t)) = &self.state.status
                && now - t < 6.0
            {
                ui.separator();
                ui.label(RichText::new(msg).color(egui::Color32::from_rgb(240, 200, 120)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("render {:.1} ms", self.state.frame_time_ms)).weak());
            });
        });
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let mut open = self.state.show_shortcuts;
        egui::Window::new("Keyboard shortcuts").open(&mut open).resizable(false).show(ctx, |ui| {
            egui::Grid::new("shortcuts").striped(true).show(ui, |ui| {
                for a in ACTIONS {
                    ui.label(a.label);
                    ui.label(a.shortcuts.iter().map(|s| s.label()).collect::<Vec<_>>().join(" / "));
                    ui.end_row();
                }
                for (what, how) in [("Orbit", "Alt+LMB or RMB drag"), ("Pan", "MMB drag"), ("Zoom", "Wheel or Alt+RMB drag"), ("Straight line", "Shift+click")] {
                    ui.label(what);
                    ui.label(how);
                    ui.end_row();
                }
            });
        });
        self.state.show_shortcuts = open;
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        ctx.request_repaint();
        if !self.screenshot_requested && self.frames >= 20 {
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

fn map_key(k: egui::Key) -> Option<Key> {
    Some(match k {
        egui::Key::B => Key::B,
        egui::Key::D => Key::D,
        egui::Key::E => Key::E,
        egui::Key::F => Key::F,
        egui::Key::G => Key::G,
        egui::Key::N => Key::N,
        egui::Key::O => Key::O,
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
}

impl egui_dock::TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::Viewport => crate::viewport::show(self.state, ui),
            Tab::UvView => panels::uv_view::show(self.state, ui),
            Tab::Layers => panels::layers::show(self.state, ui),
            Tab::Properties => panels::properties::show(self.state, ui),
            Tab::Brush => panels::brush::show(self.state, ui),
            Tab::TextureSets => panels::texture_sets::show(self.state, ui),
            Tab::History => panels::history::show(self.state, ui),
        }
    }

    fn is_closeable(&self, _tab: &Tab) -> bool {
        false
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Viewport | Tab::UvView)
    }
}

impl eframe::App for SubcraftApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        self.shortcuts(&ctx);
        if ctx.input(|i| i.pointer.any_released()) {
            let _ = self.state.session.execute(Command::Seal);
        }
        self.state.sync_gpu();

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        let style = Style::from_egui(ui.style());
        DockArea::new(&mut self.dock)
            .style(style)
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_inside(ui, &mut Viewer { state: &mut self.state });
        self.shortcuts_window(&ctx);
        let now = ctx.input(|i| i.time);
        dialogs::export_window(&mut self.state, &ctx, now);
        if let Some(action) = dialogs::unsaved_prompt(&mut self.state, &ctx, now) {
            self.pending_now = Some(action);
        }
        // Closing with unsaved work asks first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.state.allow_close && self.state.session.is_modified() && self.screenshot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.state.confirm = Some(Pending::Close);
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
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.screenshot.is_some() {
            return; // scripted runs never overwrite the user's layout
        }
        if let Ok(j) = serde_json::to_string(&self.dock) {
            storage.set_string(DOCK_KEY, j);
        }
    }
}

/// Demo stack on the sample crate: procedural base, an upward-facing "moss" fill masked by a
/// directional generator, and a few painted strokes.
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
    run(s, Command::SetFillChannel { layer: base, channel: ChannelKind::BaseColor, fill: FillChannel { enabled: true, source: wood } });
    let rough = FillSource::Procedural {
        params: ProceduralParams { scale: 8.0, ..ProceduralParams::new(ProceduralKind::Grunge) },
        low: ChannelValue::scalar(0.45),
        high: ChannelValue::scalar(0.8),
    };
    run(s, Command::SetFillChannel { layer: base, channel: ChannelKind::Roughness, fill: FillChannel { enabled: true, source: rough } });
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
    run(s, Command::SetFillChannel { layer: moss, channel: ChannelKind::BaseColor, fill: FillChannel { enabled: true, source: moss_color } });
    run(s, Command::SetFillChannel { layer: moss, channel: ChannelKind::Roughness, fill: FillChannel { enabled: true, source: FillSource::uniform(ChannelValue::scalar(0.9)) } });
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
