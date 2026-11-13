//! File > Export to Blender: a small window (texture preset, include textures, Blender path),
//! then [`subcraft_app::blender::send`]. While the link is open, a sculpt sent back from Blender
//! ("Send to SubCraft" in Blender's SubCraft sidebar tab) becomes the Bake window's high poly.

use egui::RichText;
use subcraft_app::blender::SendOptions;

use crate::i18n::tr;
use crate::state::UiState;
use crate::theme::{TEXT_DIM, WARNING};
use crate::widgets::primary_button;

pub struct BlenderUi {
    pub open: bool,
    /// Export preset key (built-in name or user preset id).
    pub preset: String,
    pub include_textures: bool,
    /// Press Export on the next frame (`--capture-tool blender-send`).
    pub auto_send: bool,
    /// Blender found for the path last checked: (path text, result).
    found: Option<(String, Result<std::path::PathBuf, String>)>,
}

impl Default for BlenderUi {
    fn default() -> Self {
        Self { open: false, preset: subcraft_export::blender::template().name, include_textures: true, auto_send: false, found: None }
    }
}

fn dim(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM)
}

impl BlenderUi {
    /// Blender for `path` (empty = auto-detect), searched again only when the text changes.
    fn blender(&mut self, path: &str) -> &Result<std::path::PathBuf, String> {
        if self.found.as_ref().is_none_or(|(p, _)| p != path) {
            self.found = Some((path.to_string(), subcraft_app::blender::find_blender(path)));
        }
        &self.found.as_ref().expect("set above").1
    }

    /// Forget the search result (the window was reopened: Blender may have been installed).
    pub fn rescan(&mut self) {
        self.found = None;
    }
}

pub fn window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if !state.blender.open {
        return;
    }
    let mut open = true;
    let mut go = false;
    let mut stop = false;
    let lib = state.user_library.clone();
    let presets: Vec<(String, String)> = state.export.presets(lib.as_ref()).all().map(|t| (t.key().to_string(), t.name.clone())).collect();
    let dir = subcraft_app::blender::exchange_dir(&state.session);
    let linked = state.blender_link.as_ref().map(|l| l.return_file().display().to_string());
    crate::widgets::window(ctx, RichText::new(tr("Export to Blender")).strong()).open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
        let b = &mut state.blender;
        let path = &mut state.recovery.prefs.blender_path;
        egui::Grid::new("blender_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(dim(tr("Texture preset")));
            let current = presets.iter().find(|(k, _)| *k == b.preset).map_or(b.preset.clone(), |(_, n)| n.clone());
            ui.add_enabled_ui(b.include_textures, |ui| {
                egui::ComboBox::from_id_salt("blender_preset").width(300.0).selected_text(current).show_ui(ui, |ui| {
                    for (key, name) in &presets {
                        ui.selectable_value(&mut b.preset, key.clone(), name);
                    }
                });
            });
            ui.end_row();
            ui.label("");
            ui.vertical(|ui| {
                ui.checkbox(&mut b.include_textures, tr("Include textures"));
                ui.label(dim(tr("Off: mesh only, ready for sculpting (Multires) in Blender")).small());
            });
            ui.end_row();
            ui.label(dim(tr("Blender")));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(path).hint_text(tr("Auto-detect")).desired_width(250.0));
                if ui.button(tr("Browse…")).clicked()
                    && let Some(p) = rfd::FileDialog::new().set_title(tr("Blender program")).pick_file()
                {
                    *path = p.display().to_string();
                }
            });
            ui.end_row();
            ui.label("");
            match b.blender(path) {
                Ok(p) => ui.label(dim(&format!("✔ {}", p.display())).small()),
                Err(e) => ui.label(RichText::new(e.as_str()).small().color(WARNING)),
            };
            ui.end_row();
            ui.label(dim(tr("Exchange folder")));
            ui.label(dim(&dir.display().to_string()).small());
            ui.end_row();
        });
        if let Some(ret) = &linked {
            ui.add_space(6.0);
            ui.separator();
            ui.label(RichText::new(tr("Linked to Blender. In Blender, open the SubCraft tab of the sidebar (N) and click “Send to SubCraft”: the sculpt becomes the high poly in the Bake window.")).small());
            ui.horizontal(|ui| {
                ui.label(dim(ret).small());
                stop = ui.small_button(tr("Stop link")).clicked();
            });
        }
        ui.add_space(8.0);
        let ready = b.blender(path).is_ok();
        go = ui
            .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.add_enabled_ui(ready, |ui| primary_button(ui, format!("{}  {}", crate::icons::EXPORT, tr("Export and open Blender")))).inner.clicked())
            .inner;
    });
    if std::mem::take(&mut state.blender.auto_send) {
        go = true;
    }
    if stop {
        state.blender_link = None;
        state.set_status("Stopped the Blender link", now);
    }
    if go {
        let lib = state.user_library.clone();
        let key = state.blender.preset.clone();
        let template = state.export.presets(lib.as_ref()).find(&key).cloned().unwrap_or_else(subcraft_export::blender::template);
        let mut settings = state.export.settings.clone();
        settings.template = template;
        settings.view_2d = None;
        let options = SendOptions { settings, include_textures: state.blender.include_textures, blender_path: state.recovery.prefs.blender_path.clone() };
        match subcraft_app::blender::send(&mut state.session, &dir, &options) {
            Ok(link) => {
                let n = link.files().textures.len();
                state.set_status(format!("Opening Blender with {} and {n} texture(s); \"Send to SubCraft\" there brings a sculpt back as the bake high poly", link.files().mesh.display()), now);
                state.blender_link = Some(link);
                open = false;
            }
            Err(e) => state.set_status(format!("Export to Blender failed: {e}"), now),
        }
    }
    state.blender.open = open;
}

/// Pick up a sculpt sent back from Blender (call once a frame): it becomes the Bake window's
/// high poly and the Bake window opens on it.
pub fn poll(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(link) = &mut state.blender_link else { return };
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(subcraft_app::blender::POLL_INTERVAL));
    let Some(path) = link.poll(now) else { return };
    use_as_high_poly(state, &path, now);
}

/// Put `path` in the Bake window's high-poly field and open the window.
pub fn use_as_high_poly(state: &mut UiState, path: &std::path::Path, now: f64) {
    state.bake.high_poly = path.display().to_string();
    state.bake.open = true;
    state.set_status(format!("Sculpt received from Blender: {} is now the bake high poly", path.display()), now);
    log::info!("blender: high poly {}", path.display());
}

