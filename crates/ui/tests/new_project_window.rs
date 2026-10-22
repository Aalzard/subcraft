//! The New Project window renders with and without a model, refuses unpaintable models and
//! creates the project (texture sets, remembered choices) when asked.
use subcraft_app::new_project::{NewProjectSettings, ProjectTemplate, summarize};
use subcraft_app::Session;
use subcraft_ui::new_project::LoadedModel;
use subcraft_ui::state::UiState;

fn run(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> usize {
    let mut out = ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), events, ..Default::default() },
        |ui| subcraft_ui::new_project::window(state, ui.ctx()),
    );
    out.textures_delta.clear();
    out.shapes.len()
}

fn loaded(mesh: subcraft_mesh::Mesh) -> LoadedModel {
    let summary = summarize(&mesh, &[]);
    LoadedModel { path: "cube.obj".into(), mesh, summary }
}

fn enter() -> Vec<egui::Event> {
    vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]
}

#[test]
fn window_renders_empty_and_with_a_model() {
    let mut state = UiState::new(Session::sample(), None);
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    state.new_project.show_window(&NewProjectSettings::default());
    assert!(run(&mut state, &ctx, vec![]) > 0);
    state.new_project.model = Some(loaded(subcraft_mesh::primitives::cube(2.0)));
    assert!(run(&mut state, &ctx, vec![]) > 0);
    assert!(state.new_project.open);
}

#[test]
fn enter_creates_the_project_with_the_chosen_template_and_remembers_it() {
    let mut state = UiState::new(Session::sample(), None);
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    state.new_project.show_window(&NewProjectSettings::default());
    state.new_project.settings.apply_template(ProjectTemplate::UnrealOrm);
    state.new_project.settings.resolution = 512;
    state.new_project.model = Some(loaded(subcraft_mesh::primitives::cube(2.0)));
    run(&mut state, &ctx, vec![]);
    run(&mut state, &ctx, enter());
    assert!(!state.new_project.open);
    let sets = &state.session.document().texture_sets;
    assert!(!sets.is_empty() && sets.iter().all(|s| s.resolution == 512));
    assert!(sets[0].channels.iter().any(|c| c.kind == subcraft_core::ChannelKind::AmbientOcclusion));
    assert_eq!(state.recovery.prefs.new_project.template, ProjectTemplate::UnrealOrm);
    assert_eq!(state.session.settings().normal_format, Some(subcraft_doc::NormalFormat::DirectX));
}

#[test]
fn a_model_without_uvs_cannot_be_created() {
    let mut state = UiState::new(Session::sample(), None);
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    let before = state.session.document().clone();
    let mut mesh = subcraft_mesh::primitives::cube(2.0);
    for uv in mesh.uvs.iter_mut() { *uv = glam::Vec2::ZERO; }
    state.new_project.show_window(&NewProjectSettings::default());
    state.new_project.model = Some(loaded(mesh));
    assert!(!state.new_project.model.as_ref().unwrap().summary.warnings.is_empty());
    run(&mut state, &ctx, vec![]);
    run(&mut state, &ctx, enter());
    assert!(state.new_project.open, "creation refused");
    assert_eq!(state.session.document(), &before);
}
