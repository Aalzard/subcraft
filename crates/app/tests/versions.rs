//! Version history: Save keeps the file it replaces, up to a set number, and a kept version
//! opens in place of the project as unsaved changes.
use std::time::{Duration, UNIX_EPOCH};

use subcraft_app::versions::{self, describe_age, utc_date};
use subcraft_app::{Command, Session};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("versions-{tag}-{}", subcraft_core::ObjectId::new()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn rename(s: &mut Session, name: &str) {
    let ts = s.selected_texture_set().id;
    s.execute(Command::RenameTextureSet { texture_set: ts, name: name.into() }).unwrap();
}

#[test]
fn nothing_is_kept_when_version_history_is_off() {
    let dir = temp_dir("off");
    let path = dir.join("p.subcraft");
    let mut s = Session::new(subcraft_mesh::primitives::cube(1.0), 128);
    assert_eq!(s.keep_versions(), 0, "off unless the app turns it on");
    s.save(Some(&path)).unwrap();
    rename(&mut s, "B");
    s.save(None).unwrap();
    assert!(versions::list(&path).is_empty());
    assert!(!versions::folder(&path).exists(), "no folder is made either");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn each_save_keeps_the_file_it_replaces_up_to_the_limit() {
    let dir = temp_dir("keep");
    let path = dir.join("p.subcraft");
    let mut s = Session::new(subcraft_mesh::primitives::cube(1.0), 128);
    s.set_keep_versions(2);
    for name in ["A", "B", "C", "D"] {
        rename(&mut s, name);
        s.save(Some(&path)).unwrap();
    }
    let list = versions::list(&path);
    assert_eq!(list.len(), 2, "four saves replaced three files; two are kept: {list:?}");
    assert_eq!(versions::folder(&path), dir.join("p.versions"));
    assert!(list.iter().all(|v| v.path.parent() == Some(versions::folder(&path).as_path()) && v.bytes > 0));
    // Newest first: the file the last save replaced (C), then B. A was pruned.
    let names: Vec<String> = list.iter().map(|v| Session::open_project(&v.path).unwrap().selected_texture_set().name.clone()).collect();
    assert_eq!(names, ["C", "B"]);
    // The project file itself is the latest save.
    assert_eq!(Session::open_project(&path).unwrap().selected_texture_set().name, "D");
    // Lowering the limit drops the oldest.
    versions::prune(&path, 1).unwrap();
    assert_eq!(versions::list(&path).len(), 1);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_restored_version_saves_back_over_the_project_and_keeps_what_it_replaced() {
    let dir = temp_dir("restore");
    let path = dir.join("p.subcraft");
    let mut s = Session::new(subcraft_mesh::primitives::cube(1.0), 128);
    s.set_keep_versions(5);
    rename(&mut s, "First");
    s.save(Some(&path)).unwrap();
    rename(&mut s, "Second");
    s.save(None).unwrap();
    let old = versions::list(&path).remove(0);
    let mut r = Session::open_version(&path, &old.path).unwrap();
    assert_eq!(r.selected_texture_set().name, "First");
    assert_eq!(r.project_path(), Some(path.as_path()), "the project path stays the user's file");
    assert!(r.is_modified(), "counts as unsaved until saved back");
    r.set_keep_versions(5);
    r.save(None).unwrap();
    assert!(!r.is_modified());
    assert_eq!(Session::open_project(&path).unwrap().selected_texture_set().name, "First");
    let names: Vec<String> = versions::list(&path).iter().map(|v| Session::open_project(&v.path).unwrap().selected_texture_set().name.clone()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Second".to_string()), "the save that restored First kept Second: {names:?}");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn other_files_in_the_folder_are_not_versions() {
    let dir = temp_dir("other");
    let path = dir.join("p.subcraft");
    let folder = versions::folder(&path);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("notes.txt"), "x").unwrap();
    std::fs::write(folder.join("p-abc.subcraft"), "x").unwrap();
    std::fs::write(folder.join("other-100.subcraft"), "x").unwrap();
    std::fs::write(folder.join("p-100.subcraft"), "x").unwrap();
    std::fs::write(folder.join("p-100-2.subcraft"), "x").unwrap();
    std::fs::write(folder.join("p-99.subcraft"), "x").unwrap();
    let list = versions::list(&path);
    let names: Vec<_> = list.iter().map(|v| v.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["p-100-2.subcraft", "p-100.subcraft", "p-99.subcraft"], "the second save in a second is the newer one");
    assert_eq!(list[1].saved, UNIX_EPOCH + Duration::from_secs(100));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn ages_and_dates_read_plainly() {
    let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    assert_eq!(utc_date(t), "2023-11-14 22:13 UTC");
    assert_eq!(utc_date(UNIX_EPOCH), "1970-01-01 00:00 UTC");
    assert_eq!(utc_date(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29 00:00 UTC");
    assert_eq!(describe_age(t, t + Duration::from_secs(20)), "just now");
    assert_eq!(describe_age(t, t + Duration::from_secs(185)), "3 min ago");
    assert_eq!(describe_age(t, t + Duration::from_secs(7_300)), "2 h ago");
    assert_eq!(describe_age(t, t + Duration::from_secs(5 * 86_400 + 9)), "5 days ago");
    assert_eq!(describe_age(t, t + Duration::from_secs(30 * 86_400)), "2023-11-14 22:13 UTC");
}
