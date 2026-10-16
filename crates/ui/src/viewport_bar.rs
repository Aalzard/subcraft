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
}

impl Default for ViewportPrefs {
    fn default() -> Self {
        Self { scale_percent: 100, antialias: true, orthographic: false, view_mode: 0 }
    }
}

impl ViewportPrefs {
    pub fn view_mode(&self) -> ViewMode {
        ViewMode::ALL.get(self.view_mode).copied().unwrap_or_default()
    }

    /// Render-target size for a 3D view of `size` physical pixels; the image is stretched back
    /// to the view, so painting and picking keep working in full-size coordinates.
    pub fn render_size(&self, size: [u32; 2]) -> [u32; 2] {
        let scale = self.scale_percent.clamp(10, 100) as f32 / 100.0;
        size.map(|s| ((s as f32 * scale).round() as u32).max(1))
    }
}

/// Draw the toolbar row, editing `prefs` in place.
pub fn show(ui: &mut Ui, prefs: &mut ViewportPrefs) {
    egui::Frame::new().fill(BG_2).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            dim(ui, "Resolution");
            egui::ComboBox::from_id_salt("viewport_resolution")
                .selected_text(format!("{}%", prefs.scale_percent))
                .width(62.0)
                .show_ui(ui, |ui| {
                    for s in SCALES {
                        ui.selectable_value(&mut prefs.scale_percent, s, format!("{s}%"));
                    }
                })
                .response
                .on_hover_text("Render the 3D view at a lower resolution to make it faster");
            ui.separator();
            ui.checkbox(&mut prefs.antialias, "Anti-aliasing").on_hover_text("Smooth edges (4x MSAA); off is faster");
            ui.checkbox(&mut prefs.orthographic, "Orthographic").on_hover_text("No perspective: parallel lines stay parallel");
            ui.separator();
            dim(ui, "View");
            let mut mode = prefs.view_mode();
            egui::ComboBox::from_id_salt("viewport_view_mode").selected_text(mode.label()).show_ui(ui, |ui| {
                for m in ViewMode::ALL {
                    ui.selectable_value(&mut mode, m, m.label());
                }
            });
            prefs.view_mode = ViewMode::ALL.iter().position(|m| *m == mode).unwrap_or(0);
        });
    });
}

fn dim(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(TEXT_DIM));
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let p = ViewportPrefs { view_mode: 99, ..Default::default() };
        assert_eq!(p.view_mode(), ViewMode::Material);
        let p = ViewportPrefs { view_mode: 2, ..Default::default() };
        assert_eq!(p.view_mode(), ViewMode::Normal);
    }
}
