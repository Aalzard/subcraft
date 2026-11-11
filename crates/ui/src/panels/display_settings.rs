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
pub fn sync_project(state: &mut UiState) {
    if !std::mem::replace(&mut state.display_loaded, true) {
        if let Some(saved) = state.session.settings().display.clone() {
            state.recovery.prefs.viewport.display = saved;
        }
        state.display_baseline = state.recovery.prefs.viewport.display.clone();
    }
    let live = &state.recovery.prefs.viewport.display;
    if *live != state.display_baseline {
        state.display_baseline = live.clone();
        let mut settings = state.session.settings().clone();
        settings.display = Some(live.clone());
        state.session.set_settings(settings);
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        jump_buttons(state, ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| crate::viewport_bar::view_mode(ui, &mut state.recovery.prefs.viewport, true));
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

/// Environment map and how it lights the model.
pub fn environment(state: &mut UiState, ui: &mut Ui) {
    grid(ui, "display_env", |ui| {
        ui.label(tr("Environment map"));
        let current = state.library.environments.get(state.env.index).map_or_else(|| "-".to_string(), |e| e.name.clone());
        egui::ComboBox::from_id_salt("display_env_map").selected_text(current).width(150.0).show_ui(ui, |ui| {
            for (i, env) in state.library.environments.iter().enumerate() {
                ui.selectable_value(&mut state.env.index, i, &env.name);
            }
        }).response.on_hover_text(tr("The lighting around the model. More maps: Assets > Environments."));
        ui.end_row();
        ui.label(tr("Environment rotation"));
        ui.add(egui::Slider::new(&mut state.env.rotation_deg, -180.0..=180.0).suffix("°").fixed_decimals(0)).on_hover_text(tr("Turn the lighting around the model (also Shift + right mouse drag in the 3D view)"));
        ui.end_row();
        ui.label(tr("Environment intensity"));
        ui.add(egui::Slider::new(&mut state.env.intensity, 0.1..=4.0).logarithmic(true)).on_hover_text(tr("Brightness of the environment light"));
        ui.end_row();
    });
}

/// Projection, anti-aliasing and the lens (focus distance and aperture of the depth of field).
pub fn camera(state: &mut UiState, ui: &mut Ui) {
    let mut pick = false;
    {
        let prefs = &mut state.recovery.prefs.viewport;
        grid(ui, "display_camera_grid", |ui| {
            ui.label(tr("Projection"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut prefs.orthographic, false, tr("Perspective"));
                ui.selectable_value(&mut prefs.orthographic, true, tr("Orthographic"));
            });
            ui.end_row();
            ui.label(tr("Focus distance"));
            ui.add_enabled(prefs.dof, egui::Slider::new(&mut prefs.dof_focus, 0.2..=3.0).logarithmic(true)).on_hover_text(tr("1 is the point the camera orbits around; it follows zooming. Needs Depth of field (Post effects)."));
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

/// The post-effect stack, in the order the effects are listed in Painter, then SubCraft's own
/// extras. Also shown in the Render quality window.
pub fn post_effects(state: &mut UiState, ui: &mut Ui) {
    let prefs = &mut state.recovery.prefs.viewport;
    sub_header(ui, tr("Depth of field"));
    grid(ui, "display_dof", |ui| {
        ui.label(tr("Enable"));
        ui.checkbox(&mut prefs.dof, "").on_hover_text(tr("Blur what is nearer or farther than the focus distance (Camera settings). Off by default."));
        ui.end_row();
        ui.label(tr("Focus range"));
        ui.add_enabled(prefs.dof, egui::Slider::new(&mut prefs.dof_range, 0.0..=50.0).suffix("%")).on_hover_text(tr("Depth that stays sharp around the focus distance, in percent of the model size"));
        ui.end_row();
    });
    sub_header(ui, tr("Bloom"));
    grid(ui, "display_bloom", |ui| {
        ui.label(tr("Amount"));
        ui.add(egui::Slider::new(&mut prefs.bloom, 0.0..=2.0)).on_hover_text(tr("Glow around bright areas. 0 is off."));
        ui.end_row();
        ui.label(tr("Threshold"));
        ui.add_enabled(prefs.bloom > 0.0, egui::Slider::new(&mut prefs.bloom_threshold, 0.0..=1.0)).on_hover_text(tr("Brightness above which an area glows"));
        ui.end_row();
    });
    sub_header(ui, tr("Glare"));
    grid(ui, "display_glare", |ui| {
        ui.label(tr("Luminance"));
        ui.add(egui::Slider::new(&mut prefs.glare, 0.0..=3.0)).on_hover_text(tr("Streaks of light from bright emission. 0 is off."));
        ui.end_row();
        ui.label(tr("Shape"));
        ui.add_enabled_ui(prefs.glare > 0.0, |ui| {
            egui::ComboBox::from_id_salt("display_glare_shape").selected_text(prefs.glare_mode.label()).show_ui(ui, |ui| {
                for m in GlareMode::ALL {
                    ui.selectable_value(&mut prefs.glare_mode, m, m.label());
                }
            });
        });
        ui.end_row();
        ui.label(tr("Streak length"));
        ui.add_enabled(prefs.glare > 0.0, egui::Slider::new(&mut prefs.glare_length, 0.0..=1.0)).on_hover_text(tr("How long the streaks reach"));
        ui.end_row();
    });
    sub_header(ui, tr("Tone mapping"));
    grid(ui, "display_tone", |ui| {
        ui.label(tr("Exposure"));
        ui.add(egui::Slider::new(&mut prefs.exposure, 0.25..=4.0).logarithmic(true)).on_hover_text(tr("Overall brightness before tone mapping"));
        ui.end_row();
        ui.label(tr("Function"));
        egui::ComboBox::from_id_salt("display_tone_map").selected_text(prefs.tone_map.label()).show_ui(ui, |ui| {
            for t in ToneMap::ALL {
                ui.selectable_value(&mut prefs.tone_map, t, t.label());
            }
        }).response.on_hover_text(tr("How bright light is squeezed into the display range. ACES is the default look; Linear clips highlights."));
        ui.end_row();
    });
    sub_header(ui, tr("Ambient occlusion"));
    grid(ui, "display_ssao", |ui| {
        ui.label(tr("Amount"));
        ui.add(egui::Slider::new(&mut prefs.ssao, 0.0..=1.0)).on_hover_text(tr("Soft shadow in creases, from the view's depth. 0 is off."));
        ui.end_row();
        ui.label(tr("Radius"));
        ui.add_enabled(prefs.ssao > 0.0, egui::Slider::new(&mut prefs.ssao_radius, 0.5..=10.0).suffix("%")).on_hover_text(tr("How far occlusion reaches, in percent of the model size"));
        ui.end_row();
    });
    sub_header(ui, tr("Emission glow"));
    grid(ui, "display_glow", |ui| {
        ui.label(tr("Amount"));
        ui.add(egui::Slider::new(&mut prefs.emission_glow, 0.0..=3.0)).on_hover_text(tr("Soft glow around emissive areas only. 0 is off; the default is subtle."));
        ui.end_row();
        ui.label(tr("Radius"));
        ui.add_enabled(prefs.emission_glow > 0.0, egui::Slider::new(&mut prefs.glow_radius, 0.25..=8.0).logarithmic(true)).on_hover_text(tr("How far the glow spreads"));
        ui.end_row();
    });
    ui.label(RichText::new(tr("Post effects are viewport effects: exported textures never contain them.")).small().color(TEXT_DIM));
    if ui.button(tr("Reset look")).on_hover_text(tr("Put every post effect back to its default")).clicked() {
        prefs.display.reset_post_effects();
    }
}

/// Render resolution and the Rendering mode (path tracing).
pub fn render(state: &mut UiState, ui: &mut Ui) {
    let prefs = &mut state.recovery.prefs.viewport;
    ui.horizontal(|ui| crate::viewport_bar::resolution(ui, prefs));
    crate::viewport_bar::rendering_settings(ui, prefs);
}

/// Wireframe, grid and the other aids drawn over the model.
pub fn visualization(state: &mut UiState, ui: &mut Ui) {
    let prefs = &mut state.recovery.prefs.viewport;
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
    sub_header(ui, tr("Grid"));
    grid(ui, "display_grid", |ui| {
        ui.label(tr("Show grid"));
        ui.checkbox(&mut prefs.grid, "");
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
    ui.checkbox(&mut prefs.hints, tr("Viewport hints")).on_hover_text(tr("The list of mouse and key actions in the bottom-left corner of the 3D view."));
}

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

    fn texts(output: &egui::FullOutput) -> Vec<(String, f32)> {
        output.shapes.iter().filter_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) => Some((t.galley.job.text.clone(), t.pos.y)),
            _ => None,
        }).collect()
    }

    #[test]
    fn the_panel_lists_painters_groups_in_order_with_jump_buttons() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut s = state();
        let mut draw = |s: &mut UiState| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 3000.0))), ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| show(s, ui));
            });
            out.textures_delta.clear();
            out
        };
        draw(&mut s);
        let out = draw(&mut s);
        let texts = texts(&out);
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
        for control in ["Environment map", "Environment rotation", "Aperture", "Anti-aliasing", "Bloom", "Glare", "Tone mapping", "Show mesh wireframe", "Show grid", "Bounces"] {
            y(control);
        }
        assert!(y("Bloom") < y("Glare") && y("Glare") < y("Tone mapping"), "effects keep Painter's stack order");
    }
}
