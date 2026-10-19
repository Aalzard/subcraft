use std::sync::Arc;
use subcraft_compositor::{CompositeCache, EvalContext, composite_full};
use subcraft_core::ChannelKind as C;
use subcraft_doc::{
    ContentEffect, ContentEffectKind, Filter, Layer, LayerContent, Mask, MaskEffect,
    MaskEffectKind, OutlinePosition as P, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
const N: u32 = 128;
fn fixture() -> (TextureSet, Layer, Vec<f32>) {
    let ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("Shape", &ts);
    let mut data = vec![0.0; (N * N) as usize];
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    for y in 0..N {
        for x in 0..N {
            let v = if (x / 19 + y / 23) % 2 == 0 { 1.0 } else { 0.0 };
            data[(y * N + x) as usize] = v;
            p.channel_mut(C::Roughness, N).set(x, y, [v, 1.0, 0.0, 0.0]);
        }
    }
    (ts, l, data)
}
fn reference(data: &[f32], filter: Filter) -> Vec<f32> {
    // Independent brute-force search of opposite texels; no production distance transform.
    let (r, t) = match filter {
        Filter::Bevel {
            radius, threshold, ..
        }
        | Filter::MaskOutline {
            radius, threshold, ..
        } => (radius, threshold),
        _ => unreachable!(),
    };
    let support = (r + 1.0).ceil() as i32;
    (0..N as i32)
        .flat_map(|y| (0..N as i32).map(move |x| (x, y)))
        .map(|(x, y)| {
            let inside = data[(y * N as i32 + x) as usize] >= t;
            let mut distance = f32::INFINITY;
            for yy in (y - support).max(0)..=(y + support).min(N as i32 - 1) {
                for xx in (x - support).max(0)..=(x + support).min(N as i32 - 1) {
                    if (data[(yy * N as i32 + xx) as usize] >= t) != inside {
                        distance =
                            distance.min((((xx - x).pow(2) + (yy - y).pow(2)) as f32).sqrt());
                    }
                }
            }
            let d = (distance - 0.5).max(0.0);
            match filter {
                Filter::Bevel { amount, .. } => {
                    if inside {
                        amount
                            * if r == 0.0 {
                                1.0
                            } else {
                                (d / r).clamp(0.0, 1.0)
                            }
                    } else {
                        0.0
                    }
                }
                Filter::MaskOutline { position, .. } => {
                    if r == 0.0
                        || matches!((position, inside), (P::Inner, false) | (P::Outer, true))
                    {
                        0.0
                    } else {
                        (r + 0.5 - d).clamp(0.0, 1.0)
                    }
                }
                _ => unreachable!(),
            }
        })
        .collect()
}
#[test]
fn contours_match_dense_goldens_on_content_masks_folders_and_tile_corners() {
    let geo = TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 1)), 0, N);
    let res = ResourceStore::default();
    let maps = MeshMaps::new();
    let ctx = EvalContext {
        geometry: &geo,
        resources: &res,
        mesh_maps: &maps,
    };
    for filter in [
        Filter::Bevel {
            radius: 3.25,
            amount: 0.8,
            threshold: 0.5,
        },
        Filter::Bevel {
            radius: 0.0,
            amount: 0.7,
            threshold: 0.5,
        },
        Filter::MaskOutline {
            radius: 3.25,
            threshold: 0.5,
            position: P::Inner,
        },
        Filter::MaskOutline {
            radius: 3.25,
            threshold: 0.5,
            position: P::Outer,
        },
        Filter::MaskOutline {
            radius: 0.5,
            threshold: 0.5,
            position: P::Both,
        },
        Filter::MaskOutline {
            radius: 0.0,
            threshold: 0.5,
            position: P::Both,
        },
    ] {
        let (ts, l, data) = fixture();
        let expected = reference(&data, filter);
        for domain in 0..3 {
            let mut ts = ts.clone();
            let mut l = l.clone();
            if domain == 0 {
                l.content_effects.push(Arc::new(ContentEffect::new(
                    ContentEffectKind::Filter {
                        channel: C::Roughness,
                        filter,
                    },
                    &ts,
                )));
                ts.insert_above(None, l);
            } else if domain == 1 {
                let mut mask = Mask::new(N, false);
                for y in 0..N {
                    for x in 0..N {
                        mask.base
                            .set(x, y, [data[(y * N + x) as usize], 0.0, 0.0, 0.0]);
                    }
                }
                mask.effects
                    .push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(filter))));
                let LayerContent::Paint(p) = &mut l.content else {
                    unreachable!()
                };
                p.channels.insert(
                    C::Roughness,
                    subcraft_image::TiledImage::new(N, N, 2, [1.0, 1.0, 0.0, 0.0]),
                );
                l.mask = Some(mask);
                ts.insert_above(None, l);
                // Zero background lets the final composite equal mask coverage.
                let mut bg = Layer::new_paint("Background", &ts);
                let LayerContent::Paint(p) = &mut bg.content else {
                    unreachable!()
                };
                p.channels.insert(
                    C::Roughness,
                    subcraft_image::TiledImage::new(N, N, 2, [0.0, 1.0, 0.0, 0.0]),
                );
                ts.layers.insert(0, Arc::new(bg));
            } else {
                let mut folder = Layer::new_folder("Adjustment", &ts);
                folder.content_effects.push(Arc::new(ContentEffect::new(
                    ContentEffectKind::Filter {
                        channel: C::Roughness,
                        filter,
                    },
                    &ts,
                )));
                ts.insert_above(None, l);
                ts.insert_above(None, folder);
            }
            let c = composite_full(&ts, &ctx);
            let image = c.channel(C::Roughness).unwrap();
            for y in 0..N {
                for x in 0..N {
                    let a = image.get(x, y)[0];
                    let b = expected[(y * N + x) as usize];
                    assert!(
                        (a - b).abs() < 0.0006,
                        "{filter:?} domain {domain} {x},{y}: {a} vs {b}"
                    );
                }
            }
        }
    }
}
#[test]
fn bevel_signed_height_and_uniform_contours_preserve_explicit_semantics() {
    let geo = TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 1)), 0, N);
    let res = ResourceStore::default();
    let maps = MeshMaps::new();
    let ctx = EvalContext {
        geometry: &geo,
        resources: &res,
        mesh_maps: &maps,
    };
    for (v, filter, expected) in [
        (
            1.0,
            Filter::Bevel {
                radius: 32.0,
                amount: -0.25,
                threshold: 0.5,
            },
            -0.25,
        ),
        (
            0.0,
            Filter::Bevel {
                radius: 32.0,
                amount: 1.0,
                threshold: 0.5,
            },
            0.0,
        ),
        (
            1.0,
            Filter::MaskOutline {
                radius: 32.0,
                threshold: 0.5,
                position: P::Both,
            },
            0.0,
        ),
    ] {
        let mut ts = TextureSet::new("T", 0, N);
        let mut l = Layer::new_paint("Height", &ts);
        let LayerContent::Paint(p) = &mut l.content else {
            unreachable!()
        };
        p.channels.insert(
            C::Height,
            subcraft_image::TiledImage::new(N, N, 2, [v, 1.0, 0.0, 0.0]),
        );
        l.content_effects.push(Arc::new(ContentEffect::new(
            ContentEffectKind::Filter {
                channel: C::Height,
                filter,
            },
            &ts,
        )));
        ts.insert_above(None, l);
        let c = composite_full(&ts, &ctx);
        for xy in [(0, 0), (63, 63), (64, 64), (127, 127)] {
            assert_eq!(c.channel(C::Height).unwrap().get(xy.0, xy.1)[0], expected);
        }
        assert_eq!(
            c.channel(C::Normal).unwrap().get(64, 64),
            [0.5, 0.5, 1.0, 1.0]
        );
    }
}
#[test]
fn contour_dirty_halo_chains_and_effect_opacity_match_full_evaluation() {
    let geo = TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 1)), 0, 256);
    let res = ResourceStore::default();
    let maps = MeshMaps::new();
    let ctx = EvalContext {
        geometry: &geo,
        resources: &res,
        mesh_maps: &maps,
    };
    let mut ts = TextureSet::new("T", 0, 256);
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
        l.content_effects.push(Arc::new(ContentEffect::new(
            ContentEffectKind::Filter {
                channel: C::Roughness,
                filter: Filter::MaskOutline {
                    radius: 32.0,
                    threshold: 0.5,
                    position: P::Both,
                },
            },
            &ts,
        )));
    }
    Arc::make_mut(&mut l.content_effects[2])
        .blend
        .get_mut(&C::Roughness)
        .unwrap()
        .opacity = 0.75;
    let id = ts.insert_above(None, l);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &ctx);
    let LayerContent::Paint(p) = &mut ts.find_mut(id).unwrap().content else {
        unreachable!()
    };
    p.channel_mut(C::Roughness, 256)
        .set(63, 63, [1.0, 1.0, 0.0, 0.0]);
    cache.update(&ts, &ctx);
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
fn contour_content_keeps_alpha_and_blends_effect_opacity_numerically() {
    use subcraft_compositor::{TileContext, tile::preview_content};
    use subcraft_materials::SampleContext;
    let (ts, mut l, _) = fixture();
    let LayerContent::Paint(p) = &mut l.content else {
        unreachable!()
    };
    let image = p.channel_mut(C::BaseColor, N);
    for y in 0..N {
        for x in 0..N {
            image.set(
                x,
                y,
                if x < 64 {
                    [1.0, 1.0, 1.0, 0.5]
                } else {
                    [0.0, 0.0, 0.0, 0.5]
                },
            );
        }
    }
    let mut e = ContentEffect::new(
        ContentEffectKind::Filter {
            channel: C::BaseColor,
            filter: Filter::MaskOutline {
                radius: 8.0,
                threshold: 0.25,
                position: P::Outer,
            },
        },
        &ts,
    );
    e.blend.get_mut(&C::BaseColor).unwrap().opacity = 0.75;
    l.content_effects.push(Arc::new(e));
    let geo = TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 1)), 0, N);
    let tile = geo.tile(3);
    let res = ResourceStore::default();
    let maps = MeshMaps::new();
    let tc = TileContext {
        geo: &tile,
        set_geometry: &geo,
        albedo_filter: ts.albedo_filter,
        sample: SampleContext {
            resources: &res,
            mesh_maps: &maps,
        },
    };
    let p = preview_content(&l, C::BaseColor, &tc)[0];
    assert_eq!(p, [0.75, 0.75, 0.75, 0.5]);
}
