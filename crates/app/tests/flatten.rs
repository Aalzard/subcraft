//! Painter's flatten (Ctrl+M) and "Export flattened group to files".
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{BlendMode, FillChannel, FillSource, LayerContent, MaskEffectKind};

fn session() -> Session {
    Session::new(subcraft_mesh::primitives::plane(2.0, 2), 128)
}

/// A fill layer with uniform Base Color and Roughness, other channels off.
fn fill(s: &mut Session, color: [f32; 3], rough: f32) -> LayerId {
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let id = s.selection().layer.unwrap();
    let mut l = s.selected_texture_set().find(id).unwrap().clone();
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for c in f.channels.values_mut() {
        c.enabled = false;
    }
    f.channels.insert(ChannelKind::BaseColor, FillChannel::new(FillSource::uniform(ChannelValue::rgb(color[0], color[1], color[2]))));
    f.channels.insert(ChannelKind::Roughness, FillChannel::new(FillSource::uniform(ChannelValue::scalar(rough))));
    s.execute(Command::ReplaceLayer { layer: id, replacement: Box::new(l), label: "fill".into() }).unwrap();
    id
}

fn composite_at(s: &mut Session, kind: ChannelKind) -> [f32; 4] {
    s.update_composites();
    let set = s.selection().texture_set;
    s.composite(set).unwrap().channel(kind).unwrap().get(64, 64)
}

#[test]
fn flattening_a_folder_makes_a_raster_layer_above_it_with_the_same_result_and_hides_the_folder() {
    let mut s = session();
    fill(&mut s, [0.2, 0.3, 0.4], 0.6);
    let red = fill(&mut s, [0.9, 0.1, 0.1], 0.2);
    // Half-transparent top layer inside a folder: the folder's result is a mix.
    s.execute(Command::SetBlendOpacity { layer: red, channel: ChannelKind::BaseColor, opacity: 0.5 }).unwrap();
    s.execute(Command::GroupLayers(vec![red])).unwrap();
    let folder = s.selection().layer.unwrap();
    s.execute(Command::Seal).unwrap();
    let before_color = composite_at(&mut s, ChannelKind::BaseColor);
    let before_rough = composite_at(&mut s, ChannelKind::Roughness);
    let steps = s.history().entries().len();

    s.execute(Command::FlattenLayers(vec![folder])).unwrap();
    assert_eq!(s.history().entries().len(), steps + 1, "one undo step");
    let ts = s.selected_texture_set();
    let flat = s.selection().layer.unwrap();
    let flat_layer = ts.find(flat).unwrap();
    assert!(matches!(flat_layer.content, LayerContent::Paint(_)), "a layer of rasters");
    assert_eq!(ts.location(flat).unwrap().index, ts.location(folder).unwrap().index + 1, "right above the source");
    assert!(!ts.find(folder).unwrap().visible, "the source stays, hidden");
    assert!(flat_layer.name.contains("flattened"));
    assert!(flat_layer.blend.values().all(|b| b.mode != BlendMode::Passthrough));
    let after_color = composite_at(&mut s, ChannelKind::BaseColor);
    let after_rough = composite_at(&mut s, ChannelKind::Roughness);
    for i in 0..3 {
        assert!((before_color[i] - after_color[i]).abs() < 2e-2, "{before_color:?} vs {after_color:?}");
    }
    assert!((before_rough[0] - after_rough[0]).abs() < 2e-2, "{before_rough:?} vs {after_rough:?}");

    s.execute(Command::Undo).unwrap();
    assert!(s.selected_texture_set().find(flat).is_none());
    assert!(s.selected_texture_set().find(folder).unwrap().visible);
}

#[test]
fn flattening_bakes_the_mask_and_skips_disabled_channels_and_refuses_an_empty_selection() {
    let mut s = session();
    let l = fill(&mut s, [0.5, 0.5, 0.5], 0.4);
    s.execute(Command::AddMask { layer: l, white: true }).unwrap();
    s.execute(Command::AddMaskEffect { layer: l, kind: MaskEffectKind::Invert }).unwrap();
    s.execute(Command::SetBlendMode { layer: l, channel: ChannelKind::Metallic, mode: BlendMode::Disable }).unwrap();
    s.execute(Command::FlattenLayers(vec![l])).unwrap();
    let flat = s.selected_texture_set().find(s.selection().layer.unwrap()).unwrap().clone();
    let mask = flat.mask.as_ref().expect("the mask is kept, flattened");
    assert!(mask.effects.is_empty(), "its effects are baked into the base");
    assert!(mask.base.get(64, 64)[0] < 1e-2, "white mask, inverted by its effect");
    let LayerContent::Paint(p) = &flat.content else { unreachable!() };
    assert!(!p.channels.contains_key(&ChannelKind::Metallic), "a disabled channel is skipped");
    assert!(p.channels.contains_key(&ChannelKind::BaseColor));

    let doc = s.document().clone();
    assert!(s.execute(Command::FlattenLayers(vec![])).is_err());
    assert_eq!(s.document(), &doc);
}

#[test]
fn export_flattened_writes_one_file_per_channel_named_by_the_pattern() {
    let mut s = session();
    let l = fill(&mut s, [0.5, 0.2, 0.1], 0.7);
    s.execute(Command::Rename { layer: l, name: "Rust".into() }).unwrap();
    let dir = std::env::temp_dir().join(format!("subcraft-flatten-{}", l.0));
    std::fs::create_dir_all(&dir).unwrap();
    let written = s.export_flattened(l, &dir, subcraft_app::flatten::DEFAULT_PATTERN).unwrap();
    let names: Vec<String> = written.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    let set = s.selected_texture_set().name.clone();
    let expect = |k| format!("{}.png", subcraft_app::flatten::file_name(subcraft_app::flatten::DEFAULT_PATTERN, &set, "Rust", k));
    assert!(expect(ChannelKind::BaseColor).ends_with("_Rust_BaseColor.png"));
    assert!(names.contains(&expect(ChannelKind::BaseColor)), "{names:?}");
    assert!(names.contains(&expect(ChannelKind::Roughness)), "{names:?}");
    assert!(written.iter().all(|p| p.exists()));
    let img = subcraft_image::load_bitmap(&written[0]).unwrap();
    assert_eq!((img.width, img.height), (128, 128));
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(subcraft_app::flatten::format_for(ChannelKind::Height).extension(), "exr");
    assert_eq!(subcraft_app::flatten::format_for(ChannelKind::Normal).extension(), "exr");
    assert_eq!(subcraft_app::flatten::format_for(ChannelKind::BaseColor).extension(), "png");
    assert_eq!(subcraft_app::flatten::file_name("$layerName-$srcMap", "S", "A/B", ChannelKind::AmbientOcclusion), "A_B-AmbientOcclusion");
}
