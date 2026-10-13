//! Texture-set list.

use egui::{RichText, Ui};
use subcraft_app::Command;

use crate::state::UiState;

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let mut cmds = Vec::new();
    let sel = state.session.selection().texture_set;
    for ts in &state.session.document().texture_sets {
        let text = format!("{}   {}²  ·  {} layers", ts.name, ts.resolution, ts.layer_count());
        if ui.selectable_label(ts.id == sel, text).clicked() && ts.id != sel {
            cmds.push(Command::SelectTextureSet(ts.id));
        }
    }
    ui.separator();
    let baked = state.session.mesh_maps(sel).map_or(0, |m| m.len());
    ui.horizontal(|ui| {
        if ui.button("Bake mesh maps…").on_hover_text("Ctrl+B").clicked() {
            state.bake.open = true;
        }
        ui.label(RichText::new(if baked == 0 { "not baked".to_string() } else { format!("{baked} maps baked") }).small().weak());
    });
    if let Some(job) = &state.bake.job {
        ui.add(egui::ProgressBar::new(job.fraction()).show_percentage());
    }
    let mesh = &state.session.scene().mesh;
    ui.label(RichText::new(format!("{}: {} triangles, {} vertices", mesh.name, mesh.triangle_count(), mesh.vertex_count())).small().weak());
    state.run(cmds, now);
}
