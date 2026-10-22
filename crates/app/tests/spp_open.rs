//! Opening a Substance Painter project end to end, from a synthetic `.spp` (see the mesh crate's
//! `tests/support/spp_fixture.rs`): the texture set, flattened channel, baked maps and notes.

#[path = "../../mesh/tests/support/spp_fixture.rs"]
mod fixture;

use subcraft_app::Session;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_doc::LayerContent;

fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("subcraft-spp-open-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.spp"));
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn a_painter_project_opens_with_its_flattened_channels_and_baked_maps() {
    let path = write("full", &fixture::project_file());
    let (session, notes) = Session::open_mesh(&path, 1024).unwrap();
    let doc = session.document();
    assert_eq!(doc.texture_sets.len(), 1);
    let ts = &doc.texture_sets[0];
    assert_eq!((ts.name.as_str(), ts.resolution), ("Cube", 256));
    assert!(ts.has_channel(ChannelKind::BaseColor) && ts.has_channel(ChannelKind::Roughness));
    // The flattened result below an empty layer to paint on.
    assert_eq!(ts.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Painter project (flattened)", "Layer 1"]);
    let LayerContent::Paint(content) = &ts.layers[0].content else { panic!("flattened layer is a paint layer") };
    let rough = content.channels.get(&ChannelKind::Roughness).expect("roughness imported from the texture cache");
    assert_eq!((rough.width(), rough.height()), (256, 256));
    // Fully opaque; the 4x2 source (value 0 .. 70/255 left to right) is smoothly resampled.
    let left = rough.get(0, 128);
    let right = rough.get(255, 128);
    assert_eq!(left[1], 1.0);
    assert!(right[0] > left[0], "{left:?} {right:?}");
    // Baked maps, with the same texture-set resolution and padded background.
    let maps = session.mesh_maps(ts.id).expect("baked maps imported");
    assert!(maps.get(MeshMapKind::AmbientOcclusion).is_some() && maps.get(MeshMapKind::Position).is_some());
    assert!(maps.get(MeshMapKind::Curvature).is_none());
    assert_eq!(maps.get(MeshMapKind::Position).unwrap().comps(), 3);
    let text = notes.join("\n");
    assert!(text.contains("Texture set 'Cube'") && text.contains("Rust") && text.contains("Dirt"), "{text}");
    assert!(text.contains("Baked maps: Ambient Occlusion, Position") || text.contains("Baked maps:"), "{text}");
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn a_project_without_a_readable_document_still_gives_the_model() {
    use std::io::Write;
    // Just a model: no document, no textures.
    let bytes = {
        // Reuse the full file but break the document by replacing it with garbage of the same kind.
        let mut b = fixture::project_file();
        // Corrupt the document magic: find it and overwrite.
        let magic = 0x1b7c_2fddu32.to_le_bytes();
        let at = b.windows(4).position(|w| w == magic).unwrap();
        b[at] ^= 0xff;
        b
    };
    let path = write("nodoc", &bytes);
    let (session, notes) = Session::open_mesh(&path, 512).unwrap();
    assert_eq!(session.scene().mesh.triangle_count(), 2);
    assert!(notes.iter().any(|n| n.contains("only the model was imported")), "{notes:?}");
    std::io::stderr().flush().ok();
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

#[test]
fn files_that_are_not_projects_are_reported() {
    let path = write("junk", b"not an hdf5 file at all");
    let err = Session::open_mesh(&path, 512).err().expect("junk is rejected");
    assert!(err.contains("Substance Painter"), "{err}");
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}
