use std::sync::Arc;
use subcraft_compositor::{
    CompositeCache, EvalContext, TileContext, composite_full, tile::preview_content,
};
use subcraft_core::ChannelKind as C;
use subcraft_doc::{
    BlendMode, ContentEffect, ContentEffectKind, Filter, Layer, LayerContent, Mask, MaskEffect,
    MaskEffectKind, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, SampleContext, TextureSetGeometry};
const N: u32 = 128;
struct Fixture {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}
impl Fixture {
    fn new() -> Self {
        Self {
            geo: TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 2)), 0, N),
            res: ResourceStore::default(),
            maps: MeshMaps::new(),
        }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext {
            geometry: &self.geo,
            resources: &self.res,
            mesh_maps: &self.maps,
        }
    }
}
fn add(layer: &mut Layer, ts: &TextureSet, kind: C, filter: Filter) {
    layer.content_effects.push(Arc::new(ContentEffect::new(
        ContentEffectKind::Filter {
            channel: kind,
            filter,
        },
        ts,
    )));
}
fn field() -> (TextureSet, Layer, Vec<f32>) {
    let ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("P", &ts);
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    let image = p.channel_mut(C::Roughness, N);
    let mut data = Vec::new();
    for y in 0..N {
        for x in 0..N {
            let v = if (x / 7 + y / 11) % 2 == 0 { 0.0 } else { 1.0 };
            image.set(x, y, [v, 1.0, 0.0, 0.0]);
            data.push(v);
        }
    }
    (ts, l, data)
}
// Dense independent 2D Gaussian reference (production uses two tiled 1D passes).
fn reference(input: &[f32], radius: f32) -> Vec<f32> {
    if radius == 0.0 {
        return input.to_vec();
    }
    let r = radius.ceil() as i32;
    let sigma = radius / 3.0;
    let mut out = vec![0.0; input.len()];
    for y in 0..N as i32 {
        for x in 0..N as i32 {
            let (mut sum, mut norm) = (0.0, 0.0);
            for dy in -r..=r {
                for dx in -r..=r {
                    let w = (-((dx * dx + dy * dy) as f32) / (2.0 * sigma * sigma)).exp();
                    let xx = (x + dx).clamp(0, N as i32 - 1);
                    let yy = (y + dy).clamp(0, N as i32 - 1);
                    sum += input[(yy * N as i32 + xx) as usize] * w;
                    norm += w;
                }
            }
            out[(y * N as i32 + x) as usize] = sum / norm;
        }
    }
    out
}
fn assert_image(ts: &TextureSet, fx: &Fixture, expected: &[f32]) {
    let c = composite_full(ts, &fx.ctx());
    let img = c.channel(C::Roughness).unwrap();
    for y in 0..N {
        for x in 0..N {
            let actual = img.get(x, y)[0];
            let wanted = expected[(y * N + x) as usize];
            assert!(
                (actual - wanted).abs() < 0.0006,
                "at {x},{y}: {actual} vs {wanted}"
            );
        }
    }
}
#[test]
fn gaussian_and_unsharp_match_dense_reference_across_tile_borders_and_texture_edges() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = field();
    add(&mut l, &ts, C::Roughness, Filter::Blur { radius: 3.0 });
    let id = ts.insert_above(None, l);
    let blurred = reference(&data, 3.0);
    assert_image(&ts, &fx, &blurred);
    // A second neighborhood stage must read the first stage, not the base image.
    let blurred_twice = reference(&blurred, 2.0);
    let tc = ts.clone();
    add(
        ts.find_mut(id).unwrap(),
        &tc,
        C::Roughness,
        Filter::Sharpen {
            radius: 2.0,
            amount: 0.7,
        },
    );
    let sharpened: Vec<_> = blurred
        .iter()
        .zip(blurred_twice)
        .map(|(a, b)| (a + 0.7 * (a - b)).clamp(0.0, 1.0))
        .collect();
    assert_image(&ts, &fx, &sharpened);
}
#[test]
fn blur_expands_premultiplied_paint_coverage_without_dark_fringes_and_sharpen_keeps_alpha() {
    let fx = Fixture::new();
    let ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("Dot", &ts);
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    p.channel_mut(C::BaseColor, N)
        .set(63, 63, [0.8, 0.2, 0.0, 0.5]);
    let geo = fx.geo.tile(3);
    let tc = TileContext {
        albedo_filter: ts.albedo_filter,
        geo: &geo,
        set_geometry: &fx.geo,
        sample: SampleContext {
            resources: &fx.res,
            mesh_maps: &fx.maps,
        },
    };
    add(&mut l, &ts, C::BaseColor, Filter::Blur { radius: 3.0 });
    let px = preview_content(&l, C::BaseColor, &tc)[0];
    let w = subcraft_materials::filter::gaussian_weights(3.0);
    assert!((px[3] - 0.5 * w[4] * w[4]).abs() < 1e-6);
    assert!((px[0] - 0.8).abs() < 0.0003);
    assert!((px[1] - 0.2).abs() < 0.0001);
    Arc::make_mut(&mut l.content_effects[0])
        .blend
        .get_mut(&C::BaseColor)
        .unwrap()
        .opacity = 0.5;
    let half = preview_content(&l, C::BaseColor, &tc)[0];
    assert!((half[3] - px[3] * 0.5).abs() < 1e-6);
    assert!((half[0] - px[0]).abs() < 1e-6);
    l.content_effects.clear();
    add(
        &mut l,
        &ts,
        C::BaseColor,
        Filter::Sharpen {
            radius: 3.0,
            amount: 1.0,
        },
    );
    assert_eq!(preview_content(&l, C::BaseColor, &tc)[0][3], 0.0);
    let geo = fx.geo.tile(0);
    let tc = TileContext { geo: &geo, ..tc };
    assert_eq!(preview_content(&l, C::BaseColor, &tc)[4095][3], 0.5);
}
#[test]
fn mask_effects_filter_preceding_stage_and_remain_separate_from_content() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = field();
    let mut mask = Mask::new(N, false);
    for y in 0..N {
        for x in 0..N {
            mask.base
                .set(x, y, [data[(y * N + x) as usize], 0.0, 0.0, 0.0]);
        }
    }
    mask.effects
        .push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(
            Filter::Blur { radius: 3.0 },
        ))));
    l.mask = Some(mask);
    ts.insert_above(None, l);
    let filtered = reference(&data, 3.0);
    let expected: Vec<_> = data
        .iter()
        .zip(filtered)
        .map(|(v, m)| v * m + 0.3 * (1.0 - m))
        .collect();
    assert_image(&ts, &fx, &expected);
}
#[test]
fn raster_edits_invalidate_neighbor_halos_and_budgeted_cache_converges_exactly() {
    let fx = Fixture::new();
    let (mut ts, mut l, _) = field();
    add(&mut l, &ts, C::Roughness, Filter::Blur { radius: 8.0 });
    add(&mut l, &ts, C::Roughness, Filter::Blur { radius: 8.0 });
    let id = ts.insert_above(None, l);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    let before = cache.composite().unwrap().clone();
    let LayerContent::Paint(p) = &mut ts.find_mut(id).unwrap().content else {
        unreachable!()
    };
    p.channel_mut(C::Roughness, N)
        .set(63, 63, [0.5, 1.0, 0.0, 0.0]);
    cache.update_within(&ts, &fx.ctx(), Some(std::time::Duration::ZERO));
    cache.update(&ts, &fx.ctx());
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));
    for tile in 0..4 {
        let (mut a, mut b) = (vec![0.0; 4096], vec![0.0; 4096]);
        before
            .channel(C::Roughness)
            .unwrap()
            .read_tile(tile, &mut a);
        cache
            .composite()
            .unwrap()
            .channel(C::Roughness)
            .unwrap()
            .read_tile(tile, &mut b);
        assert_ne!(a, b);
    }
}
#[test]
fn identity_spatial_graph_matches_point_compositor_with_folders_and_effect_blends() {
    let fx = Fixture::new();
    let (mut ts, mut l, _) = field();
    let mut mask = Mask::new(N, true);
    mask.effects
        .push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(
            Default::default(),
        ))));
    l.mask = Some(mask);
    add(
        &mut l,
        &ts,
        C::Roughness,
        Filter::Contrast {
            gain: 1.3,
            pivot: 0.4,
        },
    );
    ts.insert_above(None, l);
    let mut folder = Layer::new_folder("Adjustment", &ts);
    add(
        &mut folder,
        &ts,
        C::Roughness,
        Filter::Clamp {
            low: 0.15,
            high: 0.8,
        },
    );
    let id = ts.insert_above(None, folder);
    let expected = composite_full(&ts, &fx.ctx());
    let tc = ts.clone();
    add(
        ts.find_mut(id).unwrap(),
        &tc,
        C::Roughness,
        Filter::Blur { radius: 0.0 },
    );
    assert_eq!(composite_full(&ts, &fx.ctx()), expected);
    ts.find_mut(id)
        .unwrap()
        .blend
        .get_mut(&C::Roughness)
        .unwrap()
        .mode = BlendMode::Normal;
    let with = composite_full(&ts, &fx.ctx());
    ts.find_mut(id).unwrap().content_effects.pop();
    assert_eq!(composite_full(&ts, &fx.ctx()), with);
}
#[test]
fn sharpen_does_not_clip_signed_height_and_never_changes_normals() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("Height", &ts);
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    for y in 0..N {
        for x in 0..N {
            p.channel_mut(C::Height, N).set(x, y, [0.0, 1.0, 0.0, 0.0]);
        }
    }
    p.channel_mut(C::Height, N)
        .set(63, 63, [-0.5, 1.0, 0.0, 0.0]);
    add(
        &mut l,
        &ts,
        C::Height,
        Filter::Sharpen {
            radius: 3.0,
            amount: 2.0,
        },
    );
    ts.insert_above(None, l);
    let c = composite_full(&ts, &fx.ctx());
    assert!(c.channel(C::Height).unwrap().get(63, 63)[0] < -0.5);
    assert_eq!(
        c.channel(C::Normal).unwrap().get(63, 63),
        [0.5, 0.5, 1.0, 1.0]
    );
}

#[test]
fn chained_support_invalidates_more_than_one_neighbor_tile() {
    let geometry =
        TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 1)), 0, 256);
    let resources = ResourceStore::default();
    let maps = MeshMaps::new();
    let ctx = EvalContext {
        geometry: &geometry,
        resources: &resources,
        mesh_maps: &maps,
    };
    let mut ts = TextureSet::new("Halo", 0, 256);
    ts.channels.retain(|c| c.kind == C::Roughness);
    let mut l = Layer::new_paint("P", &ts);
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    p.channels.insert(
        C::Roughness,
        subcraft_image::TiledImage::new(256, 256, 2, [0.0, 1.0, 0.0, 0.0]),
    );
    for _ in 0..3 {
        add(&mut l, &ts, C::Roughness, Filter::Blur { radius: 32.0 });
    }
    let id = ts.insert_above(None, l);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &ctx);
    let LayerContent::Paint(p) = &mut ts.find_mut(id).unwrap().content else {
        unreachable!()
    };
    p.channel_mut(C::Roughness, 256)
        .set(63, 63, [1.0, 1.0, 0.0, 0.0]);
    assert_eq!(cache.update(&ts, &ctx), vec![0, 1, 2, 4, 5, 6, 8, 9, 10]);
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &ctx));
    assert!(
        cache
            .composite()
            .unwrap()
            .channel(C::Roughness)
            .unwrap()
            .get(128, 63)[0]
            > 0.0
    );
}

#[test]
fn graph_preserves_all_existing_blend_modes_and_nested_layer_masks() {
    use subcraft_core::ChannelValue;
    use subcraft_doc::{FillChannel, FillSource};
    let fx = Fixture::new();
    for mode in BlendMode::ALL {
        let (mut ts, paint, _) = field();
        ts.insert_above(None, paint);
        let mut fill = Layer::new_fill("Fill", &ts);
        let LayerContent::Fill(f) = &mut fill.content else {
            unreachable!()
        };
        f.channels.insert(
            C::BaseColor,
            FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.2, 0.5, 0.9))),
        );
        for b in fill.blend.values_mut() {
            b.mode = mode;
            b.opacity = 0.4;
        }
        let mut mask = Mask::new(N, true);
        mask.effects
            .push(Arc::new(MaskEffect::new(MaskEffectKind::Invert)));
        mask.effects[0] = Arc::new(MaskEffect {
            opacity: 0.3,
            ..mask.effects[0].as_ref().clone()
        });
        fill.mask = Some(mask);
        let mut group = Layer::new_folder("Group", &ts);
        let LayerContent::Group(g) = &mut group.content else {
            unreachable!()
        };
        g.children.push(Arc::new(fill));
        for b in group.blend.values_mut() {
            b.opacity = 0.65;
        }
        let id = ts.insert_above(None, group);
        let expected = composite_full(&ts, &fx.ctx());
        let tc = ts.clone();
        add(
            ts.find_mut(id).unwrap(),
            &tc,
            C::Roughness,
            Filter::Blur { radius: 0.0 },
        );
        assert_eq!(composite_full(&ts, &fx.ctx()), expected, "{mode:?}");
    }
}

#[test]
fn absent_paint_and_disabled_fill_sources_skip_replace_even_when_another_layer_needs_spatial_evaluation()
 {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("Absent", 0, N);
    let mut empty = Layer::new_paint("Empty", &ts);
    for b in empty.blend.values_mut() {
        b.mode = BlendMode::Replace;
    }
    ts.insert_above(None, empty);
    let mut fill = Layer::new_fill("Disabled", &ts);
    let LayerContent::Fill(f) = &mut fill.content else {
        unreachable!()
    };
    for c in f.channels.values_mut() {
        c.enabled = false;
    }
    for b in fill.blend.values_mut() {
        b.mode = BlendMode::Replace;
    }
    ts.insert_above(None, fill);
    let mut folder = Layer::new_folder("Adjustment", &ts);
    let id = folder.id;
    let expected = composite_full(&ts, &fx.ctx());
    add(&mut folder, &ts, C::Roughness, Filter::Blur { radius: 0.0 });
    ts.insert_above(None, folder);
    assert_eq!(composite_full(&ts, &fx.ctx()), expected);
    let tc = ts.clone();
    ts.find_mut(id).unwrap().content_effects.clear();
    add(
        ts.find_mut(id).unwrap(),
        &tc,
        C::BaseColor,
        Filter::Blur { radius: 4.0 },
    );
    assert_eq!(
        composite_full(&ts, &fx.ctx()),
        expected,
        "uniform filtered defaults stay unchanged"
    );
}

// Independent dense, double-precision line convolution with bilinear texture-edge clamping.
fn line_reference(data: &[f32], radius: f32, angle: f32) -> Vec<f32> {
    if radius == 0.0 {
        return data.to_vec();
    }
    let r = radius.ceil() as i32;
    let sigma = radius as f64 / 3.0;
    let angle = (angle as f64).to_radians();
    let mut out = vec![0.0; data.len()];
    for y in 0..N {
        for x in 0..N {
            let (mut sum, mut norm) = (0.0, 0.0);
            for step in -r..=r {
                let w = (-(step as f64).powi(2) / (2.0 * sigma * sigma)).exp();
                let xx = (x as f64 + step as f64 * angle.cos()).clamp(0.0, (N - 1) as f64);
                let yy = (y as f64 + step as f64 * angle.sin()).clamp(0.0, (N - 1) as f64);
                let (x0, y0) = (xx.floor() as u32, yy.floor() as u32);
                let (fx, fy) = (xx - x0 as f64, yy - y0 as f64);
                let mut value = 0.0;
                for (sy, wy) in [(y0, 1.0 - fy), ((y0 + 1).min(N - 1), fy)] {
                    for (sx, wx) in [(x0, 1.0 - fx), ((x0 + 1).min(N - 1), fx)] {
                        value += data[(sy * N + sx) as usize] as f64 * wx * wy;
                    }
                }
                sum += value * w;
                norm += w;
            }
            out[(y * N + x) as usize] = (sum / norm) as f32;
        }
    }
    out
}
#[test]
fn directional_angles_fractional_radius_and_image_borders_match_dense_reference() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = field();
    add(
        &mut l,
        &ts,
        C::Roughness,
        Filter::DirectionalBlur {
            radius: 3.0,
            angle_deg: 0.0,
        },
    );
    let id = ts.insert_above(None, l);
    for (radius, angle_deg) in [
        (3.0, 0.0),
        (3.0, 90.0),
        (3.0, 45.0),
        (2.5, -30.0),
        (0.0, 45.0),
    ] {
        Arc::make_mut(&mut ts.find_mut(id).unwrap().content_effects[0]).kind =
            ContentEffectKind::Filter {
                channel: C::Roughness,
                filter: Filter::DirectionalBlur { radius, angle_deg },
            };
        assert_image(&ts, &fx, &line_reference(&data, radius, angle_deg));
    }
}
#[test]
fn directional_blur_filters_premultiplied_coverage_and_uses_the_uv_downward_angle() {
    let fx = Fixture::new();
    let ts = TextureSet::new("Dot", 0, N);
    let mut l = Layer::new_paint("Dot", &ts);
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    p.channel_mut(C::BaseColor, N)
        .set(63, 63, [0.8, 0.2, 0.0, 0.5]);
    let mut alpha = vec![0.0; (N * N) as usize];
    alpha[(63 * N + 63) as usize] = 0.5;
    let expected = line_reference(&alpha, 3.0, 45.0);
    add(
        &mut l,
        &ts,
        C::BaseColor,
        Filter::DirectionalBlur {
            radius: 3.0,
            angle_deg: 45.0,
        },
    );
    let geo = fx.geo.tile(3);
    let tc = TileContext {
        albedo_filter: ts.albedo_filter,
        geo: &geo,
        set_geometry: &fx.geo,
        sample: SampleContext {
            resources: &fx.res,
            mesh_maps: &fx.maps,
        },
    };
    let px = preview_content(&l, C::BaseColor, &tc)[0];
    assert!(px[3] > 0.0);
    assert!((px[3] - expected[(64 * N + 64) as usize]).abs() < 1e-6);
    assert!((px[0] - 0.8).abs() < 0.0003 && (px[1] - 0.2).abs() < 0.0001);
    Arc::make_mut(&mut l.content_effects[0])
        .blend
        .get_mut(&C::BaseColor)
        .unwrap()
        .opacity = 0.5;
    let half = preview_content(&l, C::BaseColor, &tc)[0];
    assert!((half[3] - px[3] * 0.5).abs() < 1e-6);
    assert!((half[0] - px[0]).abs() < 1e-6);
    let geo = fx.geo.tile(1);
    let tc = TileContext { geo: &geo, ..tc };
    let across = preview_content(&l, C::BaseColor, &tc)[62 * 64];
    assert_eq!(across[3], 0.0, "45 degrees runs down-right in the image");
}
#[test]
fn highpass_matches_dense_previous_stage_and_effect_blend_opacity() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = field();
    add(
        &mut l,
        &ts,
        C::Roughness,
        Filter::Highpass {
            radius: 3.0,
            amount: 1.5,
        },
    );
    let id = ts.insert_above(None, l);
    let low = reference(&data, 3.0);
    let high: Vec<_> = data
        .iter()
        .zip(low)
        .map(|(v, b)| (0.5 + 1.5 * (v - b)).clamp(0.0, 1.0))
        .collect();
    assert_image(&ts, &fx, &high);
    let e = Arc::make_mut(&mut ts.find_mut(id).unwrap().content_effects[0]);
    let blend = e.blend.get_mut(&C::Roughness).unwrap();
    blend.opacity = 0.5;
    blend.mode = BlendMode::Multiply;
    let expected: Vec<_> = data
        .iter()
        .zip(high)
        .map(|(v, h)| v * 0.5 + v * h * 0.5)
        .collect();
    assert_image(&ts, &fx, &expected);
}
#[test]
fn highpass_zero_controls_are_neutral_and_height_and_paint_alpha_stay_signed_or_unchanged() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("Highpass", 0, N);
    let mut l = Layer::new_paint("Paint", &ts);
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    for y in 0..N {
        for x in 0..N {
            p.channel_mut(C::Height, N).set(x, y, [0.0, 1.0, 0.0, 0.0]);
        }
    }
    p.channel_mut(C::Height, N)
        .set(63, 63, [-0.5, 1.0, 0.0, 0.0]);
    p.channel_mut(C::BaseColor, N)
        .set(63, 63, [0.8, 0.2, 0.1, 0.5]);
    add(
        &mut l,
        &ts,
        C::Height,
        Filter::Highpass {
            radius: 3.0,
            amount: 2.0,
        },
    );
    add(
        &mut l,
        &ts,
        C::BaseColor,
        Filter::Highpass {
            radius: 3.0,
            amount: 1.0,
        },
    );
    let id = ts.insert_above(None, l);
    let c = composite_full(&ts, &fx.ctx());
    assert!(c.channel(C::Height).unwrap().get(63, 63)[0] < -0.5);
    assert_eq!(
        c.channel(C::Normal).unwrap().get(63, 63),
        [0.5, 0.5, 1.0, 1.0]
    );
    let geo = fx.geo.tile(0);
    let tc = TileContext {
        albedo_filter: ts.albedo_filter,
        geo: &geo,
        set_geometry: &fx.geo,
        sample: SampleContext {
            resources: &fx.res,
            mesh_maps: &fx.maps,
        },
    };
    let pixels = preview_content(ts.find(id).unwrap(), C::BaseColor, &tc);
    assert_eq!(pixels[4095][3], 0.5);
    assert_eq!(pixels[0][3], 0.0);
    for (radius, amount) in [(0.0, 2.0), (8.0, 0.0)] {
        let layer = ts.find_mut(id).unwrap();
        for e in &mut layer.content_effects {
            let e = Arc::make_mut(e);
            let ContentEffectKind::Filter { filter, .. } = &mut e.kind else {
                unreachable!()
            };
            *filter = Filter::Highpass { radius, amount };
        }
        let c = composite_full(&ts, &fx.ctx());
        assert_eq!(c.channel(C::Height).unwrap().get(63, 63)[0], 0.0);
        let pixels = preview_content(ts.find(id).unwrap(), C::BaseColor, &tc);
        assert_eq!(pixels[4095], [0.5, 0.5, 0.5, 0.5]);
    }
}
#[test]
fn directional_and_highpass_order_is_preserved_for_passthrough_and_isolated_folders() {
    let fx = Fixture::new();
    let (mut ts, l, data) = field();
    ts.insert_above(None, l);
    let mut folder = Layer::new_folder("Filters", &ts);
    add(
        &mut folder,
        &ts,
        C::Roughness,
        Filter::DirectionalBlur {
            radius: 3.0,
            angle_deg: 45.0,
        },
    );
    add(
        &mut folder,
        &ts,
        C::Roughness,
        Filter::Highpass {
            radius: 2.0,
            amount: 1.0,
        },
    );
    let id = ts.insert_above(None, folder);
    let line = line_reference(&data, 3.0, 45.0);
    let low = reference(&line, 2.0);
    let expected: Vec<_> = line
        .iter()
        .zip(low)
        .map(|(v, b)| (0.5 + v - b).clamp(0.0, 1.0))
        .collect();
    assert_image(&ts, &fx, &expected);
    let child = ts.layers.remove(0);
    let f = ts.find_mut(id).unwrap();
    f.blend.get_mut(&C::Roughness).unwrap().mode = BlendMode::Normal;
    let LayerContent::Group(g) = &mut f.content else {
        unreachable!()
    };
    g.children.push(child);
    assert_image(&ts, &fx, &expected);
}
#[test]
fn new_mask_filter_chain_blend_and_raster_halo_cache_match_full_output() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = field();
    let mut mask = Mask::new(N, false);
    for y in 0..N {
        for x in 0..N {
            mask.base
                .set(x, y, [data[(y * N + x) as usize], 0.0, 0.0, 0.0]);
        }
    }
    let mut directional = MaskEffect::new(MaskEffectKind::Filter(Filter::DirectionalBlur {
        radius: 4.0,
        angle_deg: 45.0,
    }));
    directional.opacity = 0.5;
    mask.effects.push(Arc::new(directional));
    mask.effects
        .push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(
            Filter::Highpass {
                radius: 2.0,
                amount: 1.0,
            },
        ))));
    l.mask = Some(mask);
    let id = ts.insert_above(None, l);
    let line = line_reference(&data, 4.0, 45.0);
    let mixed: Vec<_> = data.iter().zip(line).map(|(v, b)| 0.5 * (v + b)).collect();
    let low = reference(&mixed, 2.0);
    let expected: Vec<_> = data
        .iter()
        .zip(mixed.iter().zip(low))
        .map(|(v, (m, b))| {
            let mask = (0.5 + m - b).clamp(0.0, 1.0);
            v * mask + 0.3 * (1.0 - mask)
        })
        .collect();
    assert_image(&ts, &fx, &expected);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    ts.find_mut(id)
        .unwrap()
        .mask
        .as_mut()
        .unwrap()
        .base
        .set(63, 63, [0.5; 4]);
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![0, 1, 2, 3]);
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));
}

#[test]
fn flat_signed_highpass_is_exactly_zero_without_rounding_residue() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("Flat", 0, N);
    let mut l = Layer::new_paint("Uniform", &ts);
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    p.channels.insert(
        C::Height,
        subcraft_image::TiledImage::new(N, N, 2, [0.3, 0.7, 0.0, 0.0]),
    );
    add(
        &mut l,
        &ts,
        C::Height,
        Filter::Highpass {
            radius: 32.0,
            amount: 4.0,
        },
    );
    ts.insert_above(None, l);
    let c = composite_full(&ts, &fx.ctx());
    for y in 0..N {
        for x in 0..N {
            assert_eq!(c.channel(C::Height).unwrap().get(x, y)[0], 0.0);
        }
    }
}
