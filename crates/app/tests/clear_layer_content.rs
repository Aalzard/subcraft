//! Clear layer content (issue #739): a paint layer's channels go back to empty while its mask,
//! effects and blending stay; one undo step; path / text / fill layers are refused.

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{LayerContent, MaskEffectKind};

const RES: u32 = 128;

fn paint(s: &Session, layer: LayerId) -> &subcraft_doc::PaintContent {
    match &s.selected_texture_set().find(layer).unwrap().content {
        LayerContent::Paint(p) => p,
        _ => panic!("not a paint layer"),
    }
}

fn paint_square(s: &mut Session, layer: LayerId) {
    let mut l = s.selected_texture_set().find(layer).unwrap().clone();
    let LayerContent::Paint(p) = &mut l.content else { panic!("not a paint layer") };
    let img = p.channel_mut(ChannelKind::BaseColor, RES);
    for y in 10..20 {
        for x in 10..20 {
            img.set(x, y, [1.0, 0.0, 0.0, 1.0]);
        }
    }
    s.execute(Command::ReplaceLayer { layer, replacement: Box::new(l), label: "paint".into() }).unwrap();
}

#[test]
fn clearing_empties_every_channel_and_keeps_mask_effects_and_blending() {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 2), RES);
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let layer = s.selection().layer.unwrap();
    paint_square(&mut s, layer);
    s.execute(Command::AddMask { layer, white: true }).unwrap();
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Invert }).unwrap();
    let before = s.selected_texture_set().find(layer).unwrap().clone();
    assert_eq!(paint(&s, layer).channels[&ChannelKind::BaseColor].get(15, 15)[3], 1.0);

    let steps = s.history().entries().len();
    s.execute(Command::ClearLayerContent(layer)).unwrap();
    assert_eq!(s.history().entries().len(), steps + 1, "one undo step");
    let after = s.selected_texture_set().find(layer).unwrap();
    let img = &paint(&s, layer).channels[&ChannelKind::BaseColor];
    assert_eq!((img.width(), img.height()), (RES, RES), "the resolution stays");
    for (x, y) in [(15, 15), (10, 10), (19, 19), (100, 100)] {
        assert_eq!(img.get(x, y)[3], 0.0, "({x}, {y}) is transparent again");
    }
    assert_eq!(after.mask, before.mask, "the mask stays");
    assert_eq!(after.blend, before.blend, "blending stays");
    assert_eq!(after.name, before.name);

    // Undo brings the paint back; redo clears it again.
    s.execute(Command::Undo).unwrap();
    assert_eq!(paint(&s, layer).channels[&ChannelKind::BaseColor].get(15, 15)[3], 1.0);
    s.execute(Command::Redo).unwrap();
    assert_eq!(paint(&s, layer).channels[&ChannelKind::BaseColor].get(15, 15)[3], 0.0);

    // Nothing left to clear: no empty undo step.
    s.execute(Command::Seal).unwrap();
    let steps = s.history().entries().len();
    s.execute(Command::ClearLayerContent(layer)).unwrap();
    assert_eq!(s.history().entries().len(), steps);
}

#[test]
fn cleared_layer_no_longer_shows_in_the_composite() {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 2), RES);
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.update_composites();
    let base = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap().get(15, 15);
    paint_square(&mut s, layer);
    s.update_composites();
    let painted = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap().get(15, 15);
    assert!((painted[0] - 1.0).abs() < 1e-4 && painted[1].abs() < 1e-4, "{painted:?}");
    s.execute(Command::ClearLayerContent(layer)).unwrap();
    s.update_composites();
    let cleared = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap().get(15, 15);
    for c in 0..3 {
        assert!((cleared[c] - base[c]).abs() < 1e-4, "back to what was under it: {cleared:?} vs {base:?}");
    }
}

#[test]
fn fill_layers_are_refused() {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 2), RES);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let fill = s.selection().layer.unwrap();
    let steps = s.history().entries().len();
    assert!(s.execute(Command::ClearLayerContent(fill)).is_err());
    assert_eq!(s.history().entries().len(), steps);
}
