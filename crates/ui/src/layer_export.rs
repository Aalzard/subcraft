//! Layer stack right-click > Export flattened / Export mask (Painter's): folder, file name pattern
//! (remembered), and for masks the file type. A thin layer over `Session::export_flattened_layers`
//! and `Session::export_layer_masks`.

use std::path::PathBuf;

use egui::RichText;
use subcraft_core::{ChannelKind, LayerId};
use subcraft_export::layer_export::{FLATTENED_PATTERN, preview_name};
use subcraft_image::FileFormat;

use crate::i18n::tr;
use crate::state::UiState;
use crate::theme::TEXT_DIM;
use crate::widgets::primary_button;

pub struct LayerExportUi {
    pub open: bool,
    /// Layers to export (a multi-selection exports each).
    pub layers: Vec<LayerId>,
    /// Export the layers' masks instead of their content.
    pub masks: bool,
    pub folder: String,
    /// File name pattern, kept between exports.
    pub pattern: String,
    pub mask_format: FileFormat,
}

impl Default for LayerExportUi {
    fn default() -> Self {
        let folder = std::env::current_dir().map(|d| d.join("exports")).unwrap_or_else(|_| PathBuf::from("exports"));
        Self { open: false, layers: Vec::new(), masks: false, folder: folder.display().to_string(), pattern: FLATTENED_PATTERN.into(), mask_format: FileFormat::Png8 }
    }
}

impl LayerExportUi {
    /// Open the window for `layers` (content, or masks with `masks`).
    pub fn show_for(&mut self, layers: Vec<LayerId>, masks: bool, folder: &str) {
        self.open = true;
        self.layers = layers;
        self.masks = masks;
        if !folder.is_empty() {
            self.folder = folder.to_string();
        }
    }
}

fn dim(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM())
}

pub fn layer_export_window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if !state.layer_export.open {
        return;
    }
    let mut open = true;
    let mut go = false;
    let first = state.layer_export.layers.first().and_then(|id| state.session.document().texture_sets.iter().find_map(|ts| ts.find(*id).map(|l| (ts.name.clone(), l.name.clone()))));
    let title = if state.layer_export.masks { tr("Export mask") } else { tr("Export flattened") };
    crate::widgets::window(ctx, RichText::new(title).strong()).open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
        let e = &mut state.layer_export;
        let n = e.layers.len();
        ui.label(dim(&if n == 1 { first.as_ref().map_or_else(String::new, |(_, l)| l.clone()) } else { format!("{n} layers") }));
        ui.add_space(4.0);
        egui::Grid::new("layer_export_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(dim(tr("Output directory")));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut e.folder).desired_width(260.0));
                if ui.button(tr("Browse…")).clicked()
                    && let Some(p) = rfd::FileDialog::new().set_directory(&e.folder).pick_folder()
                {
                    e.folder = p.display().to_string();
                }
            });
            ui.end_row();
            ui.label(dim(tr("File name")));
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(&mut e.pattern).desired_width(300.0).font(egui::TextStyle::Monospace)).on_hover_text(tr("Keywords: $textureSet, $layerName, $srcMap (the channel), $udim, $mesh"));
                if let Some((set, layer)) = &first {
                    let kind = if e.masks { None } else { Some(ChannelKind::BaseColor) };
                    let name = match kind {
                        Some(k) => preview_name(&e.pattern, set, layer, k),
                        None => preview_name(&e.pattern, set, layer, ChannelKind::BaseColor).replace("BaseColor", "mask"),
                    };
                    ui.label(RichText::new(name).monospace().small().color(TEXT_DIM()));
                }
            });
            ui.end_row();
            if e.masks {
                ui.label(dim(tr("File type")));
                ui.horizontal(|ui| {
                    let mut f = Some(e.mask_format);
                    let mut dither = None;
                    crate::export_presets::format_combo(ui, "mask_format", &mut f, &mut dither, None);
                    e.mask_format = f.unwrap_or(e.mask_format);
                });
                ui.end_row();
            }
        });
        if !e.masks {
            ui.label(dim(tr("One file per channel at the texture set's size, 1 texel of padding; height and normal as EXR, other channels as PNG at their bit depth.")));
        }
        ui.add_space(6.0);
        go = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| primary_button(ui, format!("{}  Export", crate::icons::EXPORT)).clicked()).inner;
    });
    if go {
        let e = &state.layer_export;
        let dir = PathBuf::from(&e.folder);
        let result = if e.masks { state.session.export_layer_masks(&e.layers, &dir, &e.pattern, e.mask_format) } else { state.session.export_flattened_layers(&e.layers, &dir, &e.pattern) };
        match result {
            Ok(files) if files.is_empty() => state.set_status("Nothing to export for the chosen layers", now),
            Ok(files) => {
                state.set_status(format!("Exported {} files to {}", files.len(), dir.display()), now);
                open = false;
            }
            Err(err) => state.set_status(format!("Export failed: {err}"), now),
        }
    }
    state.layer_export.open = open;
}
