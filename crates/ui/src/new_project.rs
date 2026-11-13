//! The New Project window: pick a model, see what is in it, choose how the project starts
//! (template, resolution, texture sets, UDIM, normal format, high poly) and create it.
//! All decisions live in `subcraft_app::new_project`; this file draws and dispatches.

use std::path::{Path, PathBuf};

use egui::{Context, RichText, Ui};
use subcraft_app::new_project::{NewProjectSettings, PROJECT_RESOLUTIONS, ProjectTemplate, SplitRule, MeshSummary, plan_sets, summarize};
use subcraft_core::ChannelKind;
use subcraft_doc::NormalFormat;
use subcraft_mesh::Mesh;

use crate::state::UiState;
use crate::theme::{self, TEXT_DIM, WARNING};

/// A model read for the dialog (the mesh is kept to build the project from).
pub struct LoadedModel {
    pub path: PathBuf,
    pub mesh: Mesh,
    pub summary: MeshSummary,
}

impl LoadedModel {
    pub fn load(path: &Path, unwrap_missing: bool) -> Result<Self, String> {
        // Accept models without UVs here so the dialog can explain; creating a project refuses them.
        let report = subcraft_mesh::import_mesh_for_baking(path).map_err(|e| e.to_string())?;
        if report.mesh.triangle_count() == 0 {
            return Err("the file has no triangles".into());
        }
        let (mut mesh, mut warnings) = (report.mesh, report.warnings);
        // A model without UVs is unwrapped here (Mesh > Auto UV unwrap redoes it with other settings).
        if unwrap_missing {
            mesh = subcraft_mesh::unwrap::unwrap_if_missing(mesh, &mut warnings);
        }
        let summary = summarize(&mesh, &warnings);
        Ok(Self { path: path.to_path_buf(), mesh, summary })
    }
}

pub struct NewProjectUi {
    pub open: bool,
    pub model: Option<LoadedModel>,
    pub high: Option<LoadedModel>,
    pub error: Option<String>,
    pub settings: NewProjectSettings,
    pub advanced: bool,
    /// Already-baked mesh maps to put into the new texture sets (matched to `plan_sets`).
    pub mesh_maps: Vec<subcraft_app::mesh_map_import::MeshMapFile>,
    /// Derive curvature from an imported normal map when no curvature map is given.
    pub derive_curvature: bool,
    /// The planned texture sets the mesh maps were last matched to: they are re-matched only
    /// when these change, so a set chosen by hand in the table stays chosen.
    matched_keys: Vec<subcraft_app::mesh_map_import::SetKey>,
}

impl Default for NewProjectUi {
    fn default() -> Self {
        Self {
            open: false,
            model: None,
            high: None,
            error: None,
            settings: NewProjectSettings::default(),
            advanced: false,
            mesh_maps: Vec::new(),
            // On whichever way the window is opened (`show_window` or a launch option).
            derive_curvature: true,
            matched_keys: Vec::new(),
        }
    }
}

impl NewProjectUi {
    /// Open the window, starting from the choices made last time.
    pub fn show_window(&mut self, remembered: &NewProjectSettings) {
        self.open = true;
        self.settings = NewProjectSettings { high_poly: None, resolution_overrides: Default::default(), ..remembered.clone() };
        self.model = None;
        self.high = None;
        self.error = None;
        self.mesh_maps.clear();
        self.matched_keys.clear();
        self.derive_curvature = true;
    }

    /// The texture sets the project will get, as the mesh-map matcher sees them.
    pub fn planned_set_keys(&self) -> Vec<subcraft_app::mesh_map_import::SetKey> {
        let Some(model) = &self.model else { return Vec::new() };
        plan_sets(&model.mesh, &self.settings).into_iter().map(|p| subcraft_app::mesh_map_import::SetKey {
            name: p.name.clone(),
            udim: p.tile.map(subcraft_mesh::regroup::udim_number),
        }).collect()
    }

    pub fn add_mesh_maps(&mut self, paths: &[PathBuf]) {
        let sets = self.planned_set_keys();
        crate::mesh_map_import::add_files(&mut self.mesh_maps, paths, &sets);
    }

    pub fn load_model(&mut self, path: &Path) {
        match LoadedModel::load(path, true) {
            Ok(model) => {
                self.error = None;
                self.settings.resolution_overrides.clear();
                self.model = Some(model);
            }
            Err(e) => self.error = Some(format!("Could not read {}: {e}", file_name(path))),
        }
    }

    pub fn load_high_poly(&mut self, path: &Path) {
        match LoadedModel::load(path, false) {
            Ok(model) => {
                self.settings.high_poly = Some(path.to_path_buf());
                self.high = Some(model);
            }
            Err(e) => self.error = Some(format!("Could not read the high-poly model: {e}")),
        }
    }

    fn can_create(&self) -> bool {
        self.model.as_ref().is_some_and(|m| m.summary.paintable()) && !self.settings.channels.is_empty()
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("model").to_string()
}

pub fn window(state: &mut UiState, ctx: &Context) {
    if !state.new_project.open {
        return;
    }
    let mut open = true;
    let mut create = false;
    let mut pick_model = false;
    let mut pick_high = false;
    let mut pick_maps = false;
    let screen_width = ctx.content_rect().width();
    egui::Window::new("New Project")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(640.0_f32.min((screen_width - 32.0).max(260.0)));
            ui.spacing_mut().item_spacing.y = 7.0; // this dialog is height-budgeted; keep its original rhythm
            let np = &mut state.new_project;
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 210.0).max(80.0)).show(ui, |ui| {
                model_section(ui, np, &mut pick_model);
                ui.add_space(8.0);
                project_section(ui, np);
                ui.add_space(8.0);
                high_poly_section(ui, np, &mut pick_high);
                ui.add_space(8.0);
                if np.model.is_some() {
                    let keys = np.planned_set_keys();
                    // QA 2026-11: re-matching every frame threw away the set picked in the table
                    // (a file matching no set name reverted to "Choose a texture set").
                    if keys != np.matched_keys {
                        crate::mesh_map_import::rematch(&mut np.mesh_maps, &keys);
                        np.matched_keys = keys.clone();
                    }
                    let names: Vec<String> = keys.into_iter().map(|k| k.name).collect();
                    crate::mesh_map_import::new_project_section(ui, &mut np.mesh_maps, &mut np.derive_curvature, &names, &mut pick_maps);
                    ui.add_space(8.0);
                }
                egui::CollapsingHeader::new("Advanced").default_open(np.advanced).show(ui, |ui| advanced_section(ui, np));
            });
            if let Some(error) = &np.error {
                ui.add_space(6.0);
                egui::ScrollArea::vertical().id_salt("np_error").max_height(48.0).auto_shrink([false, true]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(error).color(egui::Color32::from_rgb(232, 120, 110))).wrap());
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ready = np.can_create();
                let hint = if np.model.is_none() { "Choose a model first" } else if !np.model.as_ref().is_some_and(|m| m.summary.paintable()) { "This model has no UVs, so it cannot be painted" } else { "Create the project (Enter)" };
                if ui.add_enabled(ready, egui::Button::new(RichText::new("Create project").strong())).on_hover_text(hint).clicked() {
                    create = true;
                }
                if ui.button("Cancel").on_hover_text("Esc").clicked() {
                    np.open = false;
                }
                if !ready {
                    ui.add(egui::Label::new(RichText::new(hint).color(TEXT_DIM)).wrap());
                }
            });
        });
    if !open {
        state.new_project.open = false;
    }
    let now = ctx.input(|i| i.time);
    if state.new_project.open && !ctx.egui_wants_keyboard_input() {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.new_project.open = false;
        } else if ctx.input(|i| i.key_pressed(egui::Key::Enter)) && state.new_project.can_create() {
            create = true;
        }
    }
    if pick_model {
        crate::files::pick(ctx, crate::files::Purpose::NewProjectModel, crate::files::MESHES, false, Some("Choose a model"));
    }
    if pick_high {
        crate::files::pick(ctx, crate::files::Purpose::NewProjectHighPoly, crate::files::MESHES, false, Some("Choose the high-poly model"));
    }
    if pick_maps {
        crate::files::pick(ctx, crate::files::Purpose::NewProjectMeshMaps, crate::files::IMAGES, true, Some("Choose baked mesh maps"));
    }
    if create {
        create_project(state, now);
    }
}

fn model_section(ui: &mut Ui, np: &mut NewProjectUi, pick: &mut bool) {
    crate::widgets::card(ui, theme::ACCENT_DIM, |ui| {
        ui.label(RichText::new("Model").font(theme::font_strong(13.0)));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let text = np.model.as_ref().map_or("No model chosen".to_string(), |m| m.path.display().to_string());
            ui.add(egui::Label::new(RichText::new(text).color(if np.model.is_some() { theme::TEXT } else { TEXT_DIM })).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(if np.model.is_some() { "Change…" } else { "Choose model…" }).on_hover_text("OBJ, FBX, glTF, GLB, USD, PLY, STL or Substance Painter .spp").clicked() {
                    *pick = true;
                }
            });
        });
        if let Some(model) = &np.model {
            summary_block(ui, &model.summary);
        } else {
            ui.add_space(4.0);
            ui.label(RichText::new("OBJ, FBX, glTF, GLB, USD, PLY, STL or Substance Painter .spp with UVs. You will see what is inside before anything is created.").color(TEXT_DIM));
        }
    });
}

fn summary_block(ui: &mut Ui, s: &MeshSummary) {
    ui.add_space(6.0);
    ui.add(egui::Label::new(RichText::new(format!("{}  ·  {} triangles", s.name, group_digits(s.triangles))).font(theme::font_strong(15.0))).wrap());
    egui::Grid::new("np_summary").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
        let materials = s.materials.iter().map(|(n, t)| format!("{n} ({})", group_digits(*t))).collect::<Vec<_>>().join(", ");
        ui.label(RichText::new("Materials").color(TEXT_DIM));
        name_list(ui, "np_materials", s.materials.len(), &materials);
        ui.end_row();
        let objects = s.objects.iter().map(|(n, t)| format!("{n} ({})", group_digits(*t))).collect::<Vec<_>>().join(", ");
        ui.label(RichText::new("Objects").color(TEXT_DIM));
        name_list(ui, "np_objects", s.objects.len(), &objects);
        ui.end_row();
        ui.label(RichText::new("UV space").color(TEXT_DIM));
        ui.label(match s.uv_bounds {
            Some((lo, hi)) if s.has_uvs => format!("u {:.2} to {:.2}, v {:.2} to {:.2}  ·  {} tile{}", lo.x, hi.x, lo.y, hi.y, s.tiles.len(), if s.tiles.len() == 1 { "" } else { "s" }),
            _ => "none".into(),
        });
        ui.end_row();
        if s.uses_udim() {
            ui.label(RichText::new("UDIM tiles").color(TEXT_DIM));
            let tiles = s.tiles.iter().map(|t| format!("{} ({})", t.number, group_digits(t.triangles))).collect::<Vec<_>>().join(", ");
            name_list(ui, "np_tiles", s.tiles.len(), &tiles);
            ui.end_row();
        }
    });
    for w in &s.warnings {
        ui.add_space(3.0);
        ui.horizontal_top(|ui| {
            ui.label(RichText::new(crate::icons::WARNING).color(WARNING));
            ui.add(egui::Label::new(RichText::new(w).color(WARNING)).wrap());
        });
    }
}

/// A count plus a long joined list of user names: wrapped, and in a bounded scroll area when long,
/// so a model with dozens of parts cannot widen the window.
fn name_list(ui: &mut Ui, id: &str, count: usize, joined: &str) {
    let text = format!("{count}: {joined}");
    ui.vertical(|ui| {
        ui.set_max_width(ui.available_width());
        if text.len() <= 160 {
            ui.add(egui::Label::new(text).wrap());
        } else {
            egui::ScrollArea::vertical().id_salt(id).max_height(64.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.add(egui::Label::new(text).wrap());
            });
        }
    });
}

fn group_digits(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn project_section(ui: &mut Ui, np: &mut NewProjectUi) {
    let uses_udim = np.model.as_ref().is_some_and(|m| m.summary.uses_udim());
    crate::widgets::card(ui, theme::channel_accent(ChannelKind::Roughness), |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new("Project").font(theme::font_strong(13.0)));
        ui.add_space(4.0);
        egui::Grid::new("np_project").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
            ui.label("Template");
            let mut template = np.settings.template;
            egui::ComboBox::from_id_salt("np_template").selected_text(template.label()).width(260.0).show_ui(ui, |ui| {
                for t in ProjectTemplate::ALL {
                    ui.selectable_value(&mut template, t, t.label()).on_hover_text(t.description());
                }
            });
            if template != np.settings.template {
                np.settings.apply_template(template);
            }
            ui.end_row();
            ui.label("");
            ui.add(egui::Label::new(RichText::new(np.settings.template.description()).small().color(TEXT_DIM)).wrap());
            ui.end_row();
            ui.label("Resolution");
            egui::ComboBox::from_id_salt("np_resolution").selected_text(format!("{0} x {0}", np.settings.resolution)).show_ui(ui, |ui| {
                for r in PROJECT_RESOLUTIONS {
                    ui.selectable_value(&mut np.settings.resolution, r, format!("{r} x {r}"));
                }
            }).response.on_hover_text("Texture size of every texture set. Individual sets can differ under Advanced.");
            ui.end_row();
            ui.label("Texture sets");
            egui::ComboBox::from_id_salt("np_split").selected_text(np.settings.split.label()).show_ui(ui, |ui| {
                for rule in SplitRule::ALL {
                    ui.selectable_value(&mut np.settings.split, rule, rule.label());
                }
            }).response.on_hover_text("How the model is divided into separately painted texture sets");
            ui.end_row();
            if uses_udim {
                ui.label("UV tiles");
                ui.checkbox(&mut np.settings.udim, "Use UDIM tiles (one texture set per tile)").on_hover_text("The model's UVs span several 0-1 squares. Off folds them onto one tile, which overlaps islands.");
                ui.end_row();
            }
            ui.label("Cameras");
            ui.checkbox(&mut np.settings.import_cameras, "Import cameras").on_hover_text("Bring the cameras stored in the model file (glTF, GLB or FBX) into the project; pick one from the 3D view's camera menu.");
            ui.end_row();
            ui.label("Normal maps");
            let mut format = np.settings.normal_format;
            egui::ComboBox::from_id_salt("np_normal").selected_text(format.label()).show_ui(ui, |ui| {
                ui.selectable_value(&mut format, NormalFormat::OpenGl, NormalFormat::OpenGl.label());
                ui.selectable_value(&mut format, NormalFormat::DirectX, NormalFormat::DirectX.label());
            }).response.on_hover_text("Convention of exported normal maps. The viewport always shows OpenGL; Unreal expects DirectX.");
            np.settings.normal_format = format;
            ui.end_row();
        });
        if let Some(model) = &np.model {
            ui.add_space(6.0);
            ui.label(RichText::new("Texture sets that will be created").color(TEXT_DIM));
            for set in plan_sets(&model.mesh, &np.settings) {
                ui.add(egui::Label::new(format!("{}  ·  {} triangles  ·  {} px", set.name, group_digits(set.triangles), np.settings.resolution_for(&set.base))).truncate());
            }
        }
    });
}

fn high_poly_section(ui: &mut Ui, np: &mut NewProjectUi, pick: &mut bool) {
    crate::widgets::card(ui, theme::channel_accent(ChannelKind::Normal), |ui| {
        ui.label(RichText::new("High-poly model (optional)").font(theme::font_strong(13.0)));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let text = np.high.as_ref().map_or("None: no baking from a high poly".to_string(), |m| m.path.display().to_string());
            ui.add(egui::Label::new(RichText::new(text).color(if np.high.is_some() { theme::TEXT } else { TEXT_DIM })).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if np.high.is_some() && ui.small_button("Remove").clicked() {
                    np.high = None;
                    np.settings.high_poly = None;
                }
                if ui.button("Choose…").on_hover_text("A detailed model to bake normals, AO and more from").clicked() {
                    *pick = true;
                }
            });
        });
        if let Some(high) = &np.high {
            ui.label(RichText::new(format!("{} triangles", group_digits(high.summary.triangles))).color(TEXT_DIM));
            if !high.summary.has_uvs {
                ui.add(egui::Label::new(RichText::new("No UVs: fine for baking, the high poly only needs its shape.").color(TEXT_DIM)).wrap());
            }
            ui.add(egui::Label::new(RichText::new("Baking mode opens with this model after the project is created.").color(TEXT_DIM)).wrap());
        }
    });
}

fn advanced_section(ui: &mut Ui, np: &mut NewProjectUi) {
    np.advanced = true;
    ui.label(RichText::new("Channels").color(TEXT_DIM));
    ui.horizontal_wrapped(|ui| {
        for kind in [ChannelKind::BaseColor, ChannelKind::Height, ChannelKind::Roughness, ChannelKind::Metallic, ChannelKind::Normal, ChannelKind::AmbientOcclusion, ChannelKind::Emissive] {
            let mut on = np.settings.has_channel(kind);
            let response = ui.add_enabled(kind != ChannelKind::BaseColor, egui::Checkbox::new(&mut on, kind.display_name()));
            if response.changed() {
                np.settings.set_channel(kind, on);
            }
        }
    });
    ui.add_space(6.0);
    if let Some(model) = &np.model {
        ui.label(RichText::new("Resolution per material (all tiles of a material share it)").color(TEXT_DIM));
        let mut bases: Vec<String> = Vec::new();
        for set in plan_sets(&model.mesh, &np.settings) {
            if !bases.contains(&set.base) {
                bases.push(set.base);
            }
        }
        for base in bases {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(&base).truncate());
                let mut current = np.settings.resolution_for(&base);
                egui::ComboBox::from_id_salt(("np_set_res", base.clone())).selected_text(format!("{current}")).show_ui(ui, |ui| {
                    for r in PROJECT_RESOLUTIONS {
                        ui.selectable_value(&mut current, r, format!("{r}"));
                    }
                });
                if current == np.settings.resolution {
                    np.settings.resolution_overrides.remove(&base);
                } else {
                    np.settings.resolution_overrides.insert(base, current);
                }
            });
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Tangent space: MikkTSpace per vertex, the convention of Unreal, Unity and Blender.").small().color(TEXT_DIM));
}

fn create_project(state: &mut UiState, now: f64) {
    let Some(model) = state.new_project.model.take() else { return };
    let settings = state.new_project.settings.clone();
    // A Painter project brings its own texture sets, baked maps and flattened textures: import
    // those instead of building empty sets from the bare model.
    let is_spp = model.path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("spp"));
    if is_spp {
        // Opens in the background; the session replaces the current one when the model and the
        // flattened textures are read (the rebuilt layers follow).
        crate::spp_import::start(state, &model.path);
        state.recovery.prefs.new_project = NewProjectSettings { high_poly: None, resolution_overrides: Default::default(), ..settings.clone() };
        if let Some(high) = state.new_project.high.take() {
            state.bake.high_poly = high.path.display().to_string();
            state.bake.open = true;
        }
        state.new_project.open = false;
        state.new_project.error = None;
        return;
    }
    let created = subcraft_app::Session::from_mesh_with(model.mesh.clone(), &settings).map(|session| (session, 0usize, None::<String>));
    match created {
        Ok((mut session, warnings, first_warning)) => {
            session.set_mesh_source(&model.path);
            let maps = std::mem::take(&mut state.new_project.mesh_maps);
            let imported = (!maps.is_empty()).then(|| {
                let assignments = session.mesh_map_assignments(&maps);
                session.import_mesh_maps(&assignments, state.new_project.derive_curvature)
            });
            // Cameras are a bonus: a file whose cameras cannot be read still makes the project.
            let cameras = if settings.import_cameras { session.import_mesh_cameras(&model.path) } else { Ok(0) };
            state.set_session(session);
            state.welcome.remember(&model.path);
            state.recovery.prefs.new_project = NewProjectSettings { high_poly: None, resolution_overrides: Default::default(), ..settings.clone() };
            if let Some(high) = state.new_project.high.take() {
                state.bake.high_poly = high.path.display().to_string();
                state.bake.open = true;
            }
            let sets = state.session.document().texture_sets.len();
            let mut status = format!("Created {} ({} texture set{})", model.summary.name, sets, if sets == 1 { "" } else { "s" });
            if let Some(first) = first_warning {
                status = format!("Imported {} with {} note(s): {first}", model.summary.name, warnings);
            }
            if let Some(report) = imported {
                status = format!("{status}. {}", report.summary());
            }
            match cameras {
                Ok(0) => {}
                Ok(n) => status.push_str(&format!(" · {n} camera{} imported (3D view, top right)", if n == 1 { "" } else { "s" })),
                Err(e) => {
                    log::warn!("{e}");
                    status.push_str(" · the model's cameras could not be read");
                }
            }
            state.set_status(status, now);
            state.new_project.open = false;
            state.new_project.error = None;
        }
        Err(e) => {
            state.new_project.error = Some(e);
            state.new_project.model = Some(model);
        }
    }
}

#[cfg(test)]
#[path = "../../mesh/tests/support/spp_fixture.rs"]
mod spp_fixture;

#[cfg(test)]
mod tests {
    use super::*;

    /// Choosing a Painter project in the New Project dialog must bring its textures along, not
    /// only the model (reported: "it imports the model, but no texturing at all").
    #[test]
    fn a_painter_project_created_from_the_dialog_keeps_its_textures() {
        let dir = std::env::temp_dir().join(format!("subcraft-np-spp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Cube.spp");
        std::fs::write(&path, crate::new_project::spp_fixture::project_file()).unwrap();
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        state.new_project.load_model(&path);
        assert!(state.new_project.error.is_none(), "{:?}", state.new_project.error);
        create_project(&mut state, 0.0);
        let ctx = egui::Context::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while state.spp_import.is_some() {
            crate::spp_import::poll(&mut state, &ctx, 0.0);
            assert!(std::time::Instant::now() < deadline, "the import did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let ts = &state.session.document().texture_sets[0];
        assert!(ts.layers.iter().any(|l| l.name == "Painter project (flattened)"), "the flattened Painter layer is missing");
    }

    /// Maps picked in the New Project window land in the new texture set without a bake, and the
    /// window shows one row per file.
    #[test]
    fn mesh_maps_picked_in_the_dialog_are_in_the_new_project() {
        let dir = std::env::temp_dir().join(format!("subcraft-np-maps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut paths = Vec::new();
        for (name, comps, v) in [("plane_normal.png", 3usize, 0.5f32), ("plane_ao.png", 1, 0.25), ("plane_id.png", 3, 0.8)] {
            let path = dir.join(name);
            subcraft_image::save_image(&path, 32, 32, comps, &vec![v; 32 * 32 * comps], subcraft_image::FileFormat::Png8).unwrap();
            paths.push(path);
        }
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let mesh = subcraft_mesh::primitives::plane(2.0, 2);
        let summary = summarize(&mesh, &[]);
        state.new_project.open = true;
        state.new_project.model = Some(LoadedModel { path: dir.join("plane.obj"), mesh, summary });
        state.new_project.settings.resolution = 256;
        state.new_project.add_mesh_maps(&paths);
        assert_eq!(state.new_project.mesh_maps.len(), 3);
        assert!(state.new_project.mesh_maps.iter().all(|f| f.kind.is_some() && f.set == Some(0)));
        // The whole window runs (and keeps the files matched to the planned set); its body is a
        // height-capped scroll area, so labels below the fold are not drawn: the section's own
        // rows are checked by drawing the section alone.
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 1000.0))), ..Default::default() };
        for _ in 0..2 {
            let mut out = ctx.run_ui(input(), |ui| window(&mut state, ui.ctx()));
            out.textures_delta.clear();
        }
        assert!(state.new_project.open, "the window stays open");
        assert!(state.new_project.mesh_maps.iter().all(|f| f.set == Some(0)), "still matched after the window ran");
        // A set chosen in the table (here: cleared) is kept while the planned sets stay the same.
        state.new_project.mesh_maps[2].set = None;
        let mut out = ctx.run_ui(input(), |ui| window(&mut state, ui.ctx()));
        out.textures_delta.clear();
        assert_eq!(state.new_project.mesh_maps[2].set, None, "the window does not re-match every frame");
        state.new_project.mesh_maps[2].set = Some(0);
        let names = vec!["Plane".to_string()];
        let mut pick = false;
        let mut texts = String::new();
        for _ in 0..2 {
            let np = &mut state.new_project;
            let mut out = ctx.run_ui(input(), |ui| crate::mesh_map_import::new_project_section(ui, &mut np.mesh_maps, &mut np.derive_curvature, &names, &mut pick));
            out.textures_delta.clear();
            texts = out.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.text().to_string()) } else { None }).collect::<Vec<_>>().join("|");
        }
        assert!(["plane_normal.png", "plane_ao.png", "plane_id.png", "Baked mesh maps"].iter().all(|t| texts.contains(t)), "the section lists the files: {texts}");
        create_project(&mut state, 0.0);
        let ts = state.session.document().texture_sets[0].id;
        let maps = state.session.mesh_maps(ts).expect("maps imported");
        for kind in [subcraft_core::MeshMapKind::Normal, subcraft_core::MeshMapKind::AmbientOcclusion, subcraft_core::MeshMapKind::Id, subcraft_core::MeshMapKind::Curvature] {
            assert!(maps.get(kind).is_some_and(|m| m.width() == 256), "{kind:?}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A model with dozens of long-named parts, shown on small screens at several scales: the
    /// window must stay inside the screen (the Discord report: thousands of pixels wide).
    #[test]
    fn many_long_named_parts_do_not_widen_the_window() {
        let long = |i: usize| format!("fur_character_body_part_with_a_very_long_object_name_{i:03}");
        let summary = MeshSummary {
            name: "character_with_a_really_long_model_name_that_keeps_going.fbx".into(),
            triangles: 250_000,
            objects: (0..40).map(|i| (long(i), 1000 + i)).collect(),
            materials: (0..40).map(|i| (format!("mat_{}", long(i)), 900 + i)).collect(),
            has_uvs: true,
            warnings: vec!["a very long warning ".repeat(40)],
            ..Default::default()
        };
        for (w, h, ppp) in [(1024.0, 600.0, 1.0), (1280.0, 720.0, 1.5), (960.0, 540.0, 2.0), (1920.0, 1080.0, 1.0)] {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
            state.new_project.open = true;
            state.new_project.error = Some("could not read ".to_string() + &"x".repeat(2000));
            state.new_project.model = Some(LoadedModel { path: PathBuf::from("C:/models/".to_string() + &"folder/".repeat(30) + "char.fbx"), mesh: subcraft_mesh::primitives::cube(2.0), summary: summary.clone() });
            let mut raw = egui::RawInput::default();
            raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w / ppp, h / ppp)));
            let screen = raw.screen_rect.unwrap();
            let mut used = egui::Rect::NOTHING;
            for _ in 0..3 {
                let mut out = ctx.run_ui(raw.clone(), |ui| {
                    window(&mut state, ui.ctx());
                    used = ctx.globally_used_rect();
                });
                out.textures_delta.clear();
            }
            assert!(used.width() <= screen.width() + 16.0 && used.height() <= screen.height() + 16.0, "New Project used {used:?} on a {screen:?} screen at {ppp}x");
        }
    }
}
