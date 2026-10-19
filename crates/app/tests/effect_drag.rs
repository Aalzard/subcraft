use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use subcraft_app::{
    Command, LayerTarget, NewLayer, Session,
    effect_stack::{DropSide, EffectAddress},
    layer_previews::{LayerPreviews, PreviewKey},
};
use subcraft_core::{ChannelKind as C, ChannelValue};
use subcraft_doc::{ContentEffectKind, FillChannel, FillSource, Filter, MaskEffectKind};
fn preview(
    cache: &mut LayerPreviews,
    s: &Session,
    key: PreviewKey,
    old: Option<&Arc<subcraft_app::layer_previews::LayerPreview>>,
) -> Arc<subcraft_app::layer_previews::LayerPreview> {
    let start = Instant::now();
    loop {
        if let Some(p) = cache.request(s, key)
            && old.is_none_or(|old| !Arc::ptr_eq(old, &p))
        {
            return p;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn dropped_effects_update_numeric_output_thumbnails_undo_export_and_reopen() {
    for mask in [false, true] {
        let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 1), 128);
        s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
        let layer = s.selection().layer.unwrap();
        let gray = if mask { 1.0 } else { 0.25 };
        s.execute(Command::SetFillChannel {
            layer,
            channel: C::BaseColor,
            fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(gray, gray, gray))),
        })
        .unwrap();
        if mask {
            s.execute(Command::AddMask {
                layer,
                white: false,
            })
            .unwrap();
            s.execute(Command::AddMaskEffect {
                layer,
                kind: MaskEffectKind::Fill {
                    value: 0.25,
                    procedural: None,
                    projection: Default::default(),
                },
            })
            .unwrap();
        }
        let mut ids = Vec::new();
        for filter in [
            Filter::Contrast {
                gain: 2.0,
                pivot: 0.0,
            },
            Filter::Threshold { cutoff: 0.4 },
        ] {
            s.execute(if mask {
                Command::AddMaskEffect {
                    layer,
                    kind: MaskEffectKind::Filter(filter),
                }
            } else {
                Command::AddContentEffect {
                    layer,
                    kind: ContentEffectKind::Filter {
                        channel: C::BaseColor,
                        filter,
                    },
                }
            })
            .unwrap();
            ids.push(s.selected_effect().unwrap());
        }
        let domain = if mask {
            LayerTarget::Mask
        } else {
            LayerTarget::Content
        };
        let source = EffectAddress {
            texture_set: s.selection().texture_set,
            layer,
            domain,
            effect: ids[0],
        };
        let target = EffectAddress {
            effect: ids[1],
            ..source
        };
        let key = PreviewKey {
            layer,
            channel: C::BaseColor,
            mask,
            effect: Some(source.effect),
            size: 64,
        };
        let mut cache = LayerPreviews::default();
        let old = preview(&mut cache, &s, key, None);
        let before = s.document().clone();
        let history = s.history().entries().len();
        s.update_composites();
        assert_eq!(
            s.composite(source.texture_set)
                .unwrap()
                .channel(C::BaseColor)
                .unwrap()
                .get(64, 64),
            [1.0; 4]
        );
        s.execute(Command::DropEffect {
            source,
            target,
            side: DropSide::Above,
        })
        .unwrap();
        assert_eq!(s.selected_effect(), Some(source.effect));
        assert_eq!(s.layer_target(), domain);
        assert_eq!(s.history().entries().len(), history + 1);
        let latest = preview(&mut cache, &s, key, Some(&old));
        assert!(
            latest
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 0, 255])
        );
        let after = s.document().clone();
        s.execute(Command::Undo).unwrap();
        assert_eq!(s.document(), &before);
        s.execute(Command::Redo).unwrap();
        assert_eq!(s.document(), &after);
        s.update_composites_within(Some(Duration::ZERO));
        let exported = s
            .export_archive(&subcraft_export::ExportSettings::default())
            .unwrap();
        let expected = if mask { 0.5 } else { 0.0 };
        let image = s
            .composite(source.texture_set)
            .unwrap()
            .channel(C::BaseColor)
            .unwrap();
        for y in 0..128 {
            for x in 0..128 {
                assert_eq!(image.get(x, y), [expected, expected, expected, 1.0]);
            }
        }
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(exported.bytes)).unwrap();
        let name = zip
            .file_names()
            .find_map(|n| {
                n.ok()
                    .filter(|n| n.contains("BaseColor"))
                    .map(|n| n.into_owned())
            })
            .unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut zip.by_name(&name).unwrap(), &mut bytes).unwrap();
        let pixel = subcraft_image::io::decode_bitmap(&bytes, Some(&name))
            .unwrap()
            .texel(64, 64, true);
        assert!((pixel[0] - expected).abs() < 0.01);
        let path = std::env::temp_dir().join(format!("subcraft-effect-drop-{}.subcraft", layer.0));
        s.save(Some(&path)).unwrap();
        let reopened = Session::open_project(&path).unwrap();
        assert_eq!(reopened.document(), s.document());
        std::fs::remove_file(path).unwrap();
        let history = s.history().entries().len();
        s.execute(Command::DropEffect {
            source,
            target,
            side: DropSide::Above,
        })
        .unwrap();
        s.execute(Command::DropEffect {
            source,
            target: EffectAddress {
                domain: if mask {
                    LayerTarget::Content
                } else {
                    LayerTarget::Mask
                },
                ..target
            },
            side: DropSide::Below,
        })
        .unwrap();
        assert_eq!(s.document(), &after);
        assert_eq!(s.history().entries().len(), history);
    }
}
#[test]
fn moved_paint_effect_keeps_raster_identity_and_brush_target_and_stale_set_is_rejected() {
    let mut s = Session::new(subcraft_mesh::primitives::sample_crate(), 128);
    let layer = s.selection().layer.unwrap();
    s.execute(Command::AddContentEffect {
        layer,
        kind: ContentEffectKind::Paint {
            channels: [(
                C::BaseColor,
                subcraft_image::TiledImage::new(128, 128, 4, [0.2, 0.1, 0.0, 0.5]),
            )]
            .into(),
        },
    })
    .unwrap();
    let paint = s.selected_effect().unwrap();
    let original = s.selected_layer().unwrap().content_effects[0].clone();
    s.execute(Command::AddContentEffect {
        layer,
        kind: ContentEffectKind::Filter {
            channel: C::BaseColor,
            filter: Filter::ALL[0],
        },
    })
    .unwrap();
    let other = s.selected_effect().unwrap();
    let source = EffectAddress {
        texture_set: s.selection().texture_set,
        layer,
        domain: LayerTarget::Content,
        effect: paint,
    };
    let target = EffectAddress {
        effect: other,
        ..source
    };
    s.execute(Command::DropEffect {
        source,
        target,
        side: DropSide::Above,
    })
    .unwrap();
    assert_eq!(
        s.brush.target,
        subcraft_paint::PaintTarget::ContentEffect(paint)
    );
    assert!(Arc::ptr_eq(
        &original,
        &s.selected_layer().unwrap().content_effects[1]
    ));
    let other_set = s
        .document()
        .texture_sets
        .iter()
        .find(|t| t.id != source.texture_set)
        .unwrap()
        .id;
    s.execute(Command::SelectTextureSet(other_set)).unwrap();
    let before = s.document().clone();
    let history = s.history().entries().len();
    assert!(
        s.execute(Command::DropEffect {
            source,
            target,
            side: DropSide::Below
        })
        .is_err()
    );
    assert_eq!(s.document(), &before);
    assert_eq!(s.history().entries().len(), history);
}
