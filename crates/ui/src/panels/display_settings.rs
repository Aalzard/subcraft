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
        ui.add_space(4.0);
        let channels: Vec<_> = state.session.selected_texture_set().channel_kinds().collect();
        ui.horizontal(|ui| crate::viewport_bar::view_mode(ui, &mut state.recovery.prefs.viewport, &channels, true));
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

/// A small heading inside a section; `text` is already translated.
fn sub_header(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).color(TEXT_DIM));
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([10.0, 6.0]).show(ui, body);
}

/// Environment map and how it lights the model, in Painter's order: map, opacity, exposure,
/// rotation, blur, alignment.
pub fn environment(state: &mut UiState, ui: &mut Ui) {
    grid(ui, "display_env", |ui| {
        ui.label(tr("Environment map"));
        let current = state.library.environments.get(state.env.index).map_or_else(|| "-".to_string(), |e| e.name.clone());
        let combo = egui::ComboBox::from_id_salt("display_env_map").selected_text(current).width(150.0).show_ui(ui, |ui| {
            for (i, env) in state.library.environments.iter().enumerate() {
                ui.selectable_value(&mut state.env.index, i, &env.name);
            }
        }).response.on_hover_text(tr("The lighting around the model. Drop a map from Assets > Environments here or on the 3D view."));
        if let Some(env) = combo.dnd_release_payload::<crate::panels::shelf::EnvironmentDrag>()
            && env.0 < state.library.environments.len()
        {
            state.env.index = env.0;
        }
        ui.end_row();
        let display = &mut state.recovery.prefs.viewport.display;
        ui.label(tr("Environment opacity"));
        ui.add(egui::Slider::new(&mut display.env_opacity, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}", v * 100.0)).custom_parser(|s| s.trim().parse::<f64>().ok().map(|v| v / 100.0)).suffix("%"))
            .on_hover_text(tr("How much of the environment map shows behind the model. 0 is the plain background."));
        ui.end_row();
        ui.label(tr("Environment exposure (EV)"));
        ui.add(egui::Slider::new(&mut display.env_exposure, -5.0..=5.0).fixed_decimals(2)).on_hover_text(tr("Brightness of the environment in stops: +1 doubles the light, -1 halves it"));
        ui.end_row();
        ui.label(tr("Environment rotation"));
        ui.add(egui::Slider::new(&mut state.env.rotation_deg, -180.0..=180.0).suffix("°").fixed_decimals(0)).on_hover_text(tr("Turn the lighting around the model (also Shift + right mouse drag in the 3D view)"));
        ui.end_row();
        ui.label(tr("Environment blur"));
        ui.add_enabled(display.env_opacity > 0.0, egui::Slider::new(&mut display.env_blur, 0.0..=1.0)).on_hover_text(tr("Blur of the environment shown behind the model"));
        ui.end_row();
        ui.label(tr("Environment alignment"));
        egui::ComboBox::from_id_salt("display_env_alignment").selected_text(tr(display.env_alignment.label())).show_ui(ui, |ui| {
            for a in subcraft_doc::EnvAlignment::ALL {
                ui.selectable_value(&mut display.env_alignment, a, tr(a.label()));
            }
        }).response.on_hover_text(tr("World: the environment stays put while the camera orbits. Local: it turns with the camera, so the lighting follows the view."));
        ui.end_row();
    });
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
    {
        let camera = &mut state.camera;
        let prefs = &mut state.recovery.prefs.viewport;
        grid(ui, "display_camera_grid", |ui| {
            ui.label(tr("Projection"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut prefs.orthographic, false, tr("Perspective"));
                ui.selectable_value(&mut prefs.orthographic, true, tr("Orthographic"));
            });
            ui.end_row();
            ui.label(tr("Field of view"));
            let mut fov = camera.fov_y_deg;
            if ui.add_enabled(!prefs.orthographic, egui::Slider::new(&mut fov, subcraft_doc::display::FOV_RANGE).suffix("°").fixed_decimals(1)).on_hover_text(tr("Vertical field of view. The camera moves so the model keeps its size.")).changed() {
                set_fov(camera, fov);
            }
            ui.end_row();
            ui.label(tr("Focal length (mm)"));
            let (near, far) = (subcraft_doc::display::focal_length_mm(*subcraft_doc::display::FOV_RANGE.end()), subcraft_doc::display::focal_length_mm(*subcraft_doc::display::FOV_RANGE.start()));
            let mut focal = subcraft_doc::display::focal_length_mm(camera.fov_y_deg);
            if ui.add_enabled(!prefs.orthographic, egui::Slider::new(&mut focal, near..=far).logarithmic(true).fixed_decimals(1)).on_hover_text(tr("The same lens as a focal length on a full-frame (36 x 24 mm) sensor")).changed() {
                set_fov(camera, subcraft_doc::display::fov_from_focal_length(focal));
            }
            ui.end_row();
            ui.label(tr("Focus distance"));
            ui.add_enabled(prefs.dof, egui::Slider::new(&mut prefs.dof_focus, 0.2..=3.0).logarithmic(true)).on_hover_text(tr("1 is the point the camera orbits around; it follows zooming. Ctrl + middle click in the 3D view focuses on the point under the cursor. Needs Depth of field (Post effects)."));
            ui.end_row();
            ui.label(tr("Focus point"));
            pick = ui.add_enabled(prefs.dof, egui::Button::new(tr("Pick in 3D view"))).on_hover_text(tr("Click the model in the 3D view to focus on that spot")).clicked();
            ui.end_row();
            ui.label(tr("Aperture"));
            ui.add_enabled(prefs.dof, egui::Slider::new(&mut prefs.dof_amount, 0.0..=1.0)).on_hover_text(tr("Strength of the depth-of-field blur. Needs Depth of field (Post effects)."));
            ui.end_row();
            ui.label(tr("Anti-aliasing"));
            ui.checkbox(&mut prefs.antialias, "").on_hover_text(tr("Anti-aliasing · smooth edges (4x MSAA); off is faster"));
            ui.end_row();
        });
    }
    if pick {
        state.picking_focus = true;
    }
}

/// One entry of the post-effect stack: a collapsible row whose header carries the effect's own
/// on/off switch. The parameters are greyed out while the effect (or the whole stack) is off.
fn effect(ui: &mut Ui, id: &str, name: &str, on: &mut bool, stack_on: bool, default_open: bool, body: impl FnOnce(&mut Ui)) {
    let header_id = ui.make_persistent_id(("display_effect", id));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), header_id, default_open)
        .show_header(ui, |ui| {
            ui.add_enabled(stack_on, egui::Checkbox::new(on, RichText::new(name).color(theme::TEXT)));
        })
        .body(|ui| {
            let enabled = *on && stack_on;
            ui.add_enabled_ui(enabled, |ui| grid(ui, id, body));
        });
}

/// A slider row of an effect's grid.
fn slider(ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, tip: &str) -> egui::Response {
    ui.label(label);
    let r = ui.add(egui::Slider::new(value, range)).on_hover_text(tip);
    ui.end_row();
    r
}

/// The post-effect stack: the master switch, then one entry per effect in the order they are
/// applied (Painter's list), then SubCraft's own extras. Also shown in the Render quality window.
pub fn post_effects(state: &mut UiState, ui: &mut Ui) {
    let mut pick = false;
    let d = &mut state.recovery.prefs.viewport.display;
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.post_effects, tr("Activate Post Effects")).on_hover_text(tr("Master switch of every effect below. Off shows the plain render."));
        // At the top, so it stays in reach however many effects are expanded below.
        if ui.button(tr("Reset look")).on_hover_text(tr("Put every post effect back to its default")).clicked() {
            d.reset_post_effects();
        }
    });
    let all = d.post_effects;
    effect(ui, "display_dof", tr("Depth of field"), &mut d.dof, all, true, |ui| {
        slider(ui, tr("Focus range"), &mut d.dof_range, 0.0..=50.0, tr("Depth that stays sharp around the focus distance, in percent of the model size"));
        ui.label(tr("Focus point"));
        pick = ui.button(tr("Pick in 3D view")).on_hover_text(tr("Click the model in the 3D view to focus on that spot. Focus distance and Aperture are in the Camera settings.")).clicked();
        ui.end_row();
    });
    effect(ui, "display_bloom", tr("Bloom"), &mut d.bloom_on, all, true, |ui| {
        slider(ui, tr("Amount"), &mut d.bloom, 0.0..=2.0, tr("Glow around bright areas. 0 is off."));
        slider(ui, tr("Threshold"), &mut d.bloom_threshold, 0.0..=1.0, tr("Brightness above which an area glows (1 is screen white)"));
        slider(ui, tr("Falloff"), &mut d.bloom_falloff, 0.0..=1.0, tr("How far the glow spreads around the bright area"));
    });
    effect(ui, "display_glare", tr("Glare"), &mut d.glare_on, all, true, |ui| {
        slider(ui, tr("Luminance"), &mut d.glare, 0.0..=16.0, tr("Brightness of the glare thrown by emissive areas. 0 is off; 0.5 to 4 looks natural."));
        slider(ui, tr("Threshold"), &mut d.glare_threshold, 0.0..=4.0, tr("Emission below this brightness throws no glare"));
        slider(ui, tr("Remap Factor"), &mut d.glare_remap, 0.25..=4.0, tr("Response to brightness above the threshold. 1 is neutral."));
        ui.label(tr("Shape"));
        egui::ComboBox::from_id_salt("display_glare_shape").selected_text(d.glare_mode.label()).width(170.0).height(400.0).show_ui(ui, |ui| {
            for m in GlareMode::MENU {
                ui.selectable_value(&mut d.glare_mode, m, m.label());
            }
        });
        ui.end_row();
        slider(ui, tr("Streak length"), &mut d.glare_length, 0.0..=1.0, tr("How long the streaks reach"));
    });
    effect(ui, "display_flare", tr("Lens flare"), &mut d.flare_on, all, false, |ui| {
        ui.label(tr("Camera"));
        egui::ComboBox::from_id_salt("display_flare_camera").selected_text(tr(d.flare_camera.label())).show_ui(ui, |ui| {
            for c in subcraft_doc::FlareCamera::ALL {
                ui.selectable_value(&mut d.flare_camera, c, tr(c.label()));
            }
        });
        ui.end_row();
        slider(ui, tr("Amount"), &mut d.flare_amount, 0.0..=4.0, tr("Brightness of the ghost images. May exceed 1."));
        slider(ui, tr("Threshold"), &mut d.flare_threshold, 0.0..=1.0, tr("Brightness above which an area throws ghosts"));
        ui.label(tr("Unique ghosts"));
        ui.add(egui::Slider::new(&mut d.flare_ghosts, 1..=8)).on_hover_text(tr("Number of ghost images"));
        ui.end_row();
        slider(ui, tr("Ghost position scale"), &mut d.flare_position_scale, 0.1..=2.0, tr("Spacing of the ghosts along the line through the view centre"));
    });
    effect(ui, "display_aberration", tr("Lateral aberration"), &mut d.aberration_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.aberration_amount, 0.0..=2.0, tr("Strength of the color fringes"));
        slider(ui, tr("Radius"), &mut d.aberration_radius, 0.0..=1.0, tr("Distance from the centre where the fringes start"));
        slider(ui, tr("Softness"), &mut d.aberration_softness, 0.0..=1.0, tr("Width of the transition around the radius"));
        slider(ui, tr("Ratio"), &mut d.aberration_ratio, 0.25..=4.0, tr("Shape of the clean zone. 1 is circular."));
        slider(ui, tr("Center X"), &mut d.aberration_center[0], 0.0..=1.0, tr("Centre of the effect across the view"));
        slider(ui, tr("Center Y"), &mut d.aberration_center[1], 0.0..=1.0, tr("Centre of the effect down the view"));
        ui.label(tr("Achromatic aberrations"));
        ui.checkbox(&mut d.aberration_achromatic, "").on_hover_text(tr("Blur toward the edges instead of splitting the colors"));
        ui.end_row();
    });
    effect(ui, "display_vignette", tr("Vignette"), &mut d.vignette_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.vignette_amount, 0.0..=1.0, tr("How dark the corners get"));
    });
    effect(ui, "display_sharpen", tr("Sharpen"), &mut d.sharpen_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.sharpen_amount, 0.0..=2.0, tr("Strength of the sharpening"));
        slider(ui, tr("Diameter"), &mut d.sharpen_diameter, 1.0..=8.0, tr("Size of the detail that is sharpened, in pixels"));
    });
    effect(ui, "display_grain", tr("Film grain"), &mut d.grain_on, all, false, |ui| {
        slider(ui, tr("Amount"), &mut d.grain_amount, 0.0..=1.0, tr("Strength of the noise"));
        slider(ui, tr("Radius"), &mut d.grain_radius, 1.0..=8.0, tr("Size of the grain, in pixels"));
        ui.label(tr("Type"));
        egui::ComboBox::from_id_salt("display_grain_type").selected_text(tr(d.grain_type.label())).show_ui(ui, |ui| {
            for t in subcraft_doc::GrainType::ALL {
                ui.selectable_value(&mut d.grain_type, t, tr(t.label()));
            }
        });
        ui.end_row();
    });
    effect(ui, "display_tone", tr("Tone mapping"), &mut d.tone_mapping_on, all, true, |ui| {
        ui.label(tr("Exposure"));
        ui.add(egui::Slider::new(&mut d.exposure, 0.25..=4.0).logarithmic(true)).on_hover_text(tr("Overall brightness before tone mapping"));
        ui.end_row();
        slider(ui, tr("Gamma"), &mut d.gamma, 0.2..=3.0, tr("Display gamma after the curve. 1 leaves the picture as it is."));
        ui.label(tr("Function"));
        egui::ComboBox::from_id_salt("display_tone_map").selected_text(d.tone_map.label()).height(400.0).show_ui(ui, |ui| {
            for t in ToneMap::MENU {
                ui.selectable_value(&mut d.tone_map, t, t.label());
            }
        }).response.on_hover_text(tr("How bright light is squeezed into the display range. ACES is the default look; Linear clips highlights."));
        ui.end_row();
        ui.label(tr("Mapping Factor"));
        ui.add_enabled(d.tone_map.uses_mapping_factor(), egui::Slider::new(&mut d.mapping_factor, 0.1..=10.0).logarithmic(true)).on_hover_text(tr("Strength of the Reinhard and Log compression. The other functions ignore it."));
        ui.end_row();
    });
    effect(ui, "display_cc", tr("Color correction"), &mut d.color_correction_on, all, false, |ui| {
        slider(ui, tr("Saturation"), &mut d.cc_saturation, 0.0..=2.0, tr("0 is grayscale, 1 leaves the colors as they are"));
        slider(ui, tr("Contrast"), &mut d.cc_contrast, 0.0..=2.0, tr("Spread of the tones around mid gray"));
        slider(ui, tr("Brightness"), &mut d.cc_brightness, 0.0..=2.0, tr("Multiplies the picture"));
        slider(ui, tr("Bias"), &mut d.cc_bias, -0.5..=0.5, tr("Added to every color"));
        slider(ui, tr("Sepia tone ratio"), &mut d.cc_sepia, 0.0..=1.0, tr("Mix toward a warm brown monochrome"));
        ui.label(tr("White Balance Temperature"));
        ui.add(egui::Slider::new(&mut d.cc_temperature, 2000.0..=12000.0).suffix(" K").fixed_decimals(0)).on_hover_text(tr("Light of this temperature becomes neutral. 6500 K changes nothing."));
        ui.end_row();
    });
    effect(ui, "display_ssao", tr("Ambient occlusion"), &mut d.ssao_on, all, true, |ui| {
        slider(ui, tr("Amount"), &mut d.ssao, 0.0..=1.0, tr("Soft shadow in creases, from the view's depth. 0 is off."));
        ui.label(tr("Radius"));
        ui.add(egui::Slider::new(&mut d.ssao_radius, 0.5..=10.0).suffix("%")).on_hover_text(tr("How far occlusion reaches, in percent of the model size"));
        ui.end_row();
    });
    effect(ui, "display_glow", tr("Emission glow"), &mut d.glow_on, all, true, |ui| {
        slider(ui, tr("Amount"), &mut d.emission_glow, 0.0..=3.0, tr("Soft glow around emissive areas only. 0 is off; the default is subtle."));
        ui.label(tr("Radius"));
        ui.add(egui::Slider::new(&mut d.glow_radius, 0.25..=8.0).logarithmic(true)).on_hover_text(tr("How far the glow spreads"));
        ui.end_row();
    });
    ui.label(RichText::new(tr("Post effects are viewport effects: exported textures never contain them.")).small().color(TEXT_DIM));
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
    ui.horizontal(|ui| crate::viewport_bar::resolution(ui, prefs));
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
        ui.label(tr("Override viewport resolution"));
        ui.checkbox(&mut prefs.pt_override, "").on_hover_text(tr("Render at a fixed size instead of the 3D view's; the picture is fitted into the view"));
        ui.end_row();
        let max = crate::viewport_bar::MAX_TRACE_SIDE;
        ui.label(tr("Width"));
        ui.add_enabled(prefs.pt_override, egui::DragValue::new(&mut prefs.pt_width).range(16..=max).suffix(" px"));
        ui.end_row();
        ui.label(tr("Height"));
        ui.add_enabled(prefs.pt_override, egui::DragValue::new(&mut prefs.pt_height).range(16..=max).suffix(" px"));
        ui.end_row();
    });

    sub_header(ui, tr("Dome"));
    grid(ui, "display_pt_dome", |ui| {
        ui.label(tr("Clear color"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut prefs.pt_clear, "").on_hover_text(tr("Show a solid color where the camera sees past the model; the lighting is unchanged"));
            ui.add_enabled_ui(prefs.pt_clear, |ui| ui.color_edit_button_rgb(&mut prefs.pt_clear_color));
        });
        ui.end_row();
    });

    sub_header(ui, tr("Ground"));
    ui.add_enabled_ui(prefs.pt_ground, |ui| {
        grid(ui, "display_pt_ground", |ui| {
            for (i, label) in [tr("Ground X"), tr("Ground Y"), tr("Ground Z")].into_iter().enumerate() {
                ui.label(label);
                ui.add(egui::DragValue::new(&mut prefs.pt_ground_offset[i]).speed(0.01).range(-1e4..=1e4))
                    .on_hover_text(tr("Offset from the default place: centred under the model, at the bottom of its bounding box"));
                ui.end_row();
            }
            ui.label(tr("Reflectivity"));
            ui.color_edit_button_rgb(&mut prefs.pt_ground_reflectivity).on_hover_text(tr("Color of the ground's reflection; black is a plain matte floor"));
            ui.end_row();
            ui.label(tr("Glossiness"));
            ui.add(egui::Slider::new(&mut prefs.pt_ground_glossiness, 0.0..=1.0)).on_hover_text(tr("Sharpness of the reflection: 1 is a mirror"));
            ui.end_row();
            ui.label(tr("Shadow intensity"));
            ui.add(egui::Slider::new(&mut prefs.pt_ground_shadow, 0.0..=1.0)).on_hover_text(tr("How dark the key light's shadow is on the ground"));
            ui.end_row();
            ui.label(tr("Visible from below"));
            ui.checkbox(&mut prefs.pt_ground_below, "").on_hover_text(tr("Show the ground when the camera is underneath it"));
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
    sub_header(ui, tr("Texture filtering"));
    grid(ui, "display_filtering", |ui| {
        let d = &mut prefs.display;
        ui.label(tr("Anisotropic filtering"));
        egui::ComboBox::from_id_salt("display_anisotropy").selected_text(tr(d.anisotropy.label())).show_ui(ui, |ui| {
            for a in subcraft_doc::Anisotropy::ALL {
                ui.selectable_value(&mut d.anisotropy, a, tr(a.label()));
            }
        }).response.on_hover_text(tr("Keeps textures sharp on surfaces seen at a grazing angle. Higher is sharper and a little slower."));
        ui.end_row();
        ui.label(tr("MipMap bias"));
        let labels = subcraft_doc::display::MIP_BIAS_LABELS;
        egui::ComboBox::from_id_salt("display_mip_bias").selected_text(tr(labels[d.mip_bias.min(3) as usize])).show_ui(ui, |ui| {
            for (i, label) in labels.iter().enumerate() {
                ui.selectable_value(&mut d.mip_bias, i as u32, tr(*label));
            }
        }).response.on_hover_text(tr("Sharper settings read textures from a more detailed level; too sharp shimmers when the model moves."));
        ui.end_row();
    });
    sub_header(ui, tr("Camera frame"));
    grid(ui, "display_frame", |ui| {
        let d = &mut prefs.display;
        ui.label(tr("Show camera frame"));
        ui.checkbox(&mut d.camera_frame, "").on_hover_text(tr("Outline the picture a camera of this aspect ratio takes"));
        ui.end_row();
        ui.label(tr("Aspect ratio"));
        ui.add_enabled_ui(d.camera_frame, |ui| {
            let current = FRAME_RATIOS.iter().find(|r| (r.1 - d.camera_frame_ratio).abs() < 1e-3).map_or_else(|| format!("{:.2}", d.camera_frame_ratio), |r| r.0.to_string());
            egui::ComboBox::from_id_salt("display_frame_ratio").selected_text(current).show_ui(ui, |ui| {
                for (name, ratio) in FRAME_RATIOS {
                    ui.selectable_value(&mut d.camera_frame_ratio, ratio, name);
                }
            });
        });
        ui.end_row();
        ui.label(tr("Gate mask opacity"));
        ui.add_enabled(d.camera_frame, egui::Slider::new(&mut d.gate_opacity, 0.0..=1.0)).on_hover_text(tr("How dark the view is outside the frame"));
        ui.end_row();
    });
    sub_header(ui, tr("Tool display"));
    // The same settings as the stencil display options in Properties > Brush.
    grid(ui, "display_tool", |ui| {
        let d = &mut state.stencil_display;
        ui.label(tr("Hide stencil when painting"));
        ui.checkbox(&mut d.hide_when_painting, "").on_hover_text(tr("The stencil image disappears during a stroke, so the paint is seen as it lands"));
        ui.end_row();
        ui.label(tr("Stencil display opacity"));
        ui.add(egui::Slider::new(&mut d.opacity, 0.0..=1.0)).on_hover_text(if stencil { tr("Opacity of the stencil image over the view") } else { tr("Opacity of the stencil image over the view (no stencil is loaded)") });
        ui.end_row();
    });
    sub_header(ui, tr("Mesh wireframe"));
    grid(ui, "display_wire", |ui| {
        ui.label(tr("Show mesh wireframe"));
        ui.checkbox(&mut prefs.wireframe, "");
        ui.end_row();
        ui.label(tr("Wireframe color"));
        ui.color_edit_button_rgb(&mut prefs.wire_color);
        ui.end_row();
        ui.label(tr("Wireframe opacity"));
        ui.add(egui::Slider::new(&mut prefs.wire_opacity, 0.05..=1.0));
        ui.end_row();
        ui.label(tr("Merge angle"));
        ui.add(egui::Slider::new(&mut prefs.wire_angle, 0.0..=90.0).suffix("°")).on_hover_text(tr("Triangle pairs from the same polygon whose normals differ less than this are drawn as one polygon"));
        ui.end_row();
    });
    sub_header(ui, tr("Channel display"));
    // Like Painter: these only apply (and are only enabled) while a single channel is shown.
    ui.add_enabled_ui(shown.is_some(), |ui| {
        grid(ui, "display_channel", |ui| {
            let d = &mut prefs.display;
            ui.label(tr("Display solo view without lighting"));
            ui.checkbox(&mut d.solo_unlit, "").on_hover_text(tr("On: the channel exactly as stored. Off: shaded by the light, so the model's shape stays readable."));
            ui.end_row();
            ui.label(tr("Scale HDR values"));
            ui.add(egui::Slider::new(&mut d.hdr_scale, 0.1..=100.0).logarithmic(true)).on_hover_text(tr("The channel is divided by this before it is shown, to bring values above 1 into view"));
            ui.end_row();
            ui.label(tr("Use +/- color for HDR values"));
            ui.checkbox(&mut d.hdr_signed_colors, "").on_hover_text(tr("Grayscale channels: positive and negative values in two colors"));
            ui.end_row();
            ui.label(tr("Positive color"));
            ui.add_enabled_ui(d.hdr_signed_colors, |ui| ui.color_edit_button_rgb(&mut d.hdr_positive));
            ui.end_row();
            ui.label(tr("Negative color"));
            ui.add_enabled_ui(d.hdr_signed_colors, |ui| ui.color_edit_button_rgb(&mut d.hdr_negative));
            ui.end_row();
            ui.label(tr("Color channels"));
            let color = shown.is_some_and(|k| k.components() == 3);
            egui::ComboBox::from_id_salt("display_component").selected_text(tr(d.channel_component.label(color))).show_ui(ui, |ui| {
                for c in subcraft_doc::ChannelComponent::ALL {
                    ui.selectable_value(&mut d.channel_component, c, tr(c.label(color)));
                }
            }).response.on_hover_text(tr("Show every component of the channel, or one of them as gray"));
            ui.end_row();
        });
    });
    sub_header(ui, tr("Grid"));
    grid(ui, "display_grid", |ui| {
        ui.label(tr("Show grid"));
        ui.checkbox(&mut prefs.grid, "");
        ui.end_row();
        ui.label(tr("Axis"));
        ui.horizontal(|ui| {
            for a in subcraft_doc::GridAxis::ALL {
                ui.selectable_value(&mut prefs.display.grid_plane, a, a.label()).on_hover_text(tr("The axis the grid is perpendicular to (Y is the floor)"));
            }
        });
        ui.end_row();
        ui.label(tr("Grid color"));
        ui.color_edit_button_rgb(&mut prefs.display.grid_color);
        ui.end_row();
        ui.label(tr("Grid opacity"));
        ui.add(egui::Slider::new(&mut prefs.display.grid_opacity, 0.0..=1.0));
        ui.end_row();
        ui.label(tr("Axis guides"));
        ui.horizontal(|ui| {
            for (index, text) in ["X", "Y", "Z"].into_iter().enumerate() {
                ui.checkbox(&mut prefs.grid_axes[index], text);
            }
        });
        ui.end_row();
    });
    sub_header(ui, tr("Guides"));
    ui.checkbox(&mut prefs.symmetry_guides, tr("Symmetry guides"));
    ui.checkbox(&mut prefs.hints, tr("Viewport hints")).on_hover_text(tr("The list of mouse and key actions in the bottom-left corner of the 3D view. While it is hidden the grid unit is shown there."));
    ui.checkbox(&mut prefs.axis_indicator, tr("Axis indicator")).on_hover_text(tr("The X, Y and Z directions of the scene in the bottom-right corner of the 3D view"));
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
        d.cc_saturation = 0.5;
        let on = p.post_params(2.0);
        assert_eq!((on.bloom, on.ssao, on.glare, on.tone_map, on.mapping_factor, on.grade.gamma), (1.0, 0.5, 2.0, ToneMap::Log, 3.0, 2.2));
        assert!(on.grade.vignette > 0.0 && on.grade.sharpen > 0.0 && on.grade.grain > 0.0 && on.grade.aberration > 0.0 && on.grade.color_correction && on.flare > 0.0 && on.dof > 0.0);
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
            PostParams { emission_glow: 0.0, glare_mode: off.glare_mode, glare_threshold: off.glare_threshold, glare_remap: off.glare_remap, glare_length: off.glare_length, glow_radius: off.glow_radius, bloom_falloff: off.bloom_falloff, ..plain });
        assert!(!off.active() && !off.wants_emission());
    }

    fn texts(output: &egui::FullOutput) -> Vec<(String, f32)> {
        output.shapes.iter().filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.job.text.clone(), t.pos.y)),
            _ => None,
        }).collect()
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
        for control in ["Field of view", "Focal length (mm)", "Focus distance", "Aperture", "Anti-aliasing", "Bloom", "Glare", "Tone mapping", "Show mesh wireframe", "Show grid", "Bounces"] {
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
}
