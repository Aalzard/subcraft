//! Interactive compositing: budgeted updates converge exactly, unchanged results are not
//! reported for upload, and cached procedural fields give the same pixels as fresh ones.

use std::sync::Arc;
use std::time::Duration;

use subcraft_compositor::{CompositeCache, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{
    FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, ProceduralKind,
    ProceduralParams, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 256; // 16 tiles

fn procedural(kind: ProceduralKind, scale: f32) -> FillSource {
    FillSource::Procedural {
        params: ProceduralParams { scale, ..ProceduralParams::new(kind) },
        low: ChannelValue::rgb(0.1, 0.2, 0.3),
        high: ChannelValue::rgb(0.8, 0.6, 0.4),
    }
}

/// Wood-like procedural fill under a moss-like fill masked by a noisy directional generator.
fn stack() -> (TextureSet, subcraft_core::LayerId, subcraft_core::LayerId) {
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut wood = Layer::new_fill("Wood", &tc);
    let LayerContent::Fill(f) = &mut wood.content else { unreachable!() };
    f.channels.insert(ChannelKind::BaseColor, FillChannel::new(procedural(ProceduralKind::WoodRings, 3.0)));
    f.channels.insert(ChannelKind::Roughness, FillChannel::new(procedural(ProceduralKind::Grunge, 8.0)));
    let wood = ts.insert_above(None, wood);
    let mut moss = Layer::new_fill("Moss", &tc);
    let LayerContent::Fill(f) = &mut moss.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = *k == ChannelKind::BaseColor;
    }
    f.channels.insert(ChannelKind::BaseColor, FillChannel::new(procedural(ProceduralKind::Clouds, 6.0)));
    let mut mask = Mask::new(RES, false);
    let mut g = Generator::new(GeneratorKind::Directional);
    g.noise_amount = 0.35;
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    moss.mask = Some(mask);
    let moss = ts.insert_above(None, moss);
    (ts, wood, moss)
}

fn set_level(ts: &mut TextureSet, moss: subcraft_core::LayerId, level: f32) {
    let mask = ts.find_mut(moss).unwrap().mask.as_mut().unwrap();
    let MaskEffectKind::Generator(g) = &mut Arc::make_mut(&mut mask.effects[0]).kind else { unreachable!() };
    g.level = level;
}

#[test]
fn budgeted_updates_converge_exactly_and_report_only_changed_tiles() {
    let geo = TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, RES);
    let (res, maps) = (ResourceStore::default(), MeshMaps::new());
    let ctx = EvalContext { geometry: &geo, resources: &res, mesh_maps: &maps };
    let (mut ts, wood, moss) = stack();
    let mut cache = CompositeCache::new();

    // A zero budget only queues; tiny budgets still progress; the end result is exact.
    assert!(cache.update_within(&ts, &ctx, Some(Duration::ZERO)).is_empty());
    assert!(cache.has_pending());
    let mut calls = 0;
    while cache.has_pending() {
        assert!(!cache.update_within(&ts, &ctx, Some(Duration::from_nanos(1))).is_empty(), "each call makes progress");
        calls += 1;
    }
    assert!(calls >= 1);
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &ctx));

    // Continuous edits while tiles are pending: every tile still converges to the newest state.
    for (i, level) in [0.3, 0.45, 0.6, 0.75].into_iter().enumerate() {
        set_level(&mut ts, moss, level);
        cache.update_within(&ts, &ctx, Some(Duration::from_nanos(1 + i as u64)));
    }
    while cache.has_pending() {
        cache.update_within(&ts, &ctx, Some(Duration::from_micros(50)));
    }
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &ctx));

    // Editing a disabled channel re-evaluates everything but changes no texel: nothing to upload.
    let mut fill = ts.find(moss).map(|l| match &l.content { LayerContent::Fill(f) => f.channels[&ChannelKind::Roughness].clone(), _ => unreachable!() }).unwrap();
    fill.source = FillSource::uniform(ChannelValue::scalar(0.123));
    let LayerContent::Fill(f) = &mut ts.find_mut(moss).unwrap().content else { unreachable!() };
    f.channels.insert(ChannelKind::Roughness, fill);
    assert_eq!(cache.update(&ts, &ctx).len(), 16, "every tile re-evaluated");
    assert!(cache.last_changed().values().all(|t| t.is_empty()), "{:?}", cache.last_changed());

    // A Roughness edit of the wood changes only Roughness tiles.
    let LayerContent::Fill(f) = &mut ts.find_mut(wood).unwrap().content else { unreachable!() };
    f.channels.insert(ChannelKind::Roughness, FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.9))));
    cache.update(&ts, &ctx);
    let changed: Vec<_> = cache.last_changed().keys().copied().collect();
    assert_eq!(changed, vec![ChannelKind::Roughness]);
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &ctx));
}

#[test]
fn cached_procedural_fields_match_fresh_evaluation() {
    let (ts, _, moss) = stack();
    let (res, maps) = (ResourceStore::default(), MeshMaps::new());
    let mesh = Arc::new(primitives::cube(2.0));
    let warm = TextureSetGeometry::new(mesh.clone(), 0, RES);
    let ctx = EvalContext { geometry: &warm, resources: &res, mesh_maps: &maps };
    let first = composite_full(&ts, &ctx);
    // Same geometry: every procedural field now comes from the cache.
    let mut edited = ts.clone();
    set_level(&mut edited, moss, 0.42);
    let cached = composite_full(&edited, &ctx);
    // A brand new geometry has an empty cache: everything evaluated from scratch.
    let cold = TextureSetGeometry::new(mesh, 0, RES);
    let fresh = composite_full(&edited, &EvalContext { geometry: &cold, resources: &res, mesh_maps: &maps });
    assert_eq!(cached, fresh, "bit-identical with and without cached fields");
    assert_ne!(cached, first, "the level change is visible");
}
