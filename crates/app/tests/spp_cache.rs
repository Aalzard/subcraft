//! The import cache: a second open of the same Painter project reads the rebuilt stack from the
//! cache and gets exactly the same layers; a changed file or importer version rebuilds.

#[path = "../../mesh/tests/support/spp_fixture.rs"]
mod fixture;

use subcraft_app::Session;

fn shape(session: &Session) -> Vec<(String, bool, Vec<(String, bool)>)> {
    session.document().texture_sets[0].layers.iter().map(|l| (l.name.clone(), l.visible, l.children().iter().map(|c| (c.name.clone(), c.visible)).collect())).collect()
}

#[test]
fn a_second_open_reads_the_stack_from_the_cache_and_a_changed_file_does_not() {
    let base = std::env::temp_dir().join(format!("subcraft-spp-cache-{}", std::process::id()));
    let cache = base.join("data").join("spp-cache");
    let config = subcraft_app::spp_cache::SppCache { dir: cache.clone(), min_rebuild: std::time::Duration::ZERO };
    std::fs::create_dir_all(&base).unwrap();
    let path = base.join("Cube.spp");
    std::fs::write(&path, fixture::project_file()).unwrap();

    let (first, first_notes) = Session::open_spp_cached(&path, Some(config.clone())).unwrap();
    assert!(!first_notes.iter().any(|n| n.contains("import cache")), "{first_notes:?}");
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 3, "the stack, its textures and its description are stored");

    let (second, second_notes) = Session::open_spp_cached(&path, Some(config.clone())).unwrap();
    assert!(second_notes.iter().any(|n| n.contains("read from the import cache")), "{second_notes:?}");
    assert_eq!(shape(&second), shape(&first));
    // The rebuilt content is the same: same fill colors and gate numbers.
    let note = |notes: &[String]| notes.iter().find(|n| n.contains("Rebuilt stack")).cloned();
    assert_eq!(note(&second_notes), note(&first_notes));
    let (a, b) = (&first.document().texture_sets[0].layers[1], &second.document().texture_sets[0].layers[1]);
    assert_eq!(a.children().len(), b.children().len());
    assert!(a.children() == b.children(), "the cached stack is the rebuilt stack");

    // Another file content is another key.
    let mut changed = fixture::project_file();
    changed.extend_from_slice(b"padding that changes the hash");
    std::fs::write(&path, changed).unwrap();
    let (_, third_notes) = Session::open_spp_cached(&path, Some(config.clone())).unwrap();
    assert!(!third_notes.iter().any(|n| n.contains("import cache")), "{third_notes:?}");
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 6);
    std::fs::remove_dir_all(&base).ok();
}
