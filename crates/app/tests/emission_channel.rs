//! Adding the Emissive channel to a fill layer (and to a texture set that lacks it), with
//! the right defaults, one undo step, save/reopen and texture-set import.
use std::path::PathBuf;
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::ChannelKind as K;
use subcraft_doc::{BlendMode, FillSource, LayerContent};
use subcraft_image::{Bitmap, encode_bitmap};

fn fill_session() -> (Session, subcraft_core::LayerId) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    (s, layer)
}
fn fill(s: &Session, layer: subcraft_core::LayerId) -> subcraft_doc::FillContent {
    let LayerContent::Fill(f) = &s.selected_texture_set().find(layer).unwrap().content else { panic!("fill layer") };
    f.clone()
}

#[test]
fn adding_emission_to_a_set_without_it_is_one_undo_step_with_white_intensity_three() {
    let (mut s, layer) = fill_session();
    // A folder made before the channel exists must pick the channel up too.
    s.execute(Command::AddLayer(NewLayer::Folder)).unwrap();
    s.execute(Command::Seal).unwrap();
    let before = s.document().clone();
    let history = s.history().entries().len();
    assert!(!s.selected_texture_set().has_channel(K::Emissive));

    s.execute(Command::AddFillChannel { layer, channel: K::Emissive }).unwrap();
    assert_eq!(s.history().entries().len(), history + 1, "channel + layer setup is one history entry");
    let ts = s.selected_texture_set();
    assert!(ts.has_channel(K::Emissive));
    assert_eq!(ts.channels.iter().filter(|c| c.kind == K::Emissive).count(), 1);
    let emission = fill(&s, layer).channels[&K::Emissive].clone();
    assert!(emission.enabled);
    assert_eq!(emission.strength, 3.0, "default Intensity");
    let FillSource::Uniform { value } = emission.source else { panic!("uniform colour") };
    assert_eq!(value.0, [1.0, 1.0, 1.0], "neutral white, not orange");
    for l in &ts.layers {
        let expected = if l.is_folder() { BlendMode::Passthrough } else { BlendMode::Normal };
        assert_eq!(l.blend_for(K::Emissive).mode, expected, "{} knows the new channel", l.name);
    }
    // The composite now has an emissive image and the white fill shows through it.
    s.update_composites();
    let ts_id = s.selection().texture_set;
    let px = s.composite(ts_id).unwrap().channel(K::Emissive).expect("composite has Emissive").get(10, 10);
    assert!(px[0] > 0.99, "white * intensity reaches the composite: {px:?}");

    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
    assert!(!s.selected_texture_set().has_channel(K::Emissive));
    s.execute(Command::Redo).unwrap();
    assert!(s.selected_texture_set().has_channel(K::Emissive));

    // Save and reopen keep the channel, the layer's emission and the composite.
    let path = std::env::temp_dir().join(format!("subcraft-emission-{}.subcraft", layer.0));
    s.save(Some(&path)).unwrap();
    let mut reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.document(), s.document());
    reopened.update_composites();
    let id = reopened.selection().texture_set;
    assert!(reopened.composite(id).unwrap().channel(K::Emissive).unwrap().get(10, 10)[0] > 0.99);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_layer_that_predates_the_channel_gets_it_with_one_toggle() {
    let (mut s, first) = fill_session();
    s.execute(Command::AddFillChannel { layer: first, channel: K::Emissive }).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let second = s.selection().layer.unwrap();
    // Make it look like a layer saved before the set had Emission: no blend entry, no channel.
    let mut layer = s.selected_texture_set().find(second).unwrap().clone();
    layer.blend.remove(&K::Emissive);
    if let LayerContent::Fill(f) = &mut layer.content {
        f.channels.remove(&K::Emissive);
    }
    s.execute(Command::ReplaceLayer { layer: second, replacement: Box::new(layer), label: "old layer".into() }).unwrap();
    assert_eq!(s.selected_texture_set().find(second).unwrap().blend_for(K::Emissive).mode, BlendMode::Disable);

    let before = s.document().clone();
    s.execute(Command::ToggleFillChannel { layer: second, effect: None, channel: K::Emissive, solo: false }).unwrap();
    let f = fill(&s, second);
    assert!(f.channels[&K::Emissive].enabled);
    assert_eq!(f.channels[&K::Emissive].strength, 3.0);
    assert_eq!(s.selected_texture_set().find(second).unwrap().blend_for(K::Emissive).mode, BlendMode::Normal, "the layer now takes part in Emissive");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
}

#[test]
fn adding_a_channel_the_set_already_has_does_not_duplicate_it_and_paint_layers_refuse() {
    let (mut s, layer) = fill_session();
    s.execute(Command::AddFillChannel { layer, channel: K::Emissive }).unwrap();
    s.execute(Command::AddFillChannel { layer, channel: K::Emissive }).unwrap();
    assert_eq!(s.selected_texture_set().channels.iter().filter(|c| c.kind == K::Emissive).count(), 1);
    s.execute(Command::AddFillChannel { layer, channel: K::AmbientOcclusion }).unwrap();
    assert!(s.selected_texture_set().has_channel(K::AmbientOcclusion));
    assert_eq!(fill(&s, layer).channels[&K::AmbientOcclusion].strength, 1.0, "other channels keep their usual defaults");

    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let paint = s.selection().layer.unwrap();
    let before = s.document().clone();
    assert!(s.execute(Command::AddFillChannel { layer: paint, channel: K::Emissive }).is_err());
    assert_eq!(s.document(), &before, "a refused command changes nothing");
}

fn image(path: &PathBuf, rgb: [u8; 3]) {
    let (bytes, _) = encode_bitmap(&Bitmap::new_u8(2, 2, 3, rgb.repeat(4))).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn importing_a_texture_set_with_an_emissive_map_adds_the_channel() {
    let dir = std::env::temp_dir().join(format!("subcraft-emission-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (base, glow) = (dir.join("Lamp_basecolor.png"), dir.join("Lamp_emissive.png"));
    image(&base, [90, 90, 90]);
    image(&glow, [255, 200, 40]);
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 128);
    let before = s.document().clone();
    assert!(!s.selected_texture_set().has_channel(K::Emissive));
    let result = s.import_texture_set(&[base, glow]).unwrap();
    assert!(result.used.iter().any(|(_, k)| *k == K::Emissive), "{:?}", result.used);
    assert!(result.skipped.is_empty(), "{:?}", result.skipped);
    assert!(s.selected_texture_set().has_channel(K::Emissive));
    let layer = s.selection().layer.unwrap();
    let f = fill(&s, layer);
    assert!(f.channels[&K::Emissive].enabled && matches!(f.channels[&K::Emissive].source, FillSource::Bitmap { .. }));
    assert_eq!(f.channels[&K::Emissive].strength, 3.0);
    assert_eq!(s.selected_texture_set().find(layer).unwrap().blend_for(K::Emissive).mode, BlendMode::Normal);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before, "import with the new channel is one undo step");
    std::fs::remove_dir_all(dir).unwrap();
}
