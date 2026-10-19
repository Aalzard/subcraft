//! Texture-set list, mesh-map bake status and mesh info.

use egui::{RichText, Ui};
use subcraft_app::Command;

use crate::icons;
use crate::state::UiState;
use crate::theme::{self, ACCENT, PAD, TEXT_DIM, TEXT_FAINT};
use crate::widgets::{caption, divider, list_row, row_text};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    let sel = state.session.selection().texture_set;
    egui::ScrollArea::vertical().id_salt("texture_sets").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add_space(6.0);
        for ts in &state.session.document().texture_sets {
            let selected = ts.id == sel;
            let resp = list_row(ui, selected, |ui| {
                ui.label(RichText::new(icons::CUBE_WIRE).font(theme::font(14.0)).color(if selected { ACCENT } else { TEXT_DIM }));
                ui.label(row_text(&ts.name, selected));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{}²  ·  {}", ts.resolution, ts.layer_count())).font(theme::font(11.0)).color(TEXT_FAINT))
                        .on_hover_text("Resolution · layers");
                });
            });
            if resp.clicked() && !selected {
                cmds.push(Command::SelectTextureSet(ts.id));
            }
        }
        ui.add_space(8.0);
        divider(ui);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(PAD, 10)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(caption("Resolution"));
            let current = state.session.selected_texture_set().resolution;
            let mut picked = current;
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("texture_set_resolution")
                    .selected_text(format!("{current} × {current}"))
                    .show_ui(ui, |ui| {
                        for r in subcraft_doc::RESOLUTIONS {
                            ui.selectable_value(&mut picked, r, format!("{r} × {r}"));
                        }
                    })
                    .response
                    .on_hover_text("Working resolution of this texture set. Painted strokes are resampled; Ctrl+Z restores them.");
                let all_same = state.session.document().texture_sets.iter().all(|ts| ts.resolution == current);
                if ui.add_enabled(!all_same, egui::Button::new("Apply to all")).on_hover_text("Use this resolution for every texture set").clicked() {
                    cmds.push(Command::SetResolution { texture_set: None, resolution: current });
                }
            });
            if picked != current {
                cmds.push(Command::SetResolution { texture_set: Some(sel), resolution: picked });
            }
            ui.horizontal(|ui| crate::viewport_bar::albedo_filter(ui, state));
            ui.label(RichText::new("Lower is faster to paint and fill. Raise it before exporting for sharper maps.").small().color(TEXT_FAINT));
        });
        divider(ui);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(PAD, 10)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(caption("Mesh maps"));
            let baked = state.session.mesh_maps(sel).map_or(0, |m| m.len());
            ui.horizontal(|ui| {
                let (icon, text, color) =
                    if baked == 0 { (icons::WARNING, "Not baked".to_string(), theme::WARNING) } else { (icons::CHECK, format!("{baked} maps baked"), TEXT_DIM) };
                ui.label(RichText::new(format!("{icon}  {text}")).color(color));
            });
            if let Some(job) = &state.bake.job {
                ui.add(egui::ProgressBar::new(job.fraction()).show_percentage().fill(ACCENT));
            }
            let w = ui.available_width();
            if ui.add_sized([w, 28.0], egui::Button::new(format!("{}  Bake mesh maps…", icons::MAGIC_WAND))).on_hover_text("Ctrl+B").clicked() {
                state.bake.open = true;
            }
            ui.add_space(6.0);
            ui.label(caption("Mesh"));
            let mesh = &state.session.scene().mesh;
            egui::Grid::new("mesh_info").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                for (k, v) in [("Name", mesh.name.clone()), ("Triangles", mesh.triangle_count().to_string()), ("Vertices", mesh.vertex_count().to_string())] {
                    ui.label(RichText::new(k).color(TEXT_FAINT));
                    ui.label(RichText::new(v).color(TEXT_DIM));
                    ui.end_row();
                }
            });
        });
    });
    state.run(cmds, now);
}
