//! Texture Set List header and rows: visibility menu, Focus mode, descriptions, Reset name.
use glam::{Vec2, Vec3};
use subcraft_app::{Action, Command, Session, SetsVisibility};
use subcraft_mesh::{Corner, Mesh, MeshBuilder};

/// Three quads, one material each, so the project has three texture sets.
fn three_materials() -> Mesh {
    let mut b = MeshBuilder::new("Parts");
    for (i, name) in ["Body", "Glass", "Trim"].into_iter().enumerate() {
        let m = b.material(name);
        let origin = Vec3::new(2.0 * i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    b.build().unwrap()
}

fn sets(s: &Session) -> Vec<subcraft_core::TextureSetId> { s.document().texture_sets.iter().map(|t| t.id).collect() }
fn hidden(s: &Session) -> Vec<bool> { sets(s).iter().map(|&id| s.is_set_hidden(id)).collect() }

#[test]
fn show_all_hide_all_and_invert_change_view_state_only() {
    let mut s = Session::new(three_materials(), 64);
    let ids = sets(&s);
    let steps = s.history().entries().len();
    s.execute(Command::SetTextureSetHidden { texture_set: ids[1], hidden: true }).unwrap();
    s.execute(Command::SetTextureSetsVisibility(SetsVisibility::Invert)).unwrap();
    assert_eq!(hidden(&s), [true, false, true]);
    s.execute(Command::SetTextureSetsVisibility(SetsVisibility::HideAll)).unwrap();
    assert_eq!(hidden(&s), [true, true, true]);
    s.execute(Command::SetTextureSetsVisibility(SetsVisibility::Invert)).unwrap();
    assert_eq!(hidden(&s), [false, false, false]);
    s.execute(Command::SetTextureSetsVisibility(SetsVisibility::HideAll)).unwrap();
    s.execute(Command::SetTextureSetsVisibility(SetsVisibility::ShowAll)).unwrap();
    assert_eq!(hidden(&s), [false, false, false]);
    assert_eq!(s.history().entries().len(), steps, "visibility is not an undo step");
}

#[test]
fn focus_mode_shows_only_the_selected_set_and_follows_the_selection() {
    let mut s = Session::new(three_materials(), 64);
    let ids = sets(&s);
    s.execute(Command::SelectTextureSet(ids[0])).unwrap();
    assert!(!s.texture_set_focus());
    s.execute(Command::SetTextureSetFocus(true)).unwrap();
    assert!(s.texture_set_focus());
    assert_eq!(hidden(&s), [false, true, true]);
    s.execute(Command::SelectTextureSet(ids[2])).unwrap();
    assert_eq!(hidden(&s), [true, true, false], "the focus follows the selected set");
    s.execute(Command::SetTextureSetFocus(false)).unwrap();
    assert_eq!(hidden(&s), [false, false, false], "leaving Focus mode shows the others again");
    // The visibility menu leaves Focus mode.
    s.execute(Command::SetTextureSetFocus(true)).unwrap();
    s.execute(Command::SetTextureSetsVisibility(SetsVisibility::ShowAll)).unwrap();
    assert!(!s.texture_set_focus());
    assert_eq!(hidden(&s), [false, false, false]);
}

#[test]
fn description_is_one_undo_step_and_is_saved_with_the_project() {
    let mut s = Session::new(three_materials(), 64);
    let ids = sets(&s);
    s.execute(Command::SetTextureSetDescription { texture_set: ids[1], description: "Windows, tinted  ".into() }).unwrap();
    assert_eq!(s.document().texture_sets[1].description, "Windows, tinted");
    let steps = s.history().entries().len();
    // Same text again: no new step.
    s.execute(Command::SetTextureSetDescription { texture_set: ids[1], description: "Windows, tinted".into() }).unwrap();
    assert_eq!(s.history().entries().len(), steps);
    s.perform(Action::Undo).unwrap();
    assert_eq!(s.document().texture_sets[1].description, "");
    s.perform(Action::Redo).unwrap();
    assert_eq!(s.document().texture_sets[1].description, "Windows, tinted");

    let dir = std::env::temp_dir().join(format!("subcraft-tsl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("described.subcraft");
    s.save(Some(&path)).unwrap();
    let back = Session::open_project(&path).unwrap();
    let descriptions: Vec<_> = back.document().texture_sets.iter().map(|t| t.description.clone()).collect();
    assert_eq!(descriptions, ["", "Windows, tinted", ""]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn reset_name_goes_back_to_the_mesh_material_name() {
    let mut s = Session::new(three_materials(), 64);
    let ids = sets(&s);
    assert_eq!(s.original_set_name(ids[2]).as_deref(), Some("Trim"));
    s.execute(Command::RenameTextureSet { texture_set: ids[2], name: "Chrome trim".into() }).unwrap();
    assert_eq!(s.document().texture_sets[2].name, "Chrome trim");
    s.execute(Command::ResetTextureSetName(ids[2])).unwrap();
    assert_eq!(s.document().texture_sets[2].name, "Trim");
    s.perform(Action::Undo).unwrap();
    assert_eq!(s.document().texture_sets[2].name, "Chrome trim", "reset is one undo step");
    let steps = s.history().entries().len();
    s.execute(Command::ResetTextureSetName(ids[1])).unwrap();
    assert_eq!(s.history().entries().len(), steps, "already the original name: nothing to undo");
}
