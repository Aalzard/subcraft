use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, MaskEffectKind, Projection};
use subcraft_image::TiledImage;

fn setup() -> (Session, subcraft_core::LayerId) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 64);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let id = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel {
        layer: id,
        channel: ChannelKind::BaseColor,
        fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 0.0, 0.0))),
    })
    .unwrap();
    s.execute(Command::AddMask {
        layer: id,
        white: true,
    })
    .unwrap();
    (s, id)
}

fn color(s: &mut Session, expected: [f32; 3]) {
    s.update_composites();
    let image = s
        .composite(s.selection().texture_set)
        .unwrap()
        .channel(ChannelKind::BaseColor)
        .unwrap();
    for y in 0..64 {
        for x in 0..64 {
            let p = image.get(x, y);
            for i in 0..3 {
                assert!(
                    (p[i] - expected[i]).abs() < 0.001,
                    "({x},{y}) {p:?} != {expected:?}"
                );
            }
        }
    }
}

#[test]
fn content_mask_selection_is_transient_and_invalid_selection_is_atomic() {
    let (mut s, id) = setup();
    let before = s.document().clone();
    let history = s.history().entries().len();
    s.execute(Command::SelectLayerTarget {
        layer: id,
        target: LayerTarget::Mask,
    })
    .unwrap();
    assert_eq!(s.layer_target(), LayerTarget::Mask);
    assert_eq!(s.brush.target, subcraft_paint::PaintTarget::MaskBase);
    assert_eq!(s.document(), &before);
    assert_eq!(s.history().entries().len(), history);
    s.execute(Command::SelectLayerTarget {
        layer: id,
        target: LayerTarget::Content,
    })
    .unwrap();
    assert_eq!(s.layer_target(), LayerTarget::Content);
    s.execute(Command::RemoveMask(id)).unwrap();
    assert!(
        s.execute(Command::SelectLayerTarget {
            layer: id,
            target: LayerTarget::Mask
        })
        .is_err()
    );
    assert_eq!(s.selection().layer, Some(id));
    assert_eq!(s.layer_target(), LayerTarget::Content);
    s.execute(Command::Undo).unwrap();
    s.execute(Command::SelectLayerTarget {
        layer: id,
        target: LayerTarget::Mask,
    })
    .unwrap();
    s.execute(Command::RemoveMask(id)).unwrap();
    assert_eq!(s.brush.target, subcraft_paint::PaintTarget::Content);
}

#[test]
fn invert_disable_remove_are_non_destructive_and_undo_exact() {
    let (mut s, id) = setup();
    let source = s
        .selected_layer()
        .unwrap()
        .mask
        .as_ref()
        .unwrap()
        .base
        .clone();
    let history = s.history().entries().len();
    color(&mut s, [1.0, 0.0, 0.0]);
    s.execute(Command::InvertMask(id)).unwrap();
    assert_eq!(s.history().entries().len(), history + 1);
    assert_eq!(
        s.selected_layer().unwrap().mask.as_ref().unwrap().base,
        source
    );
    let neutral = ChannelKind::BaseColor.default_value().0;
    color(&mut s, neutral);
    s.execute(Command::Undo).unwrap();
    color(&mut s, [1.0, 0.0, 0.0]);
    s.execute(Command::Redo).unwrap();
    color(&mut s, neutral);
    s.execute(Command::SetMaskEnabled {
        layer: id,
        enabled: false,
    })
    .unwrap();
    color(&mut s, [1.0, 0.0, 0.0]);
    s.execute(Command::RemoveMask(id)).unwrap();
    color(&mut s, [1.0, 0.0, 0.0]);
    s.execute(Command::Undo).unwrap();
    assert!(!s.selected_layer().unwrap().mask.as_ref().unwrap().enabled);
}

#[test]
fn copy_paste_resamples_paint_and_refreshes_ids_with_independent_cow_and_save_load() {
    let (mut s, source) = setup();
    let image = TiledImage::new(64, 64, 2, [0.25, 1.0, 0.0, 0.0]);
    s.execute(Command::AddMaskEffect {
        layer: source,
        kind: MaskEffectKind::Paint { image },
    })
    .unwrap();
    let source_mask = s.selected_layer().unwrap().mask.clone().unwrap();
    let history = s.history().entries().len();
    s.execute(Command::CopyMask(source)).unwrap();
    assert_eq!(s.history().entries().len(), history);
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    let dest = s.selection().layer.unwrap();
    s.execute(Command::SetResolution {
        texture_set: Some(s.selection().texture_set),
        resolution: 128,
    })
    .unwrap();
    let history = s.history().entries().len();
    s.execute(Command::PasteMask(dest)).unwrap();
    assert_eq!(s.history().entries().len(), history + 1);
    let copy = s.selected_layer().unwrap().mask.as_ref().unwrap();
    assert_eq!(copy.base.width(), 128);
    assert_ne!(copy.effects[0].id, source_mask.effects[0].id);
    let MaskEffectKind::Paint { image } = &copy.effects[0].kind else {
        panic!()
    };
    assert_eq!(image.width(), 128);
    assert_eq!(image.get(100, 100)[..2], [0.25, 1.0]);
    s.execute(Command::InvertMask(dest)).unwrap();
    assert_eq!(
        s.selected_texture_set()
            .find(source)
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .effects
            .len(),
        1
    );
    let file = std::env::temp_dir().join(format!(
        "mask-workflow-{}.subcraft",
        subcraft_core::ObjectId::new()
    ));
    s.save(Some(&file)).unwrap();
    let loaded = Session::open_project(&file).unwrap();
    assert_eq!(loaded.document(), s.document());
    std::fs::remove_file(file).unwrap();
    s.execute(Command::Undo).unwrap();
    s.execute(Command::Undo).unwrap();
    assert!(s.selected_layer().unwrap().mask.is_none());
}

#[test]
fn effect_reordering_duplication_and_errors_preserve_evaluation_and_identity() {
    let (mut s, id) = setup();
    s.execute(Command::AddMaskEffect {
        layer: id,
        kind: MaskEffectKind::Fill {
            value: 0.25,
            procedural: None,
            projection: Projection::default(),
        },
    })
    .unwrap();
    let fill = s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].id;
    s.execute(Command::InvertMask(id)).unwrap();
    let invert = s.selected_layer().unwrap().mask.as_ref().unwrap().effects[1].id;
    s.update_composites();
    let before = s.composite(s.selection().texture_set).unwrap().clone();
    s.execute(Command::MoveMaskEffect {
        layer: id,
        effect: invert,
        index: 0,
    })
    .unwrap();
    s.update_composites();
    assert_ne!(s.composite(s.selection().texture_set).unwrap(), &before);
    s.execute(Command::Undo).unwrap();
    s.update_composites();
    assert_eq!(s.composite(s.selection().texture_set).unwrap(), &before);
    s.execute(Command::DuplicateMaskEffect {
        layer: id,
        effect: fill,
    })
    .unwrap();
    let effects = &s.selected_layer().unwrap().mask.as_ref().unwrap().effects;
    assert_eq!(effects.len(), 3);
    assert_ne!(effects[0].id, effects[1].id);
    let before = s.document().clone();
    let history = s.history().entries().len();
    assert!(
        s.execute(Command::MoveMaskEffect {
            layer: id,
            effect: invert,
            index: 99
        })
        .is_err()
    );
    assert_eq!(s.document(), &before);
    assert_eq!(s.history().entries().len(), history);
}
