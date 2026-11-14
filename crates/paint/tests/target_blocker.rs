//! `target_blocker` says why a stroke would be refused, matching `begin_targets` without snapshots.

use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{Layer, TextureSet};
use subcraft_paint::target::{begin_targets, target_blocker};
use subcraft_paint::{BrushMaterial, PaintError, PaintTarget};

#[test]
fn the_blocker_agrees_with_begin_targets() {
    let mut ts = TextureSet::new("T", 0, 64);
    let tc = ts.clone();
    let paint = ts.insert_above(None, Layer::new_paint("Paint", &tc));
    let tc = ts.clone();
    let fill = ts.insert_above(None, Layer::new_fill("Fill", &tc));
    let red = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    let none = BrushMaterial::with(&[] as &[(ChannelKind, ChannelValue)]);
    let cases = [
        (paint, PaintTarget::Content, &red, None),
        (fill, PaintTarget::Content, &red, Some(PaintError::NotAPaintLayer)),
        (paint, PaintTarget::MaskBase, &red, Some(PaintError::NoMask)),
        (paint, PaintTarget::Content, &none, Some(PaintError::NothingToPaint)),
        (subcraft_core::LayerId::new(), PaintTarget::Content, &red, Some(PaintError::LayerNotFound)),
    ];
    for (layer, target, material, expected) in cases {
        assert_eq!(target_blocker(&ts, layer, target, material), expected, "{target:?}");
        assert_eq!(begin_targets(&ts, layer, target, material).err(), expected, "begin_targets agrees for {target:?}");
    }
}
