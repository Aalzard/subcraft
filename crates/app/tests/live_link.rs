//! Live link: engine projects are recognised from their files, a send writes the engine's
//! textures (plus the model once, and Godot materials) into the project's watched asset folder,
//! and the target is saved with the SubCraft project.
use std::path::{Path, PathBuf};

use subcraft_app::Session;
use subcraft_app::live_link::{Engine, LiveLinkTarget};

fn temp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("live-link-{tag}-{}", subcraft_core::ObjectId::new()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn godot_project() -> PathBuf {
    let root = temp("godot");
    std::fs::write(root.join("project.godot"), "config_version=5\n").unwrap();
    root
}

fn session() -> Session {
    let mut s = Session::new(subcraft_mesh::primitives::sample_crate(), 128);
    s.update_composites();
    s
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn engine_projects_are_recognised_from_any_folder_inside_them() {
    let godot = godot_project();
    std::fs::create_dir_all(godot.join("art/props")).unwrap();
    assert_eq!(Engine::detect(&godot.join("art/props")), Some((Engine::Godot, godot.clone())));

    let unity = temp("unity");
    std::fs::create_dir_all(unity.join("Assets/Models")).unwrap();
    std::fs::create_dir_all(unity.join("ProjectSettings")).unwrap();
    assert_eq!(Engine::detect(&unity.join("Assets/Models")), Some((Engine::Unity, unity.clone())));

    let unreal = temp("unreal");
    std::fs::write(unreal.join("Shooter.uproject"), "{}").unwrap();
    std::fs::create_dir_all(unreal.join("Content")).unwrap();
    assert_eq!(LiveLinkTarget::for_folder(&unreal.join("Content")).map(|t| t.engine), Some(Engine::Unreal));

    let plain = temp("plain");
    assert_eq!(Engine::detect(&plain), None);
    for d in [godot, unity, unreal, plain] {
        std::fs::remove_dir_all(d).ok();
    }
}

#[test]
fn a_godot_send_writes_textures_the_model_once_and_materials_that_point_at_them() {
    let root = godot_project();
    let s = session();
    let target = LiveLinkTarget::for_folder(&root).unwrap();
    let report = s.live_link_snapshot(&target, true).send().unwrap();
    let dir = root.join("subcraft").join(subcraft_export::sanitize(&s.export_name()));
    assert_eq!(report.dir, dir);
    let names = files(&dir);
    assert!(names.iter().any(|n| n.ends_with("_ORM.png")), "{names:?}");
    assert!(names.iter().any(|n| n.ends_with("_BaseColor.png")), "{names:?}");
    assert!(report.model.as_ref().is_some_and(|m| m.is_file()), "the model goes along");
    assert_eq!(report.materials.len(), s.document().texture_sets.len());
    for mat in &report.materials {
        let tres = std::fs::read_to_string(mat).unwrap();
        assert!(tres.starts_with("[gd_resource type=\"StandardMaterial3D\""), "{tres}");
        assert!(tres.contains("roughness_texture_channel = 1") && tres.contains("metallic_texture_channel = 2"), "{tres}");
        // Every referenced texture exists at its res:// path.
        let paths: Vec<&str> = tres.lines().filter_map(|l| l.split("path=\"res://").nth(1)).map(|r| r.split('"').next().unwrap()).collect();
        assert!(paths.len() >= 3, "{tres}");
        for p in paths {
            assert!(root.join(p).is_file(), "{p} exists");
        }
    }
    // A later send without the model rewrites textures only.
    let model = report.model.unwrap();
    std::fs::remove_file(&model).unwrap();
    let again = s.live_link_snapshot(&target, false).send().unwrap();
    assert!(again.model.is_none() && !model.exists());
    assert_eq!(again.textures, report.textures);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn unity_and_unreal_get_their_own_maps_in_their_asset_folders() {
    let unity = temp("unity-send");
    std::fs::create_dir_all(unity.join("Assets")).unwrap();
    std::fs::create_dir_all(unity.join("ProjectSettings")).unwrap();
    let unreal = temp("unreal-send");
    std::fs::write(unreal.join("Game.uproject"), "{}").unwrap();
    let s = session();
    let name = subcraft_export::sanitize(&s.export_name());
    let u = s.live_link_snapshot(&LiveLinkTarget::for_folder(&unity).unwrap(), false).send().unwrap();
    assert_eq!(u.dir, unity.join("Assets/SubCraft").join(&name));
    assert!(u.materials.is_empty(), "no generated materials for Unity");
    let names = files(&u.dir);
    assert!(names.iter().any(|n| n.ends_with("_MetallicSmoothness.png")), "URP maps: {names:?}");
    let e = s.live_link_snapshot(&LiveLinkTarget::for_folder(&unreal).unwrap(), false).send().unwrap();
    assert_eq!(e.dir, unreal.join("Content/SubCraft").join(&name));
    assert!(files(&e.dir).iter().any(|n| n.contains("OcclusionRoughnessMetallic") || n.contains("ORM")), "{:?}", files(&e.dir));
    std::fs::remove_dir_all(unity).ok();
    std::fs::remove_dir_all(unreal).ok();
}

#[test]
fn the_target_is_saved_with_the_project_and_a_missing_engine_folder_is_an_error() {
    let root = godot_project();
    let mut s = session();
    let target = LiveLinkTarget::for_folder(&root).unwrap();
    s.set_live_link_target(Some(target.clone()));
    assert!(s.is_modified(), "choosing a target is a change to save");
    let path = temp("project").join("p.subcraft");
    s.save(Some(&path)).unwrap();
    let reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.live_link_target(), Some(&target));

    std::fs::remove_dir_all(&root).unwrap();
    let err = reopened.live_link_snapshot(&target, false).send().unwrap_err();
    assert!(err.contains("project folder is missing"), "{err}");
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}
