//! File > Save as template: a project's setup saved under a name and used for a new project.
use std::path::PathBuf;

use subcraft_app::Session;
use subcraft_app::new_project::{NewProjectSettings, ProjectTemplate};
use subcraft_app::user_templates::{self, UserTemplate};
use subcraft_core::ChannelKind as K;
use subcraft_doc::NormalFormat;
use subcraft_mesh::primitives;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("subcraft-templates-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_project_saved_as_a_template_makes_new_projects_with_its_setup() {
    // A Godot-style project at 1024 with DirectX normals and its own export preset.
    let mut settings = NewProjectSettings { resolution: 1024, ..Default::default() };
    settings.apply_template(ProjectTemplate::Godot);
    settings.normal_format = NormalFormat::DirectX;
    let mut project = Session::from_mesh_with(primitives::sample_crate(), &settings).unwrap();
    let mut ps = project.settings().clone();
    ps.export_template = "My studio preset".into();
    project.set_settings(ps);

    let from_project = project.template_settings();
    assert_eq!(from_project.channels, ProjectTemplate::Godot.channels());
    assert_eq!(from_project.resolution, 1024);
    assert_eq!(from_project.normal_format, NormalFormat::DirectX);
    assert_eq!(from_project.export_template.as_deref(), Some("My studio preset"));

    let dir = tmp("roundtrip");
    let template = UserTemplate::new("  Studio / Godot  ", from_project);
    assert_eq!(template.name, "Studio / Godot");
    let file = user_templates::save(&dir, &template).unwrap();
    assert_eq!(file.file_name().unwrap(), "Studio _ Godot.json");
    user_templates::save(&dir, &UserTemplate::new("Another", NewProjectSettings::default())).unwrap();
    std::fs::write(dir.join("broken.json"), "{ not json").unwrap();
    let listed = user_templates::list(&dir);
    assert_eq!(listed.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Another", "Studio / Godot"], "sorted, unreadable files skipped");
    assert_eq!(listed[1], template);

    // Picking it in New Project keeps what belongs to the model being opened.
    let mut np = NewProjectSettings { high_poly: Some(PathBuf::from("ship_high.fbx")), import_cameras: false, ..Default::default() };
    np.resolution_overrides.insert("Glass".into(), 512);
    listed[1].apply_to(&mut np);
    assert_eq!(np.high_poly, Some(PathBuf::from("ship_high.fbx")));
    assert_eq!(np.resolution_overrides.get("Glass"), Some(&512));
    assert!(!np.import_cameras);
    assert_eq!((np.resolution, np.normal_format), (1024, NormalFormat::DirectX));

    let fresh = Session::from_mesh_with(primitives::cube(2.0), &np).unwrap();
    let set = &fresh.document().texture_sets[0];
    assert_eq!(set.resolution, 1024);
    assert_eq!(set.channel_kinds().collect::<Vec<_>>(), ProjectTemplate::Godot.channels());
    assert!(set.has_channel(K::Emissive));
    assert_eq!(fresh.settings().normal_format, Some(NormalFormat::DirectX));
    assert_eq!(fresh.settings().export_template, "My studio preset");

    // A built-in template picked afterwards brings its own export preset back.
    np.apply_template(ProjectTemplate::UnrealOrm);
    let unreal = Session::from_mesh_with(primitives::cube(2.0), &np).unwrap();
    assert_eq!(unreal.settings().export_template, ProjectTemplate::UnrealOrm.export_template());

    user_templates::delete(&dir, "Another").unwrap();
    assert_eq!(user_templates::list(&dir).len(), 1);
    assert!(user_templates::save(&dir, &UserTemplate::new("   ", NewProjectSettings::default())).is_err(), "a name is needed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn old_remembered_settings_without_an_export_preset_still_load() {
    let json = r#"{"template":"unity","channels":["base_color"],"resolution":512}"#;
    let s: NewProjectSettings = serde_json::from_str(json).unwrap();
    assert_eq!(s.export_template, None);
    assert_eq!(s.resolution, 512);
}
