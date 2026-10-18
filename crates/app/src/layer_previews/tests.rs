use super::*;
use crate::{Command, NewLayer};
use subcraft_core::ChannelValue;
use subcraft_doc::{FillChannel, FillSource, MaskEffectKind};

fn setup(resolution: u32) -> (Session, PreviewKey) {
    let mut session = Session::new(subcraft_mesh::primitives::cube(2.0), resolution);
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = session.selection().layer.unwrap();
    session
        .execute(Command::SetFillChannel {
            layer,
            channel: ChannelKind::BaseColor,
            fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.25, 0.0, 0.0))),
        })
        .unwrap();
    session
        .execute(Command::AddMask {
            layer,
            white: false,
        })
        .unwrap();
    (
        session,
        PreviewKey {
            layer,
            channel: ChannelKind::BaseColor,
            mask: false,
            size: 64,
        },
    )
}
fn wait(
    cache: &mut LayerPreviews,
    session: &Session,
    key: PreviewKey,
    old: Option<&Arc<LayerPreview>>,
) -> Arc<LayerPreview> {
    let start = std::time::Instant::now();
    loop {
        if let Some(image) = cache.request(session, key)
            && old.is_none_or(|old| !Arc::ptr_eq(old, &image))
        {
            return image;
        }
        assert!(
            start.elapsed().as_secs() < 5,
            "preview worker did not publish"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn content_color_data_and_mask_are_independent_and_use_correct_display_encoding() {
    let (s, key) = setup(4096);
    let mut cache = LayerPreviews::default();
    let image = wait(&mut cache, &s, key, None);
    assert_eq!(image.rgba.len(), 64 * 64 * 4);
    assert!(
        image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| *px == [137, 0, 0, 255])
    );
    let mask = wait(&mut cache, &s, PreviewKey { mask: true, ..key }, None);
    assert!(
        mask.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| *px == [0, 0, 0, 255])
    );
    let normal = wait(
        &mut cache,
        &s,
        PreviewKey {
            channel: ChannelKind::Normal,
            ..key
        },
        None,
    );
    assert!(
        normal
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| *px == [128, 128, 255, 255])
    );
    assert_eq!(
        s.selected_layer()
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .base
            .allocated_tiles(),
        0
    );
}

#[test]
fn masks_match_production_texels_including_effects_disabled_state_and_large_inspection() {
    let (mut s, mut key) = setup(64);
    key.mask = true;
    s.execute(Command::AddMaskEffect {
        layer: key.layer,
        kind: MaskEffectKind::Fill {
            value: 0.25,
            procedural: None,
            projection: Default::default(),
        },
    })
    .unwrap();
    let mut cache = LayerPreviews::default();
    let image = wait(&mut cache, &s, key, None);
    s.update_composites();
    let composite = s
        .composite(s.selection().texture_set)
        .unwrap()
        .channel(ChannelKind::BaseColor)
        .unwrap();
    let backdrop = ChannelKind::BaseColor.default_value().0[0];
    for y in 0..64 {
        for x in 0..64 {
            let mask = image.rgba[((y * 64 + x) * 4) as usize] as f32 / 255.0;
            assert!((mask - 0.25).abs() <= 1.0 / 255.0);
            assert!((composite.get(x, y)[0] - (backdrop * 0.75 + 0.25 * 0.25)).abs() < 0.001);
        }
    }
    s.execute(Command::SetMaskEnabled {
        layer: key.layer,
        enabled: false,
    })
    .unwrap();
    let disabled = wait(&mut cache, &s, key, Some(&image));
    assert_eq!(
        disabled.rgba, image.rgba,
        "bypass must not hide the authored mask"
    );
    let large = wait(&mut cache, &s, PreviewKey { size: 4096, ..key }, None);
    assert_eq!(large.size, 256);
    assert_eq!(large.rgba.len(), 256 * 256 * 4);
    assert!(
        large
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| *px == [64, 64, 64, 255])
    );
}

#[test]
fn cache_reuses_selection_changes_rejects_old_edits_and_clears_on_session_replacement() {
    let (mut s, mut key) = setup(64);
    key.mask = true;
    let mut cache = LayerPreviews::default();
    let before = wait(&mut cache, &s, key, None);
    s.execute(Command::SelectLayer(Some(key.layer))).unwrap();
    assert!(Arc::ptr_eq(&cache.request(&s, key).unwrap(), &before));
    s.execute(Command::InvertMask(key.layer)).unwrap();
    let inverted = wait(&mut cache, &s, key, Some(&before));
    assert!(
        inverted
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| *px == [255; 4])
    );
    s.execute(Command::Undo).unwrap();
    let restored = wait(&mut cache, &s, key, Some(&inverted));
    assert_eq!(restored.rgba, before.rgba);
    let replacement = Session::from_parts(
        (*s.scene().mesh).clone(),
        s.document().clone(),
        s.resources().clone(),
        "Reopen",
    );
    assert!(
        cache.request(&replacement, key).is_none(),
        "old scene previews must never leak into a reopened project"
    );
    let reopened = wait(&mut cache, &replacement, key, None);
    assert_eq!(reopened.rgba, before.rgba);
}

#[test]
fn preview_cache_is_bounded_and_transparent_paint_does_not_include_other_layers() {
    let (mut s, key) = setup(64);
    let mut cache = LayerPreviews::default();
    for _ in 0..CACHE_LIMIT + 5 {
        s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
        cache.request(
            &s,
            PreviewKey {
                layer: s.selection().layer.unwrap(),
                ..key
            },
        );
    }
    assert_eq!(cache.len(), CACHE_LIMIT);
    let paint = wait(
        &mut cache,
        &s,
        PreviewKey {
            layer: s.selection().layer.unwrap(),
            ..key
        },
        None,
    );
    assert!(paint.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 0));
    assert!(cache.queue.len() <= CACHE_LIMIT);
}
