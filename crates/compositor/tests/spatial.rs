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
fn assert_image(ts: &TextureSet, fx: &Fixture, expected: &[f32]) { assert_image_tol(ts, fx, expected, 0.0006) }
/// Neighbourhood stages store f16 tiles, so chains of stages round a little more than one.
fn assert_image_tol(ts: &TextureSet, fx: &Fixture, expected: &[f32], tol: f32) {
    let c = composite_full(ts, &fx.ctx());
    let img = c.channel(C::Roughness).unwrap();
    for y in 0..N {
        for x in 0..N {
            let actual = img.get(x, y)[0];
            let wanted = expected[(y * N + x) as usize];
            assert!(
                (actual - wanted).abs() < tol,
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

fn noisy_field() -> (TextureSet, Layer, Vec<f32>) {
    let ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("P", &ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let image = p.channel_mut(C::Roughness, N);
    let mut data = Vec::new();
    let mut s = 12345u32;
    for y in 0..N {
        for x in 0..N {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            // Smooth ramp plus speckle with ties.
            let v = (x as f32 / N as f32) * 0.5 + ((s >> 24) % 5) as f32 * 0.1;
            image.set(x, y, [v, 1.0, 0.0, 0.0]);
            data.push(v);
        }
    }
    (ts, l, data)
}
fn median_reference(input: &[f32], r: i32) -> Vec<f32> {
    let n = N as i32;
    (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).map(|(x, y)| {
        let mut w: Vec<f32> = (-r..=r).flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
            .map(|(dx, dy)| input[((y + dy).clamp(0, n - 1) * n + (x + dx).clamp(0, n - 1)) as usize]).collect();
        w.sort_by(f32::total_cmp);
        w[w.len() / 2]
    }).collect()
}
#[test]
fn median_matches_dense_reference_across_tile_borders_texture_edges_and_chained_stages() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = noisy_field();
    add(&mut l, &ts, C::Roughness, Filter::Median { radius: 3.0 });
    let id = ts.insert_above(None, l);
    let m3 = median_reference(&data, 3);
    assert_image(&ts, &fx, &m3);
    // Fractional radii round to whole pixels; zero is the identity.
    let set = |ts: &mut TextureSet, radius: f32| {
        let e = Arc::make_mut(&mut ts.find_mut(id).unwrap().content_effects[0]);
        e.kind = ContentEffectKind::Filter { channel: C::Roughness, filter: Filter::Median { radius } };
    };
    set(&mut ts, 1.6);
    assert_image(&ts, &fx, &median_reference(&data, 2));
    set(&mut ts, 0.0);
    assert_image(&ts, &fx, &data);
    // Two median stages read the previous stage (support adds up across tile borders).
    let shape = ts.clone();
    ts.find_mut(id).unwrap().content_effects = vec![
        Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter: Filter::Median { radius: 2.0 } }, &shape)),
        Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter: Filter::Median { radius: 3.0 } }, &shape)),
    ];
    assert_image(&ts, &fx, &median_reference(&median_reference(&data, 2), 3));
}
#[test]
fn median_works_as_a_mask_effect_and_keeps_paint_coverage() {
    let fx = Fixture::new();
    let (mut ts, mut l, _) = field();
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    p.channel_mut(C::BaseColor, N).set(63, 63, [0.8, 0.2, 0.1, 0.5]);
    add(&mut l, &ts, C::BaseColor, Filter::Median { radius: 2.0 });
    let id = ts.insert_above(None, l);
    let tc = ts.clone();
    let geo = fx.geo.tile(0);
    let ctx = TileContext { albedo_filter: tc.albedo_filter, geo: &geo, set_geometry: &fx.geo, sample: SampleContext { resources: &fx.res, mesh_maps: &fx.maps } };
    let px = preview_content(ts.find(id).unwrap(), C::BaseColor, &ctx);
    assert_eq!(px[63 * 64 + 63][3], 0.5, "coverage is untouched");
    assert_eq!(px[0][3], 0.0);
    // A speckled mask: the median removes isolated white pixels.
    let mut layer = Layer::new_fill("M", &ts);
    let mut mask = Mask::new(N, false);
    mask.base.set(30, 30, [1.0; 4]);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(Filter::Median { radius: 1.0 }))));
    layer.mask = Some(mask);
    let mut ts2 = TextureSet::new("T2", 0, N);
    ts2.insert_above(None, layer);
    let c = composite_full(&ts2, &fx.ctx());
    let r = c.channel(C::Roughness).unwrap();
    assert_eq!(r.get(30, 30)[0], r.get(5, 5)[0], "the isolated mask pixel is gone");
}

fn warp_reference(input: &[f32], intensity: f32, scale: u32, seed: u32) -> Vec<f32> {
    let n = N as i32;
    let at = |x: i32, y: i32| input[(y.clamp(0, n - 1) * n + x.clamp(0, n - 1)) as usize];
    (0..N).flat_map(|y| (0..N).map(move |x| (x, y))).map(|(x, y)| {
        let [dx, dy] = subcraft_materials::filter::warp::offset(x, y, N, intensity, scale, seed);
        let (sx, sy) = ((x as f32 + dx).clamp(0.0, (N - 1) as f32), (y as f32 + dy).clamp(0.0, (N - 1) as f32));
        let (x0, y0) = (sx.floor() as i32, sy.floor() as i32);
        let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
        let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }).collect()
}
#[test]
fn warp_matches_dense_bilinear_reference_across_tiles_and_is_deterministic() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = noisy_field();
    add(&mut l, &ts, C::Roughness, Filter::Warp { intensity: 6.0, scale: 3, seed: 5 });
    let id = ts.insert_above(None, l);
    let expected = warp_reference(&data, 6.0, 3, 5);
    assert_image(&ts, &fx, &expected);
    // The result moved real texels (otherwise the comparison proves nothing).
    assert!(expected.iter().zip(&data).filter(|(a, b)| (**a - **b).abs() > 0.02).count() > 2000);
    // Same settings, same result; another seed, another result; intensity 0 is the identity.
    let a = composite_full(&ts, &fx.ctx());
    let b = composite_full(&ts, &fx.ctx());
    assert_eq!(a.channel(C::Roughness).unwrap().get(77, 31), b.channel(C::Roughness).unwrap().get(77, 31));
    let set = |ts: &mut TextureSet, filter: Filter| {
        Arc::make_mut(&mut ts.find_mut(id).unwrap().content_effects[0]).kind = ContentEffectKind::Filter { channel: C::Roughness, filter };
    };
    set(&mut ts, Filter::Warp { intensity: 6.0, scale: 3, seed: 6 });
    assert_image(&ts, &fx, &warp_reference(&data, 6.0, 3, 6));
    set(&mut ts, Filter::Warp { intensity: 0.0, scale: 3, seed: 5 });
    assert_image(&ts, &fx, &data);
}
fn slope_reference(input: &[f32], intensity: f32, scale: u32, seed: u32, samples: u32) -> Vec<f32> {
    let n = N as i32;
    let at = |x: i32, y: i32| input[(y.clamp(0, n - 1) * n + x.clamp(0, n - 1)) as usize];
    (0..N).flat_map(|y| (0..N).map(move |x| (x, y))).map(|(x, y)| {
        let [dx, dy] = subcraft_materials::filter::warp::offset(x, y, N, intensity, scale, seed);
        (0..samples).map(|k| {
            let t = k as f32 / (samples - 1) as f32;
            let (sx, sy) = ((x as f32 + dx * t).clamp(0.0, (N - 1) as f32), (y as f32 + dy * t).clamp(0.0, (N - 1) as f32));
            let (x0, y0) = (sx.floor() as i32, sy.floor() as i32);
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
            let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
            top * (1.0 - fy) + bottom * fy
        }).sum::<f32>() / samples as f32
    }).collect()
}
#[test]
fn blur_slope_averages_along_the_noise_offset() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = noisy_field();
    add(&mut l, &ts, C::Roughness, Filter::BlurSlope { intensity: 6.0, scale: 3, seed: 5, samples: 6 });
    let id = ts.insert_above(None, l);
    let expected = slope_reference(&data, 6.0, 3, 5, 6);
    assert_image(&ts, &fx, &expected);
    assert!(expected.iter().zip(&data).filter(|(a, b)| (**a - **b).abs() > 0.02).count() > 2000, "the blur changed real texels");
    // It smooths: along the slope the result varies less than the Warp of the same field.
    let rough = |v: &[f32]| v.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
    assert!(rough(&expected) < rough(&warp_reference(&data, 6.0, 3, 5)));
    let set = |ts: &mut TextureSet, filter: Filter| {
        Arc::make_mut(&mut ts.find_mut(id).unwrap().content_effects[0]).kind = ContentEffectKind::Filter { channel: C::Roughness, filter };
    };
    // More reads, another seed: each matches its own reference; no intensity is the identity.
    set(&mut ts, Filter::BlurSlope { intensity: 6.0, scale: 3, seed: 6, samples: 16 });
    assert_image(&ts, &fx, &slope_reference(&data, 6.0, 3, 6, 16));
    set(&mut ts, Filter::BlurSlope { intensity: 0.0, scale: 3, seed: 5, samples: 6 });
    assert_image(&ts, &fx, &data);
}
#[test]
fn warp_moves_paint_coverage_with_the_pixels() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("P", &ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    for y in 0..N { for x in 0..N / 4 { p.channel_mut(C::BaseColor, N).set(x, y, [0.8, 0.2, 0.1, 1.0]); } }
    add(&mut l, &ts, C::BaseColor, Filter::Warp { intensity: 10.0, scale: 2, seed: 1 });
    let id = ts.insert_above(None, l);
    let geo = fx.geo.tile(0);
    let ctx = TileContext { albedo_filter: ts.albedo_filter, geo: &geo, set_geometry: &fx.geo, sample: SampleContext { resources: &fx.res, mesh_maps: &fx.maps } };
    let px = preview_content(ts.find(id).unwrap(), C::BaseColor, &ctx);
    // Colour stays the paint colour wherever there is coverage; the straight edge becomes ragged.
    let mut partial = 0;
    for p in &px { if p[3] > 0.01 { assert!((p[0] - 0.8).abs() < 1e-3 && (p[1] - 0.2).abs() < 1e-3, "{p:?}"); } if p[3] > 0.01 && p[3] < 0.99 { partial += 1; } }
    assert!(partial > 0, "the warped edge has partial coverage");
}

// ---- Grayscale / Invert / Posterize / Histogram Shift / Edge Detect / Glow / Mosaic / Directional
// Warp / Normal Invert (independent dense references; tiles are 64 px so 128 px crosses borders).
fn roughness_stack(filters: &[Filter]) -> (TextureSet, Vec<f32>) {
    let (mut ts, mut l, data) = noisy_field();
    for f in filters { add(&mut l, &ts, C::Roughness, *f); }
    ts.insert_above(None, l);
    (ts, data)
}
#[test]
fn invert_posterize_and_shift_chain_matches_hand_math() {
    let fx = Fixture::new();
    let (ts, data) = roughness_stack(&[Filter::Invert, Filter::Posterize { levels: 3 }, Filter::HistogramShift { shift: 0.25 }]);
    let expected: Vec<f32> = data.iter().map(|v| {
        let inv = 1.0 - v.clamp(0.0, 1.0); // paint stores the raw value, composite clamps 0..1
        ((inv * 2.0).round() / 2.0 + 0.25).clamp(0.0, 1.0)
    }).collect();
    assert_image(&ts, &fx, &expected);
}
#[test]
fn edge_detect_matches_dense_sobel_reference_across_tiles() {
    let fx = Fixture::new();
    for (radius, strength) in [(1.0f32, 1.5f32), (3.0, 0.7)] {
        let (ts, data) = roughness_stack(&[Filter::EdgeDetect { radius, strength }]);
        let n = N as i32;
        let d = radius.round().max(1.0) as i32;
        let at = |x: i32, y: i32| data[(y.clamp(0, n - 1) * n + x.clamp(0, n - 1)) as usize];
        let expected: Vec<f32> = (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).map(|(x, y)| {
            let p = |i: i32, j: i32| at(x + i * d, y + j * d);
            let gx = p(1, -1) + 2.0 * p(1, 0) + p(1, 1) - p(-1, -1) - 2.0 * p(-1, 0) - p(-1, 1);
            let gy = p(-1, 1) + 2.0 * p(0, 1) + p(1, 1) - p(-1, -1) - 2.0 * p(0, -1) - p(1, -1);
            ((gx * gx + gy * gy).sqrt() / 4.0 * strength).clamp(0.0, 1.0)
        }).collect();
        assert!(expected.iter().filter(|v| **v > 0.1).count() > 1000, "the field must contain edges");
        assert_image_tol(&ts, &fx, &expected, 0.002);
    }
}
#[test]
fn glow_adds_blurred_highlights_like_a_dense_reference() {
    let fx = Fixture::new();
    let (ts, data) = roughness_stack(&[Filter::Glow { radius: 3.0, amount: 1.5, threshold: 0.4 }]);
    let blurred = reference(&data, 3.0);
    let expected: Vec<f32> = data.iter().zip(&blurred).map(|(s, b)| (s + 1.5 * (b - 0.4).max(0.0) / 0.6).clamp(0.0, 1.0)).collect();
    assert!(expected.iter().zip(&data).filter(|(a, b)| (**a - **b).abs() > 0.05).count() > 1000);
    assert_image_tol(&ts, &fx, &expected, 0.002);
}
#[test]
fn mosaic_takes_the_block_centre_pixel() {
    let fx = Fixture::new();
    let (ts, data) = roughness_stack(&[Filter::Mosaic { size: 10 }]);
    let n = N as usize;
    let expected: Vec<f32> = (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).map(|(x, y)| {
        let at = |v: usize| (v / 10 * 10 + 5).min(n - 1);
        data[at(y) * n + at(x)]
    }).collect();
    assert_image(&ts, &fx, &expected);
    // Block of 10 does not divide 128: the last partial block (120..128) still shares one value.
    let c = composite_full(&ts, &fx.ctx());
    let img = c.channel(C::Roughness).unwrap();
    assert_eq!(img.get(121, 3)[0], img.get(127, 3)[0]);
}
#[test]
fn directional_warp_matches_dense_bilinear_reference() {
    let fx = Fixture::new();
    let (ts, data) = roughness_stack(&[Filter::DirectionalWarp { intensity: 6.0, angle_deg: 30.0 }]);
    let n = N as i32;
    let at = |x: i32, y: i32| data[(y.clamp(0, n - 1) * n + x.clamp(0, n - 1)) as usize];
    let expected: Vec<f32> = (0..n).flat_map(|y| (0..n).map(move |x| (x, y))).map(|(x, y)| {
        let k = (2.0 * at(x, y).clamp(0.0, 1.0) - 1.0) * 6.0;
        let a = 30.0f32.to_radians();
        let (sx, sy) = ((x as f32 + a.cos() * k).clamp(0.0, (n - 1) as f32), (y as f32 + a.sin() * k).clamp(0.0, (n - 1) as f32));
        let (x0, y0) = (sx.floor() as i32, sy.floor() as i32);
        let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
        let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }).collect();
    assert!(expected.iter().zip(&data).filter(|(a, b)| (**a - **b).abs() > 0.02).count() > 2000);
    assert_image_tol(&ts, &fx, &expected, 0.002);
}
#[test]
fn grayscale_and_normal_invert_apply_to_their_channels_only() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("P", &ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    for y in 0..N { for x in 0..N {
        p.channel_mut(C::BaseColor, N).set(x, y, [0.8, 0.2, 0.1, 1.0]);
        p.channel_mut(C::Normal, N).set(x, y, [0.7, 0.2, 0.9, 1.0]);
    } }
    add(&mut l, &ts, C::BaseColor, Filter::Grayscale);
    add(&mut l, &ts, C::Normal, Filter::NormalInvert { flip_x: true, flip_y: false, flip_z: true });
    ts.insert_above(None, l);
    let c = composite_full(&ts, &fx.ctx());
    let lum = 0.8 * 0.2126 + 0.2 * 0.7152 + 0.1 * 0.0722;
    let b = c.channel(C::BaseColor).unwrap().get(70, 70);
    assert!(b[..3].iter().all(|v| (v - lum).abs() < 2e-3), "{b:?} vs {lum}");
    let n = c.channel(C::Normal).unwrap().get(70, 70);
    // Flipped X and Z of (0.4, -0.6, 0.8) is (-0.4, -0.6, -0.8); the composite renormalises normals.
    let len = (0.4f32 * 0.4 + 0.6 * 0.6 + 0.8 * 0.8).sqrt();
    let want = [-0.4 / len * 0.5 + 0.5, -0.6 / len * 0.5 + 0.5, -0.8 / len * 0.5 + 0.5];
    assert!(n[..3].iter().zip(want).all(|(a, b)| (a - b).abs() < 2e-3), "{n:?} vs {want:?}");
}

// ---- Mask generators: position gradient axes and concave curvature.
#[test]
fn position_gradient_runs_along_the_chosen_axis_and_concave_inverts_curvature() {
    use subcraft_doc::{Generator, GeneratorKind, GradientAxis};
    let fx = Fixture::new();
    let run = |g: Generator, curvature: Option<f32>| {
        let mut maps = MeshMaps::new();
        if let Some(c) = curvature { maps.insert(subcraft_core::MeshMapKind::Curvature, subcraft_image::TiledImage::new(N, N, 1, [c; 4])); }
        let mut ts = TextureSet::new("T", 0, N);
        let mut l = Layer::new_paint("P", &ts);
        let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
        for y in 0..N { for x in 0..N { p.channel_mut(C::Roughness, N).set(x, y, [1.0, 1.0, 0.0, 0.0]); } }
        let mut mask = Mask::new(N, false);
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
        l.mask = Some(mask);
        ts.insert_above(None, l);
        let ctx = EvalContext { geometry: &fx.geo, resources: &fx.res, mesh_maps: &maps };
        composite_full(&ts, &ctx).channel(C::Roughness).unwrap().to_dense()
    };
    // The plane lies in XZ (Y is constant), so a Y gradient is flat while X and Z vary.
    let mut g = Generator::new(GeneratorKind::PositionGradient);
    g.noise_amount = 0.0; g.softness = 1.0; g.level = 0.5;
    let spread = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min);
    let y = run(g, None);
    g.axis = GradientAxis::X; let x = run(g, None);
    g.axis = GradientAxis::Z; let z = run(g, None);
    assert!(spread(&y) < 1e-3, "Y gradient on a flat plane is constant: {}", spread(&y));
    assert!(spread(&x) > 0.3 && spread(&z) > 0.3, "X and Z gradients span the plane: {} {}", spread(&x), spread(&z));
    assert_ne!(x, z, "axes are independent");
    // Concave: curvature 0.2 (concave side) gives more coverage than 0.8 (convex).
    let mut c = Generator::new(GeneratorKind::Concave);
    c.noise_amount = 0.0; c.softness = 0.2; c.level = 0.5;
    let (concave, convex) = (run(c, Some(0.2)), run(c, Some(0.8)));
    // Roughness below the mask is the 0.3 default: coverage 1 gives 1.0, coverage 0 leaves 0.3.
    assert!(concave.iter().all(|v| *v > 0.99) && convex.iter().all(|v| (*v - 0.3).abs() < 0.01), "{} {}", concave[N as usize * 5 + 5], convex[N as usize * 5 + 5]);
}

// ---- Standard filter gaps: Blur wrap, Transform, Mirror ----

/// [`reference`] with taps that continue from the opposite texture border.
fn wrapped_reference(input: &[f32], radius: f32) -> Vec<f32> {
    let r = radius.ceil() as i32;
    let sigma = radius / 3.0;
    let n = N as i32;
    let mut out = vec![0.0; input.len()];
    for y in 0..n {
        for x in 0..n {
            let (mut sum, mut norm) = (0.0, 0.0);
            for dy in -r..=r {
                for dx in -r..=r {
                    let w = (-((dx * dx + dy * dy) as f32) / (2.0 * sigma * sigma)).exp();
                    sum += input[((y + dy).rem_euclid(n) * n + (x + dx).rem_euclid(n)) as usize] * w;
                    norm += w;
                }
            }
            out[(y * n + x) as usize] = sum / norm;
        }
    }
    out
}

#[test]
fn wrapped_blur_continues_across_the_texture_borders() {
    let fx = Fixture::new();
    let (mut ts, mut l, data) = field();
    add(&mut l, &ts, C::Roughness, Filter::blur(5.0, true));
    ts.insert_above(None, l);
    let wrapped = wrapped_reference(&data, 5.0);
    assert_image(&ts, &fx, &wrapped);
    // It is the clamping blur away from the borders and differs from it next to them.
    let clamped = reference(&data, 5.0);
    let at = |x: u32, y: u32| (y * N + x) as usize;
    assert!((wrapped[at(64, 64)] - clamped[at(64, 64)]).abs() < 1e-6);
    let differing = (0..N).filter(|&y| (wrapped[at(0, y)] - clamped[at(0, y)]).abs() > 0.02).count();
    assert!(differing > 20, "{differing} border texels differ");
}

fn transform(repeat: subcraft_doc::RepeatMode, flip: subcraft_doc::FlipMode, scale: [f32; 2], offset: [f32; 2], rotation_deg: f32) -> Filter {
    Filter::Transform { repeat, flip, scale, offset, rotation_deg, tile_safe: false }
}

#[test]
fn transform_filter_moves_rotates_flips_and_scales_the_image_in_uv_space() {
    use subcraft_doc::{FlipMode, RepeatMode};
    let fx = Fixture::new();
    let n = N as usize;
    let run = |filter: Filter, expected: &dyn Fn(usize, usize) -> f32| {
        let (mut ts, mut l, _) = field();
        add(&mut l, &ts, C::Roughness, filter);
        ts.insert_above(None, l);
        let expected: Vec<f32> = (0..n * n).map(|i| expected(i % n, i / n)).collect();
        assert_image(&ts, &fx, &expected);
    };
    let (_, _, data) = field();
    let at = |x: usize, y: usize| data[y * n + x];
    // The identity changes nothing.
    run(transform(RepeatMode::Both, FlipMode::None, [1.0; 2], [0.0; 2], 0.0), &|x, y| at(x, y));
    // Offset a quarter of the texture right and an eighth up, wrapping.
    run(transform(RepeatMode::Both, FlipMode::None, [1.0; 2], [0.25, -0.125], 0.0), &|x, y| at((x + n - 32) % n, (y + 16) % n));
    // A quarter turn counter-clockwise: the right edge of the image becomes the top edge.
    run(transform(RepeatMode::Both, FlipMode::None, [1.0; 2], [0.0; 2], 90.0), &|x, y| at(n - 1 - y, x));
    // Flips mirror the image.
    run(transform(RepeatMode::Both, FlipMode::Horizontal, [1.0; 2], [0.0; 2], 0.0), &|x, y| at(n - 1 - x, y));
    run(transform(RepeatMode::Both, FlipMode::Both, [1.0; 2], [0.0; 2], 0.0), &|x, y| at(n - 1 - x, n - 1 - y));
    // Half size without repetition: the image fills the centre (each texel the mean of 2x2
    // source texels), and the layer is empty around it, so the 0.3 default shows.
    let half = |x: usize, y: usize| -> Option<f32> {
        ((n / 4..3 * n / 4).contains(&x) && (n / 4..3 * n / 4).contains(&y)).then(|| {
            let (sx, sy) = (2 * x - n / 2, 2 * y - n / 2);
            (at(sx, sy) + at(sx + 1, sy) + at(sx, sy + 1) + at(sx + 1, sy + 1)) * 0.25
        })
    };
    run(transform(RepeatMode::None, FlipMode::None, [0.5; 2], [0.0; 2], 0.0), &|x, y| half(x, y).unwrap_or(0.3));
    // Half size repeating: four copies.
    run(transform(RepeatMode::Both, FlipMode::None, [0.5; 2], [0.0; 2], 0.0), &|x, y| half((x + n / 4) % (n / 2) + n / 4, (y + n / 4) % (n / 2) + n / 4).unwrap());
    // Repeating along U only: the row of copies, empty above and below.
    run(transform(RepeatMode::Horizontal, FlipMode::None, [0.5; 2], [0.0; 2], 0.0), &|x, y| half((x + n / 4) % (n / 2) + n / 4, y).unwrap_or(0.3));
    // Tile-safe rotation snaps 80 degrees to the quarter turn.
    run(Filter::Transform { repeat: RepeatMode::Both, flip: FlipMode::None, scale: [1.0; 2], offset: [0.0; 2], rotation_deg: 80.0, tile_safe: true }, &|x, y| at(n - 1 - y, x));
}

#[test]
fn mirror_filter_copies_the_kept_side_onto_the_other_in_content_and_masks() {
    use subcraft_doc::{MirrorCorner, MirrorMode};
    let fx = Fixture::new();
    let n = N as usize;
    let (_, _, data) = field();
    let at = |x: usize, y: usize| data[y * n + x];
    let mirror = |mode, invert_x, invert_y, corner| Filter::Mirror { mode, offset: [0.5; 2], invert_x, invert_y, corner };
    let run = |filter: Filter, expected: &dyn Fn(usize, usize) -> f32| {
        let (mut ts, mut l, _) = field();
        add(&mut l, &ts, C::Roughness, filter);
        ts.insert_above(None, l);
        let expected: Vec<f32> = (0..n * n).map(|i| expected(i % n, i / n)).collect();
        assert_image(&ts, &fx, &expected);
    };
    let near = |v: usize| v.min(n - 1 - v);
    let far = |v: usize| v.max(n - 1 - v);
    run(mirror(MirrorMode::AxisX, false, false, MirrorCorner::TopLeft), &|x, y| at(near(x), y));
    run(mirror(MirrorMode::AxisX, true, false, MirrorCorner::TopLeft), &|x, y| at(far(x), y));
    run(mirror(MirrorMode::AxisY, false, false, MirrorCorner::TopLeft), &|x, y| at(x, near(y)));
    run(mirror(MirrorMode::Corner, false, false, MirrorCorner::TopLeft), &|x, y| at(near(x), near(y)));
    run(mirror(MirrorMode::Corner, false, false, MirrorCorner::BottomRight), &|x, y| at(far(x), far(y)));
    run(mirror(MirrorMode::Corner, true, false, MirrorCorner::BottomRight), &|x, y| at(near(x), far(y)));
    // Axis a quarter in: the first quarter is reflected once, then clamps to the left border.
    run(Filter::Mirror { mode: MirrorMode::AxisX, offset: [0.25, 0.5], invert_x: false, invert_y: false, corner: MirrorCorner::TopLeft },
        &|x, y| at(if x < n / 4 { x } else { (n / 2 - 1).saturating_sub(x) }, y));
    // On a mask: the mirrored mask reveals the layer symmetrically.
    let (mut ts, mut l, _) = field();
    let mut mask = Mask::new(N, false);
    for y in 0..N {
        for x in 0..N {
            mask.base.set(x, y, [if x < 20 && y < 90 { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0]);
        }
    }
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(mirror(MirrorMode::AxisX, false, false, MirrorCorner::TopLeft)))));
    l.mask = Some(mask);
    ts.insert_above(None, l);
    let expected: Vec<f32> = (0..n * n).map(|i| {
        let (x, y) = (i % n, i / n);
        if near(x) < 20 && y < 90 { at(x, y) } else { 0.3 }
    }).collect();
    assert_image(&ts, &fx, &expected);
}

/// QA 2026-11: a Transform that leaves gaps (no repeat, scaled down) over a flat layer was marked
/// "uniform", so it was evaluated for one tile and that tile's result reused for every tile: the
/// shrunken image and its empty border landed in the wrong places.
#[test]
fn a_gap_leaving_transform_of_a_flat_layer_is_evaluated_per_tile() {
    use subcraft_core::ChannelValue;
    use subcraft_doc::{FillChannel, FillSource, FlipMode, RepeatMode};
    let fx = Fixture::new();
    let ts = TextureSet::new("T", 0, N);
    let mut fill = Layer::new_fill("Flat", &ts);
    let LayerContent::Fill(f) = &mut fill.content else { unreachable!() };
    f.channels.insert(C::Roughness, FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.8))));
    add(&mut fill, &ts, C::Roughness, transform(RepeatMode::None, FlipMode::None, [0.5; 2], [0.0; 2], 0.0));
    let mut ts = ts;
    ts.insert_above(None, fill);
    let c = composite_full(&ts, &fx.ctx());
    let img = c.channel(C::Roughness).unwrap();
    let flat = img.get(0, 0)[0];
    // Inside the centre half: the (flat) image. Well outside it: the empty border, whatever
    // tile it is in.
    for (x, y) in [(64, 64), (70, 70), (40, 40), (90, 40), (40, 90)] {
        assert!((img.get(x, y)[0] - 0.8).abs() < 0.01, "({x},{y}) = {} should be the image", img.get(x, y)[0]);
    }
    for (x, y) in [(5, 5), (120, 120), (64, 120), (120, 64), (5, 64)] {
        assert!((img.get(x, y)[0] - flat).abs() < 1e-4 && (flat - 0.8).abs() > 0.1, "({x},{y}) = {} should be the empty border ({flat})", img.get(x, y)[0]);
    }
}

#[test]
fn quantize_uses_one_palette_from_the_whole_texture_in_every_tile() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("P", &ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let image = p.channel_mut(C::Roughness, N);
    for y in 0..N {
        for x in 0..N {
            image.set(x, y, [x as f32 / (N - 1) as f32, 1.0, 0.0, 0.0]);
        }
    }
    let quantize = |colors: u32, dithering: f32| Filter::Quantize {
        colors,
        contour_smoothing: 0.0,
        dithering,
        pattern: subcraft_doc::DitherPattern::Bayer4,
        lab: true,
        apply_to_alpha: false,
        alpha_threshold: 0.5,
    };
    add(&mut l, &ts, C::Roughness, quantize(4, 0.0));
    let id = ts.insert_above(None, l);
    // A ramp in 4 colours: the 4 quarter means, the same in all 2 x 2 tiles (the palette
    // is picked from the whole texture, not per tile). Lab is ignored on a data channel.
    let levels = [15.5f32, 47.5, 79.5, 111.5].map(|v| v / (N - 1) as f32);
    let expected: Vec<f32> = (0..N * N).map(|i| levels[((i % N) / 32) as usize]).collect();
    assert_image_tol(&ts, &fx, &expected, 0.003);
    // Two colours with full dithering: each 4 x 4 Bayer block averages close to its input.
    let tc = ts.clone();
    ts.find_mut(id).unwrap().content_effects.clear();
    add(ts.find_mut(id).unwrap(), &tc, C::Roughness, quantize(2, 1.0));
    let c = composite_full(&ts, &fx.ctx());
    let img = c.channel(C::Roughness).unwrap();
    let mut values = std::collections::BTreeSet::new();
    for by in (0..N).step_by(4) {
        for bx in (0..N).step_by(4) {
            let mut sum = 0.0;
            for y in by..by + 4 {
                for x in bx..bx + 4 {
                    let v = img.get(x, y)[0];
                    values.insert((v * 1000.0).round() as i32);
                    sum += v;
                }
            }
            // Inside the palette's span (about 0.25 .. 0.75) the mix follows the input.
            let input = (bx as f32 + 1.5) / (N - 1) as f32;
            if (0.3..=0.7).contains(&input) {
                assert!((sum / 16.0 - input).abs() < 0.05, "block {bx},{by}: {} vs {input}", sum / 16.0);
            }
        }
    }
    assert_eq!(values.len(), 2, "two colours: {values:?}");
}

#[test]
fn anisotropic_kuwahara_reaches_the_composite() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("P", &ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let image = p.channel_mut(C::Roughness, N);
    for y in 0..N {
        for x in 0..N {
            image.set(x, y, [if (x + y) % 2 == 0 { 0.55 } else { 0.45 }, 1.0, 0.0, 0.0]);
        }
    }
    let kuwahara = Filter::AnisotropicKuwahara { radius: 4.0, smoothness: 0.5, sharpness: 8.0, tensor_smoothness: 2.0, anisotropy: 1.0, angle: 0.0, extract_direction: true };
    add(&mut l, &ts, C::Roughness, kuwahara);
    ts.insert_above(None, l);
    // The checker noise is smoothed towards 0.5, also across the tile border at x = 64.
    let c = composite_full(&ts, &fx.ctx());
    let img = c.channel(C::Roughness).unwrap();
    for (x, y) in [(20, 20), (63, 30), (64, 31)] {
        let v = img.get(x, y)[0];
        assert!((v - 0.5).abs() < 0.03, "at {x},{y}: {v}");
    }
}
