//! Autosave + crash recovery end to end on a real session: copies go into the recovery folder
//! only, rotate, survive a "crash" and restore the layers.

use std::path::PathBuf;
use std::time::Duration;

use subcraft_app::recovery::{Autosaver, MIN_GAP_AFTER_OPERATION, RecoveryStore, SessionInfo, list_autosaves};
use subcraft_app::{AutosaveEvent, Command, NewLayer, Session};
use web_time::Instant;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-recovery-it-{name}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn layer_names(s: &Session) -> Vec<String> {
    s.document().texture_sets.iter().flat_map(|ts| ts.layers.iter().map(|l| l.name.clone())).collect()
}

fn saved(event: Option<AutosaveEvent>) -> PathBuf {
    match event {
        Some(AutosaveEvent::Saved(p)) => p,
        other => panic!("expected a finished autosave, got {other:?}"),
    }
}

#[test]
fn autosave_never_touches_the_users_file_and_recovers_after_a_crash() {
    let root = temp_dir("flow");
    let user_file = root.join("my-crate.subcraft");
    let store = RecoveryStore::new(root.join("recovery"));

    let mut session = Session::sample();
    session.save(Some(&user_file)).unwrap();
    let user_bytes = std::fs::read(&user_file).unwrap();
    let user_mtime = std::fs::metadata(&user_file).unwrap().modified().unwrap();

    let live = store.begin_session(&SessionInfo::for_session(&session), 2).unwrap();
    let mut autosaver = Autosaver::new(Some(live), Duration::from_secs(60));
    let t0 = Instant::now();

    // Unmodified: nothing to protect.
    assert!(!autosaver.is_due(&session, t0 + Duration::from_secs(3600)));

    // Edit, then three timed autosaves (each after a further edit).
    for i in 0..3 {
        session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
        let id = session.selection().layer.unwrap();
        session.execute(Command::Rename { layer: id, name: format!("Rust {i}") }).unwrap();
        let now = t0 + Duration::from_secs(61 * (i + 1));
        assert!(autosaver.is_due(&session, now), "copy {i} due");
        autosaver.tick(&session, now);
        let path = saved(autosaver.wait());
        let dir = autosaver.live().unwrap().dir();
        assert!(path.starts_with(dir), "autosave {} outside the recovery folder", path.display());
        // Nothing new to save until the next edit.
        assert!(!autosaver.is_due(&session, now + Duration::from_secs(3600)));
    }
    let dir = autosaver.live().unwrap().dir().to_path_buf();
    assert_eq!(list_autosaves(&dir).len(), 2, "only the newest copies are kept");

    // The user's own file is untouched.
    assert_eq!(std::fs::read(&user_file).unwrap(), user_bytes);
    assert_eq!(std::fs::metadata(&user_file).unwrap().modified().unwrap(), user_mtime);
    let expected = layer_names(&session);

    // "Crash": the process ends without a clean exit.
    drop(autosaver);

    let crashed = store.crashed();
    assert_eq!(crashed.len(), 1);
    assert_eq!(crashed[0].info.project_path.as_deref(), Some(user_file.as_path()));
    let recovered = Session::recover(&crashed[0]).unwrap();
    assert_eq!(layer_names(&recovered), expected, "recovered layers");
    assert_eq!(recovered.project_path(), Some(user_file.as_path()), "Save goes back to the user's file");
    assert!(recovered.is_modified() && recovered.is_recovered());

    // The next session adopts the copies; saving the recovered work updates the user's file.
    let live = store.begin_session(&SessionInfo::for_session(&recovered), 2).unwrap();
    live.adopt(crashed.into_iter().next().unwrap()).unwrap();
    assert!(store.crashed().is_empty());
    let mut recovered = recovered;
    recovered.save(None).unwrap();
    assert!(!recovered.is_modified());
    let reopened = Session::open_project(&user_file).unwrap();
    assert_eq!(layer_names(&reopened), expected);

    let mut autosaver = Autosaver::new(Some(live), Duration::from_secs(60));
    autosaver.finish_clean();
    assert!(store.crashed().is_empty());
    assert!(std::fs::read_dir(store.root()).unwrap().next().is_none(), "clean exit leaves no folder");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn big_operations_autosave_soon_but_not_back_to_back() {
    let root = temp_dir("soon");
    let store = RecoveryStore::new(&root);
    let mut session = Session::sample();
    let live = store.begin_session(&SessionInfo::for_session(&session), 3).unwrap();
    // Timer off: only operations trigger copies.
    let mut autosaver = Autosaver::new(Some(live), Duration::ZERO);
    let t0 = Instant::now();
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    assert!(!autosaver.is_due(&session, t0 + Duration::from_secs(3600)), "timer is off");

    autosaver.request_soon();
    let t1 = t0 + MIN_GAP_AFTER_OPERATION;
    assert!(autosaver.is_due(&session, t1));
    autosaver.tick(&session, t1);
    saved(autosaver.wait());

    // A second operation right after: waits for the minimum gap.
    session.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    autosaver.request_soon();
    assert!(!autosaver.is_due(&session, t1 + Duration::from_secs(1)));
    assert!(autosaver.is_due(&session, t1 + MIN_GAP_AFTER_OPERATION));

    // Baking (new mesh maps) is a change worth saving even with an unchanged document.
    autosaver.tick(&session, t1 + MIN_GAP_AFTER_OPERATION);
    saved(autosaver.wait());
    let ts = session.document().texture_sets[0].id;
    let mut maps = std::collections::BTreeMap::new();
    maps.insert(subcraft_app::MeshMapKind::AmbientOcclusion, subcraft_image::TiledImage::new(64, 64, 1, [1.0; 4]));
    session.set_mesh_maps(ts, maps);
    autosaver.request_soon();
    assert!(autosaver.is_due(&session, t1 + MIN_GAP_AFTER_OPERATION * 2));
    autosaver.finish_clean();
    std::fs::remove_dir_all(root).unwrap();
}
