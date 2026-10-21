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
    pub fn load(path: &Path) -> Result<Self, String> {
        // Accept models without UVs here so the dialog can explain; creating a project refuses them.
        let report = subcraft_mesh::import_mesh_for_baking(path).map_err(|e| e.to_string())?;
        if report.mesh.triangle_count() == 0 {
            return Err("the file has no triangles".into());
        }
        let summary = summarize(&report.mesh, &report.warnings);
        Ok(Self { path: path.to_path_buf(), mesh: report.mesh, summary })
    }
}

#[derive(Default)]
pub struct NewProjectUi {
    pub open: bool,
    pub model: Option<LoadedModel>,
    pub high: Option<LoadedModel>,
    pub error: Option<String>,
    pub settings: NewProjectSettings,
    pub advanced: bool,
}

impl NewProjectUi {
    /// Open the window, starting from the choices made last time.
    pub fn show_window(&mut self, remembered: &NewProjectSettings) {
        self.open = true;
        self.settings = NewProjectSettings { high_poly: None, resolution_overrides: Default::default(), ..remembered.clone() };
        self.model = None;
        self.high = None;
        self.error = None;
    }

    pub fn load_model(&mut self, path: &Path) {
        match LoadedModel::load(path) {
            Ok(model) => {
                self.error = None;
                self.settings.resolution_overrides.clear();
                self.model = Some(model);
            }
            Err(e) => self.error = Some(format!("Could not read {}: {e}", file_name(path))),
        }
    }

    pub fn load_high_poly(&mut self, path: &Path) {
        match LoadedModel::load(path) {
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
    let screen_width = ctx.content_rect().width();
    egui::Window::new("New Project")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_width(640.0_f32.min((screen_width - 32.0).max(260.0)));
            let np = &mut state.new_project;
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 150.0).max(240.0)).show(ui, |ui| {
                model_section(ui, np, &mut pick_model);
                ui.add_space(8.0);
                project_section(ui, np);
                ui.add_space(8.0);
                high_poly_section(ui, np, &mut pick_high);
                ui.add_space(8.0);
                egui::CollapsingHeader::new("Advanced").default_open(np.advanced).show(ui, |ui| advanced_section(ui, np));
            });
            if let Some(error) = &np.error {
                ui.add_space(6.0);
                ui.colored_label(egui::Color32::from_rgb(232, 120, 110), error);
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
                    ui.label(RichText::new(hint).color(TEXT_DIM));
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
        crate::files::pick(ctx, crate::files::Purpose::NewProjectModel, crate::files::MESHES, cfg!(target_arch = "wasm32"), Some("Choose a model"));
    }
    if pick_high {
        crate::files::pick(ctx, crate::files::Purpose::NewProjectHighPoly, crate::files::MESHES, cfg!(target_arch = "wasm32"), Some("Choose the high-poly model"));
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
                if ui.button(if np.model.is_some() { "Change…" } else { "Choose model…" }).on_hover_text("OBJ, FBX, glTF or GLB").clicked() {
                    *pick = true;
                }
            });
        });
        if let Some(model) = &np.model {
            summary_block(ui, &model.summary);
        } else {
            ui.add_space(4.0);
            ui.label(RichText::new("OBJ, FBX, glTF or GLB with UVs. You will see what is inside before anything is created.").color(TEXT_DIM));
        }
    });
}

fn summary_block(ui: &mut Ui, s: &MeshSummary) {
    ui.add_space(6.0);
    ui.label(RichText::new(format!("{}  ·  {} triangles", s.name, group_digits(s.triangles))).font(theme::font_strong(15.0)));
    egui::Grid::new("np_summary").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
        let materials = s.materials.iter().map(|(n, t)| format!("{n} ({})", group_digits(*t))).collect::<Vec<_>>().join(", ");
        ui.label(RichText::new("Materials").color(TEXT_DIM));
        ui.label(format!("{}: {}", s.materials.len(), materials));
        ui.end_row();
        let objects = s.objects.iter().map(|(n, t)| format!("{n} ({})", group_digits(*t))).collect::<Vec<_>>().join(", ");
        ui.label(RichText::new("Objects").color(TEXT_DIM));
        ui.label(format!("{}: {}", s.objects.len(), objects));
        ui.end_row();
        ui.label(RichText::new("UV space").color(TEXT_DIM));
        ui.label(match s.uv_bounds {
            Some((lo, hi)) if s.has_uvs => format!("u {:.2} to {:.2}, v {:.2} to {:.2}  ·  {} tile{}", lo.x, hi.x, lo.y, hi.y, s.tiles.len(), if s.tiles.len() == 1 { "" } else { "s" }),
            _ => "none".into(),
        });
        ui.end_row();
        if s.uses_udim() {
            ui.label(RichText::new("UDIM tiles").color(TEXT_DIM));
            ui.label(s.tiles.iter().map(|t| format!("{} ({})", t.number, group_digits(t.triangles))).collect::<Vec<_>>().join(", "));
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
            ui.label(RichText::new(np.settings.template.description()).small().color(TEXT_DIM));
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
                ui.label(format!("{}  ·  {} triangles  ·  {} px", set.name, group_digits(set.triangles), np.settings.resolution_for(&set.name)));
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
                ui.label(RichText::new("No UVs: fine for baking, the high poly only needs its shape.").color(TEXT_DIM));
            }
            ui.label(RichText::new("The Bake window opens with this model after the project is created.").color(TEXT_DIM));
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
        ui.label(RichText::new("Resolution per texture set").color(TEXT_DIM));
        for set in plan_sets(&model.mesh, &np.settings) {
            ui.horizontal(|ui| {
                ui.label(&set.name);
                let mut current = np.settings.resolution_for(&set.name);
                egui::ComboBox::from_id_salt(("np_set_res", set.name.clone())).selected_text(format!("{current}")).show_ui(ui, |ui| {
                    for r in PROJECT_RESOLUTIONS {
                        ui.selectable_value(&mut current, r, format!("{r}"));
                    }
                });
                if current == np.settings.resolution {
                    np.settings.resolution_overrides.remove(&set.name);
                } else {
                    np.settings.resolution_overrides.insert(set.name.clone(), current);
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
    match subcraft_app::Session::from_mesh_with(model.mesh.clone(), &settings) {
        Ok(session) => {
            state.set_session(session);
            state.welcome.remember(&model.path);
            state.recovery.prefs.new_project = NewProjectSettings { high_poly: None, resolution_overrides: Default::default(), ..settings.clone() };
            if let Some(high) = state.new_project.high.take() {
                state.bake.high_poly = high.path.display().to_string();
                state.bake.open = true;
            }
            let sets = state.session.document().texture_sets.len();
            state.set_status(format!("Created {} ({} texture set{})", model.summary.name, sets, if sets == 1 { "" } else { "s" }), now);
            state.new_project.open = false;
            state.new_project.error = None;
        }
        Err(e) => {
            state.new_project.error = Some(e);
            state.new_project.model = Some(model);
        }
    }
}
