//! Importing already-baked mesh maps: the file table shared by the New Project window and the
//! "Import mesh maps…" window of Texture Set Settings. Recognition, matching and loading live in
//! `subcraft_app::mesh_map_import`; this file draws and dispatches.

use std::path::PathBuf;

use egui::{Context, RichText, Ui};
use subcraft_app::mesh_map_import::{MeshMapFile, SetKey, plan_import};
use subcraft_core::MeshMapKind;

use crate::state::UiState;
use crate::theme::{self, TEXT, TEXT_DIM, WARNING};

/// Maps a user can import (Bent normal is baked only).
pub const IMPORTABLE: [MeshMapKind; 9] = MeshMapKind::ALL;

/// The "Import mesh maps…" window (Texture Set Settings).
#[derive(Default)]
pub struct MeshMapImportUi {
    pub open: bool,
    pub files: Vec<MeshMapFile>,
    /// Derive curvature from an imported normal map when no curvature map comes with it.
    pub derive_curvature: bool,
}

impl MeshMapImportUi {
    pub fn start(&mut self, paths: &[PathBuf], sets: &[SetKey]) {
        self.files = plan_import(paths, sets);
        self.derive_curvature = true;
        self.open = true;
    }
}

/// Add files to a plan (re-matching keeps what the user chose for files already listed).
pub fn add_files(files: &mut Vec<MeshMapFile>, paths: &[PathBuf], sets: &[SetKey]) {
    for f in plan_import(paths, sets) {
        if !files.iter().any(|g| g.path == f.path) {
            files.push(f);
        }
    }
}

/// Re-match the texture set of every file (after the set list changed, e.g. another split rule).
pub fn rematch(files: &mut [MeshMapFile], sets: &[SetKey]) {
    let paths: Vec<PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    for (f, fresh) in files.iter_mut().zip(plan_import(&paths, sets)) {
        f.set = fresh.set;
    }
}

/// How many files would be imported (a kind and a set chosen).
pub fn ready_count(files: &[MeshMapFile]) -> usize {
    files.iter().filter(|f| f.kind.is_some() && f.set.is_some()).count()
}

/// One row per file: name, map kind, texture set, DirectX switch for normal maps, remove.
pub fn file_table(ui: &mut Ui, id: &str, files: &mut Vec<MeshMapFile>, set_names: &[String]) {
    let mut remove = None;
    egui::Grid::new(id).num_columns(5).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
        for (i, f) in files.iter_mut().enumerate() {
            let name = f.path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
            ui.add_sized([180.0, 18.0], egui::Label::new(RichText::new(&name).color(TEXT())).truncate()).on_hover_text(f.path.display().to_string());
            egui::ComboBox::from_id_salt((id, "kind", i)).width(120.0)
                .selected_text(f.kind.map_or("Skip (not a mesh map)", |k| k.display_name()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.kind, None, "Skip");
                    for k in IMPORTABLE {
                        ui.selectable_value(&mut f.kind, Some(k), k.display_name());
                    }
                });
            let set_text = f.set.and_then(|s| set_names.get(s)).map_or("Choose a texture set", |s| s.as_str()).to_string();
            egui::ComboBox::from_id_salt((id, "set", i)).width(130.0).selected_text(RichText::new(set_text).color(if f.set.is_some() { TEXT() } else { WARNING() }))
                .show_ui(ui, |ui| {
                    for (s, n) in set_names.iter().enumerate() {
                        ui.selectable_value(&mut f.set, Some(s), n);
                    }
                });
            if matches!(f.kind, Some(MeshMapKind::Normal | MeshMapKind::BentNormal)) {
                ui.checkbox(&mut f.directx, "DirectX").on_hover_text("The green channel points down (DirectX / Unreal); it is flipped to SubCraft's OpenGL convention");
            } else {
                ui.label("");
            }
            if ui.small_button(crate::icons::X).on_hover_text("Leave this file out").clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        files.remove(i);
    }
}

/// The window opened from Texture Set Settings.
pub fn window(state: &mut UiState, ctx: &Context, now: f64) {
    if !state.mesh_map_import.open {
        return;
    }
    let set_names: Vec<String> = state.session.document().texture_sets.iter().map(|t| t.name.clone()).collect();
    let mut open = true;
    let mut import = false;
    let mut add = false;
    egui::Window::new("Import mesh maps").open(&mut open).collapsible(false).resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let w = &mut state.mesh_map_import;
            ui.add(egui::Label::new(RichText::new("Already-baked maps go straight into the texture sets' mesh-map slots: no re-bake. The map is recognised from the file-name suffix (_normal, _ao, _curvature, _id…), the texture set from its name and UDIM number in the file name.").color(TEXT_DIM())).wrap());
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| file_table(ui, "mm_import", &mut w.files, &set_names));
            ui.add_space(6.0);
            ui.checkbox(&mut w.derive_curvature, "Derive curvature from imported normal maps (when no curvature map is given)");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let n = ready_count(&w.files);
                if ui.add_enabled(n > 0, egui::Button::new(RichText::new(format!("Import {n} map{}", if n == 1 { "" } else { "s" })).strong())).clicked() {
                    import = true;
                }
                if ui.button("Add files…").clicked() { add = true; }
                if ui.button("Cancel").clicked() { w.open = false; }
            });
        });
    if !open {
        state.mesh_map_import.open = false;
    }
    if add {
        crate::files::pick(ctx, crate::files::Purpose::ImportMeshMaps, crate::files::IMAGES, true, Some("Choose baked mesh maps"));
    }
    if import {
        let files = std::mem::take(&mut state.mesh_map_import.files);
        let assignments = state.session.mesh_map_assignments(&files);
        let report = state.session.import_mesh_maps(&assignments, state.mesh_map_import.derive_curvature);
        state.mesh_map_import.open = false;
        state.set_status(report.summary(), now);
        state.recovery.autosaver.request_soon();
    }
}

/// The New Project window's card: optional baked maps for the sets the project will have.
pub fn new_project_section(ui: &mut Ui, files: &mut Vec<MeshMapFile>, derive: &mut bool, set_names: &[String], pick: &mut bool) {
    crate::widgets::card(ui, theme::channel_accent(subcraft_core::ChannelKind::AmbientOcclusion), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Baked mesh maps (optional)").font(theme::font_strong(13.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !files.is_empty() && ui.small_button("Clear").clicked() { files.clear(); }
                if ui.button("Choose…").on_hover_text("Normal, world normal, AO, curvature, position, thickness, ID and height maps baked elsewhere").clicked() {
                    *pick = true;
                }
            });
        });
        if files.is_empty() {
            ui.add(egui::Label::new(RichText::new("None: bake later, or pick maps baked elsewhere (matched by suffix: _normal, _ao, _curvature, _id… and by UDIM tile).").color(TEXT_DIM())).wrap());
            return;
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("np_maps").max_height(150.0).show(ui, |ui| file_table(ui, "np_mesh_maps", files, set_names));
        ui.checkbox(derive, "Derive curvature from imported normal maps (when no curvature map is given)");
        let n = ready_count(files);
        ui.label(RichText::new(format!("{n} of {} file(s) will go into the new texture sets.", files.len())).color(TEXT_DIM()));
    });
}
