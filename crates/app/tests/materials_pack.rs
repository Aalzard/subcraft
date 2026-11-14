//! "SubCraft Materials" packs: a pack material applied as one fill layer with all its maps.

use std::path::Path;

use subcraft_app::Session;
use subcraft_assets::materials_pack::load_pack;
use subcraft_core::ChannelKind;
use subcraft_doc::{FillSource, LayerContent};
use subcraft_mesh::primitives;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/tests/fixtures/materials_pack")
}

#[test]
fn a_pack_material_becomes_one_fill_layer_with_every_map() {
    let pack = load_pack(&fixtures());
    let metal = pack.iter().find(|m| m.key == "metal_test").expect("fixture material");
    let mut s = Session::new(primitives::cube(2.0), 128);
    let before = s.selected_texture_set().layers.len();
    let r = s.apply_pack_material(metal).unwrap();
    assert_eq!(r.layer_name, "Metal Test Plate", "named after the material");
    let ts = s.selected_texture_set();
    assert_eq!(ts.layers.len(), before + 1);
    let layer = ts.layers.iter().find(|l| l.name == "Metal Test Plate").unwrap();
    let LayerContent::Fill(fill) = &layer.content else { panic!("a fill layer") };
    for kind in [ChannelKind::BaseColor, ChannelKind::Normal, ChannelKind::Height, ChannelKind::Roughness, ChannelKind::Metallic, ChannelKind::AmbientOcclusion] {
        let ch = fill.channels.get(&kind).unwrap_or_else(|| panic!("{kind:?} channel"));
        assert!(ch.enabled, "{kind:?} enabled");
        let FillSource::Bitmap { resource } = &ch.source else { panic!("{kind:?} is a bitmap") };
        assert!(s.resources().get(*resource).is_some(), "{kind:?} image is a project resource");
    }
    // One undo step.
    s.execute(subcraft_app::Command::Undo).unwrap();
    assert_eq!(s.selected_texture_set().layers.len(), before);
}
