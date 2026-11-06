//! Responsive texture-set list, resolution/filter controls, bake status and mesh info.
use egui::{RichText, Ui};
use subcraft_app::Command;
use crate::{icons, state::UiState, theme::{self, PAD, TEXT_DIM, TEXT_FAINT}};
use crate::widgets::{caption, list_row, row_text};

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    let height = ui.available_height();
    // The list takes what the resolution row leaves.
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), (height - 46.0).max(24.0)), egui::Layout::top_down(egui::Align::Min), |ui| {
        set_list(state, ui, &mut cmds, true);
    });
    egui::Frame::new().inner_margin(egui::Margin::symmetric(PAD, 4)).show(ui, |ui| resolution(state, ui, &mut cmds, true));
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
                    ui.label(RichText::new(format!("{}² · {}", ts.resolution, ts.layer_count())).font(theme::font(10.0)).color(TEXT_FAINT)).on_hover_text("Resolution · layers");
                });
            });
            if resp.clicked() && !selected { cmds.push(Command::SelectTextureSet(ts.id)); }
            if resp.double_clicked() { rename = Some((ts.id, ts.name.clone(), true)); }
            resp.context_menu(|ui| {
                if ui.button("Rename").clicked() { rename = Some((ts.id, ts.name.clone(), true)); ui.close(); }
                if ui.button(if hidden { "Show in 3D view" } else { "Hide in 3D view" }).clicked() {
                    cmds.push(Command::SetTextureSetHidden { texture_set: ts.id, hidden: !hidden }); ui.close();
                }
                ui.menu_button("Resolution", |ui| {
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
    let current = state.session.selected_texture_set().resolution;
    let mut picked = current;
    if !compact { ui.label(caption("Resolution")).on_hover_text(help); }
    ui.horizontal(|ui| {
        if compact { ui.label(RichText::new("Size").color(TEXT_DIM)).on_hover_text(help); }
        egui::ComboBox::from_id_salt("texture_set_resolution").width(126.0)
            .selected_text(format!("{current} × {current}"))
            .show_ui(ui, |ui| {
                for r in subcraft_doc::RESOLUTIONS { ui.selectable_value(&mut picked, r, format!("{r} × {r}")); }
            }).response.on_hover_text(crate::keys::native("Working resolution. Painted strokes are resampled; Ctrl+Z restores them."));
        let all_same = state.session.document().texture_sets.iter().all(|ts| ts.resolution == current);
        if ui.add_enabled(!all_same, egui::Button::new("Apply to all")).on_hover_text("Use this resolution for every texture set").clicked() {
            cmds.push(Command::SetResolution { texture_set: None, resolution: current });
        }
    });
    if picked != current { cmds.push(Command::SetResolution { texture_set: Some(state.session.selection().texture_set), resolution: picked }); }
    if !compact { ui.label(RichText::new(help).small().color(TEXT_FAINT)); }
}
