//! Shelf → Import textures: named maps become one material and stay on the shelf.

use std::path::{Path, PathBuf};

use subcraft_app::Session;
use subcraft_core::ChannelKind;
use subcraft_doc::LayerContent;
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::primitives;
use subcraft_ui::files::import_shelf_textures;
use subcraft_ui::state::UiState;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-shelf-import-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_uniform(path: &Path, v: f32) {
    let data: Vec<f32> = (0..16 * 16).flat_map(|_| [v, v, v]).collect();
    save_image(path, 16, 16, 3, &data, FileFormat::Png8).unwrap();
}

#[test]
fn named_maps_become_one_material_with_ao() {
    let dir = temp_dir("maps");
    let files: Vec<PathBuf> = ["basecolor.png", "roughness.png", "ambient_occlusion.png"].iter().map(|f| dir.join(f)).collect();
    for f in &files {
        write_uniform(f, 0.5);
    }
    let mut s = UiState::new(Session::new(primitives::sample_crate(), 128), None);
    let layers = s.session.selected_texture_set().layers.len();

    import_shelf_textures(&mut s, &files, 0.0);

    assert_eq!(s.textures.items.len(), 3, "all files are on the shelf");
    assert_eq!(s.session.selected_texture_set().layers.len(), layers + 1, "one material layer");
    assert!(s.session.selected_texture_set().has_channel(ChannelKind::AmbientOcclusion));
    let LayerContent::Fill(fill) = &s.session.selected_layer().unwrap().content else { panic!("fill layer") };
    for ch in [ChannelKind::BaseColor, ChannelKind::Roughness, ChannelKind::AmbientOcclusion] {
        assert!(fill.channels[&ch].enabled, "{ch:?} uses its map");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn plain_images_only_go_to_the_shelf() {
    let dir = temp_dir("plain");
    let photo = dir.join("holiday_photo.png");
    write_uniform(&photo, 0.5);
    let mut s = UiState::new(Session::new(primitives::sample_crate(), 128), None);
    let doc = s.session.document().clone();

    import_shelf_textures(&mut s, std::slice::from_ref(&photo), 0.0);

    assert_eq!(s.textures.items.len(), 1);
    assert_eq!(s.session.document(), &doc, "no layer for an image that names no map");
    std::fs::remove_dir_all(&dir).ok();
}
