//! Responsive texture-set list, resolution/filter controls, bake status and mesh info.
use crate::i18n::tr;
use egui::{RichText, Ui};
use subcraft_app::Command;
use crate::{icons, state::UiState, theme::{self, ACCENT, PAD, TEXT_DIM, TEXT_FAINT}};
use crate::widgets::{caption, divider, list_row, row_text};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    if ui.available_width() >= 660.0 {
        let height = ui.available_height().max(0.0);
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.allocate_ui_with_layout(egui::vec2(180.0, (height - 8.0).max(0.0)), egui::Layout::top_down(egui::Align::Min), |ui| {
                    set_list(state, ui, &mut cmds, true);
                });
                ui.separator();
                ui.allocate_ui_with_layout(egui::vec2(230.0, (height - 8.0).clamp(0.0, 80.0)), egui::Layout::top_down(egui::Align::Min), |ui| {
                    resolution(state, ui, &mut cmds, true);
                });
                ui.separator();
                ui.vertical(|ui| maps_and_mesh(state, ui, true));
            });
        });
    } else {
        egui::ScrollArea::vertical().id_salt("texture_sets").auto_shrink([false, false]).show(ui, |ui| {
            set_list(state, ui, &mut cmds, false);
            divider(ui);
            egui::Frame::new().inner_margin(egui::Margin::symmetric(PAD, 10)).show(ui, |ui| resolution(state, ui, &mut cmds, false));
            divider(ui);
            egui::Frame::new().inner_margin(egui::Margin::symmetric(PAD, 10)).show(ui, |ui| maps_and_mesh(state, ui, false));
        });
    }
    state.run(cmds, now);
}

fn set_list(state: &UiState, ui: &mut Ui, cmds: &mut Vec<Command>, bounded: bool) {
    let sel = state.session.selection().texture_set;
    let rename_id = egui::Id::new("texture_set_rename");
    // (set being renamed, text, first frame)
    let mut rename: Option<(subcraft_core::TextureSetId, String, bool)> = ui.data(|d| d.get_temp(rename_id));
    let mut draw = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for ts in &state.session.document().texture_sets {
            let selected = ts.id == sel;
            let hidden = state.session.is_set_hidden(ts.id);
            let resp = list_row(ui, selected, |ui| {
                let (eye, tip) = if hidden { (icons::EYE_SLASH, "Show this texture set in the 3D view") } else { (icons::EYE, "Hide this texture set in the 3D view") };
                if ui.add(egui::Button::new(RichText::new(eye).font(theme::font(14.0)).color(if hidden { TEXT_FAINT } else { TEXT_DIM })).frame(false)).on_hover_text(tip).clicked() {
                    cmds.push(Command::SetTextureSetHidden { texture_set: ts.id, hidden: !hidden });
                }
                let name_width = (ui.available_width() - 70.0).max(20.0);
                if let Some((id, text, first)) = rename.as_mut().filter(|(id, ..)| *id == ts.id) {
                    let _ = id;
                    let r = ui.add_sized([name_width, 18.0], egui::TextEdit::singleline(text));
                    if *first { r.request_focus(); *first = false; }
                    else if r.lost_focus() {
                        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) { cmds.push(Command::RenameTextureSet { texture_set: ts.id, name: text.clone() }); }
                        rename = None;
                    }
                } else {
                    let text = if hidden { row_text(&ts.name, false).color(TEXT_FAINT) } else { row_text(&ts.name, selected) };
                    ui.add_sized([name_width, 18.0], egui::Label::new(text).truncate().selectable(false)).on_hover_text(&ts.name);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{}² · {}", ts.resolution, ts.layer_count())).font(theme::font(10.0)).color(TEXT_FAINT)).on_hover_text(tr("Resolution · layers"));
                });
            });
            if resp.clicked() && !selected { cmds.push(Command::SelectTextureSet(ts.id)); }
            if resp.double_clicked() { rename = Some((ts.id, ts.name.clone(), true)); }
            resp.context_menu(|ui| {
                if ui.button(tr("Rename")).clicked() { rename = Some((ts.id, ts.name.clone(), true)); ui.close(); }
                if ui.button(if hidden { "Show in 3D view" } else { "Hide in 3D view" }).clicked() {
                    cmds.push(Command::SetTextureSetHidden { texture_set: ts.id, hidden: !hidden }); ui.close();
                }
                ui.menu_button(tr("Resolution"), |ui| {
                    for r in subcraft_doc::RESOLUTIONS {
                        if ui.selectable_label(ts.resolution == r, format!("{r} × {r}")).clicked() {
                            cmds.push(Command::SetResolution { texture_set: Some(ts.id), resolution: r }); ui.close();
                        }
                    }
                });
            });
        }
    };
    if bounded {
        egui::ScrollArea::vertical().id_salt("texture_set_names").auto_shrink([false, false]).min_scrolled_height(0.0).show(ui, draw);
    } else { draw(ui); }
    ui.data_mut(|d| match rename { Some(r) => { d.insert_temp(rename_id, r); }, None => { d.remove_temp::<(subcraft_core::TextureSetId, String, bool)>(rename_id); } });
}

fn resolution(state: &mut UiState, ui: &mut Ui, cmds: &mut Vec<Command>, compact: bool) {
    ui.spacing_mut().item_spacing.y = if compact { 4.0 } else { 8.0 };
    let help = "Lower is faster to paint and fill. Raise it before exporting for sharper maps.";
    ui.label(caption("Resolution")).on_hover_text(help);
    let current = state.session.selected_texture_set().resolution;
    let mut picked = current;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("texture_set_resolution").width(126.0)
            .selected_text(format!("{current} × {current}"))
            .show_ui(ui, |ui| {
                for r in subcraft_doc::RESOLUTIONS { ui.selectable_value(&mut picked, r, format!("{r} × {r}")); }
            }).response.on_hover_text(crate::keys::native("Working resolution. Painted strokes are resampled; Ctrl+Z restores them."));
        let all_same = state.session.document().texture_sets.iter().all(|ts| ts.resolution == current);
        if ui.add_enabled(!all_same, egui::Button::new("Apply to all")).on_hover_text(tr("Use this resolution for every texture set")).clicked() {
            cmds.push(Command::SetResolution { texture_set: None, resolution: current });
        }
    });
    if picked != current { cmds.push(Command::SetResolution { texture_set: Some(state.session.selection().texture_set), resolution: picked }); }
    ui.horizontal(|ui| crate::viewport_bar::albedo_filter(ui, state));
    if !compact { ui.label(RichText::new(help).small().color(TEXT_FAINT)); }
}

fn maps_and_mesh(state: &mut UiState, ui: &mut Ui, compact: bool) {
    ui.spacing_mut().item_spacing.y = if compact { 4.0 } else { 8.0 };
    ui.label(caption("Mesh maps"));
    let baked = state.session.mesh_maps(state.session.selection().texture_set).map_or(0, |m| m.len());
    let (icon, text, color) = if baked == 0 { (icons::WARNING, "Not baked".to_string(), theme::WARNING) }
        else { (icons::CHECK, format!("{baked} maps baked"), TEXT_DIM) };
    let status = RichText::new(format!("{icon}  {text}")).small().color(color);
    if compact {
        ui.horizontal(|ui| {
            ui.label(status);
            if ui.button(format!("{} Bake…", icons::MAGIC_WAND)).on_hover_text(crate::keys::native("Bake mesh maps (Ctrl+B)")).clicked() { state.bake.open = true; }
        });
    } else {
        ui.label(status);
        if ui.add_sized([ui.available_width(), 28.0], egui::Button::new(format!("{}  Bake mesh maps…", icons::MAGIC_WAND))).on_hover_text(crate::keys::native("Ctrl+B")).clicked() { state.bake.open = true; }
    }
    if let Some(job) = &state.bake.job { ui.add(egui::ProgressBar::new(job.fraction()).show_percentage().fill(ACCENT)); }
    let mesh = &state.session.scene().mesh;
    if compact {
        ui.add(egui::Label::new(RichText::new(&mesh.name).color(TEXT_DIM)).truncate()).on_hover_text(tr("Mesh name"));
        ui.label(RichText::new(format!("{} tris · {} verts", mesh.triangle_count(), mesh.vertex_count())).small().color(TEXT_FAINT));
    } else {
        ui.add_space(6.0);
        ui.label(caption("Mesh"));
        egui::Grid::new("mesh_info").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
            for (k, v) in [("Name", mesh.name.clone()), ("Triangles", mesh.triangle_count().to_string()), ("Vertices", mesh.vertex_count().to_string())] {
                ui.label(RichText::new(k).color(TEXT_FAINT)); ui.label(RichText::new(v).color(TEXT_DIM)); ui.end_row();
            }
        });
    }
}
