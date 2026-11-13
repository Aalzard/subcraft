//! Toolbar row at the top of the 3D view: render resolution, anti-aliasing, orthographic
//! projection and what the model shows (the full material or one channel unlit).

use crate::i18n::tr;
use egui::{RichText, Ui};
use serde::{Deserialize, Serialize};
use subcraft_render::ViewMode;

use crate::theme::{BG_2, TEXT_DIM};

/// Render resolutions offered, in percent of the 3D view's pixels.
pub const SCALES: [u32; 4] = [100, 75, 50, 25];

/// Seconds per unit of the Rendering mode's time limit: seconds, minutes, hours.
pub const TIME_UNITS: [f32; 3] = [1.0, 60.0, 3600.0];

/// Name of time unit `i` (an index into [`TIME_UNITS`]), translated.
pub fn time_unit_label(i: usize) -> &'static str {
    match i {
        0 => tr("seconds"),
        1 => tr("minutes"),
        _ => tr("hours"),
    }
}
/// Longest time limit (one day) and largest override side the Rendering mode accepts.
pub const MAX_RENDER_TIME_S: f32 = 86_400.0;
pub const MAX_TRACE_SIDE: u32 = 8192;

/// 3D view settings, kept between runs with the other preferences. The display settings (also
/// saved in each project, see [`crate::panels::display_settings::sync_project`]) are reached
/// directly: `prefs.bloom` is `prefs.display.bloom`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewportPrefs {
    /// Render resolution in percent of the 3D view's pixels (lower is faster).
    pub scale_percent: u32,
    pub orthographic: bool,
    /// Index into [`ViewMode::ALL`].
    pub view_mode: usize,
    /// A baked mesh map shown raw on the model instead of the view mode (the mesh map viewer;
    /// not remembered between runs).
    #[serde(skip)]
    pub mesh_map: Option<subcraft_core::MeshMapKind>,
    /// A channel shown alone that the fixed view modes do not cover (Emissive, Opacity,
    /// Specular, the user channels). Not remembered between runs.
    #[serde(skip)]
    pub solo: Option<subcraft_core::ChannelKind>,
    pub symmetry_guides: bool,
    /// Bottom-left list of the mouse / key actions that apply now.
    pub hints: bool,
    /// X / Y / Z orientation indicator in the 3D view's bottom-right corner.
    pub axis_indicator: bool,
    /// Environment, camera, post-effect and visualisation settings. Flattened, so preference
    /// files from before the Display Settings panel (same keys, top level) still load.
    #[serde(flatten)]
    pub display: subcraft_doc::DisplaySettings,
    /// Rendering mode (path tracing) on. Never saved: the raster viewport is the default.
    #[serde(skip)]
    pub path_tracing: bool,
    /// Rendering mode settings, kept between runs.
    pub pt_bounces: u32,
    /// Stop accumulating at this many samples per pixel (0 = never).
    pub pt_max_samples: u32,
    pub pt_denoise: bool,
    pub pt_ground: bool,
    /// Samples always taken before the time limit may stop the render.
    pub pt_min_samples: u32,
    /// Stop accumulating after this much tracing time, in seconds (0 = no limit), shown in
    /// unit `pt_time_unit` ([`TIME_UNITS`]).
    pub pt_max_time_s: f32,
    pub pt_time_unit: usize,
    /// Clamp very bright indirect samples (fireflies).
    pub pt_firefly: bool,
    /// Trace at a fixed size instead of the 3D view's (the image is letterboxed in the view).
    pub pt_override: bool,
    pub pt_width: u32,
    pub pt_height: u32,
    /// Camera rays that miss the model see this sRGB color instead of the background.
    pub pt_clear: bool,
    pub pt_clear_color: [f32; 3],
    /// Ground: offset from its default place (scene units), sRGB reflectivity color,
    /// glossiness, key-light shadow intensity and visibility from underneath.
    pub pt_ground_offset: [f32; 3],
    pub pt_ground_reflectivity: [f32; 3],
    pub pt_ground_glossiness: f32,
    pub pt_ground_shadow: f32,
    pub pt_ground_below: bool,
    /// Saved and imported camera viewpoints.
    pub camera_bookmarks: Vec<crate::view_extras::CameraBookmark>,
}

impl Default for ViewportPrefs {
    fn default() -> Self {
        Self {
            scale_percent: 100,
            orthographic: false,
            view_mode: 0,
            mesh_map: None,
            solo: None,
            symmetry_guides: true,
            hints: true,
            axis_indicator: true,
            display: Default::default(),
            path_tracing: false,
            pt_bounces: 4,
            pt_max_samples: 1024,
            pt_denoise: true,
            pt_ground: true,
            pt_min_samples: 0,
            pt_max_time_s: 0.0,
            pt_time_unit: 1,
            pt_firefly: true,
            pt_override: false,
            pt_width: 1920,
            pt_height: 1080,
            pt_clear: false,
            pt_clear_color: [0.0; 3],
            pt_ground_offset: [0.0; 3],
            pt_ground_reflectivity: [0.0; 3],
            pt_ground_glossiness: 0.5,
            pt_ground_shadow: 1.0,
            pt_ground_below: true,
            camera_bookmarks: Vec::new(),
        }
    }
}

impl std::ops::Deref for ViewportPrefs {
    type Target = subcraft_doc::DisplaySettings;
    fn deref(&self) -> &Self::Target {
        &self.display
    }
}

impl std::ops::DerefMut for ViewportPrefs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.display
    }
}

impl ViewportPrefs {
    pub fn view_mode(&self) -> ViewMode {
        ViewMode::ALL
            .get(self.view_mode)
            .copied()
            .unwrap_or_default()
    }

    /// The channel the 3D view shows alone, if any (the mesh map viewer is not a channel).
    pub fn shown_channel(&self) -> Option<subcraft_core::ChannelKind> {
        use subcraft_core::ChannelKind;
        if self.mesh_map.is_some() {
            return None;
        }
        self.solo.or(match self.view_mode() {
            ViewMode::BaseColor => Some(ChannelKind::BaseColor),
            ViewMode::Normal => Some(ChannelKind::Normal),
            ViewMode::Roughness => Some(ChannelKind::Roughness),
            ViewMode::Metallic => Some(ChannelKind::Metallic),
            ViewMode::Height => Some(ChannelKind::Height),
            ViewMode::AmbientOcclusion => Some(ChannelKind::AmbientOcclusion),
            ViewMode::Material | ViewMode::BakedNormal | ViewMode::BakedNormalMap => None,
        })
    }

    /// Show one channel alone on the model.
    pub fn show_channel(&mut self, kind: subcraft_core::ChannelKind) {
        use subcraft_core::ChannelKind;
        self.mesh_map = None;
        let mode = match kind {
            ChannelKind::BaseColor => Some(ViewMode::BaseColor),
            ChannelKind::Normal => Some(ViewMode::Normal),
            ChannelKind::Roughness => Some(ViewMode::Roughness),
            ChannelKind::Metallic => Some(ViewMode::Metallic),
            ChannelKind::Height => Some(ViewMode::Height),
            ChannelKind::AmbientOcclusion => Some(ViewMode::AmbientOcclusion),
            _ => None,
        };
        self.solo = if mode.is_some() { None } else { Some(kind) };
        self.set_view_mode(mode.unwrap_or(ViewMode::Material));
    }

    /// Back to the full material.
    pub fn show_material(&mut self) {
        self.mesh_map = None;
        self.solo = None;
        self.view_mode = 0;
    }

    fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = ViewMode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
    }

    /// Display next / previous channel: the material, then each of `channels` (the selected
    /// texture set's, in its order), wrapping around.
    pub fn cycle_channel(&mut self, channels: &[subcraft_core::ChannelKind], forward: bool) {
        let n = channels.len() + 1;
        let at = self.shown_channel().and_then(|k| channels.iter().position(|c| *c == k)).map_or(0, |i| i + 1);
        match if forward { (at + 1) % n } else { (at + n - 1) % n } {
            0 => self.show_material(),
            i => self.show_channel(channels[i - 1]),
        }
    }

    /// How a channel shown alone is displayed (Display Settings > Channel display).
    pub fn channel_display(&self) -> subcraft_render::ChannelDisplay {
        let d = &self.display;
        subcraft_render::ChannelDisplay {
            unlit: d.solo_unlit,
            hdr_scale: d.hdr_scale.clamp(1e-3, 1e6),
            signed_colors: d.hdr_signed_colors,
            positive: d.hdr_positive.map(|c| c.clamp(0.0, 1.0)),
            negative: d.hdr_negative.map(|c| c.clamp(0.0, 1.0)),
            component: d.channel_component,
        }
    }

    /// Grid of the 3D view for a model of `bounds`, or `None` while it is hidden.
    pub fn grid_params(&self, bounds: subcraft_mesh::Aabb) -> Option<subcraft_render::GridParams> {
        let d = &self.display;
        d.grid.then(|| subcraft_render::GridParams {
            bounds,
            axes: d.grid_axes,
            plane: d.grid_plane.index(),
            color: subcraft_core::color::srgb3_to_linear(d.grid_color.map(|c| c.clamp(0.0, 1.0))),
            // The default opacity draws the lines at the strength they always had.
            opacity: d.grid_opacity.clamp(0.0, 1.0) / subcraft_doc::display::DEFAULT_GRID_OPACITY,
        })
    }

    pub fn tone_map(&self) -> subcraft_render::ToneMap {
        self.display.tone_map
    }

    /// Post-processing for a model of bounding-box `diagonal`.
    pub fn post_params(&self, diagonal: f32) -> subcraft_render::PostParams {
        self.post_params_at(diagonal, 1.0)
    }

    /// Exposure of the frame: the Tone mapping effect's, or 1 while it (or every post effect) is off.
    pub fn exposure_value(&self) -> f32 {
        // Clamped like every other post parameter: 0, a negative or a huge value from a project
        // file gave a black or NaN frame.
        if self.post_effects && self.tone_mapping_on && self.exposure.is_finite() { self.exposure.clamp(1e-3, 1e3) } else { 1.0 }
    }

    /// Like [`post_params`](Self::post_params) with the camera's orbit distance, which scales the
    /// focus distance. An effect contributes only while the master switch and its own are on.
    pub fn post_params_at(&self, diagonal: f32, orbit_distance: f32) -> subcraft_render::PostParams {
        let d = &self.display;
        let all = d.post_effects;
        let amount = |on: bool, value: f32, max: f32| if all && on { value.clamp(0.0, max) } else { 0.0 };
        let mut grade = subcraft_render::Grade::default();
        if all && d.aberration_on {
            grade.aberration = d.aberration_amount.clamp(0.0, 4.0);
            grade.aberration_radius = d.aberration_radius.clamp(0.0, 2.0);
            grade.aberration_softness = d.aberration_softness.clamp(0.0, 2.0);
            grade.aberration_ratio = d.aberration_ratio.clamp(0.1, 10.0);
            grade.aberration_center = d.aberration_center.map(|c| c.clamp(0.0, 1.0));
            grade.aberration_achromatic = d.aberration_achromatic;
        }
        if all && d.vignette_on {
            grade.vignette = d.vignette_amount.clamp(0.0, 1.0);
        }
        if all && d.sharpen_on {
            grade.sharpen = d.sharpen_amount.clamp(0.0, 4.0);
            grade.sharpen_diameter = d.sharpen_diameter.clamp(0.5, 16.0);
        }
        if all && d.grain_on {
            grade.grain = d.grain_amount.clamp(0.0, 1.0);
            grade.grain_radius = d.grain_radius.clamp(1.0, 16.0);
            grade.grain_colored = d.grain_type == subcraft_doc::GrainType::Colored;
        }
        if all && d.color_correction_on {
            grade.color_correction = true;
            grade.saturation = d.cc_saturation.clamp(0.0, 4.0);
            grade.contrast = d.cc_contrast.clamp(0.0, 4.0);
            grade.brightness = d.cc_brightness.clamp(0.0, 4.0);
            grade.bias = d.cc_bias.clamp(-1.0, 1.0);
            grade.sepia = d.cc_sepia.clamp(0.0, 1.0);
            grade.temperature = d.cc_temperature.clamp(1000.0, 40000.0);
        }
        let toned = all && d.tone_mapping_on;
        if toned {
            grade.gamma = d.gamma.clamp(0.1, 10.0);
        }
        let flare = all && d.flare_on && d.flare_amount > 0.0;
        let defaults = subcraft_render::PostParams::default();
        subcraft_render::PostParams {
            dof: amount(d.dof, d.dof_amount, 1.0),
            dof_focus: (d.dof_focus.clamp(0.05, 4.0) * orbit_distance).max(1e-4),
            dof_range: d.dof_range.clamp(0.0, 100.0) * 0.01 * diagonal,
            // Tone mapping off: linear light, clamped. Every effect off: the default curve.
            tone_map: if toned { d.tone_map } else if all { subcraft_render::ToneMap::LinearSat } else { defaults.tone_map },
            mapping_factor: if toned { d.mapping_factor.clamp(0.01, 100.0) } else { 1.0 },
            bloom: amount(d.bloom_on, d.bloom, 4.0),
            bloom_threshold: d.bloom_threshold.clamp(0.0, 4.0),
            bloom_falloff: d.bloom_falloff.clamp(0.0, 1.0),
            ssao: amount(d.ssao_on, d.ssao, 1.0),
            ssao_radius: (d.ssao_radius.clamp(0.1, 50.0) * 0.01 * diagonal).max(1e-4),
            emission_glow: amount(d.glow_on, d.emission_glow, 4.0),
            glow_radius: d.glow_radius.clamp(0.25, 8.0),
            glare: amount(d.glare_on, d.glare, 16.0),
            glare_mode: d.glare_mode,
            glare_length: d.glare_length.clamp(0.0, 1.0),
            glare_threshold: d.glare_threshold.clamp(0.0, 16.0),
            glare_remap: d.glare_remap.clamp(0.05, 8.0),
            flare: if flare { d.flare_amount.clamp(0.0, 4.0) } else { 0.0 },
            flare_threshold: if flare { d.flare_threshold.clamp(0.0, 1.0) } else { defaults.flare_threshold },
            flare_ghosts: if flare { d.flare_ghosts.clamp(1, 8) } else { defaults.flare_ghosts },
            flare_scale: if flare { d.flare_position_scale.clamp(0.05, 4.0) * if d.flare_camera == subcraft_doc::FlareCamera::Telephoto { 0.5 } else { 1.0 } } else { defaults.flare_scale },
            grade,
        }
    }

    /// Path tracer settings from these preferences (clamped to sane ranges).
    pub fn path_trace_settings(&self, paused: bool) -> subcraft_render::PathTraceSettings {
        let finite = |v: f32, default: f32| if v.is_finite() { v } else { default };
        subcraft_render::PathTraceSettings {
            max_bounces: self.pt_bounces.clamp(1, 16),
            max_samples: self.pt_max_samples.min(1 << 16),
            min_samples: self.pt_min_samples.min(1 << 16),
            max_time_s: finite(self.pt_max_time_s, 0.0).clamp(0.0, MAX_RENDER_TIME_S),
            denoise: self.pt_denoise,
            ground: self.pt_ground,
            ground_settings: subcraft_render::GroundSettings {
                offset: self.pt_ground_offset.map(|v| finite(v, 0.0).clamp(-1e4, 1e4)),
                reflectivity: subcraft_core::color::srgb3_to_linear(self.pt_ground_reflectivity.map(|v| finite(v, 0.0).clamp(0.0, 1.0))),
                glossiness: finite(self.pt_ground_glossiness, 0.5).clamp(0.0, 1.0),
                shadow_intensity: finite(self.pt_ground_shadow, 1.0).clamp(0.0, 1.0),
                visible_from_below: self.pt_ground_below,
            },
            firefly_filter: self.pt_firefly,
            clear_color: self.pt_clear.then(|| subcraft_core::color::srgb3_to_linear(self.pt_clear_color.map(|v| finite(v, 0.0).clamp(0.0, 1.0)))),
            paused,
            ..Default::default()
        }
    }

    /// Size the Rendering mode traces at for a 3D view of `size` pixels: the override when it is
    /// on, otherwise the view's render size.
    pub fn trace_size(&self, size: [u32; 2]) -> [u32; 2] {
        if self.pt_override {
            [self.pt_width.clamp(16, MAX_TRACE_SIDE), self.pt_height.clamp(16, MAX_TRACE_SIDE)]
        } else {
            self.render_size(size)
        }
    }

    /// Render-target size for a 3D view of `size` physical pixels; the image is stretched back
    /// to the view, so painting and picking keep working in full-size coordinates.
    pub fn render_size(&self, size: [u32; 2]) -> [u32; 2] {
        let scale = self.scale_percent.clamp(10, 100) as f32 / 100.0;
        size.map(|s| ((s as f32 * scale).round() as u32).max(1))
    }
}

/// Narrowest dock where the brush bar sits beside the view controls. Measured: the inline row is
/// 1228.3 wide incl. 16 of frame margin (view combo 150, wireframe + camera buttons, five tool icons); the brush_bar test
/// fails if this drifts below the real content width.
// The tool icons left this row (they live in the tool rail), so it fits about 140 points sooner.
// The symmetry settings button and the lazy mouse button added 48 points (65 with margin for rounding).
pub(crate) const INLINE_MIN_WIDTH: f32 = 1140.0;

/// Narrowest dock width (inside the bar's margins) that fits the wireframe and camera buttons
/// beside the 92-wide View combo, and the one that also fits the 150-wide combo that shows
/// "Baked Normal Map" in full. Measured by the brush_bar test.
const EXTRAS_MIN_WIDTH: f32 = 600.0;
const WIDE_VIEW_COMBO_MIN_WIDTH: f32 = 660.0;

/// View and brush settings share the top strip. Narrow docks use a second row, then menus.
pub fn show(ui: &mut Ui, state: &mut crate::state::UiState) {
    let mut settings = state.session.brush.settings;
    let original = settings;
    let mut options = state.session.brush.options;
    let original_options = options;
    let painting = crate::brush_bar::active(state.tool);
    let frame = egui::Frame::new()
        .fill(BG_2)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            // Keep labels/combos aligned with the fixed 26-point tool/grid icons.
            ui.spacing_mut().interact_size.y = 26.0;
            ui.set_min_width(ui.available_width());
            let width = ui.available_width();
            let inline = width >= INLINE_MIN_WIDTH;
            ui.spacing_mut().item_spacing = egui::vec2(7.0, 6.0);
            let channels: Vec<_> = state.session.selected_texture_set().channel_kinds().collect();
            ui.horizontal(|ui| {
                if width < 520.0 {
                    ui.menu_button(tr("View"), |ui| {
                        ui.horizontal(|ui| resolution(ui, &mut state.recovery.prefs.viewport));
                        let prefs = &mut state.recovery.prefs.viewport;
                        ui.checkbox(&mut prefs.antialias, tr("Anti-aliasing"));
                        ui.checkbox(&mut prefs.orthographic, tr("Orthographic"));
                        ui.horizontal(|ui| view_mode(ui, prefs, &channels, false));
                    });
                    grid_control(ui, &mut state.recovery.prefs.viewport);
                    crate::view_extras::wireframe_control(ui, &mut state.recovery.prefs.viewport);
                    crate::view_extras::camera_menu(ui, state);
                } else {
                    view_controls(ui, state, width);
                }
                if painting && inline {
                    ui.add_space(5.0);
                    ui.separator();
                    ui.add_space(5.0);
                    crate::brush_bar::show(ui, &mut state.tool, &mut settings, &mut options, &mut state.recovery.prefs.viewport.symmetry_guides, width);
                    if state.tool == crate::state::Tool::PolygonFill {
                        crate::brush_bar::fill_options(ui, &mut state.fill_mode, Some(&mut state.fill_visible_only), "fill_mode_3d");
                    }
                }
            });
            if painting && !inline {
                ui.separator();
                ui.horizontal(|ui| {
                    crate::brush_bar::show(ui, &mut state.tool, &mut settings, &mut options, &mut state.recovery.prefs.viewport.symmetry_guides, width);
                    if state.tool == crate::state::Tool::PolygonFill {
                        crate::brush_bar::fill_options(ui, &mut state.fill_mode, Some(&mut state.fill_visible_only), "fill_mode_3d");
                    }
                });
            }
            if painting {
                crate::brush_bar::channel_switches(ui, state);
            }
        });
    ui.painter().hline(
        frame.response.rect.x_range(),
        frame.response.rect.bottom(),
        egui::Stroke::new(1.0, crate::theme::BORDER),
    );
    if settings != original {
        let now = ui.input(|i| i.time);
        if let Err(error) = state
            .session
            .execute(subcraft_app::Command::SetBrushSettings(settings))
        {
            state.set_status(error.to_string(), now);
        }
    }
    if options != original_options {
        let now = ui.input(|i| i.time);
        if let Err(error) = state
            .session
            .execute(subcraft_app::Command::SetStrokeOptions(options))
        {
            state.set_status(error.to_string(), now);
        }
    }
}

fn view_controls(ui: &mut Ui, state: &mut crate::state::UiState, width: f32) {
    resolution(ui, &mut state.recovery.prefs.viewport);
    let prefs = &mut state.recovery.prefs.viewport;
    ui.separator();
    ui.checkbox(&mut prefs.antialias, tr("Anti-aliasing"))
        .on_hover_text(tr("Anti-aliasing · smooth edges (4x MSAA); off is faster"));
    ui.checkbox(&mut prefs.orthographic, tr("Ortho"))
        .on_hover_text(tr("Orthographic · no perspective: parallel lines stay parallel"));
    ui.separator();
    let channels: Vec<_> = state.session.selected_texture_set().channel_kinds().collect();
    let prefs = &mut state.recovery.prefs.viewport;
    view_mode(ui, prefs, &channels, width >= WIDE_VIEW_COMBO_MIN_WIDTH);
    grid_control(ui, prefs);
    // Wireframe and camera buttons would push the row past a narrow dock's edge.
    if width >= EXTRAS_MIN_WIDTH {
        crate::view_extras::wireframe_control(ui, prefs);
        crate::view_extras::camera_menu(ui, state);
    }
}

/// Rendering mode settings (the 3D view's Render button menu and Preferences).
pub fn rendering_settings(ui: &mut Ui, prefs: &mut ViewportPrefs) {
    ui.label(RichText::new(tr("Rendering mode")).color(TEXT_DIM));
    egui::Grid::new("pt_settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label(tr("Bounces"));
        ui.add(egui::Slider::new(&mut prefs.pt_bounces, 1..=16)).on_hover_text(tr("Light bounces after the first hit; more is slower and brighter in enclosed areas"));
        ui.end_row();
        ui.label(tr("Min samples"));
        ui.add(egui::Slider::new(&mut prefs.pt_min_samples, 0..=8192).logarithmic(true))
            .on_hover_text(tr("Samples always taken before the time limit can stop the render"));
        ui.end_row();
        ui.label(tr("Max samples"));
        ui.add(egui::Slider::new(&mut prefs.pt_max_samples, 0..=8192).logarithmic(true).custom_formatter(|v, _| if v == 0.0 { "unlimited".into() } else { format!("{v}") }))
            .on_hover_text(tr("Stop refining at this many samples per pixel"));
        ui.end_row();
        ui.label(tr("Max time"));
        ui.horizontal(|ui| {
            let unit = prefs.pt_time_unit.min(TIME_UNITS.len() - 1);
            let per = TIME_UNITS[unit];
            let mut value = prefs.pt_max_time_s / per;
            let max = MAX_RENDER_TIME_S / per;
            if ui.add(egui::DragValue::new(&mut value).range(0.0..=max).speed(0.1).custom_formatter(|v, _| if v == 0.0 { "unlimited".into() } else { format!("{v:.1}") }))
                .on_hover_text(tr("Stop refining after this much rendering time (0 is no limit)"))
                .changed()
            {
                prefs.pt_max_time_s = (value * per).clamp(0.0, MAX_RENDER_TIME_S);
            }
            egui::ComboBox::from_id_salt("pt_time_unit").selected_text(time_unit_label(unit)).width(80.0).show_ui(ui, |ui| {
                for i in 0..TIME_UNITS.len() {
                    ui.selectable_value(&mut prefs.pt_time_unit, i, time_unit_label(i));
                }
            });
        });
        ui.end_row();
    });
    ui.checkbox(&mut prefs.pt_denoise, tr("Denoise")).on_hover_text(tr("Edge-aware filter for a clean preview while samples are few"));
    ui.checkbox(&mut prefs.pt_firefly, tr("Firefly filter")).on_hover_text(tr("Tone down rare, very bright speckles from bright lights seen through rough surfaces"));
    ui.checkbox(&mut prefs.pt_ground, tr("Ground shadow")).on_hover_text(tr("A gray floor under the model that catches its shadow"));
}

/// Per-set document state: shared by material evaluation, preview and export.
pub fn albedo_filter(ui: &mut Ui, state: &mut crate::state::UiState) {
    use subcraft_image::TextureFilter;
    let ts = state.session.selected_texture_set();
    let (id, original) = (ts.id, ts.albedo_filter);
    let mut filter = original;
    let label_width = 55.0;
    ui.add_sized([label_width, ui.spacing().interact_size.y], egui::Label::new(RichText::new(tr("Albedo")).color(TEXT_DIM)));
    egui::ComboBox::from_id_salt("albedo_filter").width(76.0).selected_text(filter.label()).show_ui(ui, |ui| {
        ui.selectable_value(&mut filter, TextureFilter::Bilinear, tr("Bilinear"));
        ui.selectable_value(&mut filter, TextureFilter::Nearest, tr("Nearest"));
    }).response.on_hover_text(tr("Base Color interpolation · Nearest preserves hard pixels in imported images, the viewport and exports"));
    if filter != original
        && let Err(error) = state.session.execute(subcraft_app::Command::SetAlbedoFilter { texture_set: id, filter }) {
        state.set_status(error.to_string(), ui.input(|i| i.time));
    }
}

fn grid_control(ui: &mut Ui, prefs: &mut ViewportPrefs) {
    let response = crate::widgets::icon_toggle(
        ui,
        crate::icons::GRID,
        prefs.grid,
        "Floor grid · click to show/hide · right-click for axis settings",
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, prefs.grid, "Floor grid")
    });
    if response.clicked() {
        prefs.grid = !prefs.grid;
    }
    egui::Popup::context_menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.label(RichText::new(tr("Grid & axes")).color(TEXT_DIM));
            ui.checkbox(&mut prefs.grid, tr("Show grid"));
            ui.separator();
            for (index, text, color) in [
                (0, "X · red", egui::Color32::from_rgb(190, 108, 100)),
                (1, "Y · up", egui::Color32::from_rgb(112, 172, 126)),
                (2, "Z · blue", egui::Color32::from_rgb(106, 144, 198)),
            ] {
                ui.checkbox(
                    &mut prefs.grid_axes[index],
                    RichText::new(text).color(color),
                );
            }
            ui.label(
                RichText::new(tr("Y is hidden by default"))
                    .small()
                    .color(TEXT_DIM),
            );
        });
}

pub(crate) fn resolution(ui: &mut Ui, prefs: &mut ViewportPrefs) {
    ui.add_sized([55.0, ui.spacing().interact_size.y], egui::Label::new(RichText::new(tr("Resolution")).color(TEXT_DIM)));
    egui::ComboBox::from_id_salt("viewport_resolution")
        .selected_text(format!("{}%", prefs.scale_percent))
        .width(76.0)
        .show_ui(ui, |ui| {
            for s in SCALES {
                ui.selectable_value(&mut prefs.scale_percent, s, format!("{s}%"));
            }
        })
        .response
        .on_hover_text(tr("Render the 3D view at a lower resolution to make it faster"));
}

/// The view mode dropdown, sectioned like Painter's: Lighting (the material), Single channel (one
/// entry per channel of the selected texture set), the baked-normal views, and Mesh maps.
pub(crate) fn view_mode(ui: &mut Ui, prefs: &mut ViewportPrefs, channels: &[subcraft_core::ChannelKind], wide: bool) {
    dim(ui, "View");
    let mode = prefs.view_mode();
    let shown_channel = prefs.shown_channel();
    let shown = match (prefs.mesh_map, shown_channel) {
        (Some(k), _) => format!("Mesh map: {}", k.display_name()),
        (None, Some(k)) => k.display_name().to_string(),
        (None, None) => mode.label().to_string(),
    };
    egui::ComboBox::from_id_salt("viewport_view_mode")
        .width(if wide { 150.0 } else { 92.0 })
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.label(RichText::new(tr("Lighting")).color(TEXT_DIM));
            if ui.selectable_label(prefs.mesh_map.is_none() && shown_channel.is_none() && mode == ViewMode::Material, ViewMode::Material.label()).clicked() {
                prefs.show_material();
            }
            ui.separator();
            ui.label(RichText::new(tr("Single channel")).color(TEXT_DIM));
            for &k in channels {
                if ui.selectable_label(shown_channel == Some(k), k.display_name()).clicked() {
                    prefs.show_channel(k);
                }
            }
            ui.separator();
            for m in [ViewMode::BakedNormal, ViewMode::BakedNormalMap] {
                if ui.selectable_label(prefs.mesh_map.is_none() && mode == m, m.label()).clicked() {
                    prefs.show_material();
                    prefs.set_view_mode(m);
                }
            }
            ui.separator();
            ui.label(RichText::new(tr("Mesh maps (baked)")).color(TEXT_DIM));
            for k in subcraft_core::MeshMapKind::ALL {
                if ui.selectable_label(prefs.mesh_map == Some(k), k.display_name()).clicked() {
                    prefs.mesh_map = Some(k);
                }
            }
        });
}

/// Cycle the mesh map viewer through the baked maps (B), starting from the material view.
pub fn cycle_mesh_map(prefs: &mut ViewportPrefs) {
    let all = subcraft_core::MeshMapKind::ALL;
    prefs.mesh_map = match prefs.mesh_map {
        None => Some(all[0]),
        Some(k) => all.iter().position(|&m| m == k).and_then(|i| all.get(i + 1)).copied(),
    };
}

/// [`cycle_mesh_map`] backwards (Shift+B): material view, then the last map down to the first.
pub fn cycle_mesh_map_back(prefs: &mut ViewportPrefs) {
    let all = subcraft_core::MeshMapKind::ALL;
    prefs.mesh_map = match prefs.mesh_map {
        None => all.last().copied(),
        Some(k) => all.iter().position(|&m| m == k).and_then(|i| i.checked_sub(1)).map(|i| all[i]),
    };
}

fn dim(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(TEXT_DIM));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_map_cycle_runs_both_ways_through_the_material_view() {
        let all = subcraft_core::MeshMapKind::ALL;
        let mut p = ViewportPrefs::default();
        for kind in all.iter().copied() {
            cycle_mesh_map(&mut p);
            assert_eq!(p.mesh_map, Some(kind));
        }
        cycle_mesh_map(&mut p);
        assert_eq!(p.mesh_map, None, "after the last map the material view returns");
        for kind in all.iter().rev().copied() {
            cycle_mesh_map_back(&mut p);
            assert_eq!(p.mesh_map, Some(kind));
        }
        cycle_mesh_map_back(&mut p);
        assert_eq!(p.mesh_map, None);
    }

    #[test]
    fn texture_set_settings_albedo_dropdown_is_below_size_and_dispatches_an_undoable_edit() {
        let ctx = egui::Context::default(); crate::theme::apply(&ctx);
        let mut state = crate::state::UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.), 128), None);
        let initial = state.session.document().clone();
        let draw = |state: &mut crate::state::UiState, events| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420., 940.))), events, ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| crate::panels::texture_sets::settings(state, ui));
            }); out.textures_delta.clear(); out
        };
        draw(&mut state, vec![]); let output = draw(&mut state, vec![]);
        let text_position = |output: &egui::FullOutput, text: &str| output.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text == text => Some(t.pos + t.galley.size() * 0.5), _ => None,
        }).unwrap_or_else(|| panic!("missing {text}"));
        assert!(text_position(&output, "Albedo").y > text_position(&output, "Size").y);
        let click = |state: &mut crate::state::UiState, pos| {
            draw(state, vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
            draw(state, vec![egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }])
        };
        click(&mut state, text_position(&output, "Bilinear"));
        let menu = draw(&mut state, vec![]);
        click(&mut state, text_position(&menu, "Nearest"));
        assert_eq!(state.session.selected_texture_set().albedo_filter, subcraft_image::TextureFilter::Nearest);
        state.session.execute(subcraft_app::Command::Undo).unwrap();
        assert_eq!(state.session.document(), &initial);
    }

    #[test]
    fn rendering_mode_settings_round_trip_but_the_mode_always_starts_off() {
        let old: ViewportPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!((old.path_tracing, old.pt_bounces, old.pt_max_samples, old.pt_denoise, old.pt_ground), (false, 4, 1024, true, true));
        let p = ViewportPrefs { path_tracing: true, pt_bounces: 7, pt_max_samples: 300, pt_denoise: false, pt_ground: false, ..Default::default() };
        let restored: ViewportPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(restored, ViewportPrefs { path_tracing: false, ..p.clone() }, "everything but the on/off state is kept");
        let s = restored.path_trace_settings(true);
        assert_eq!((s.max_bounces, s.max_samples, s.denoise, s.ground, s.paused), (7, 300, false, false, true));
        let wild = ViewportPrefs { pt_bounces: 0, pt_max_samples: u32::MAX, ..Default::default() }.path_trace_settings(false);
        assert_eq!((wild.max_bounces, wild.max_samples), (1, 1 << 16));
    }

    #[test]
    fn the_new_rendering_settings_default_to_the_old_picture_and_reach_the_path_tracer() {
        // Defaults (also for preference files written before these settings existed) keep the
        // look the Rendering mode always had.
        let old: ViewportPrefs = serde_json::from_str(r#"{"pt_bounces":5}"#).unwrap();
        let s = old.path_trace_settings(false);
        let d = subcraft_render::PathTraceSettings::default();
        assert_eq!((s.min_samples, s.max_time_s, s.firefly_filter, s.clear_color, s.ground_settings), (0, 0.0, true, None, d.ground_settings));
        assert_eq!(old.trace_size([800, 600]), old.render_size([800, 600]), "no override: the view's size");
        // Every setting reaches the path tracer; colors are picked in sRGB and traced linear.
        let p = ViewportPrefs {
            pt_min_samples: 32,
            pt_max_time_s: 90.0,
            pt_firefly: false,
            pt_clear: true,
            pt_clear_color: [1.0, 0.5, 0.0],
            pt_ground_offset: [0.5, -1.0, 2.0],
            pt_ground_reflectivity: [0.5; 3],
            pt_ground_glossiness: 0.8,
            pt_ground_shadow: 0.25,
            pt_ground_below: false,
            pt_override: true,
            pt_width: 1280,
            pt_height: 720,
            ..Default::default()
        };
        let s = p.path_trace_settings(false);
        assert_eq!((s.min_samples, s.max_time_s, s.firefly_filter), (32, 90.0, false));
        let clear = s.clear_color.unwrap();
        assert!((clear[0] - 1.0).abs() < 1e-6 && (clear[1] - 0.214_041).abs() < 1e-4 && clear[2] == 0.0, "linear clear color {clear:?}");
        let g = s.ground_settings;
        assert_eq!((g.offset, g.glossiness, g.shadow_intensity, g.visible_from_below), ([0.5, -1.0, 2.0], 0.8, 0.25, false));
        assert!((g.reflectivity[0] - 0.214_041).abs() < 1e-4);
        assert_eq!(p.trace_size([800, 600]), [1280, 720]);
        // Out-of-range and non-finite values are clamped, never passed on.
        let wild = ViewportPrefs { pt_max_time_s: f32::NAN, pt_ground_glossiness: 7.0, pt_ground_shadow: -1.0, pt_override: true, pt_width: 0, pt_height: 99_999, ..Default::default() };
        let s = wild.path_trace_settings(false);
        assert_eq!((s.max_time_s, s.ground_settings.glossiness, s.ground_settings.shadow_intensity), (0.0, 1.0, 0.0));
        assert_eq!(wild.trace_size([800, 600]), [16, MAX_TRACE_SIDE]);
        assert_eq!(ViewportPrefs { pt_max_time_s: 1e9, ..Default::default() }.path_trace_settings(false).max_time_s, MAX_RENDER_TIME_S);
        assert_eq!(TIME_UNITS, [1.0, 60.0, 3600.0]);
    }

    #[test]
    fn every_channel_of_the_texture_set_can_be_shown_alone_and_cycled() {
        use subcraft_core::ChannelKind as K;
        let channels = [K::BaseColor, K::Height, K::Roughness, K::Emissive, K::Opacity, K::User0];
        let mut p = ViewportPrefs::default();
        assert_eq!(p.shown_channel(), None);
        let mut seen = Vec::new();
        for _ in 0..channels.len() {
            p.cycle_channel(&channels, true);
            seen.push(p.shown_channel().expect("a channel is shown"));
        }
        assert_eq!(seen, channels, "C walks the texture set's channels in order");
        // Channels the fixed view modes cover use them; the others are shown through `solo`.
        p.show_channel(K::Roughness);
        assert_eq!((p.view_mode(), p.solo), (ViewMode::Roughness, None));
        p.show_channel(K::Opacity);
        assert_eq!((p.view_mode(), p.solo, p.shown_channel()), (ViewMode::Material, Some(K::Opacity), Some(K::Opacity)));
        p.cycle_channel(&channels, true);
        assert_eq!(p.shown_channel(), Some(K::User0));
        p.cycle_channel(&channels, true);
        assert_eq!((p.shown_channel(), p.view_mode()), (None, ViewMode::Material), "after the last channel comes the material");
        p.cycle_channel(&channels, false);
        assert_eq!(p.shown_channel(), Some(K::User0), "Shift+C goes backwards");
        // A channel that is not in this texture set: cycling starts from the material again.
        p.show_channel(K::Specular);
        p.cycle_channel(&channels, true);
        assert_eq!(p.shown_channel(), Some(K::BaseColor));
        p.mesh_map = Some(subcraft_core::MeshMapKind::Normal);
        assert_eq!(p.shown_channel(), None, "the mesh map viewer is not a channel view");
        p.show_material();
        assert_eq!((p.mesh_map, p.solo, p.view_mode), (None, None, 0));
    }

    #[test]
    fn grid_and_channel_display_settings_reach_the_renderer() {
        let bounds = subcraft_mesh::Aabb { min: glam::Vec3::splat(-1.0), max: glam::Vec3::splat(1.0) };
        let mut p = ViewportPrefs::default();
        let g = p.grid_params(bounds).expect("the grid is on by default");
        assert_eq!((g.plane, g.axes, g.opacity), (1, [true, false, true], 1.0));
        for (got, want) in g.color.iter().zip(subcraft_render::grid::DEFAULT_GRID_COLOR) {
            assert!((got - want).abs() < 1e-3, "the default color is the gray the grid always had: {:?}", g.color);
        }
        p.grid_plane = subcraft_doc::GridAxis::Z;
        p.grid_opacity = 1.0;
        p.grid_color = [1.0, 0.0, 0.0];
        let g = p.grid_params(bounds).unwrap();
        assert_eq!((g.plane, g.opacity, g.color), (2, 4.0, [1.0, 0.0, 0.0]));
        p.grid = false;
        assert_eq!(p.grid_params(bounds), None);
        (p.solo_unlit, p.hdr_scale, p.hdr_signed_colors, p.channel_component) = (false, 8.0, true, subcraft_doc::ChannelComponent::G);
        let c = p.channel_display();
        assert_eq!((c.unlit, c.hdr_scale, c.signed_colors, c.component), (false, 8.0, true, subcraft_doc::ChannelComponent::G));
    }

    #[test]
    fn render_size_scales_and_never_reaches_zero() {
        let mut p = ViewportPrefs::default();
        assert_eq!(p.render_size([1000, 600]), [1000, 600]);
        p.scale_percent = 50;
        assert_eq!(p.render_size([1000, 600]), [500, 300]);
        p.scale_percent = 25;
        assert_eq!(p.render_size([2, 1]), [1, 1]);
    }

    #[test]
    fn old_preferences_load_with_defaults_and_bad_modes_fall_back() {
        let p: ViewportPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, ViewportPrefs::default());
        assert!(p.grid);
        assert_eq!(p.grid_axes, [true, false, true]);
        let p: ViewportPrefs =
            serde_json::from_str(r#"{"grid":false,"grid_axes":[false,true,false]}"#).unwrap();
        let restored: ViewportPrefs =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(restored, p);
        let p = ViewportPrefs {
            view_mode: 99,
            ..Default::default()
        };
        assert_eq!(p.view_mode(), ViewMode::Material);
        let p = ViewportPrefs {
            view_mode: 2,
            ..Default::default()
        };
        assert_eq!(p.view_mode(), ViewMode::Normal);
    }
}
