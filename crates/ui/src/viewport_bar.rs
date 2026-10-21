//! Toolbar row at the top of the 3D view: render resolution, anti-aliasing, orthographic
//! projection and what the model shows (the full material or one channel unlit).

use egui::{RichText, Ui};
use serde::{Deserialize, Serialize};
use subcraft_render::ViewMode;

use crate::theme::{BG_2, TEXT_DIM};

/// Render resolutions offered, in percent of the 3D view's pixels.
pub const SCALES: [u32; 4] = [100, 75, 50, 25];

/// 3D view settings, kept between runs with the other preferences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewportPrefs {
    /// Render resolution in percent of the 3D view's pixels (lower is faster).
    pub scale_percent: u32,
    pub antialias: bool,
    pub orthographic: bool,
    /// Index into [`ViewMode::ALL`].
    pub view_mode: usize,
    /// Display-only floor grid and axis guides. Old preference files receive these defaults.
    pub grid: bool,
    pub grid_axes: [bool; 3],
    pub symmetry_guides: bool,
    /// Index into [`subcraft_render::ToneMap::ALL`].
    pub tone_map: usize,
    pub exposure: f32,
    pub bloom: f32,
    pub bloom_threshold: f32,
    pub ssao: f32,
    /// Ambient-occlusion reach in percent of the model's size.
    pub ssao_radius: f32,
    /// Glow around emissive areas (0 = off) and its reach multiplier.
    pub emission_glow: f32,
    pub glow_radius: f32,
    /// Streaks from bright emission (0 = off), index into [`subcraft_render::GlareMode::ALL`], length 0..1.
    pub glare: f32,
    pub glare_mode: usize,
    pub glare_length: f32,
}

impl Default for ViewportPrefs {
    fn default() -> Self {
        Self {
            scale_percent: 100,
            antialias: true,
            orthographic: false,
            view_mode: 0,
            grid: true,
            grid_axes: [true, false, true],
            symmetry_guides: true,
            tone_map: 0,
            exposure: 1.0,
            bloom: 0.0,
            bloom_threshold: 0.8,
            ssao: 0.0,
            ssao_radius: 3.0,
            emission_glow: 0.35,
            glow_radius: 1.0,
            glare: 0.0,
            glare_mode: 0,
            glare_length: 0.5,
        }
    }
}

impl ViewportPrefs {
    pub fn view_mode(&self) -> ViewMode {
        ViewMode::ALL
            .get(self.view_mode)
            .copied()
            .unwrap_or_default()
    }

    pub fn tone_map(&self) -> subcraft_render::ToneMap {
        subcraft_render::ToneMap::ALL.get(self.tone_map).copied().unwrap_or_default()
    }

    /// Post-processing for a model of bounding-box `diagonal`.
    pub fn post_params(&self, diagonal: f32) -> subcraft_render::PostParams {
        subcraft_render::PostParams {
            tone_map: self.tone_map(),
            bloom: self.bloom.clamp(0.0, 4.0),
            bloom_threshold: self.bloom_threshold.clamp(0.0, 4.0),
            ssao: self.ssao.clamp(0.0, 1.0),
            ssao_radius: (self.ssao_radius.clamp(0.1, 50.0) * 0.01 * diagonal).max(1e-4),
            emission_glow: self.emission_glow.clamp(0.0, 4.0),
            glow_radius: self.glow_radius.clamp(0.25, 8.0),
            glare: self.glare.clamp(0.0, 4.0),
            glare_mode: subcraft_render::GlareMode::ALL.get(self.glare_mode).copied().unwrap_or_default(),
            glare_length: self.glare_length.clamp(0.0, 1.0),
        }
    }

    /// Render-target size for a 3D view of `size` physical pixels; the image is stretched back
    /// to the view, so painting and picking keep working in full-size coordinates.
    pub fn render_size(&self, size: [u32; 2]) -> [u32; 2] {
        let scale = self.scale_percent.clamp(10, 100) as f32 / 100.0;
        size.map(|s| ((s as f32 * scale).round() as u32).max(1))
    }
}

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
            let inline = width >= 1094.0; // five tool icons (Clone and Smudge joined the three paint tools)
            ui.spacing_mut().item_spacing = egui::vec2(7.0, 6.0);
            ui.horizontal(|ui| {
                if width < 520.0 {
                    ui.menu_button("View", |ui| {
                        ui.horizontal(|ui| resolution(ui, &mut state.recovery.prefs.viewport));
                        let prefs = &mut state.recovery.prefs.viewport;
                        ui.checkbox(&mut prefs.antialias, "Anti-aliasing");
                        ui.checkbox(&mut prefs.orthographic, "Orthographic");
                        ui.horizontal(|ui| view_mode(ui, prefs));
                    });
                    grid_control(ui, &mut state.recovery.prefs.viewport);
                } else {
                    view_controls(ui, state);
                }
                if painting && inline {
                    ui.add_space(5.0);
                    ui.separator();
                    ui.add_space(5.0);
                    crate::brush_bar::show(ui, &mut state.tool, &mut settings, &mut options, &mut state.recovery.prefs.viewport.symmetry_guides, width);
                }
            });
            if painting && !inline {
                ui.separator();
                ui.horizontal(|ui| {
                    crate::brush_bar::show(ui, &mut state.tool, &mut settings, &mut options, &mut state.recovery.prefs.viewport.symmetry_guides, width)
                });
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

fn view_controls(ui: &mut Ui, state: &mut crate::state::UiState) {
    resolution(ui, &mut state.recovery.prefs.viewport);
    let prefs = &mut state.recovery.prefs.viewport;
    ui.separator();
    ui.checkbox(&mut prefs.antialias, "Anti-aliasing")
        .on_hover_text("Anti-aliasing · smooth edges (4x MSAA); off is faster");
    ui.checkbox(&mut prefs.orthographic, "Ortho")
        .on_hover_text("Orthographic · no perspective: parallel lines stay parallel");
    ui.separator();
    view_mode(ui, prefs);
    grid_control(ui, prefs);
}

/// Per-set document state: shared by material evaluation, preview and export.
pub fn albedo_filter(ui: &mut Ui, state: &mut crate::state::UiState) {
    use subcraft_image::TextureFilter;
    let ts = state.session.selected_texture_set();
    let (id, original) = (ts.id, ts.albedo_filter);
    let mut filter = original;
    let label_width = 55.0;
    ui.add_sized([label_width, ui.spacing().interact_size.y], egui::Label::new(RichText::new("Albedo").color(TEXT_DIM)));
    egui::ComboBox::from_id_salt("albedo_filter").width(76.0).selected_text(filter.label()).show_ui(ui, |ui| {
        ui.selectable_value(&mut filter, TextureFilter::Bilinear, "Bilinear");
        ui.selectable_value(&mut filter, TextureFilter::Nearest, "Nearest");
    }).response.on_hover_text("Base Color interpolation · Nearest preserves hard pixels in imported images, the viewport and exports");
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
            ui.label(RichText::new("Grid & axes").color(TEXT_DIM));
            ui.checkbox(&mut prefs.grid, "Show grid");
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
                RichText::new("Y is hidden by default")
                    .small()
                    .color(TEXT_DIM),
            );
        });
}

fn resolution(ui: &mut Ui, prefs: &mut ViewportPrefs) {
    ui.add_sized([55.0, ui.spacing().interact_size.y], egui::Label::new(RichText::new("Resolution").color(TEXT_DIM)));
    egui::ComboBox::from_id_salt("viewport_resolution")
        .selected_text(format!("{}%", prefs.scale_percent))
        .width(76.0)
        .show_ui(ui, |ui| {
            for s in SCALES {
                ui.selectable_value(&mut prefs.scale_percent, s, format!("{s}%"));
            }
        })
        .response
        .on_hover_text("Render the 3D view at a lower resolution to make it faster");
}

fn view_mode(ui: &mut Ui, prefs: &mut ViewportPrefs) {
    dim(ui, "View");
    let mut mode = prefs.view_mode();
    egui::ComboBox::from_id_salt("viewport_view_mode")
        .width(92.0)
        .selected_text(mode.label())
        .show_ui(ui, |ui| {
            for m in ViewMode::ALL {
                ui.selectable_value(&mut mode, m, m.label());
            }
        });
    prefs.view_mode = ViewMode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
}

fn dim(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(TEXT_DIM));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_sets_albedo_dropdown_is_below_resolution_and_dispatches_an_undoable_edit() {
        let ctx = egui::Context::default(); crate::theme::apply(&ctx);
        let mut state = crate::state::UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.), 128), None);
        let initial = state.session.document().clone();
        let draw = |state: &mut crate::state::UiState, events| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600., 940.))), events, ..Default::default() }, |ui| {
                egui::CentralPanel::default().show(ui, |ui| crate::panels::texture_sets::show(state, ui));
            }); out.textures_delta.clear(); out
        };
        draw(&mut state, vec![]); let output = draw(&mut state, vec![]);
        let text_position = |output: &egui::FullOutput, text: &str| output.shapes.iter().find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t) if t.galley.job.text == text => Some(t.pos + t.galley.size() * 0.5), _ => None,
        }).unwrap_or_else(|| panic!("missing {text}"));
        assert!(text_position(&output, "Albedo").y > text_position(&output, "RESOLUTION").y);
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
