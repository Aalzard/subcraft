//! Settings > General options that live in the app crate: the UV tiling of new fill layers and the
//! cache folder.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_doc::LayerContent;

fn new_fill_scale(s: &mut Session) -> [f32; 2] {
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let id = s.selection().layer.unwrap();
    let ts = s.document().texture_set(s.selection().texture_set).unwrap();
    match &ts.find(id).unwrap().content {
        LayerContent::Fill(f) => f.projection.scale,
        _ => panic!("not a fill layer"),
    }
}

#[test]
fn new_fill_layers_take_the_default_uv_scale() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    assert_eq!(new_fill_scale(&mut s), [1.0, 1.0], "1 by default, as before");
    s.default_uv_scale = 4.0;
    assert_eq!(new_fill_scale(&mut s), [4.0, 4.0]);
    for broken in [0.0, -2.0, f32::NAN] {
        s.default_uv_scale = broken;
        assert_eq!(new_fill_scale(&mut s), [1.0, 1.0], "{broken} falls back to 1");
    }
}

#[test]
fn the_cache_folder_follows_the_setting() {
    let dir = std::env::temp_dir().join("subcraft-cache-setting-test");
    subcraft_app::recovery::set_cache_dir(Some(dir.clone()));
    assert_eq!(subcraft_app::recovery::cache_dir(), Some(dir));
    assert_eq!(subcraft_app::updater::cache_dir().ok(), subcraft_app::recovery::cache_dir().map(|d| d.join("updates")));
    subcraft_app::recovery::set_cache_dir(Some(std::path::PathBuf::new()));
    assert_eq!(subcraft_app::recovery::cache_dir(), subcraft_app::recovery::data_dir(), "an empty path means the default");
    subcraft_app::recovery::set_cache_dir(None);
    assert_eq!(subcraft_app::recovery::cache_dir(), subcraft_app::recovery::data_dir());
}
