//! Export Textures window: per-map checkboxes, Select all / None / Invert, Alt+click solo, and the
//! UDIM folder checkbox appearing only for projects with two or more tiles.
use subcraft_app::Session;
use subcraft_ui::state::UiState;

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>, _modifiers: egui::Modifiers) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0))), ..Default::default() },
        |ui| subcraft_ui::dialogs::export_window(state, ui.ctx(), 0.0),
    );
    out.textures_delta.clear();
    out
}

fn position(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().rev().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    })
}

fn click(state: &mut UiState, ctx: &egui::Context, text: &str, modifiers: egui::Modifiers) {
    let out = frame(state, ctx, vec![], modifiers);
    let p = position(&out, text).unwrap_or_else(|| panic!("no {text:?}"));
    let b = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers };
    frame(state, ctx, vec![egui::Event::ModifiersChanged(modifiers), egui::Event::PointerMoved(p), b(true)], modifiers);
    frame(state, ctx, vec![b(false)], modifiers);
    frame(state, ctx, vec![egui::Event::ModifiersChanged(Default::default())], Default::default());
}

fn setup() -> (UiState, egui::Context) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    state.export.open = true;
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 {
        frame(&mut state, &ctx, vec![], Default::default());
    }
    (state, ctx)
}

fn enabled(state: &mut UiState) -> Vec<String> {
    let settings = state.export.settings.clone();
    state.session.export_choices(&settings).into_iter().filter(|f| f.enabled).map(|f| f.suffix).collect()
}

#[test]
fn none_then_one_checkbox_exports_only_that_map_and_all_restores() {
    let (mut state, ctx) = setup();
    assert!(!state.session.is_modified(), "merely opening the window changes nothing");
    let total = enabled(&mut state).len();
    assert!(total >= 4);
    click(&mut state, &ctx, "None", Default::default());
    assert!(enabled(&mut state).is_empty());
    let out = frame(&mut state, &ctx, vec![], Default::default());
    assert!(position(&out, "Check at least one map to export.").is_some(), "hint shown when nothing is checked");
    click(&mut state, &ctx, "Roughness", Default::default());
    assert_eq!(enabled(&mut state), ["Roughness"]);
    click(&mut state, &ctx, "Invert", Default::default());
    assert_eq!(enabled(&mut state).len(), total - 1);
    assert!(state.session.is_modified(), "the selection is saved with the project");
    click(&mut state, &ctx, "Select all", Default::default());
    assert_eq!(enabled(&mut state).len(), total);
}

#[test]
fn alt_click_solos_a_map_and_alt_click_again_restores_the_selection() {
    let (mut state, ctx) = setup();
    let total = enabled(&mut state).len();
    click(&mut state, &ctx, "Metallic", Default::default());
    assert_eq!(enabled(&mut state).len(), total - 1);
    let alt = egui::Modifiers { alt: true, ..Default::default() };
    click(&mut state, &ctx, "Normal", alt);
    assert_eq!(enabled(&mut state), ["Normal"]);
    click(&mut state, &ctx, "Normal", alt);
    let back = enabled(&mut state);
    assert_eq!(back.len(), total - 1);
    assert!(!back.contains(&"Metallic".to_string()));
}

#[test]
fn the_udim_folder_checkbox_is_only_offered_for_two_or_more_tiles() {
    let (mut state, ctx) = setup();
    let out = frame(&mut state, &ctx, vec![], Default::default());
    assert!(position(&out, "Export Each UDIM to a Separate Folder").is_none());
    let mesh = udim_mesh();
    let settings = subcraft_app::new_project::NewProjectSettings::default();
    state.set_session(Session::from_mesh_with(mesh, &settings).unwrap());
    assert!(state.session.udim_tile_count() >= 2);
    let out = frame(&mut state, &ctx, vec![], Default::default());
    assert!(position(&out, "Export Each UDIM to a Separate Folder").is_some());
    click(&mut state, &ctx, "Export Each UDIM to a Separate Folder", Default::default());
    assert!(state.session.settings().udim_folders);
    let paths: Vec<String> = state.session.export_plan(&state.export.settings.clone()).into_iter().map(|(_, p)| p).collect();
    assert!(paths.iter().any(|p| p.starts_with("1001/")) && paths.iter().any(|p| p.starts_with("1002/")), "{paths:?}");
}

/// Two quads on two UDIM tiles (1001 and 1002), one material.
fn udim_mesh() -> subcraft_mesh::Mesh {
    use glam::{Vec2, Vec3};
    use subcraft_mesh::{Corner, MeshBuilder};
    let mut b = MeshBuilder::new("Tiled");
    let m = b.material("Body");
    for (i, shift) in [Vec2::ZERO, Vec2::new(1.0, 0.0)].into_iter().enumerate() {
        let origin = Vec3::new(2.0 * i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(shift + Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    b.build().unwrap()
}
