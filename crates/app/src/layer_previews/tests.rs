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

#[test]
fn long_4k_mask_stroke_updates_previews_and_stays_one_undo_step() {
    let (mut s, mut key) = setup(4096);
    key.mask = true;
    s.execute(Command::SelectLayerTarget {
        layer: key.layer,
        target: crate::LayerTarget::Mask,
    })
    .unwrap();
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
    let mut cache = LayerPreviews::default();
    wait(&mut cache, &s, key, None);
    let history = s.history().entries().len();
    let original = s
        .selected_layer()
        .unwrap()
        .mask
        .as_ref()
        .unwrap()
        .base
        .clone();
    s.begin_stroke().unwrap();
    let mut samples = Vec::new();
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
    assert_eq!(
        original.allocated_tiles(),
        0,
        "background snapshots and history retain original storage"
    );
    // Flush the final document revision, not an earlier intermediate stroke thumbnail.
    let mut fresh = LayerPreviews::default();
    let image = wait(&mut fresh, &s, key, None);
    let mask = &s.selected_layer().unwrap().mask.as_ref().unwrap().base;
    assert!(image.rgba.as_chunks::<4>().0.iter().any(|p| p[0] > 0));
    for y in 0..64 {
        for x in 0..64 {
            let expected = (mask.get(x * 64 + 32, y * 64 + 32)[0] * 255.0).round() as u8;
            assert_eq!(image.rgba[((y * 64 + x) * 4) as usize], expected);
        }
    }
    let cold = samples[0];
    samples[16..].sort_by(f64::total_cmp);
    let warm = &samples[16..];
    println!(
        "4K mask stroke + preview request: cold {cold:.3} ms, warm p95 {:.3} ms, max {:.3} ms (CPU only; no composite/upload/frame timing)",
        warm[(warm.len() as f32 * 0.95) as usize],
        warm[warm.len() - 1]
    );
}

#[test]
fn neighborhood_previews_scale_radius_down_and_up_without_changing_document_data() {
    use subcraft_doc::Filter;
    let mut s=Session::new(subcraft_mesh::primitives::plane(2.0,1),128);let layer=s.selection().layer.unwrap();
    s.execute(Command::AddMask {layer,white:false}).unwrap();
    let mut image=subcraft_image::TiledImage::new(128,128,2,[0.0;4]);
    for y in 0..128 {for x in 0..64 {image.set(x,y,[1.0,1.0,0.0,0.0]);}}
    s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Paint {image}}).unwrap();
    s.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Filter(Filter::Blur {radius:32.0})}).unwrap();
    let original=s.document().clone();let ts=&s.document().texture_sets[0];
    let source=Source {layer:s.selected_layer().unwrap().clone().into(),resources:s.resources().clone(),maps:MeshMaps::new(),maps_generation:0,
        resolution:128,albedo_filter:ts.albedo_filter,slot:0,geometry:Arc::new(GeometryCache::new(s.scene().mesh.clone()))};
    for filter in [Filter::Blur {radius:32.0},Filter::DirectionalBlur {radius:32.0,angle_deg:0.0},Filter::Highpass {radius:32.0,amount:1.0}] {
        let mut source=source.clone();let preview_layer=Arc::make_mut(&mut source.layer);
        Arc::make_mut(&mut preview_layer.mask.as_mut().unwrap().effects[1]).kind=MaskEffectKind::Filter(filter);
    for size in [64,256] {
        let result=evaluate(PreviewKey {layer,channel:ChannelKind::BaseColor,mask:true,size},&source);
        let radius=32.0*size as f32/128.0;let weights=subcraft_materials::filter::gaussian_weights(radius);
        let x=size/2+(radius/2.0) as u32;let y=size/2;
        let expected:f32=weights.iter().enumerate().filter(|(i,_)|(x as i32+*i as i32-(weights.len()/2) as i32) < (size/2) as i32).map(|(_,w)|*w).sum();
        let expected=if matches!(filter,Filter::Highpass {..}) {0.5-expected}else{expected};
        let pixel=result.rgba[((y*size+x)*4) as usize] as f32/255.0;
        assert!((pixel-expected).abs()<0.005,"size {size}: {pixel} vs {expected}");assert!(pixel>0.02);
    }
    }
    assert_eq!(s.document(),&original);
}
