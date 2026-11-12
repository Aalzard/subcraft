//! Export Textures window: per-map checkboxes, Select all / None / Invert, Alt+click solo, and the
//! UDIM folder checkbox appearing only for projects with two or more tiles.
use subcraft_app::Session;
use subcraft_ui::state::UiState;

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>, _modifiers: egui::Modifiers) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1400.0))), ..Default::default() },
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

/// Issue #93: the Baked Mesh Maps section exports the checked raw maps of every texture set.
#[test]
fn baked_mesh_maps_section_exports_the_checked_maps_of_every_set() {
    use subcraft_app::{BakeRequest, BakeSettings, MeshMapKind};
    let mut session = Session::new(subcraft_mesh::primitives::sample_crate(), 64);
    let sets: Vec<_> = session.document().texture_sets.iter().map(|ts| (ts.id, 0)).collect();
    let mut job = session.start_bake(BakeRequest {
        settings: BakeSettings { size: 64, maps: vec![MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature], ao_samples: 4, ..BakeSettings::default() },
        high_poly: None,
        texture_sets: sets,
    });
    while job.poll(&mut session).is_none() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let dir = std::env::temp_dir().join(format!("subcraft-ui-baked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut state = UiState::new(session, None);
    state.export.open = true;
    state.export.folder = dir.display().to_string();
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 {
        frame(&mut state, &ctx, vec![], Default::default());
    }
    click(&mut state, &ctx, "Baked Mesh Maps", Default::default());
    // Both baked maps start checked; uncheck Curvature.
    click(&mut state, &ctx, "Curvature", Default::default());
    let out = frame(&mut state, &ctx, vec![], Default::default());
    let export = out.shapes.iter().rev().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text().ends_with("Export baked maps") => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    }).expect("Export baked maps button");
    let b = |pressed| egui::Event::PointerButton { pos: export, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(&mut state, &ctx, vec![egui::Event::PointerMoved(export), b(true)], Default::default());
    frame(&mut state, &ctx, vec![b(false)], Default::default());
    let mut files: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files.len(), 2, "AO of Body and Trim, no curvature: {files:?}");
    assert!(files.iter().all(|f| f.contains("_baked_ambient_occlusion.")), "{files:?}");
    assert!(files.iter().any(|f| f.contains("_Body_")) && files.iter().any(|f| f.contains("_Trim_")), "{files:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Painter's Export window: three tabs, Save settings keeps the settings in the project, Cancel
/// goes back to how the window opened, and a texture set can have its own file type.
#[test]
fn tabs_save_settings_cancel_and_texture_set_overrides() {
    let (mut state, ctx) = setup();
    let out = frame(&mut state, &ctx, vec![], Default::default());
    for text in ["Settings", "Output templates", "List of exports", "Global settings", "Reset all", "Save settings", "Cancel"] {
        assert!(position(&out, text).is_some(), "{text}");
    }
    click(&mut state, &ctx, "List of exports", Default::default());
    let out = frame(&mut state, &ctx, vec![], Default::default());
    assert!(position(&out, "Nothing exported yet in this session. Press Export to write the checked maps.").is_some());
    click(&mut state, &ctx, "Output templates", Default::default());
    let out = frame(&mut state, &ctx, vec![], Default::default());
    assert!(position(&out, "Built-in templates are read-only: Duplicate to make an editable copy.").is_some(), "the preset editor");
    click(&mut state, &ctx, "Settings", Default::default());

    // Save settings writes the window's settings into the project.
    state.export.settings.size = Some(512);
    state.export.settings.padding = subcraft_app::Padding::Diffusion(4);
    click(&mut state, &ctx, "Save settings", Default::default());
    let saved = state.session.settings().export.clone().expect("saved");
    assert_eq!((saved.size, saved.padding), (Some(512), Some(subcraft_app::Padding::Diffusion(4))));
    assert!(state.session.is_modified());

    // Changes after that, then Cancel: back to the saved state and the window closes.
    state.export.settings.size = Some(128);
    click(&mut state, &ctx, "Roughness", Default::default());
    click(&mut state, &ctx, "Cancel", Default::default());
    assert!(!state.export.open);
    assert_eq!(state.export.settings.size, Some(512));
    assert!(enabled(&mut state).contains(&"Roughness".to_string()), "map selection restored");

    // A texture set's own file type: only its files change.
    state.export.open = true;
    let id = state.session.document().texture_sets[0].id;
    let name = state.session.document().texture_sets[0].name.clone();
    state.export.set_overrides.insert(id, subcraft_doc::ExportOverrides { format: Some(subcraft_app::FileFormat::Tga8), ..Default::default() });
    frame(&mut state, &ctx, vec![], Default::default());
    let plan = state.session.export_plan(&state.export.settings.clone());
    assert!(plan.iter().filter(|(s, _)| *s == name).all(|(_, p)| p.ends_with(".tga")), "{plan:?}");
    assert!(plan.iter().filter(|(s, _)| *s != name).all(|(_, p)| !p.ends_with(".tga")), "{plan:?}");
    // Its entry in the list is marked; selecting it shows its own settings.
    let out = frame(&mut state, &ctx, vec![], Default::default());
    assert!(position(&out, &format!("{name} *")).is_some());
    click(&mut state, &ctx, &format!("{name} *"), Default::default());
    assert_eq!(state.export.selected_set, Some(id));
    let out = frame(&mut state, &ctx, vec![], Default::default());
    assert!(position(&out, "Settings left on Global follow Global settings.").is_some());
}
