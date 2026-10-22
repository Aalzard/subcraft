//! Export map checkboxes in the session: saved with the project, applied by every export path,
//! Alt+click solo, normal-map format and the project's UDIM folder choice.
use subcraft_app::Session;
use subcraft_export::ExportSettings;

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-exportsel-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn unchecked_maps_are_not_exported_and_survive_save_and_reopen() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 256);
    let settings = ExportSettings::default();
    let all = s.export_choices(&settings);
    assert!(all.len() >= 4 && all.iter().all(|f| f.enabled));
    s.edit_export_selection(&settings, |sel, all| sel.select_none(all));
    assert!(s.export_plan(&settings).is_empty());
    assert!(s.is_modified(), "a changed selection marks the project as modified");
    let set = all[0].set.clone();
    s.edit_export_selection(&settings, |sel, _| {
        sel.set_on(&set, "Roughness", true);
        sel.set_on(&set, "BaseColor", true);
    });
    let dir = temp("out");
    let files = s.export_textures(&dir.join("a"), &settings).unwrap();
    assert_eq!(files.len(), 2);
    let path = dir.join("p.subcraft");
    s.save(Some(&path)).unwrap();
    let mut reopened = Session::open_project(&path).unwrap();
    let choices = reopened.export_choices(&settings);
    let on: Vec<&str> = choices.iter().filter(|f| f.enabled).map(|f| f.suffix.as_str()).collect();
    assert_eq!(on.len(), 2);
    assert!(on.contains(&"Roughness") && on.contains(&"BaseColor"));
    reopened.export_textures(&dir.join("b"), &settings).unwrap();
    assert_eq!(names(&dir.join("a")).len(), names(&dir.join("b")).len());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn alt_click_solo_through_the_session_and_back() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let settings = ExportSettings::default();
    let total = s.export_choices(&settings).len();
    let f = s.export_choices(&settings).into_iter().find(|f| f.suffix == "Normal").unwrap();
    s.edit_export_selection(&settings, |sel, _| sel.set_on(&f.set, "Roughness", false));
    s.edit_export_selection(&settings, |sel, all| sel.solo(&f.set, "Normal", all));
    let plan = s.export_plan(&settings);
    assert_eq!(plan.len(), 1);
    assert!(plan[0].1.ends_with("_Normal.png"));
    s.edit_export_selection(&settings, |sel, all| sel.solo(&f.set, "Normal", all));
    assert_eq!(s.export_plan(&settings).len(), total - 1, "previous selection restored (Roughness stays off)");
    assert!(!s.export_choices(&settings).iter().find(|c| c.suffix == "Roughness").unwrap().enabled);
}

#[test]
fn project_normal_format_is_applied_to_exports() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    let mut project = s.settings().clone();
    project.normal_format = Some(subcraft_doc::NormalFormat::DirectX);
    s.set_settings(project);
    let applied = s.effective_export_settings(&ExportSettings::default());
    assert_eq!(applied.normal_directx, Some(true));
    let mut project = s.settings().clone();
    project.normal_format = None;
    s.set_settings(project);
    assert_eq!(s.effective_export_settings(&ExportSettings::default()).normal_directx, None);
}
