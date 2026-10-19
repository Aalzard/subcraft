use super::*;
use crate::{Command, NewLayer};
use subcraft_core::ChannelValue;
use subcraft_doc::{
    BlendMode, ContentEffect, ContentEffectKind, Document, FillChannel, FillSource, Filter,
    Generator, GeneratorKind, Layer, Levels, Mask, MaskEffectKind, TextureSet,
};

fn setup() -> (Session, PreviewKey) {
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 1), 128);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    (
        s,
        PreviewKey {
            layer,
            channel: ChannelKind::BaseColor,
            mask: false,
            effect: None,
            size: 64,
        },
    )
}
fn wait(
    cache: &mut LayerPreviews,
    s: &Session,
    key: PreviewKey,
    old: Option<&Arc<LayerPreview>>,
) -> Arc<LayerPreview> {
    let start = std::time::Instant::now();
    loop {
        if let Some(image) = cache.request(s, key)
            && old.is_none_or(|old| !Arc::ptr_eq(old, &image))
        {
            return image;
        }
        assert!(start.elapsed().as_secs() < 10);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
fn gray(image: &LayerPreview, value: u8) {
    assert!(
        image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3].iter().all(|&c| (c as i32 - value as i32).abs() <= 1) && p[3] == 255),
        "expected gray {value}, got {:?}",
        &image.rgba[..4]
    );
}
#[test]
fn mask_stage_thumbnails_show_paint_fill_generator_levels_invert_and_filter_independently() {
    let (mut s, key) = setup();
    let layer = key.layer;
    s.execute(Command::AddMask {
        layer,
        white: false,
    })
    .unwrap();
    let mut generator = Generator::new(GeneratorKind::Cavity);
    generator.noise_amount = 0.0;
    let stages = [
        MaskEffectKind::Paint {
            image: subcraft_image::TiledImage::new(128, 128, 2, [0.2, 1.0, 0.0, 0.0]),
        },
        MaskEffectKind::Fill {
            value: 0.8,
            procedural: None,
            projection: Default::default(),
        },
        MaskEffectKind::Generator(generator),
        MaskEffectKind::Levels(Levels {
            out_low: 0.3,
            out_high: 0.3,
            ..Levels::default()
        }),
        MaskEffectKind::Invert,
        MaskEffectKind::Filter(Filter::Threshold { cutoff: 0.6 }),
    ];
    let mut keys = Vec::new();
    for kind in stages {
        s.execute(Command::AddMaskEffect { layer, kind }).unwrap();
        keys.push(PreviewKey {
            mask: true,
            effect: s.selected_effect(),
            ..key
        });
    }
    s.execute(Command::SetMaskEnabled {
        layer,
        enabled: false,
    })
    .unwrap();
    let before = s.document().clone();
    let history = s.history().entries().len();
    let mut cache = LayerPreviews::default();
    for (key, value) in keys.iter().zip([51, 204, 0, 77, 179, 255]) {
        gray(&wait(&mut cache, &s, *key, None), value);
    }
    assert_eq!(s.document(), &before);
    assert_eq!(s.history().entries().len(), history);
    // Visibility is a true bypass at this stage; the parent mask's disable is ignored for inspection.
    let last = *keys.last().unwrap();
    let mut effect = s
        .selected_layer()
        .unwrap()
        .mask
        .as_ref()
        .unwrap()
        .effects
        .last()
        .unwrap()
        .as_ref()
        .clone();
    effect.visible = false;
    s.execute(Command::SetMaskEffect { layer, effect }).unwrap();
    gray(&wait(&mut LayerPreviews::default(), &s, last, None), 179);
}
#[test]
fn content_stages_pick_the_adjustment_channel_and_ignore_later_effects_and_own_mask() {
    let (mut s, key) = setup();
    let layer = key.layer;
    s.execute(Command::SetFillChannel {
        layer,
        channel: ChannelKind::BaseColor,
        fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.25, 0.0, 0.0))),
    })
    .unwrap();
    let image = subcraft_image::TiledImage::new(128, 128, 2, [0.8, 1.0, 0.0, 0.0]);
    s.execute(Command::AddContentEffect {
        layer,
        kind: ContentEffectKind::Paint {
            channels: [(ChannelKind::Roughness, image)].into(),
        },
    })
    .unwrap();
    let paint = s.selected_effect();
    s.execute(Command::AddContentEffect {
        layer,
        kind: ContentEffectKind::Filter {
            channel: ChannelKind::Roughness,
            filter: Filter::Contrast {
                gain: 0.5,
                pivot: 0.4,
            },
        },
    })
    .unwrap();
    let adjusted = s.selected_effect();
    s.execute(Command::AddContentEffect {
        layer,
        kind: ContentEffectKind::Fill(subcraft_doc::FillContent {
            projection: Default::default(),
            channels: [(
                ChannelKind::BaseColor,
                FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.0, 0.0, 1.0))),
            )]
            .into(),
        }),
    })
    .unwrap();
    s.execute(Command::AddMask {
        layer,
        white: false,
    })
    .unwrap();
    s.execute(Command::SetVisible {
        layer,
        visible: false,
    })
    .unwrap();
    let mut cache = LayerPreviews::default();
    let rgba = wait(
        &mut cache,
        &s,
        PreviewKey {
            effect: paint,
            ..key
        },
        None,
    );
    assert_eq!(&rgba.rgba[..4], &[137, 0, 0, 255]);
    let rough = wait(
        &mut cache,
        &s,
        PreviewKey {
            effect: adjusted,
            ..key
        },
        None,
    );
    gray(&rough, 153); // .4+(.8-.4)*.5=.6, raw not sRGB.
    let final_image = wait(&mut cache, &s, key, None);
    assert_eq!(&final_image.rgba[..4], &[0, 0, 255, 255]);
}
#[test]
fn effect_preview_latest_edits_move_undo_removal_and_reopen_are_safe() {
    let (mut s, key) = setup();
    let layer = key.layer;
    s.execute(Command::AddMask {
        layer,
        white: false,
    })
    .unwrap();
    s.execute(Command::AddMaskEffect {
        layer,
        kind: MaskEffectKind::Fill {
            value: 0.4,
            procedural: None,
            projection: Default::default(),
        },
    })
    .unwrap();
    let fill = s.selected_effect().unwrap();
    s.execute(Command::AddMaskEffect {
        layer,
        kind: MaskEffectKind::Invert,
    })
    .unwrap();
    let invert = s.selected_effect().unwrap();
    let key = PreviewKey {
        mask: true,
        effect: Some(invert),
        ..key
    };
    let mut cache = LayerPreviews::default();
    let first = wait(&mut cache, &s, key, None);
    gray(&first, 153);
    s.execute(Command::Seal).unwrap();
    let before = s.document().clone();
    let history = s.history().entries().len();
    for value in [0.1, 0.9, 0.2] {
        let mut effect = s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0]
            .as_ref()
            .clone();
        effect.kind = MaskEffectKind::Fill {
            value,
            procedural: None,
            projection: Default::default(),
        };
        s.execute(Command::SetMaskEffect { layer, effect }).unwrap();
        cache.request(&s, key);
    }
    let latest = wait(&mut cache, &s, key, Some(&first));
    gray(&latest, 204);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
    assert_eq!(s.history().entries().len(), history + 1);
    gray(&wait(&mut cache, &s, key, Some(&latest)), 153);
    s.execute(Command::MoveMaskEffect {
        layer,
        effect: fill,
        index: 1,
    })
    .unwrap();
    gray(&wait(&mut LayerPreviews::default(), &s, key, None), 255);
    s.execute(Command::RemoveMaskEffect {
        layer,
        effect: invert,
    })
    .unwrap();
    assert!(cache.request(&s, key).is_none());
    assert!(!cache.entries.iter().any(|e| e.key == key));
    s.execute(Command::Undo).unwrap();
    gray(&wait(&mut cache, &s, key, None), 255);
    let path = std::env::temp_dir().join(format!("subcraft-stage-{}.subcraft", layer.0));
    s.save(Some(&path)).unwrap();
    let reopened = Session::open_project(&path).unwrap();
    let preview = wait(&mut LayerPreviews::default(), &reopened, key, None);
    gray(&preview, 255);
    assert_eq!(reopened.document(), s.document());
    std::fs::remove_file(path).unwrap();
}
#[test]
fn neighborhood_stage_preview_reads_its_prefix_and_scales_radius() {
    let (mut s, key) = setup();
    let layer = key.layer;
    s.execute(Command::AddMask {
        layer,
        white: false,
    })
    .unwrap();
    let mut image = subcraft_image::TiledImage::new(128, 128, 2, [0.0; 4]);
    for y in 0..128 {
        for x in 0..64 {
            image.set(x, y, [1.0, 1.0, 0.0, 0.0]);
        }
    }
    s.execute(Command::AddMaskEffect {
        layer,
        kind: MaskEffectKind::Paint { image },
    })
    .unwrap();
    s.execute(Command::AddMaskEffect {
        layer,
        kind: MaskEffectKind::Filter(Filter::Blur { radius: 16.0 }),
    })
    .unwrap();
    let blur = s.selected_effect();
    s.execute(Command::AddMaskEffect {
        layer,
        kind: MaskEffectKind::Invert,
    })
    .unwrap();
    for size in [64, 256] {
        let preview = wait(
            &mut LayerPreviews::default(),
            &s,
            PreviewKey {
                mask: true,
                effect: blur,
                size,
                ..key
            },
            None,
        );
        let weights = subcraft_materials::filter::gaussian_weights(16.0 * size as f32 / 128.0);
        // Just outside the step: only the negative half of the Gaussian sees white.
        let expected = weights[..weights.len() / 2].iter().sum::<f32>();
        let v = preview.rgba[((size / 2 * size + size / 2) * 4) as usize] as f32 / 255.0;
        assert!((v - expected).abs() < 0.005, "{size}: {v} vs {expected}");
        assert!(v < 0.5);
    }
}
#[test]
fn nested_folder_stages_respect_lower_stack_isolation_and_invalidate_on_lower_edits() {
    for isolated in [false, true] {
        let mut ts = TextureSet::new("T", 0, 128);
        let mut lower = Layer::new_fill("Lower", &ts);
        let lower_id = lower.id;
        let LayerContent::Fill(f) = &mut lower.content else {
            unreachable!()
        };
        f.channels.insert(
            ChannelKind::BaseColor,
            FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.25, 0.0, 0.0))),
        );
        let mut target = Layer::new_folder("Stage", &ts);
        let id = target.id;
        target.mask = Some(Mask::new(128, false));
        target.visible = false;
        target.content_effects.push(Arc::new(ContentEffect::new(
            ContentEffectKind::Filter {
                channel: ChannelKind::BaseColor,
                filter: Filter::Hsl {
                    hue: 120.0,
                    saturation: 1.0,
                    lightness: 0.0,
                },
            },
            &ts,
        )));
        let effect = target.content_effects[0].id;
        let mut parent = Layer::new_folder("Parent", &ts);
        if isolated {
            parent.blend.get_mut(&ChannelKind::BaseColor).unwrap().mode = BlendMode::Normal;
        }
        let LayerContent::Group(g) = &mut parent.content else {
            unreachable!()
        };
        g.children.push(Arc::new(target));
        // Neither parent effects nor a later sibling may enter the target's input.
        parent.mask = Some(Mask::new(128, false));
        parent.content_effects.push(Arc::new(ContentEffect::new(
            ContentEffectKind::Filter {
                channel: ChannelKind::BaseColor,
                filter: Filter::Threshold { cutoff: 0.0 },
            },
            &ts,
        )));
        let above = Layer::new_fill("Above", &ts);
        ts.layers = vec![Arc::new(lower), Arc::new(parent), Arc::new(above)];
        let mut s = Session::from_parts(
            subcraft_mesh::primitives::plane(2.0, 1),
            Document {
                texture_sets: vec![ts],
            },
            ResourceStore::default(),
            "Stage",
        );
        let key = PreviewKey {
            layer: id,
            channel: ChannelKind::BaseColor,
            mask: false,
            effect: Some(effect),
            size: 64,
        };
        let mut cache = LayerPreviews::default();
        let before = wait(&mut cache, &s, key, None);
        assert_eq!(
            &before.rgba[..4],
            if isolated {
                &[0, 0, 0, 0]
            } else {
                &[0, 137, 0, 255]
            }
        );
        s.execute(Command::SetFillChannel {
            layer: lower_id,
            channel: ChannelKind::BaseColor,
            fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 0.0, 0.0))),
        })
        .unwrap();
        let after = wait(&mut cache, &s, key, Some(&before));
        assert_eq!(
            &after.rgba[..4],
            if isolated {
                &[0, 0, 0, 0]
            } else {
                &[0, 255, 0, 255]
            }
        );
    }
}
#[test]
fn effect_preview_queue_and_cache_remain_bounded() {
    let (mut s, key) = setup();
    let layer = key.layer;
    let mut keys = Vec::new();
    for _ in 0..CACHE_LIMIT + 8 {
        s.execute(Command::AddContentEffect {
            layer,
            kind: ContentEffectKind::Levels {
                channel: ChannelKind::BaseColor,
                levels: Levels::default(),
            },
        })
        .unwrap();
        keys.push(PreviewKey {
            effect: s.selected_effect(),
            ..key
        });
    }
    let mut cache = LayerPreviews::default();
    for key in &keys {
        cache.request(&s, *key);
    }
    assert_eq!(cache.len(), CACHE_LIMIT);
    assert!(cache.queue.len() <= CACHE_LIMIT);
    let image = wait(&mut cache, &s, *keys.last().unwrap(), None);
    assert_eq!(image.size, 64);
    assert_eq!(image.rgba.len(), 64 * 64 * 4);
}

#[test]
fn long_4k_stroke_updates_the_paint_effect_thumbnail_and_remains_one_undo_step() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), 4096);
    let layer = s.selection().layer.unwrap();
    s.execute(Command::AddMask {
        layer,
        white: false,
    })
    .unwrap();
    s.execute(Command::AddMaskEffect {
        layer,
        kind: MaskEffectKind::Paint {
            image: subcraft_image::TiledImage::new(4096, 4096, 2, [0.0; 4]),
        },
    })
    .unwrap();
    let effect = s.selected_effect().unwrap();
    let key = PreviewKey {
        layer,
        mask: true,
        effect: Some(effect),
        channel: ChannelKind::BaseColor,
        size: 64,
    };
    let mut cache = LayerPreviews::default();
    wait(&mut cache, &s, key, None);
    s.brush.settings.size = 5.0;
    s.brush.settings.hardness = 1.0;
    let camera = subcraft_paint::Camera::orthographic(
        glam::Vec3::new(0.0, 0.0, 4.0),
        glam::Vec3::ZERO,
        glam::Vec3::Y,
        1.3,
        0.01,
        20.0,
        [512, 512],
    );
    let before = s.document().clone();
    let history = s.history().entries().len();
    let mut samples = Vec::new();
    s.begin_stroke().unwrap();
    for i in 0..160 {
        let start = std::time::Instant::now();
        s.stroke_to(
            &camera,
            glam::Vec2::new(
                90.0 + 330.0 * i as f32 / 159.0,
                256.0 + (i as f32 * 0.1).sin() * 20.0,
            ),
            false,
        )
        .unwrap();
        cache.request(&s, key);
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    s.end_stroke();
    assert_eq!(s.history().entries().len(), history + 1);
    let preview = wait(&mut LayerPreviews::default(), &s, key, None);
    let MaskEffectKind::Paint { image } =
        &s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].kind
    else {
        unreachable!()
    };
    assert!(preview.rgba.as_chunks::<4>().0.iter().any(|p| p[0] > 0));
    for y in 0..64 {
        for x in 0..64 {
            assert_eq!(
                preview.rgba[((y * 64 + x) * 4) as usize],
                (image.get(x * 64 + 32, y * 64 + 32)[0] * 255.0).round() as u8
            );
        }
    }
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
    let cold = samples[0];
    samples[16..].sort_by(f64::total_cmp);
    let warm = &samples[16..];
    println!(
        "4K paint-effect stroke + thumbnail request: cold {cold:.3} ms; warm p95 {:.3}, max {:.3} ms. CPU only, excludes compositing/render/upload.",
        warm[warm.len() * 95 / 100],
        warm[warm.len() - 1]
    );
}
