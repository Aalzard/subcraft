//! File > Export mesh (Painter's Export mesh window): file type, then Without displacement
//! (triangulated) or With displacement/tessellation (subdivision, height scale, recomputed
//! normals). A thin layer over `subcraft_export::mesh_export`.

use std::path::PathBuf;

use egui::RichText;
use subcraft_export::mesh_export::{Displacement, MAX_SUBDIVISIONS, MeshExport};
use subcraft_export::scene::SceneFormat;

use crate::i18n::tr;
use crate::state::UiState;
use crate::theme::TEXT_DIM;
use crate::widgets::primary_button;

pub struct ExportMeshUi {
    pub open: bool,
    pub folder: String,
    pub format: usize,
    pub displace: bool,
    pub displacement: Displacement,
}

impl Default for ExportMeshUi {
    fn default() -> Self {
        let folder = std::env::current_dir().map(|d| d.join("exports")).unwrap_or_else(|_| PathBuf::from("exports"));
        Self { open: false, folder: folder.display().to_string(), format: 0, displace: false, displacement: Displacement::default() }
    }
}

fn dim(text: &str) -> RichText {
    RichText::new(text).color(TEXT_DIM)
}

pub fn export_mesh_window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if !state.export_mesh.open {
        return;
    }
    let mut open = true;
    let mut go = false;
    let triangles = state.session.scene().mesh.triangle_count();
    crate::widgets::window(ctx, RichText::new(tr("Export mesh")).strong()).open(&mut open).collapsible(false).default_width(440.0).show(ctx, |ui| {
        let m = &mut state.export_mesh;
        egui::Grid::new("export_mesh_grid").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            ui.label(dim(tr("Output directory")));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut m.folder).desired_width(260.0));
                if ui.button(tr("Browse…")).clicked()
                    && let Some(p) = rfd::FileDialog::new().set_directory(&m.folder).pick_folder()
                {
                    m.folder = p.display().to_string();
                }
            });
            ui.end_row();
            ui.label(dim(tr("File type")));
            let formats = SceneFormat::ALL;
            egui::ComboBox::from_id_salt("export_mesh_format").selected_text(formats[m.format.min(formats.len() - 1)].label()).show_ui(ui, |ui| {
                for (i, f) in formats.iter().enumerate() {
                    ui.selectable_value(&mut m.format, i, f.label());
                }
            });
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.radio_value(&mut m.displace, false, tr("Without displacement/tessellation"));
        ui.indent("no_disp", |ui| {
            ui.add_enabled(false, egui::Checkbox::new(&mut true, tr("Apply triangulation"))).on_disabled_hover_text(tr("SubCraft keeps meshes as triangles, so the mesh is always written triangulated"));
        });
        ui.radio_value(&mut m.displace, true, tr("With displacement/tessellation"));
        ui.add_enabled_ui(m.displace, |ui| {
            ui.indent("disp", |ui| {
                let d = &mut m.displacement;
                egui::Grid::new("export_mesh_disp").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    ui.label(dim(tr("Subdivisions")));
                    ui.add(egui::Slider::new(&mut d.subdivisions, 0..=MAX_SUBDIVISIONS)).on_hover_text(tr("Each level splits every triangle into four before displacing"));
                    ui.end_row();
                    ui.label(dim(tr("Height scale")));
                    ui.add(egui::DragValue::new(&mut d.scale).speed(0.001).range(-10.0..=10.0).max_decimals(4)).on_hover_text(tr("World units per unit of height (the viewport uses the same scale)"));
                    ui.end_row();
                });
                ui.checkbox(&mut d.recompute_normals, tr("Recompute vertex normals"));
                let out = triangles.saturating_mul(4usize.saturating_pow(d.subdivisions));
                ui.label(dim(&format!("{out} triangles")));
            });
        });
        ui.add_space(8.0);
        go = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| primary_button(ui, format!("{}  Export", crate::icons::EXPORT)).clicked()).inner;
    });
    if go {
        let m = &state.export_mesh;
        let options = MeshExport { format: SceneFormat::ALL[m.format.min(SceneFormat::ALL.len() - 1)], displacement: m.displace.then_some(m.displacement) };
        let folder = PathBuf::from(&m.folder);
        match state.session.export_mesh(&folder, &options) {
            Ok(files) => state.set_status(format!("Exported {}", files[0].display()), now),
            Err(e) => state.set_status(format!("Mesh export failed: {e}"), now),
        }
    }
    state.export_mesh.open = open;
}
