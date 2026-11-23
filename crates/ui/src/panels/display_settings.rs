//! Display Settings panel: every setting of how the 3D view shows the project, in one dock
//! panel grouped like Substance 3D Painter's (Environment, Camera, Post effects, Render,
//! Visualization) with a row of jump buttons on top. The values live in
//! [`subcraft_doc::DisplaySettings`] and are saved with the project.

use crate::i18n::tr;
use egui::{RichText, Ui};
use subcraft_render::{GlareMode, ToneMap};

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};

/// The groups of the panel, in Painter's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Environment,
    Camera,
    PostEffects,
    Render,
    Visualization,
}

impl Section {
    pub const ALL: [Section; 5] = [Section::Environment, Section::Camera, Section::PostEffects, Section::Render, Section::Visualization];

    pub fn label(self) -> &'static str {
        match self {
            Section::Environment => "Environment settings",
            Section::Camera => "Camera settings",
            Section::PostEffects => "Post effects",
            Section::Render => "Render settings",
            Section::Visualization => "Visualization settings",
        }
    }

    /// Heading of the section, translated.
    fn title(self) -> &'static str {
        match self {
            Section::Environment => tr("Environment settings"),
            Section::Camera => tr("Camera settings"),
            Section::PostEffects => tr("Post effects"),
            Section::Render => tr("Render settings"),
            Section::Visualization => tr("Visualization settings"),
        }
    }

    /// Jump-button text, translated.
    fn button(self) -> &'static str {
        match self {
            Section::Environment => tr("Environment"),
            Section::Camera => tr("Camera"),
            Section::PostEffects => tr("Post effects"),
            Section::Render => tr("Render"),
            Section::Visualization => tr("Visualization"),
        }
    }

    /// Short English name: the `--display-settings <group>` launch option matches it.
    pub fn short(self) -> &'static str {
        match self {
            Section::Environment => "Environment",
            Section::Camera => "Camera",
            Section::PostEffects => "Post effects",
            Section::Render => "Render",
            Section::Visualization => "Visualization",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Section::Environment => "display_environment",
            Section::Camera => "display_camera",
            Section::PostEffects => "display_post",
            Section::Render => "display_render",
            Section::Visualization => "display_visualization",
        }
    }
}

/// Keep the project's copy of the display settings and the live ones in step. A project that
/// carries display settings replaces the live ones when it is opened; a project without them
/// keeps the user's current ones and only receives a copy once the user changes something, so
/// merely opening a project never marks it as modified.
///
/// The environment choice and the camera's field of view live outside the settings struct (the
/// Shelf selects the map, navigation and camera bookmarks drive the lens): they are copied into
/// it every frame, and out of it when a project brings its own.
pub fn sync_project(state: &mut UiState) {
    if !std::mem::replace(&mut state.display_loaded, true) {
        if let Some(saved) = state.session.settings().display.clone() {
            if let Some(index) = saved.env_map.as_deref().and_then(|name| state.library.environments.iter().position(|e| e.name == name)) {
                state.env.index = index;
                state.env.rotation_deg = crate::panels::shelf::wrap_rotation(saved.env_rotation);
            }
            state.recovery.prefs.viewport.display = saved;
        }
        // A new session frames the model with the default lens: put the saved one back.
        let fov = state.recovery.prefs.viewport.fov;
        set_fov(&mut state.camera, fov);
        capture_live(state);
        state.display_baseline = state.recovery.prefs.viewport.display.clone();
    }
    capture_live(state);
    let live = &state.recovery.prefs.viewport.display;
    if *live != state.display_baseline {
        state.display_baseline = live.clone();
        let mut settings = state.session.settings().clone();
        settings.display = Some(live.clone());
        state.session.set_settings(settings);
    }
}

/// Copy the environment selection and the lens into the display settings.
fn capture_live(state: &mut UiState) {
    let display = &mut state.recovery.prefs.viewport.display;
    let name = state.library.environments.get(state.env.index).map(|e| e.name.as_str());
    if display.env_map.as_deref() != name {
        display.env_map = name.map(str::to_string);
    }
    display.env_rotation = state.env.rotation_deg;
    display.fov = state.camera.fov_y_deg;
}

/// Change the lens and move the camera along its axis so the orbit target keeps its size on
/// screen (a wider lens steps closer), the way a dolly zoom does.
pub fn set_fov(camera: &mut crate::orbit::OrbitCamera, fov_deg: f32) {
    let fov = fov_deg.clamp(*subcraft_doc::display::FOV_RANGE.start(), *subcraft_doc::display::FOV_RANGE.end());
    if !fov.is_finite() || (fov - camera.fov_y_deg).abs() < 1e-4 {
        return;
    }
    let half = |deg: f32| (deg.to_radians() * 0.5).tan();
    camera.distance *= half(camera.fov_y_deg) / half(fov);
    camera.fov_y_deg = fov;
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        jump_buttons(state, ui);
        ui.horizontal_wrapped(|ui| {
            ui.menu_button(format!("{}  {}", crate::icons::UNDO, tr("Reset all")), |ui| {
                ui.set_max_width(260.0);
                ui.label(RichText::new(tr("Put every display setting back to its default. The environment map and the colour LUT stay.")).color(TEXT_DIM()));
                if ui.button(tr("Reset all display settings")).clicked() {
                    reset_all(state);
                    ui.close();
                }
            }).response.on_hover_text(tr("Put every setting of this panel back to its default"));
            ui.label(RichText::new(tr("Ctrl+click a setting or right-click its name to reset it; a dot marks the changed ones.")).small().color(TEXT_DIM()));
        });
        ui.add_space(4.0);
        let channels: Vec<_> = state.session.selected_texture_set().channel_kinds().collect();
        {
            let mask = crate::mask_view::dropdown_state(state);
            let request = ui.horizontal(|ui| crate::viewport_bar::view_mode(ui, &mut state.recovery.prefs.viewport, &channels, true, mask)).inner;
            if let Some(show) = request { crate::mask_view::apply_dropdown(state, show); }
        }
        let jump = state.display_jump.take();
        for section in Section::ALL {
            if jump == Some(section) {
                // Open the group and bring its header to the top of the panel.
                let id = ui.make_persistent_id(("section", section.id()));
                let mut open = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
                open.set_open(true);
                open.store(ui.ctx());
                ui.scroll_to_cursor(Some(egui::Align::TOP));
            }
            crate::widgets::section(ui, section.id(), section.title(), true, |ui| match section {
                Section::Environment => environment(state, ui),
                Section::Camera => camera(state, ui),
                Section::PostEffects => post_effects(state, ui),
                Section::Render => render(state, ui),
                Section::Visualization => visualization(state, ui),
            });
        }
    });
}

fn jump_buttons(state: &mut UiState, ui: &mut Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for section in Section::ALL {
            if ui.add(egui::Button::new(RichText::new(section.button()).font(theme::font(11.5)))).on_hover_text(section.title()).clicked() {
                state.display_jump = Some(section);
            }
        }
    });
}

/// Put every setting of the panel back to its default. The environment map and the colour LUT
/// are choices of an asset rather than settings, so they stay; so do the view mode, the camera
/// bookmarks and the brush / symmetry options the panel does not show.
pub fn reset_all(state: &mut UiState) {
    let prefs = &mut state.recovery.prefs.viewport;
    let mut fresh = crate::viewport_bar::ViewportPrefs::default();
    fresh.view_mode = prefs.view_mode;
    fresh.mesh_map = prefs.mesh_map;
    fresh.solo = prefs.solo;
    fresh.path_tracing = prefs.path_tracing;
    fresh.symmetry_display = std::mem::take(&mut prefs.symmetry_display);
    fresh.pressure_curve = std::mem::take(&mut prefs.pressure_curve);
    fresh.camera_bookmarks = std::mem::take(&mut prefs.camera_bookmarks);
    fresh.display.env_map = prefs.display.env_map.take();
    fresh.display.color_lut = prefs.display.color_lut;
    *prefs = fresh;
    state.env.rotation_deg = default_env_rotation(state);
    set_fov(&mut state.camera, DEFAULTS.fov);
    state.stencil_display = Default::default();
}

/// The environment rotation a reset goes back to: the startup environment is turned so its sun
/// lights the model from the front; any other map starts unturned.
fn default_env_rotation(state: &UiState) -> f32 {
    use crate::panels::shelf::{DEFAULT_ENVIRONMENT, DEFAULT_ENVIRONMENT_ROTATION_DEG};
    let startup = state.library.environments.get(state.env.index).is_some_and(|e| e.name == DEFAULT_ENVIRONMENT);
    if startup { DEFAULT_ENVIRONMENT_ROTATION_DEG } else { DEFAULTS.env_rotation }
}

static DEFAULTS: std::sync::LazyLock<subcraft_doc::DisplaySettings> = std::sync::LazyLock::new(Default::default);
use crate::viewport_bar::DEFAULTS as VIEW_DEFAULTS;

use crate::reset_row::label;

/// A small heading inside a section; `text` is already translated.
fn sub_header(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).color(TEXT_DIM()));
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([10.0, 6.0]).show(ui, body);
}

/// Size and columns of the environment mini-shelf's tiles.
const ENV_TILE: f32 = 72.0;
const ENV_SHELF_COLUMNS: usize = 4;

/// Environment map and how it lights the model, in Painter's order: map, opacity, exposure,
/// rotation, blur, alignment.
pub fn environment(state: &mut UiState, ui: &mut Ui) {
    grid(ui, "display_env", |ui| {
        ui.label(tr("Environment map"));
        let current = state.library.environments.get(state.env.index).map_or_else(|| "-".to_string(), |e| e.name.clone());
        // Painter's mini-shelf: the environments as thumbnails, one click to choose.
        let combo = ui.menu_button(format!("{current}  {}", crate::icons::CARET_DOWN), |ui| {
            state.shelf.want_environment_thumbs = true;
            // Thumbnails render here too: the Assets panel may be hidden or show another type.
            crate::panels::shelf::generate_one_thumbnail(state, ui.ctx());
            ui.set_max_width(ENV_SHELF_COLUMNS as f32 * (ENV_TILE + 8.0));
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, env) in state.library.environments.iter().enumerate() {
                        let thumb = crate::panels::shelf::thumbnail(&state.shelf, &env.id);
                        if crate::panels::shelf::tile(ui, ENV_TILE, thumb, &env.name, i == state.env.index, &env.name).clicked() {
                            state.env.choose(&state.library, i);
                            ui.close();
                        }
                    }
                });
            });
        }).response.on_hover_text(tr("The lighting around the model. Drop a map from Assets > Environments here or on the 3D view."));
        if let Some(env) = combo.dnd_release_payload::<crate::panels::shelf::EnvironmentDrag>()
            && env.0 < state.library.environments.len()
        {
            state.env.choose(&state.library, env.0);
        }
        ui.end_row();
        let rotation = default_env_rotation(state);
        let display = &mut state.recovery.prefs.viewport.display;
        let def = &*DEFAULTS;
        let l = label(ui, tr("Environment opacity"));
        let r = ui.add(egui::Slider::new(&mut display.env_opacity, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}", v * 100.0)).custom_parser(|s| s.trim().parse::<f64>().ok().map(|v| v / 100.0)).suffix("%"))
            .on_hover_text(tr("How much of the environment map shows behind the model. 0 is the plain background."));
        crate::reset_row::reset(ui, &l, &r, &mut display.env_opacity, &def.env_opacity);
        ui.end_row();
        let l = label(ui, tr("Environment exposure (EV)"));
        let r = ui.add(egui::Slider::new(&mut display.env_exposure, -5.0..=5.0).fixed_decimals(2)).on_hover_text(tr("Brightness of the environment in stops: +1 doubles the light, -1 halves it"));
        crate::reset_row::reset(ui, &l, &r, &mut display.env_exposure, &def.env_exposure);
        ui.end_row();
        let l = label(ui, tr("Environment rotation"));
        // One decimal: a slider rounds its value to the decimals it shows, and with none it turned
        // the startup map's -165.6° into -166° on the first frame, which showed the "changed" dot.
        let r = ui.add(egui::Slider::new(&mut state.env.rotation_deg, -180.0..=180.0).suffix("°").min_decimals(0).max_decimals(1)).on_hover_text(tr("Turn the lighting around the model (also Shift + right mouse drag in the 3D view)"));
        if crate::reset_row::wants_reset(ui, &l, &r, (state.env.rotation_deg - rotation).abs() > 0.05) {
            state.env.rotation_deg = rotation;
        }
        ui.end_row();
        let l = label(ui, tr("Environment blur"));
        let r = ui.add_enabled(display.env_opacity > 0.0, egui::Slider::new(&mut display.env_blur, 0.0..=1.0)).on_hover_text(tr("Blur of the environment shown behind the model"));
        crate::reset_row::reset(ui, &l, &r, &mut display.env_blur, &def.env_blur);
        ui.end_row();
        let l = label(ui, tr("Environment alignment"));
        let r = egui::ComboBox::from_id_salt("display_env_alignment").selected_text(tr(display.env_alignment.label())).show_ui(ui, |ui| {
            for a in subcraft_doc::EnvAlignment::ALL {
                ui.selectable_value(&mut display.env_alignment, a, tr(a.label()));
            }
        }).response.on_hover_text(tr("World: the environment stays put while the camera orbits. Local: it turns with the camera, so the lighting follows the view."));
        crate::reset_row::reset(ui, &l, &r, &mut display.env_alignment, &def.env_alignment);
        ui.end_row();
    });
    // Painter's Environment settings > Shadows: cast by the key light, in the raster view.
    let display = &mut state.recovery.prefs.viewport.display;
    let def = &*DEFAULTS;
    sub_header(ui, tr("Shadows"));
    grid(ui, "display_shadows", |ui| {
        let l = label(ui, tr("Enable"));
        let r = ui.checkbox(&mut display.shadows, "").on_hover_text(tr("The model casts the key light's shadow on itself in the 3D view (the Rendering mode always has shadows)"));
        crate::reset_row::reset(ui, &l, &r, &mut display.shadows, &def.shadows);
        ui.end_row();
        let l = label(ui, tr("Computation mode"));
        let r = ui.add_enabled_ui(display.shadows, |ui| {
            egui::ComboBox::from_id_salt("display_shadow_mode").selected_text(shadow_mode_label(display.shadow_mode)).show_ui(ui, |ui| {
                for m in subcraft_doc::ShadowMode::MENU {
                    ui.selectable_value(&mut display.shadow_mode, m, shadow_mode_label(m));
                }
            }).response.on_hover_text(tr("Intensive is sharper and slower, Lightweight softer and faster"))
        }).inner;
        crate::reset_row::reset(ui, &l, &r, &mut display.shadow_mode, &def.shadow_mode);
        ui.end_row();
        let l = label(ui, tr("Shadows opacity"));
        let r = ui.add_enabled(display.shadows, egui::Slider::new(&mut display.shadow_opacity, 0.0..=1.0)).on_hover_text(tr("How dark the shadows are: 1 blocks all of the key light"));
        crate::reset_row::reset(ui, &l, &r, &mut display.shadow_opacity, &def.shadow_opacity);
        ui.end_row();
    });
}

/// Painter's name of a shadow computation mode, translated.
fn shadow_mode_label(mode: subcraft_doc::ShadowMode) -> &'static str {
    match mode {
        subcraft_doc::ShadowMode::Lightweight => tr("Lightweight"),
        subcraft_doc::ShadowMode::Average => tr("Average"),
        subcraft_doc::ShadowMode::Intensive => tr("Intensive"),
    }
}

/// Projection, anti-aliasing and the lens (focus distance and aperture of the depth of field).
pub fn camera(state: &mut UiState, ui: &mut Ui) {
    // Painter's Camera Preset row, lock and Restore: only with cameras imported from the mesh.
    if !state.session.scene_cameras().is_empty() {
        grid(ui, "display_camera_preset", |ui| {
            ui.label(tr("Camera preset"));
            crate::view_extras::camera_dropdown(ui, state, "display_camera_preset_combo");
            ui.end_row();
            if let Some(i) = state.active_camera {
                ui.label(tr("Lock camera"));
                ui.checkbox(&mut state.camera_locked, "").on_hover_text(tr("Lock the camera: navigation leaves it where it is"));
                ui.end_row();
                ui.label(tr("Display ratio"));
                let ratio = state.session.scene_cameras().get(i).and_then(|c| c.aspect);
                ui.label(ratio.map_or_else(|| tr("Unspecified").to_string(), |r| format!("{r:.3}")));
                ui.end_row();
                ui.label("");
                if ui.add_enabled(crate::view_extras::camera_modified(state), egui::Button::new(tr("Restore")))
                    .on_hover_text(tr("Put the camera's field of view and projection back to the imported values"))
                    .clicked()
                {
                    crate::view_extras::restore_camera(state);
                }
                ui.end_row();
            }
        });
    }
    let mut pick = false;
    // Focus distances offered: from very close to well past the model.
    let scene = state.session.scene().mesh.bounds.diagonal().max(1e-3);
    // The project's colour LUTs (the grid below holds the preferences mutably).
    let lut_names = crate::color_lut::names(state.session.resources());
    let mut load_lut = false;
    {
        let camera = &mut state.camera;
        let prefs = &mut state.recovery.prefs.viewport;
        grid(ui, "display_camera_grid", |ui| {
            let def = &*DEFAULTS;
            let view = &*VIEW_DEFAULTS;
            let l = label(ui, tr("Projection"));
            let r = ui.horizontal(|ui| {
                ui.selectable_value(&mut prefs.orthographic, false, tr("Perspective")) | ui.selectable_value(&mut prefs.orthographic, true, tr("Orthographic"))
            }).inner;
            crate::reset_row::reset(ui, &l, &r, &mut prefs.orthographic, &view.orthographic);
            ui.end_row();
            let l = label(ui, tr("Camera rotation"));
            let r = ui.horizontal(|ui| {
                ui.selectable_value(&mut prefs.free_rotation, false, tr("Constrained")).on_hover_text(tr("Orbit around the world vertical; the horizon stays level"))
                    | ui.selectable_value(&mut prefs.free_rotation, true, tr("Free")).on_hover_text(tr("Orbit around the view's own axes: the camera can roll and go over the top"))
            }).inner;
            crate::reset_row::reset(ui, &l, &r, &mut prefs.free_rotation, &view.free_rotation);
            ui.end_row();
            let l = label(ui, tr("Field of view"));
            let mut fov = camera.fov_y_deg;
            let r = ui.add_enabled(!prefs.orthographic, egui::Slider::new(&mut fov, subcraft_doc::display::FOV_RANGE).suffix("°").fixed_decimals(1)).on_hover_text(tr("Vertical field of view. The camera moves so the model keeps its size."));
            if r.changed() {
                set_fov(camera, fov);
            }
            if crate::reset_row::wants_reset(ui, &l, &r, (camera.fov_y_deg - def.fov).abs() > 1e-3) {
                set_fov(camera, def.fov);
            }
            ui.end_row();
            let l = label(ui, tr("Focal length (mm)"));
            let (near, far) = (subcraft_doc::display::focal_length_mm(*subcraft_doc::display::FOV_RANGE.end()), subcraft_doc::display::focal_length_mm(*subcraft_doc::display::FOV_RANGE.start()));
            let mut focal = subcraft_doc::display::focal_length_mm(camera.fov_y_deg);
            let r = ui.add_enabled(!prefs.orthographic, egui::Slider::new(&mut focal, near..=far).logarithmic(true).fixed_decimals(1)).on_hover_text(tr("The same lens as a focal length on a full-frame (36 x 24 mm) sensor"));
            if r.changed() {
                set_fov(camera, subcraft_doc::display::fov_from_focal_length(focal));
            }
            if crate::reset_row::wants_reset(ui, &l, &r, (camera.fov_y_deg - def.fov).abs() > 1e-3) {
                set_fov(camera, def.fov);
            }
            ui.end_row();
            let l = label(ui, tr("Focus distance"));
            let mut focus = prefs.focus_distance(camera.eye_distance());
            let r = ui.add_enabled(prefs.dof, egui::Slider::new(&mut focus, scene * 0.01..=scene * 20.0).logarithmic(true)).on_hover_text(tr("Distance from the camera, in scene units, that is sharp; it stays put while zooming. Ctrl + middle click in the 3D view focuses on the point under the cursor. Needs Depth of field (Post effects)."));
            if r.changed() {
                prefs.dof_focus_distance = Some(focus);
            }
            crate::reset_row::reset(ui, &l, &r, &mut prefs.display.dof_focus_distance, &def.dof_focus_distance);
            ui.end_row();
            ui.label(tr("Focus point"));
            pick = ui.add_enabled(prefs.dof, egui::Button::new(tr("Pick in 3D view"))).on_hover_text(tr("Click the model in the 3D view to focus on that spot")).clicked();
            ui.end_row();
            let l = label(ui, tr("Auto-focus"));
            let r = ui.add_enabled(prefs.dof, egui::Checkbox::without_text(&mut prefs.display.dof_autofocus)).on_hover_text(tr("Keep the focus on whatever is at the centre of the 3D view"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.display.dof_autofocus, &def.dof_autofocus);
            ui.end_row();
            let l = label(ui, tr("Aperture"));
            let r = ui.add_enabled(prefs.dof, egui::Slider::new(&mut prefs.dof_amount, 0.0..=1.0)).on_hover_text(tr("Strength of the depth-of-field blur. Needs Depth of field (Post effects)."));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.display.dof_amount, &def.dof_amount);
            ui.end_row();
            let l = label(ui, tr("Anti-aliasing"));
            let r = ui.checkbox(&mut prefs.antialias, "").on_hover_text(tr("Anti-aliasing · smooth edges (4x MSAA); off is faster"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.display.antialias, &def.antialias);
            ui.end_row();
            let l = label(ui, tr("Temporal anti-aliasing"));
            let r = ui.checkbox(&mut prefs.display.taa, "").on_hover_text(tr("While the view is still, frames drawn with sub-pixel shifts are averaged into a smoother image"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.display.taa, &def.taa);
            ui.end_row();
            let l = label(ui, tr("Accumulations"));
            let r = ui.add_enabled_ui(prefs.display.taa, |ui| {
                egui::ComboBox::from_id_salt("taa_accumulations").selected_text(prefs.display.taa_accumulations.to_string()).show_ui(ui, |ui| {
                    for n in crate::viewport::TAA_ACCUMULATIONS {
                        ui.selectable_value(&mut prefs.display.taa_accumulations, n, n.to_string());
                    }
                }).response
            }).inner.on_hover_text(tr("Frames averaged before the image is final: 16 suits most views, 64 very contrasted ones"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.display.taa_accumulations, &def.taa_accumulations);
            ui.end_row();
            // Painter's Color profile: the material view raw (linear values, not sRGB) or
            // through the project's colour LUT.
            let l = label(ui, tr("Color profile"));
            let r = ui.checkbox(&mut prefs.display.color_profile, "").on_hover_text(tr("Show the material view as linear raw values instead of sRGB, or through the project's colour LUT"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.display.color_profile, &def.color_profile);
            ui.end_row();
            ui.label(tr("Color LUT"));
            ui.add_enabled_ui(prefs.display.color_profile, |ui| {
                ui.horizontal(|ui| {
                    let name = prefs.display.color_lut.and_then(|id| crate::color_lut::name_in(&lut_names, id)).unwrap_or_else(|| tr("None (linear raw)").to_string());
                    ui.label(name);
                    if ui.button(tr("Load…")).on_hover_text(tr("A 3D LUT image (EXR) in OpenColorIO's ociolutimage layout, such as 2048 x 128; it is saved with the project")).clicked() {
                        load_lut = true;
                    }
                    if prefs.display.color_lut.is_some() && ui.button(tr("Clear")).clicked() {
                        prefs.display.color_lut = None;
                    }
                });
            });
            ui.end_row();
            let l = label(ui, tr("White point"));
            let r = ui.add_enabled(prefs.display.color_profile, egui::Slider::new(&mut prefs.display.white_point, 0.1..=100.0).logarithmic(true))
                .on_hover_text(tr("The brightest value shown: values are divided by it and clamped before the colour LUT. Above 1 for very bright content such as the sun"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.display.white_point, &def.white_point);
            ui.end_row();
        });
    }
    if pick {
        state.picking_focus = true;
    }
    if load_lut {
        crate::color_lut::load_with_dialog(state, ui.input(|i| i.time));
    }
}

/// One entry of the post-effect stack: a collapsible row whose header carries the effect's own
/// on/off switch. The parameters are greyed out while the effect (or the whole stack) is off.
fn effect(ui: &mut Ui, id: &str, name: &str, on: &mut bool, on_default: bool, stack_on: bool, default_open: bool, body: impl FnOnce(&mut Ui)) {
    let header_id = ui.make_persistent_id(("display_effect", id));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), header_id, default_open)
        .show_header(ui, |ui| {
            let r = ui.add_enabled(stack_on, egui::Checkbox::new(on, RichText::new(name).color(theme::TEXT())));
            crate::reset_row::reset(ui, &r, &r, on, &on_default);
        })
        .body(|ui| {
            let enabled = *on && stack_on;
            ui.add_enabled_ui(enabled, |ui| grid(ui, id, body));
        });
}

/// A slider row of an effect's grid, resettable to `default`.
fn slider(ui: &mut Ui, text: &str, value: &mut f32, default: f32, range: std::ops::RangeInclusive<f32>, tip: &str) -> egui::Response {
    let l = label(ui, text);
    let r = ui.add(egui::Slider::new(value, range)).on_hover_text(tip);
    crate::reset_row::reset(ui, &l, &r, value, &default);
    ui.end_row();
    r
}

/// Painter's project-wide Subsurface scattering switch and Sample Count, below the post effects.
/// It is not one of them: the master switch leaves it alone.
fn subsurface(d: &mut subcraft_doc::DisplaySettings, def: &subcraft_doc::DisplaySettings, ui: &mut Ui) {
    sub_header(ui, tr("Subsurface scattering"));
    let r = ui.checkbox(&mut d.sss_on, tr("Subsurface scattering")).on_hover_text(tr("Light spreads under the surface of texture sets with a Scattering channel, softening shadows and tinting them with the scattering colour"));
    crate::reset_row::reset(ui, &r, &r, &mut d.sss_on, &def.sss_on);
    grid(ui, "display_sss", |ui| {
        let on = d.sss_on;
        let l = label(ui, tr("Radius"));
        let r = ui.add_enabled(on, egui::Slider::new(&mut d.sss_radius, 0.1..=10.0).suffix("%")).on_hover_text(tr("How far light scatters at full Scattering, in percent of the model size"));
        crate::reset_row::reset(ui, &l, &r, &mut d.sss_radius, &def.sss_radius);
        ui.end_row();
        let l = label(ui, tr("Sample count"));
        let r = ui.add_enabled_ui(on, |ui| {
            egui::ComboBox::from_id_salt("display_sss_samples").selected_text(d.sss_samples.to_string()).show_ui(ui, |ui| {
                for n in [8u32, 16, 32, 64] {
                    ui.selectable_value(&mut d.sss_samples, n, n.to_string());
                }
            }).response
        }).inner.on_hover_text(tr("Samples per pixel: more is smoother and slower"));
        crate::reset_row::reset(ui, &l, &r, &mut d.sss_samples, &def.sss_samples);
        ui.end_row();
    });
}

/// The post-effect stack: the master switch, then one entry per effect in the order they are
/// applied (Painter's list), then SubCraft's own extras. Also shown in the Render quality window.
pub fn post_effects(state: &mut UiState, ui: &mut Ui) {
    let mut pick = false;
    let d = &mut state.recovery.prefs.viewport.display;
    let def = &*DEFAULTS;
    ui.horizontal(|ui| {
        let r = ui.checkbox(&mut d.post_effects, tr("Activate Post Effects")).on_hover_text(tr("Master switch of every effect below. Off shows the plain render."));
        crate::reset_row::reset(ui, &r, &r, &mut d.post_effects, &def.post_effects);
        // At the top, so it stays in reach however many effects are expanded below.
        if ui.button(tr("Reset look")).on_hover_text(tr("Put every post effect back to its default")).clicked() {
            d.reset_post_effects();
        }
    });
    let all = d.post_effects;
    effect(ui, "display_dof", tr("Depth of field"), &mut d.dof, def.dof, all, true, |ui| {
        slider(ui, tr("Focus range"), &mut d.dof_range, def.dof_range, 0.0..=50.0, tr("Depth that stays sharp around the focus distance, in percent of the model size"));
        ui.label(tr("Focus point"));
        pick = ui.button(tr("Pick in 3D view")).on_hover_text(tr("Click the model in the 3D view to focus on that spot. Focus distance and Aperture are in the Camera settings.")).clicked();
        ui.end_row();
    });
    effect(ui, "display_bloom", tr("Bloom"), &mut d.bloom_on, def.bloom_on, all, true, |ui| {
        slider(ui, tr("Amount"), &mut d.bloom, def.bloom, 0.0..=2.0, tr("Glow around bright areas. 0 is off."));
        slider(ui, tr("Threshold"), &mut d.bloom_threshold, def.bloom_threshold, 0.0..=1.0, tr("Brightness above which an area glows (1 is screen white)"));
        slider(ui, tr("Falloff"), &mut d.bloom_falloff, def.bloom_falloff, 0.0..=1.0, tr("How far the glow spreads around the bright area"));
    });
    effect(ui, "display_glare", tr("Glare"), &mut d.glare_on, def.glare_on, all, true, |ui| {
        slider(ui, tr("Luminance"), &mut d.glare, def.glare, 0.0..=16.0, tr("Brightness of the glare thrown by emissive areas. 0 is off; 0.5 to 4 looks natural."));
        slider(ui, tr("Threshold"), &mut d.glare_threshold, def.glare_threshold, 0.0..=4.0, tr("Emission below this brightness throws no glare"));
        slider(ui, tr("Remap Factor"), &mut d.glare_remap, def.glare_remap, 0.25..=4.0, tr("Response to brightness above the threshold. 1 is neutral."));
        let l = label(ui, tr("Shape"));
        let r = egui::ComboBox::from_id_salt("display_glare_shape").selected_text(d.glare_mode.label()).width(170.0).height(400.0).show_ui(ui, |ui| {
            for m in GlareMode::MENU {
                ui.selectable_value(&mut d.glare_mode, m, m.label());
            }
        }).response;
        crate::reset_row::reset(ui, &l, &r, &mut d.glare_mode, &def.glare_mode);
        ui.end_row();
        slider(ui, tr("Streak length"), &mut d.glare_length, def.glare_length, 0.0..=1.0, tr("How long the streaks reach"));
    });
    effect(ui, "display_flare", tr("Lens flare"), &mut d.flare_on, def.flare_on, all, false, |ui| {
        let l = label(ui, tr("Camera"));
        let r = egui::ComboBox::from_id_salt("display_flare_camera").selected_text(tr(d.flare_camera.label())).show_ui(ui, |ui| {
            for c in subcraft_doc::FlareCamera::ALL {
                ui.selectable_value(&mut d.flare_camera, c, tr(c.label()));
            }
        }).response;
        crate::reset_row::reset(ui, &l, &r, &mut d.flare_camera, &def.flare_camera);
        ui.end_row();
        slider(ui, tr("Amount"), &mut d.flare_amount, def.flare_amount, 0.0..=4.0, tr("Brightness of the ghost images. May exceed 1."));
        slider(ui, tr("Threshold"), &mut d.flare_threshold, def.flare_threshold, 0.0..=1.0, tr("Brightness above which an area throws ghosts"));
        let l = label(ui, tr("Unique ghosts"));
        let r = ui.add(egui::Slider::new(&mut d.flare_ghosts, 1..=8)).on_hover_text(tr("Number of ghost images"));
        crate::reset_row::reset(ui, &l, &r, &mut d.flare_ghosts, &def.flare_ghosts);
        ui.end_row();
        slider(ui, tr("Ghost position scale"), &mut d.flare_position_scale, def.flare_position_scale, 0.1..=2.0, tr("Spacing of the ghosts along the line through the view centre"));
    });
    effect(ui, "display_aberration", tr("Lateral aberration"), &mut d.aberration_on, def.aberration_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.aberration_amount, def.aberration_amount, 0.0..=2.0, tr("Strength of the color fringes"));
        slider(ui, tr("Radius"), &mut d.aberration_radius, def.aberration_radius, 0.0..=1.0, tr("Distance from the centre where the fringes start"));
        slider(ui, tr("Softness"), &mut d.aberration_softness, def.aberration_softness, 0.0..=1.0, tr("Width of the transition around the radius"));
        slider(ui, tr("Ratio"), &mut d.aberration_ratio, def.aberration_ratio, 0.25..=4.0, tr("Shape of the clean zone. 1 is circular."));
        slider(ui, tr("Center X"), &mut d.aberration_center[0], def.aberration_center[0], 0.0..=1.0, tr("Centre of the effect across the view"));
        slider(ui, tr("Center Y"), &mut d.aberration_center[1], def.aberration_center[1], 0.0..=1.0, tr("Centre of the effect down the view"));
        let l = label(ui, tr("Achromatic aberrations"));
        let r = ui.checkbox(&mut d.aberration_achromatic, "").on_hover_text(tr("Blur toward the edges instead of splitting the colors"));
        crate::reset_row::reset(ui, &l, &r, &mut d.aberration_achromatic, &def.aberration_achromatic);
        ui.end_row();
    });
    effect(ui, "display_vignette", tr("Vignette"), &mut d.vignette_on, def.vignette_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.vignette_amount, def.vignette_amount, 0.0..=1.0, tr("How dark the corners get"));
    });
    effect(ui, "display_lens", tr("Lens distortion"), &mut d.lens_on, def.lens_on, all, false, |ui| {
        slider(ui, tr("Power"), &mut d.lens_power, def.lens_power, -1.0..=1.0, tr("Positive bends the picture like a wide lens, negative like a telephoto"));
        slider(ui, tr("FOV"), &mut d.lens_fov, def.lens_fov, 0.0..=1.0, tr("How far a wide-lens distortion zooms in to hide the stretched border"));
        slider(ui, tr("Edge roundness"), &mut d.lens_roundness, def.lens_roundness, 0.0..=1.0, tr("Shape of the picture's edge: 0 a rectangle, 1 an oval"));
        slider(ui, tr("Edge smoothness"), &mut d.lens_smoothness, def.lens_smoothness, 0.0..=1.0, tr("Softness of that edge"));
    });
    effect(ui, "display_sharpen", tr("Sharpen"), &mut d.sharpen_on, def.sharpen_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.sharpen_amount, def.sharpen_amount, 0.0..=2.0, tr("Strength of the sharpening"));
        slider(ui, tr("Diameter"), &mut d.sharpen_diameter, def.sharpen_diameter, 1.0..=8.0, tr("Size of the detail that is sharpened, in pixels"));
    });
    effect(ui, "display_grain", tr("Film grain"), &mut d.grain_on, def.grain_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.grain_amount, def.grain_amount, 0.0..=1.0, tr("Strength of the noise"));
        slider(ui, tr("Radius"), &mut d.grain_radius, def.grain_radius, 1.0..=8.0, tr("Size of the grain, in pixels"));
        let l = label(ui, tr("Type"));
        let r = egui::ComboBox::from_id_salt("display_grain_type").selected_text(tr(d.grain_type.label())).show_ui(ui, |ui| {
            for t in subcraft_doc::GrainType::ALL {
                ui.selectable_value(&mut d.grain_type, t, tr(t.label()));
            }
        }).response;
        crate::reset_row::reset(ui, &l, &r, &mut d.grain_type, &def.grain_type);
        ui.end_row();
    });
    effect(ui, "display_tone", tr("Tone mapping"), &mut d.tone_mapping_on, def.tone_mapping_on, all, true, |ui| {
        let l = label(ui, tr("Exposure"));
        let r = ui.add(egui::Slider::new(&mut d.exposure, 0.25..=4.0).logarithmic(true)).on_hover_text(tr("Overall brightness before tone mapping"));
        crate::reset_row::reset(ui, &l, &r, &mut d.exposure, &def.exposure);
        ui.end_row();
        slider(ui, tr("Gamma"), &mut d.gamma, def.gamma, 0.2..=3.0, tr("Display gamma after the curve. 1 leaves the picture as it is."));
        let l = label(ui, tr("Function"));
        let r = egui::ComboBox::from_id_salt("display_tone_map").selected_text(d.tone_map.label()).height(400.0).show_ui(ui, |ui| {
            for t in ToneMap::MENU {
                ui.selectable_value(&mut d.tone_map, t, t.label());
            }
        }).response.on_hover_text(tr("How bright light is squeezed into the display range. ACES is the default look; Linear clips highlights."));
        crate::reset_row::reset(ui, &l, &r, &mut d.tone_map, &def.tone_map);
        ui.end_row();
        let l = label(ui, tr("Mapping Factor"));
        let r = ui.add_enabled(d.tone_map.uses_mapping_factor(), egui::Slider::new(&mut d.mapping_factor, 0.1..=10.0).logarithmic(true)).on_hover_text(tr("Strength of the Reinhard and Log compression. The other functions ignore it."));
        crate::reset_row::reset(ui, &l, &r, &mut d.mapping_factor, &def.mapping_factor);
        ui.end_row();
    });
    effect(ui, "display_cc", tr("Color correction"), &mut d.color_correction_on, def.color_correction_on, all, false, |ui| {
        slider(ui, tr("Saturation"), &mut d.cc_saturation, def.cc_saturation, 0.0..=2.0, tr("0 is grayscale, 1 leaves the colors as they are"));
        slider(ui, tr("Contrast"), &mut d.cc_contrast, def.cc_contrast, 0.0..=2.0, tr("Spread of the tones around mid gray"));
        slider(ui, tr("Brightness"), &mut d.cc_brightness, def.cc_brightness, 0.0..=2.0, tr("Multiplies the picture"));
        slider(ui, tr("Bias"), &mut d.cc_bias, def.cc_bias, -0.5..=0.5, tr("Added to every color"));
        slider(ui, tr("Sepia tone ratio"), &mut d.cc_sepia, def.cc_sepia, 0.0..=1.0, tr("Mix toward a warm brown monochrome"));
        let l = label(ui, tr("White Balance Temperature"));
        let r = ui.add(egui::Slider::new(&mut d.cc_temperature, 2000.0..=12000.0).suffix(" K").fixed_decimals(0)).on_hover_text(tr("Light of this temperature becomes neutral. 6500 K changes nothing."));
        crate::reset_row::reset(ui, &l, &r, &mut d.cc_temperature, &def.cc_temperature);
        ui.end_row();
    });
    effect(ui, "display_ssao", tr("Ambient occlusion"), &mut d.ssao_on, def.ssao_on, all, true, |ui| {
        slider(ui, tr("Amount"), &mut d.ssao, def.ssao, 0.0..=1.0, tr("Soft shadow in creases, from the view's depth. 0 is off."));
        let l = label(ui, tr("Radius"));
        let r = ui.add(egui::Slider::new(&mut d.ssao_radius, 0.5..=10.0).suffix("%")).on_hover_text(tr("How far occlusion reaches, in percent of the model size"));
        crate::reset_row::reset(ui, &l, &r, &mut d.ssao_radius, &def.ssao_radius);
        ui.end_row();
    });
    effect(ui, "display_contact_shadows", tr("Contact shadows"), &mut d.contact_shadows_on, def.contact_shadows_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.contact_shadows, def.contact_shadows, 0.0..=1.0, tr("Fine shadows of the key light where parts touch or nearly touch, from the view's depth. Only the key light is taken away."));
        let l = label(ui, tr("Length"));
        let r = ui.add(egui::Slider::new(&mut d.contact_length, 0.5..=20.0).suffix("%")).on_hover_text(tr("How far a contact shadow reaches, in percent of the model size"));
        crate::reset_row::reset(ui, &l, &r, &mut d.contact_length, &def.contact_length);
        ui.end_row();
    });
    effect(ui, "display_ssr", tr("Screen-space reflections"), &mut d.ssr_on, def.ssr_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.ssr, def.ssr, 0.0..=1.0, tr("Glossy surfaces reflect what the view shows of the model. Rough surfaces barely change; what is off screen is not reflected."));
        let l = label(ui, tr("Distance"));
        let r = ui.add(egui::Slider::new(&mut d.ssr_distance, 5.0..=200.0).suffix("%")).on_hover_text(tr("How far reflection rays travel, in percent of the model size"));
        crate::reset_row::reset(ui, &l, &r, &mut d.ssr_distance, &def.ssr_distance);
        ui.end_row();
    });
    effect(ui, "display_glow", tr("Emission glow"), &mut d.glow_on, def.glow_on, all, true, |ui| {
        slider(ui, tr("Amount"), &mut d.emission_glow, def.emission_glow, 0.0..=3.0, tr("Soft glow around emissive areas only. 0 is off; the default is subtle."));
        let l = label(ui, tr("Radius"));
        let r = ui.add(egui::Slider::new(&mut d.glow_radius, 0.25..=8.0).logarithmic(true)).on_hover_text(tr("How far the glow spreads"));
        crate::reset_row::reset(ui, &l, &r, &mut d.glow_radius, &def.glow_radius);
        ui.end_row();
    });
    ui.label(RichText::new(tr("Post effects are viewport effects: exported textures never contain them.")).small().color(TEXT_DIM()));
    subsurface(d, def, ui);
    if pick {
        state.picking_focus = true;
    }
}

/// Render resolution and the Rendering mode (path tracing).
pub fn render(state: &mut UiState, ui: &mut Ui) {
    let extent = state.session.scene().mesh.bounds.max - state.session.scene().mesh.bounds.min;
    let status = state.pt_status;
    let prefs = &mut state.recovery.prefs.viewport;
    let tracing = prefs.path_tracing;
    ui.horizontal(|ui| crate::viewport_bar::resolution_row(ui, prefs, true));
    let view = &*VIEW_DEFAULTS;
    let r = ui.checkbox(&mut prefs.display.oit, tr("Order-independent transparency"))
        .on_hover_text(tr("Alpha-blended texture sets look the same whatever order their parts are drawn in: overlapping transparent surfaces are averaged by depth and coverage"));
    crate::reset_row::reset(ui, &r, &r, &mut prefs.display.oit, &view.display.oit);
    crate::viewport_bar::rendering_settings(ui, prefs);

    // Painter's Iray status block: what the Rendering mode is doing right now.
    sub_header(ui, tr("Status"));
    grid(ui, "display_pt_status", |ui| {
        let off = || tr("Rendering mode is off (F10)");
        ui.label(tr("Resolution"));
        ui.label(status.filter(|_| tracing).map_or_else(|| off().to_string(), |s| format!("{} x {}", s.size[0], s.size[1])));
        ui.end_row();
        ui.label(tr("Scene size"));
        ui.label(format!("{:.3} x {:.3} x {:.3}", extent.x, extent.y, extent.z)).on_hover_text(tr("Width, height and depth of the model's bounding box, in scene units"));
        ui.end_row();
        ui.label(tr("Iterations"));
        ui.label(status.filter(|_| tracing).map_or_else(|| "-".to_string(), |s| if s.max_samples > 0 { format!("{} / {}", s.samples, s.max_samples) } else { s.samples.to_string() }));
        ui.end_row();
        ui.label(tr("Rendering time"));
        ui.label(status.filter(|_| tracing).map_or_else(|| "-".to_string(), |s| {
            if s.max_time_s > 0.0 { format!("{} / {}", format_duration(s.elapsed_s), format_duration(s.max_time_s)) } else { format_duration(s.elapsed_s) }
        }));
        ui.end_row();
    });

    sub_header(ui, tr("Resolution"));
    grid(ui, "display_pt_size", |ui| {
        let l = label(ui, tr("Override viewport resolution"));
        let r = ui.checkbox(&mut prefs.pt_override, "").on_hover_text(tr("Render at a fixed size instead of the 3D view's; the picture is fitted into the view"));
        crate::reset_row::reset(ui, &l, &r, &mut prefs.pt_override, &view.pt_override);
        ui.end_row();
        let max = crate::viewport_bar::MAX_TRACE_SIDE;
        let l = label(ui, tr("Width"));
        let r = ui.add_enabled(prefs.pt_override, egui::DragValue::new(&mut prefs.pt_width).range(16..=max).suffix(" px"));
        crate::reset_row::reset(ui, &l, &r, &mut prefs.pt_width, &view.pt_width);
        ui.end_row();
        let l = label(ui, tr("Height"));
        let r = ui.add_enabled(prefs.pt_override, egui::DragValue::new(&mut prefs.pt_height).range(16..=max).suffix(" px"));
        crate::reset_row::reset(ui, &l, &r, &mut prefs.pt_height, &view.pt_height);
        ui.end_row();
    });

    sub_header(ui, tr("Dome"));
    grid(ui, "display_pt_dome", |ui| {
        let l = label(ui, tr("Clear color"));
        let r = ui.horizontal(|ui| {
            let r = ui.checkbox(&mut prefs.pt_clear, "").on_hover_text(tr("Show a solid color where the camera sees past the model; the lighting is unchanged"));
            ui.add_enabled_ui(prefs.pt_clear, |ui| crate::color_picker::swatch_srgb(ui, &mut prefs.pt_clear_color));
            r
        }).inner;
        if crate::reset_row::wants_reset(ui, &l, &r, (prefs.pt_clear, prefs.pt_clear_color) != (view.pt_clear, view.pt_clear_color)) {
            (prefs.pt_clear, prefs.pt_clear_color) = (view.pt_clear, view.pt_clear_color);
        }
        ui.end_row();
    });

    sub_header(ui, tr("Ground"));
    ui.add_enabled_ui(prefs.pt_ground, |ui| {
        grid(ui, "display_pt_ground", |ui| {
            for (i, text) in [tr("Ground X"), tr("Ground Y"), tr("Ground Z")].into_iter().enumerate() {
                let l = label(ui, text);
                let r = ui.add(egui::DragValue::new(&mut prefs.pt_ground_offset[i]).speed(0.01).range(-1e4..=1e4))
                    .on_hover_text(tr("Offset from the default place: centred under the model, at the bottom of its bounding box"));
                crate::reset_row::reset(ui, &l, &r, &mut prefs.pt_ground_offset[i], &view.pt_ground_offset[i]);
                ui.end_row();
            }
            let l = label(ui, tr("Reflectivity"));
            let r = crate::color_picker::swatch_srgb(ui, &mut prefs.pt_ground_reflectivity).on_hover_text(tr("Color of the ground's reflection; black is a plain matte floor"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.pt_ground_reflectivity, &view.pt_ground_reflectivity);
            ui.end_row();
            let l = label(ui, tr("Glossiness"));
            let r = ui.add(egui::Slider::new(&mut prefs.pt_ground_glossiness, 0.0..=1.0)).on_hover_text(tr("Sharpness of the reflection: 1 is a mirror"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.pt_ground_glossiness, &view.pt_ground_glossiness);
            ui.end_row();
            let l = label(ui, tr("Shadow intensity"));
            let r = ui.add(egui::Slider::new(&mut prefs.pt_ground_shadow, 0.0..=1.0)).on_hover_text(tr("How dark the key light's shadow is on the ground"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.pt_ground_shadow, &view.pt_ground_shadow);
            ui.end_row();
            let l = label(ui, tr("Visible from below"));
            let r = ui.checkbox(&mut prefs.pt_ground_below, "").on_hover_text(tr("Show the ground when the camera is underneath it"));
            crate::reset_row::reset(ui, &l, &r, &mut prefs.pt_ground_below, &view.pt_ground_below);
            ui.end_row();
        });
    });

    if ui.add_enabled(tracing && status.is_some(), egui::Button::new(tr("Save render…")))
        .on_hover_text(tr("Save the path-traced picture as it is shown now, as a PNG"))
        .on_disabled_hover_text(tr("Turn on the Rendering mode (F10) first"))
        .clicked()
    {
        state.render_request = Some(crate::render_export::RenderRequest::PathTraced);
    }
}

/// "12.3 s", "4 min 05 s" or "1 h 02 min": a rendering time for the status readout.
pub fn format_duration(seconds: f32) -> String {
    let s = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
    if s < 60.0 {
        format!("{s:.1} s")
    } else if s < 3600.0 {
        let whole = s as u32;
        format!("{} min {:02} s", whole / 60, whole % 60)
    } else {
        let minutes = (s / 60.0) as u32;
        format!("{} h {:02} min", minutes / 60, minutes % 60)
    }
}

/// Texture filtering, camera frame, tool display, wireframe, channel display and grid: the
/// groups of Painter's Viewport settings, in its order.
pub fn visualization(state: &mut UiState, ui: &mut Ui) {
    let stencil = state.session.brush.stencil.is_some();
    let prefs = &mut state.recovery.prefs.viewport;
    let shown = prefs.shown_channel();
    let def = &*DEFAULTS;
    let view = &*VIEW_DEFAULTS;
    sub_header(ui, tr("Texture filtering"));
    grid(ui, "display_filtering", |ui| {
        let d = &mut prefs.display;
        let l = label(ui, tr("Anisotropic filtering"));
        let r = egui::ComboBox::from_id_salt("display_anisotropy").selected_text(tr(d.anisotropy.label())).show_ui(ui, |ui| {
            for a in subcraft_doc::Anisotropy::ALL {
                ui.selectable_value(&mut d.anisotropy, a, tr(a.label()));
            }
        }).response.on_hover_text(tr("Keeps textures sharp on surfaces seen at a grazing angle. Higher is sharper and a little slower."));
        crate::reset_row::reset(ui, &l, &r, &mut d.anisotropy, &def.anisotropy);
        ui.end_row();
        let l = label(ui, tr("MipMap bias"));
        let labels = subcraft_doc::display::MIP_BIAS_LABELS;
        let r = egui::ComboBox::from_id_salt("display_mip_bias").selected_text(tr(labels[d.mip_bias.min(3) as usize])).show_ui(ui, |ui| {
            for (i, label) in labels.iter().enumerate() {
                ui.selectable_value(&mut d.mip_bias, i as u32, tr(*label));
            }
        }).response.on_hover_text(tr("Sharper settings read textures from a more detailed level; too sharp shimmers when the model moves."));
        crate::reset_row::reset(ui, &l, &r, &mut d.mip_bias, &def.mip_bias);
        ui.end_row();
    });
    sub_header(ui, tr("Camera frame"));
    grid(ui, "display_frame", |ui| {
        let d = &mut prefs.display;
        let l = label(ui, tr("Show camera frame"));
        let r = ui.checkbox(&mut d.camera_frame, "").on_hover_text(tr("Outline the picture a camera of this aspect ratio takes"));
        crate::reset_row::reset(ui, &l, &r, &mut d.camera_frame, &def.camera_frame);
        ui.end_row();
        let l = label(ui, tr("Aspect ratio"));
        let r = ui.add_enabled_ui(d.camera_frame, |ui| {
            let current = FRAME_RATIOS.iter().find(|r| (r.1 - d.camera_frame_ratio).abs() < 1e-3).map_or_else(|| format!("{:.2}", d.camera_frame_ratio), |r| r.0.to_string());
            egui::ComboBox::from_id_salt("display_frame_ratio").selected_text(current).show_ui(ui, |ui| {
                for (name, ratio) in FRAME_RATIOS {
                    ui.selectable_value(&mut d.camera_frame_ratio, ratio, name);
                }
            }).response
        }).inner;
        crate::reset_row::reset(ui, &l, &r, &mut d.camera_frame_ratio, &def.camera_frame_ratio);
        ui.end_row();
        let l = label(ui, tr("Gate mask opacity"));
        let r = ui.add_enabled(d.camera_frame, egui::Slider::new(&mut d.gate_opacity, 0.0..=1.0)).on_hover_text(tr("How dark the view is outside the frame"));
        crate::reset_row::reset(ui, &l, &r, &mut d.gate_opacity, &def.gate_opacity);
        ui.end_row();
    });
    sub_header(ui, tr("Tool display"));
    let projection_channels: Vec<subcraft_core::ChannelKind> = state.session.brush.material.textures.keys().copied().collect();
    // The same settings as the stencil display options in Properties > Brush.
    grid(ui, "display_tool", |ui| {
        let d = &mut state.stencil_display;
        let tool = crate::state::StencilDisplay::default();
        let l = label(ui, tr("Projection preview"));
        let r = ui.add(egui::Slider::new(&mut d.projection_opacity, 0.0..=1.0)).on_hover_text(tr("Opacity of the Projection tool's pattern over the 3D view (0 hides it)"));
        crate::reset_row::reset(ui, &l, &r, &mut d.projection_opacity, &tool.projection_opacity);
        ui.end_row();
        let l = label(ui, tr("Projection preview channel"));
        let r = egui::ComboBox::from_id_salt("projection_preview_channel").selected_text(d.projection_channel.display_name()).show_ui(ui, |ui| {
            for k in &projection_channels {
                ui.selectable_value(&mut d.projection_channel, *k, k.display_name());
            }
        }).response.on_hover_text(tr("Which image of the brush material the Projection tool shows over the view"));
        crate::reset_row::reset(ui, &l, &r, &mut d.projection_channel, &tool.projection_channel);
        ui.end_row();
        let l = label(ui, tr("Hide stencil when painting"));
        let r = ui.checkbox(&mut d.hide_when_painting, "").on_hover_text(tr("The stencil image disappears during a stroke, so the paint is seen as it lands"));
        crate::reset_row::reset(ui, &l, &r, &mut d.hide_when_painting, &tool.hide_when_painting);
        ui.end_row();
        let l = label(ui, tr("Stencil display opacity"));
        let r = ui.add(egui::Slider::new(&mut d.opacity, 0.0..=1.0)).on_hover_text(if stencil { tr("Opacity of the stencil image over the view") } else { tr("Opacity of the stencil image over the view (no stencil is loaded)") });
        crate::reset_row::reset(ui, &l, &r, &mut d.opacity, &tool.opacity);
        ui.end_row();
    });
    sub_header(ui, tr("Mesh wireframe"));
    grid(ui, "display_wire", |ui| {
        let d = &mut prefs.display;
        let l = label(ui, tr("Show mesh wireframe"));
        let r = ui.checkbox(&mut d.wireframe, "");
        crate::reset_row::reset(ui, &l, &r, &mut d.wireframe, &def.wireframe);
        ui.end_row();
        let l = label(ui, tr("Wireframe color"));
        let r = crate::color_picker::swatch_srgb(ui, &mut d.wire_color);
        crate::reset_row::reset(ui, &l, &r, &mut d.wire_color, &def.wire_color);
        ui.end_row();
        let l = label(ui, tr("Wireframe opacity"));
        let r = ui.add(egui::Slider::new(&mut d.wire_opacity, 0.05..=1.0));
        crate::reset_row::reset(ui, &l, &r, &mut d.wire_opacity, &def.wire_opacity);
        ui.end_row();
        let l = label(ui, tr("Merge angle"));
        let r = ui.add(egui::Slider::new(&mut d.wire_angle, 0.0..=90.0).suffix("°")).on_hover_text(tr("Triangle pairs from the same polygon whose normals differ less than this are drawn as one polygon"));
        crate::reset_row::reset(ui, &l, &r, &mut d.wire_angle, &def.wire_angle);
        ui.end_row();
    });
    sub_header(ui, tr("Channel display"));
    // Like Painter: these only apply (and are only enabled) while a single channel is shown.
    ui.add_enabled_ui(shown.is_some(), |ui| {
        grid(ui, "display_channel", |ui| {
            let d = &mut prefs.display;
            let l = label(ui, tr("Display solo view without lighting"));
            let r = ui.checkbox(&mut d.solo_unlit, "").on_hover_text(tr("On: the channel exactly as stored. Off: shaded by the light, so the model's shape stays readable."));
            crate::reset_row::reset(ui, &l, &r, &mut d.solo_unlit, &def.solo_unlit);
            ui.end_row();
            let l = label(ui, tr("Scale HDR values"));
            let r = ui.add(egui::Slider::new(&mut d.hdr_scale, 0.1..=100.0).logarithmic(true)).on_hover_text(tr("The channel is divided by this before it is shown, to bring values above 1 into view"));
            crate::reset_row::reset(ui, &l, &r, &mut d.hdr_scale, &def.hdr_scale);
            ui.end_row();
            let l = label(ui, tr("Use +/- color for HDR values"));
            let r = ui.checkbox(&mut d.hdr_signed_colors, "").on_hover_text(tr("Grayscale channels: positive and negative values in two colors"));
            crate::reset_row::reset(ui, &l, &r, &mut d.hdr_signed_colors, &def.hdr_signed_colors);
            ui.end_row();
            let l = label(ui, tr("Positive color"));
            let r = ui.add_enabled_ui(d.hdr_signed_colors, |ui| crate::color_picker::swatch_srgb(ui, &mut d.hdr_positive)).inner;
            crate::reset_row::reset(ui, &l, &r, &mut d.hdr_positive, &def.hdr_positive);
            ui.end_row();
            let l = label(ui, tr("Negative color"));
            let r = ui.add_enabled_ui(d.hdr_signed_colors, |ui| crate::color_picker::swatch_srgb(ui, &mut d.hdr_negative)).inner;
            crate::reset_row::reset(ui, &l, &r, &mut d.hdr_negative, &def.hdr_negative);
            ui.end_row();
            let l = label(ui, tr("Color channels"));
            let color = shown.is_some_and(|k| k.components() == 3);
            let r = egui::ComboBox::from_id_salt("display_component").selected_text(tr(d.channel_component.label(color))).show_ui(ui, |ui| {
                for c in subcraft_doc::ChannelComponent::ALL {
                    ui.selectable_value(&mut d.channel_component, c, tr(c.label(color)));
                }
            }).response.on_hover_text(tr("Show every component of the channel, or one of them as gray"));
            crate::reset_row::reset(ui, &l, &r, &mut d.channel_component, &def.channel_component);
            ui.end_row();
        });
    });
    sub_header(ui, tr("Grid"));
    grid(ui, "display_grid", |ui| {
        let d = &mut prefs.display;
        let l = label(ui, tr("Show grid"));
        let r = ui.checkbox(&mut d.grid, "");
        crate::reset_row::reset(ui, &l, &r, &mut d.grid, &def.grid);
        ui.end_row();
        let l = label(ui, tr("Axis"));
        let r = ui.horizontal(|ui| {
            let mut r = ui.selectable_value(&mut d.grid_plane, subcraft_doc::GridAxis::ALL[0], subcraft_doc::GridAxis::ALL[0].label());
            for a in &subcraft_doc::GridAxis::ALL[1..] {
                r |= ui.selectable_value(&mut d.grid_plane, *a, a.label());
            }
            r.on_hover_text(tr("The axis the grid is perpendicular to (Y is the floor)"))
        }).inner;
        crate::reset_row::reset(ui, &l, &r, &mut d.grid_plane, &def.grid_plane);
        ui.end_row();
        let l = label(ui, tr("Grid color"));
        let r = crate::color_picker::swatch_srgb(ui, &mut d.grid_color);
        crate::reset_row::reset(ui, &l, &r, &mut d.grid_color, &def.grid_color);
        ui.end_row();
        let l = label(ui, tr("Grid opacity"));
        let r = ui.add(egui::Slider::new(&mut d.grid_opacity, 0.0..=1.0));
        crate::reset_row::reset(ui, &l, &r, &mut d.grid_opacity, &def.grid_opacity);
        ui.end_row();
        let l = label(ui, tr("Axis guides"));
        let r = ui.horizontal(|ui| {
            let mut r = ui.checkbox(&mut d.grid_axes[0], "X");
            r |= ui.checkbox(&mut d.grid_axes[1], "Y");
            r | ui.checkbox(&mut d.grid_axes[2], "Z")
        }).inner;
        crate::reset_row::reset(ui, &l, &r, &mut d.grid_axes, &def.grid_axes);
        ui.end_row();
    });
    sub_header(ui, tr("Guides"));
    let r = ui.checkbox(&mut prefs.symmetry_guides, tr("Symmetry guides"));
    crate::reset_row::reset(ui, &r, &r, &mut prefs.symmetry_guides, &view.symmetry_guides);
    let r = ui.checkbox(&mut prefs.symmetry_cursor, tr("Symmetry cursors")).on_hover_text(tr("A second brush cursor where each symmetry copy of the stroke paints"));
    crate::reset_row::reset(ui, &r, &r, &mut prefs.symmetry_cursor, &view.symmetry_cursor);
    let r = ui.add_enabled(prefs.symmetry_cursor, egui::Checkbox::new(&mut prefs.symmetry_cursor_hide_painting, tr("Hide while painting")));
    crate::reset_row::reset(ui, &r, &r, &mut prefs.symmetry_cursor_hide_painting, &view.symmetry_cursor_hide_painting);
    ui.horizontal(|ui| {
        let l = label(ui, tr("Brush cursor"));
        let r = egui::ComboBox::from_id_salt("brush_cursor_style").selected_text(prefs.brush_cursor.label()).show_ui(ui, |ui| {
            for style in crate::viewport_bar::BrushCursor::ALL {
                ui.selectable_value(&mut prefs.brush_cursor, style, style.label());
            }
        }).response;
        crate::reset_row::reset(ui, &l, &r, &mut prefs.brush_cursor, &view.brush_cursor);
    });
    let r = ui.checkbox(&mut prefs.symmetry_plane, tr("Mirror planes")).on_hover_text(tr("Each active mirror plane as a translucent sheet in its axis colour"));
    crate::reset_row::reset(ui, &r, &r, &mut prefs.symmetry_plane, &view.symmetry_plane);
    let r = ui.checkbox(&mut prefs.hints, tr("Viewport hints")).on_hover_text(tr("The list of mouse and key actions in the bottom-left corner of the 3D view. The grid unit is shown just above it."));
    crate::reset_row::reset(ui, &r, &r, &mut prefs.hints, &view.hints);
    let r = ui.checkbox(&mut prefs.axis_indicator, tr("Axis indicator")).on_hover_text(tr("The X, Y and Z directions of the scene in the bottom-right corner of the 3D view"));
    crate::reset_row::reset(ui, &r, &r, &mut prefs.axis_indicator, &view.axis_indicator);
}

/// Camera frame aspect ratios offered (name, width / height).
pub const FRAME_RATIOS: [(&str, f32); 6] = [("16:9", 16.0 / 9.0), ("3:2", 1.5), ("4:3", 4.0 / 3.0), ("1:1", 1.0), ("2.39:1", 2.39), ("9:16", 9.0 / 16.0)];

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_app::Session;

    fn state() -> UiState {
        UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None)
    }

    #[test]
    fn opening_a_project_does_not_modify_it_and_a_change_is_saved_into_it() {
        let mut s = state();
        sync_project(&mut s);
        assert_eq!(s.session.settings().display, None, "nothing is written until the user changes a setting");
        assert!(!s.session.is_modified());
        s.recovery.prefs.viewport.bloom = 0.9;
        s.recovery.prefs.viewport.grid = false;
        sync_project(&mut s);
        let saved = s.session.settings().display.clone().expect("the change is stored in the project");
        assert_eq!((saved.bloom, saved.grid), (0.9, false));
        assert!(s.session.is_modified(), "a display change is something Save keeps");
    }

    #[test]
    fn a_project_with_display_settings_replaces_the_live_ones_when_it_is_opened() {
        let mut s = state();
        s.recovery.prefs.viewport.bloom = 0.4;
        sync_project(&mut s);
        let mut other = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
        let mut settings = other.settings().clone();
        settings.display = Some(subcraft_doc::DisplaySettings { bloom: 1.5, wireframe: true, tone_map: ToneMap::Reinhard, ..Default::default() });
        other.set_settings(settings.clone());
        s.set_session(other);
        sync_project(&mut s);
        let live = &s.recovery.prefs.viewport;
        assert_eq!((live.bloom, live.wireframe, live.tone_map), (1.5, true, ToneMap::Reinhard));
        assert_eq!(s.session.settings(), &settings, "loading leaves the project's settings as they were");
        // A project without display settings keeps what is live.
        s.set_session(Session::new(subcraft_mesh::primitives::cube(2.0), 64));
        sync_project(&mut s);
        assert_eq!(s.recovery.prefs.viewport.bloom, 1.5);
        assert_eq!(s.session.settings().display, None);
    }

    #[test]
    fn the_environment_and_lens_travel_with_the_project() {
        let mut s = state();
        sync_project(&mut s);
        let glasshouse = s.env.index;
        let other = (0..s.library.environments.len()).find(|i| *i != glasshouse).expect("more than one environment");
        let other_name = s.library.environments[other].name.clone();
        s.env.index = other;
        s.env.rotation_deg = 40.0;
        s.recovery.prefs.viewport.env_exposure = 1.5;
        let before = s.camera.distance;
        set_fov(&mut s.camera, 80.0);
        // A wider lens steps closer by the ratio of the half-angle tangents: the target keeps its size.
        let expected = before * (20f32.to_radians().tan() / 40f32.to_radians().tan());
        assert!((s.camera.distance - expected).abs() < 1e-4 * before, "{} vs {expected}", s.camera.distance);
        sync_project(&mut s);
        let saved = s.session.settings().display.clone().unwrap();
        assert_eq!((saved.env_map.as_deref(), saved.env_rotation, saved.env_exposure, saved.fov), (Some(other_name.as_str()), 40.0, 1.5, 80.0));
        // Opening that project elsewhere brings the map, its rotation and the lens back.
        let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
        let mut settings = session.settings().clone();
        settings.display = Some(saved);
        session.set_settings(settings);
        let mut fresh = state();
        fresh.set_session(session);
        sync_project(&mut fresh);
        assert_eq!((fresh.env.index, fresh.env.rotation_deg, fresh.camera.fov_y_deg), (other, 40.0, 80.0));
        assert_eq!(fresh.recovery.prefs.viewport.env_exposure, 1.5);
    }

    #[test]
    fn a_local_environment_turns_with_the_camera_and_exposure_is_in_stops() {
        use crate::panels::shelf::environment_view;
        let mut d = subcraft_doc::DisplaySettings { env_exposure: 2.0, ..Default::default() };
        assert_eq!(environment_view(&d, 30.0, 1.0), (30.0, 4.0), "world aligned: the camera does not matter");
        d.env_alignment = subcraft_doc::EnvAlignment::Local;
        let (rotation, _) = environment_view(&d, 30.0, std::f32::consts::FRAC_PI_2);
        assert!((rotation - 120.0).abs() < 1e-3, "a quarter turn of the camera turns the map a quarter turn: {rotation}");
        let (wrapped, _) = environment_view(&d, 170.0, std::f32::consts::FRAC_PI_2);
        assert!((wrapped + 100.0).abs() < 1e-3, "stays in the slider's range: {wrapped}");
    }

    #[test]
    fn the_master_switch_and_each_effects_own_switch_gate_the_frame() {
        use subcraft_render::PostParams;
        let mut p = crate::viewport_bar::ViewportPrefs::default();
        let d = &mut p.display;
        (d.bloom, d.ssao, d.glare, d.exposure, d.tone_map, d.gamma, d.mapping_factor) = (1.0, 0.5, 2.0, 2.0, ToneMap::Log, 2.2, 3.0);
        (d.vignette_on, d.sharpen_on, d.grain_on, d.aberration_on, d.color_correction_on, d.flare_on, d.dof) = (true, true, true, true, true, true, true);
        (d.contact_shadows_on, d.ssr_on) = (true, true);
        d.cc_saturation = 0.5;
        let on = p.post_params(2.0);
        assert_eq!((on.bloom, on.ssao, on.glare, on.tone_map, on.mapping_factor, on.grade.gamma), (1.0, 0.5, 2.0, ToneMap::Log, 3.0, 2.2));
        assert!(on.grade.vignette > 0.0 && on.grade.sharpen > 0.0 && on.grade.grain > 0.0 && on.grade.aberration > 0.0 && on.grade.color_correction && on.flare > 0.0 && on.dof > 0.0);
        assert!(on.contact_shadows > 0.0 && on.ssr > 0.0 && on.contact_length > 0.0 && on.ssr_distance > 0.0);
        assert_eq!(on.grade.saturation, 0.5);
        assert_eq!(p.exposure_value(), 2.0);
        // One effect's switch removes just that effect.
        p.display.bloom_on = false;
        p.display.vignette_on = false;
        let partly = p.post_params(2.0);
        assert_eq!((partly.bloom, partly.grade.vignette, partly.ssao), (0.0, 0.0, 0.5));
        // Tone mapping off: linear clamped light at exposure 1, no gamma.
        p.display.tone_mapping_on = false;
        let flat = p.post_params(2.0);
        assert_eq!((flat.tone_map, flat.mapping_factor, flat.grade.gamma, p.exposure_value()), (ToneMap::LinearSat, 1.0, 1.0, 1.0));
        // The master switch: the plain render, whatever the effects are set to.
        p.display.post_effects = false;
        let off = p.post_params(2.0);
        let plain = PostParams::default();
        assert_eq!(PostParams { ssao_radius: plain.ssao_radius, dof_range: plain.dof_range, dof_focus: plain.dof_focus, bloom_threshold: plain.bloom_threshold, ..off },
            PostParams { emission_glow: 0.0, glare_mode: off.glare_mode, glare_threshold: off.glare_threshold, glare_remap: off.glare_remap, glare_length: off.glare_length, glow_radius: off.glow_radius, bloom_falloff: off.bloom_falloff, contact_length: off.contact_length, ssr_distance: off.ssr_distance, sss_radius: off.sss_radius, ..plain });
        // Scattering is not a post effect: the master switch leaves it on.
        p.display.sss_on = true;
        assert_eq!(p.post_params(2.0).sss, 1.0);
        assert!(!off.active() && !off.wants_emission());
    }

    fn texts(output: &egui::FullOutput) -> Vec<(String, f32)> {
        output.shapes.iter().filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.job.text.clone(), t.pos.y)),
            _ => None,
        }).collect()
    }

    #[test]
    fn ctrl_click_on_a_settings_label_puts_only_that_setting_back() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut s = state();
        s.recovery.prefs.viewport.display.env_exposure = 2.0;
        s.recovery.prefs.viewport.display.bloom = 1.2;
        let frame = |s: &mut UiState, mut events: Vec<egui::Event>, modifiers: egui::Modifiers| {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 8000.0))), events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| { egui::CentralPanel::default().show(ui, |ui| show(s, ui)); });
            out.textures_delta.clear();
            out
        };
        frame(&mut s, vec![], egui::Modifiers::NONE);
        let out = frame(&mut s, vec![], egui::Modifiers::NONE);
        let at = out.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text == "Environment exposure (EV)" => Some(t.pos + egui::vec2(4.0, 4.0)),
            _ => None,
        }).expect("the exposure row is drawn");
        let click = |s: &mut UiState, modifiers: egui::Modifiers| {
            frame(s, vec![egui::Event::PointerMoved(at)], modifiers);
            frame(s, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers }], modifiers);
            frame(s, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers }], modifiers);
        };
        click(&mut s, egui::Modifiers::NONE);
        assert_eq!(s.recovery.prefs.viewport.display.env_exposure, 2.0, "a plain click changes nothing");
        click(&mut s, egui::Modifiers { ctrl: true, command: true, ..Default::default() });
        assert_eq!(s.recovery.prefs.viewport.display.env_exposure, 0.0, "Ctrl+click puts the default back");
        assert_eq!(s.recovery.prefs.viewport.display.bloom, 1.2, "the other settings stay");
    }

    #[test]
    fn drawing_the_panel_leaves_a_fractional_environment_rotation_alone() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut s = state();
        s.env.rotation_deg = crate::panels::shelf::DEFAULT_ENVIRONMENT_ROTATION_DEG;
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 8000.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| { egui::CentralPanel::default().show(ui, |ui| show(&mut s, ui)); });
            out.textures_delta.clear();
        }
        assert!((s.env.rotation_deg - crate::panels::shelf::DEFAULT_ENVIRONMENT_ROTATION_DEG).abs() < 0.01, "the slider must not round -165.6° to -166° (it showed the changed dot), got {}", s.env.rotation_deg);
    }

    #[test]
    fn reset_all_puts_every_setting_back_but_keeps_the_chosen_assets_and_the_view_mode() {
        let mut s = state();
        let prefs = &mut s.recovery.prefs.viewport;
        (prefs.display.bloom, prefs.display.grid, prefs.display.tone_map, prefs.display.dof_focus_distance) = (1.5, false, ToneMap::Reinhard, Some(3.0));
        (prefs.orthographic, prefs.hints, prefs.pt_ground_glossiness, prefs.view_mode) = (true, false, 0.9, 2);
        prefs.display.env_map = Some("Studio".into());
        prefs.display.color_lut = Some(subcraft_core::ResourceId::new());
        let lut = prefs.display.color_lut;
        s.env.rotation_deg = 45.0;
        set_fov(&mut s.camera, 70.0);
        s.stencil_display.opacity = 0.1;
        reset_all(&mut s);
        let prefs = &s.recovery.prefs.viewport;
        let def = subcraft_doc::DisplaySettings::default();
        assert_eq!((prefs.display.bloom, prefs.display.grid, prefs.display.tone_map, prefs.display.dof_focus_distance), (def.bloom, def.grid, def.tone_map, None));
        assert_eq!((prefs.orthographic, prefs.hints, prefs.pt_ground_glossiness), (false, true, 0.5));
        assert_eq!((s.env.rotation_deg, s.camera.fov_y_deg), (default_env_rotation(&s), def.fov), "the startup map keeps its front lighting");
        assert_eq!(s.stencil_display, crate::state::StencilDisplay::default());
        assert_eq!((prefs.display.env_map.as_deref(), prefs.display.color_lut, prefs.view_mode), (Some("Studio"), lut, 2), "chosen assets and the view mode stay");
    }

    #[test]
    fn rendering_times_read_in_seconds_minutes_or_hours() {
        assert_eq!(format_duration(0.0), "0.0 s");
        assert_eq!(format_duration(12.34), "12.3 s");
        assert_eq!(format_duration(245.0), "4 min 05 s");
        assert_eq!(format_duration(3720.0), "1 h 02 min");
        assert_eq!(format_duration(f32::NAN), "0.0 s");
        assert_eq!(format_duration(-3.0), "0.0 s");
    }

    #[test]
    fn the_panel_lists_painters_groups_in_order_with_jump_buttons() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut s = state();
        let draw = |s: &mut UiState| {
            // Tall enough for every group expanded (Render settings has the status, resolution,
            // dome and ground blocks), so every control is laid out and drawn.
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 8000.0))), ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| show(s, ui));
            });
            out.textures_delta.clear();
            out
        };
        draw(&mut s);
        let out = draw(&mut s);
        let texts = texts(&out);
        for control in ["Min samples", "Max time", "Firefly filter", "Scene size", "Override viewport resolution", "Clear color", "Glossiness", "Visible from below", "Save render…"] {
            assert!(texts.iter().any(|t| t.0 == control), "Render settings shows {control:?}");
        }
        let y = |text: &str| texts.iter().find(|t| t.0 == text).unwrap_or_else(|| panic!("missing {text:?} in {:?}", texts.iter().map(|t| &t.0).collect::<Vec<_>>())).1;
        // Jump buttons on top, then the group headers in Painter's order.
        let headers = ["ENVIRONMENT SETTINGS", "CAMERA SETTINGS", "POST EFFECTS", "RENDER SETTINGS", "VISUALIZATION SETTINGS"];
        for pair in headers.windows(2) {
            assert!(y(pair[0]) < y(pair[1]), "{} is above {}", pair[0], pair[1]);
        }
        for button in ["Environment", "Camera", "Render", "Visualization"] {
            assert!(y(button) < y(headers[0]), "jump button {button} sits above the groups");
        }
        // The controls that used to be spread over the toolbar, the Render quality window and the shelf.
        let env = ["Environment map", "Environment opacity", "Environment exposure (EV)", "Environment rotation", "Environment blur", "Environment alignment"];
        for pair in env.windows(2) {
            assert!(y(pair[0]) < y(pair[1]), "{} is above {} (Painter's order)", pair[0], pair[1]);
        }
        for control in ["Field of view", "Focal length (mm)", "Focus distance", "Aperture", "Anti-aliasing", "Bloom", "Glare", "Tone mapping", "Show mesh wireframe", "Show grid", "Bounces", "Computation mode", "Shadows opacity"] {
            y(control);
        }
        let stack = ["Activate Post Effects", "Depth of field", "Bloom", "Glare", "Lens flare", "Lateral aberration", "Vignette", "Sharpen", "Film grain", "Tone mapping", "Color correction"];
        for pair in stack.windows(2) {
            assert!(y(pair[0]) < y(pair[1]), "{} is above {} (the order the effects are applied in)", pair[0], pair[1]);
        }
        for control in ["Falloff", "Remap Factor", "Gamma", "Mapping Factor", "Function"] {
            y(control);
        }
        // Visualization: Painter's Viewport settings groups, in its order.
        let groups = ["Texture filtering", "Camera frame", "Tool display", "Mesh wireframe", "Channel display", "Grid"];
        for pair in groups.windows(2) {
            assert!(y(pair[0]) < y(pair[1]), "{} is above {}", pair[0], pair[1]);
        }
        for control in ["Anisotropic filtering", "MipMap bias", "Show camera frame", "Gate mask opacity", "Hide stencil when painting", "Stencil display opacity",
            "Display solo view without lighting", "Scale HDR values", "Use +/- color for HDR values", "Positive color", "Negative color", "Color channels", "Axis", "Grid color", "Grid opacity"] {
            y(control);
        }
    }

    /// Drives the panel with real egui input, in a tall window so every row is on screen.
    struct Panel {
        ctx: egui::Context,
        s: UiState,
    }

    const CTRL: egui::Modifiers = egui::Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };

    impl Panel {
        fn new(s: UiState) -> Self {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            let mut p = Panel { ctx, s };
            p.frame(vec![], egui::Modifiers::NONE);
            p.frame(vec![], egui::Modifiers::NONE);
            p
        }
        fn frame(&mut self, mut events: Vec<egui::Event>, modifiers: egui::Modifiers) -> egui::FullOutput {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 8000.0))), events, ..Default::default() };
            let s = &mut self.s;
            let mut out = self.ctx.run_ui(input, |ui| { egui::CentralPanel::default().show(ui, |ui| show(s, ui)); });
            out.textures_delta.clear();
            out
        }
        /// The galley rect of the first text `text` drawn after the text `after`.
        fn text_rect(&mut self, after: &str, text: &str) -> egui::Rect {
            let out = self.frame(vec![], egui::Modifiers::NONE);
            let mut seen = after.is_empty();
            out.shapes.iter().find_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) if seen && t.galley.job.text == text => Some(t.visual_bounding_rect()),
                egui::epaint::Shape::Text(t) => { seen |= t.galley.job.text == after; None }
                _ => None,
            }).unwrap_or_else(|| panic!("{text:?} is drawn after {after:?}"))
        }
        /// Ctrl is pressed first (a frame of its own, as when a hand goes to the key), then the click.
        fn click(&mut self, at: egui::Pos2, modifiers: egui::Modifiers) {
            self.frame(vec![egui::Event::PointerMoved(at)], modifiers);
            self.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers }], modifiers);
            self.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers }], modifiers);
            self.frame(vec![], egui::Modifiers::NONE);
        }
        /// Whether the accent "changed" dot is drawn just before the label `text`.
        fn dot_before(&mut self, text: &str) -> bool {
            let out = self.frame(vec![], egui::Modifiers::NONE);
            let label = out.shapes.iter().find_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) if t.galley.job.text == text => Some(t.visual_bounding_rect()),
                _ => None,
            }).unwrap_or_else(|| panic!("{text:?} is drawn"));
            out.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Circle(c)
                if c.fill == crate::theme::ACCENT() && (c.center.y - label.center().y).abs() < 4.0 && c.center.x < label.left() && c.center.x > label.left() - 40.0))
        }
    }

    #[test]
    fn ctrl_click_on_a_slider_puts_the_default_back_instead_of_moving_it_to_the_click() {
        let mut s = state();
        s.recovery.prefs.viewport.display.env_exposure = 2.0;
        let mut p = Panel::new(s);
        let value = p.text_rect("Environment exposure (EV)", "2.00");
        // On the rail, well left of the shown value: a plain click there would set about -3 EV.
        let rail = egui::pos2(value.left() - 120.0, value.center().y);
        p.click(rail, CTRL);
        assert_eq!(p.s.recovery.prefs.viewport.display.env_exposure, 0.0, "Ctrl+click on the rail resets");
        // At the default, Ctrl+click on the rail must not move the slider either.
        p.click(rail, CTRL);
        assert_eq!(p.s.recovery.prefs.viewport.display.env_exposure, 0.0, "Ctrl+click at the default leaves it");
        p.click(rail, egui::Modifiers::NONE);
        assert!(p.s.recovery.prefs.viewport.display.env_exposure < -1.0, "a plain click still moves the slider, got {}", p.s.recovery.prefs.viewport.display.env_exposure);
    }

    #[test]
    fn ctrl_click_on_a_number_field_resets_it_without_starting_to_edit_it() {
        let mut s = state();
        s.recovery.prefs.viewport.display.env_exposure = 2.0;
        s.recovery.prefs.viewport.pt_ground_offset[1] = 0.5;
        let mut p = Panel::new(s);
        let value = p.text_rect("Environment exposure (EV)", "2.00");
        p.click(value.center(), CTRL);
        assert_eq!(p.s.recovery.prefs.viewport.display.env_exposure, 0.0);
        assert_eq!(p.ctx.memory(|m| m.focused()), None, "the slider's number is not in edit mode");
        // A plain DragValue row (the ground offset of the Rendering mode).
        let field = p.text_rect("Ground Y", "0.50");
        p.click(field.center(), CTRL);
        assert_eq!(p.s.recovery.prefs.viewport.pt_ground_offset[1], 0.0);
        assert_eq!(p.ctx.memory(|m| m.focused()), None, "the number field is not in edit mode");
        // Without Ctrl a click still opens the field for typing.
        let field = p.text_rect("Ground X", "0.00");
        p.click(field.center(), egui::Modifiers::NONE);
        assert!(p.ctx.memory(|m| m.focused()).is_some(), "a plain click edits the number");
    }

    #[test]
    fn ctrl_click_on_a_checkbox_resets_it_and_never_toggles_it_away_from_the_default() {
        let mut s = state();
        s.recovery.prefs.viewport.display.shadows = !DEFAULTS.shadows;
        let mut p = Panel::new(s);
        let label = p.text_rect("Shadows", "Enable");
        // The checkbox sits in the control column, right of the grid's widest label.
        let widest = p.text_rect("", "Computation mode");
        let at = egui::pos2(widest.right() + 19.0, label.center().y);
        p.click(at, CTRL);
        assert_eq!(p.s.recovery.prefs.viewport.display.shadows, DEFAULTS.shadows, "Ctrl+click resets the checkbox");
        p.click(at, CTRL);
        assert_eq!(p.s.recovery.prefs.viewport.display.shadows, DEFAULTS.shadows, "and a second one does not toggle it");
        // A checkbox that carries its own text (the Rendering mode's Denoise).
        p.s.recovery.prefs.viewport.pt_denoise = false;
        let denoise = p.text_rect("", "Denoise");
        p.click(denoise.center(), CTRL);
        assert!(p.s.recovery.prefs.viewport.pt_denoise);
        p.click(denoise.center(), CTRL);
        assert!(p.s.recovery.prefs.viewport.pt_denoise, "Ctrl+click at the default does not toggle");
    }

    #[test]
    fn every_setting_reset_all_touches_shows_the_changed_dot() {
        let mut s = state();
        let prefs = &mut s.recovery.prefs.viewport;
        (prefs.pt_denoise, prefs.pt_firefly, prefs.pt_ground) = (false, false, false);
        (prefs.pt_bounces, prefs.pt_min_samples, prefs.pt_max_samples, prefs.pt_max_time_s) = (8, 16, 64, 30.0);
        prefs.scale_percent = 50;
        (prefs.display.shadows, prefs.display.taa, prefs.display.oit, prefs.display.grid) = (!DEFAULTS.shadows, !DEFAULTS.taa, !VIEW_DEFAULTS.display.oit, !DEFAULTS.grid);
        prefs.display.bloom_on = !DEFAULTS.bloom_on;
        (prefs.hints, prefs.symmetry_guides) = (!VIEW_DEFAULTS.hints, !VIEW_DEFAULTS.symmetry_guides);
        let mut p = Panel::new(s);
        for row in ["Denoise", "Firefly filter", "Ground shadow", "Bounces", "Min samples", "Max samples", "Max time", "Resolution",
            "Enable", "Temporal anti-aliasing", "Order-independent transparency", "Show grid", "Bloom", "Viewport hints", "Symmetry guides"] {
            assert!(p.dot_before(row), "{row} shows the changed dot");
        }
        assert!(!p.dot_before("Anti-aliasing"), "unchanged rows have no dot");
        reset_all(&mut p.s);
        for row in ["Denoise", "Bounces", "Max time", "Resolution", "Enable", "Bloom"] {
            assert!(!p.dot_before(row), "{row} has no dot after Reset all");
        }
    }
}
