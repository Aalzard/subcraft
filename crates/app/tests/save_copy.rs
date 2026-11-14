//! File > Save as copy: the copy holds the project, the open project keeps its own file and its
//! unsaved-changes state.

use subcraft_app::{Command, NewLayer, Session};

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("save-copy-{name}-{}.subcraft", subcraft_core::ObjectId::new()))
}

#[test]
fn a_copy_holds_the_project_and_the_open_file_stays_current() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
    let own = temp("own");
    s.save(Some(&own)).unwrap();
    assert!(!s.is_modified());

    // An unsaved edit, then a copy: the copy has it, the project still counts as unsaved.
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    assert!(s.is_modified());
    let copy = temp("copy");
    s.save_copy(&copy).unwrap();
    assert_eq!(s.project_path(), Some(own.as_path()), "the open project keeps its own file");
    assert!(s.is_modified(), "the edit is still unsaved in the open project");
    let reopened = Session::open_project(&copy).unwrap();
    assert_eq!(reopened.document(), s.document(), "the copy holds the current document");
    assert_eq!(reopened.project_path(), Some(copy.as_path()));
    let saved = Session::open_project(&own).unwrap();
    assert_ne!(saved.document(), s.document(), "the project's own file was not rewritten");

    // A copy onto the project's own file is refused (that is Save).
    assert!(s.save_copy(&own).is_err());
    std::fs::remove_file(own).unwrap();
    std::fs::remove_file(copy).unwrap();
}

#[test]
fn an_unsaved_project_can_be_copied_and_stays_without_a_file() {
    let s = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
    let copy = temp("new");
    s.save_copy(&copy).unwrap();
    assert_eq!(s.project_path(), None);
    assert_eq!(Session::open_project(&copy).unwrap().document(), s.document());
    std::fs::remove_file(copy).unwrap();
}
