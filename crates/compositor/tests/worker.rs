//! Background compositing: the worker converges exactly to a full composite, the newest
//! snapshot wins, reported tiles are exactly what an uploaded copy needs, and a full
//! re-evaluation request survives being replaced by a newer snapshot.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use subcraft_compositor::{Composite, CompositeWorker, EvalContext, Published, SetSnapshot, Snapshot, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, LayerId, TextureSetId};
use subcraft_doc::{
    FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, ProceduralKind,
    ProceduralParams, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

fn procedural(kind: ProceduralKind, scale: f32) -> FillSource {
    FillSource::Procedural {
        params: ProceduralParams { scale, ..ProceduralParams::new(kind) },
        low: ChannelValue::rgb(0.1, 0.2, 0.3),
        high: ChannelValue::rgb(0.8, 0.6, 0.4),
    }
}

/// Procedural wood under a procedural moss fill masked by a noisy directional generator.
fn stack(res: u32) -> (TextureSet, LayerId) {
    let mut ts = TextureSet::new("T", 0, res);
    let tc = ts.clone();
    let mut wood = Layer::new_fill("Wood", &tc);
    let LayerContent::Fill(f) = &mut wood.content else { unreachable!() };
    f.channels.insert(ChannelKind::BaseColor, FillChannel::new(procedural(ProceduralKind::WoodRings, 3.0)));
    f.channels.insert(ChannelKind::Roughness, FillChannel::new(procedural(ProceduralKind::Grunge, 8.0)));
    ts.insert_above(None, wood);
    let mut moss = Layer::new_fill("Moss", &tc);
    let LayerContent::Fill(f) = &mut moss.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = *k == ChannelKind::BaseColor;
    }
    f.channels.insert(ChannelKind::BaseColor, FillChannel::new(procedural(ProceduralKind::Clouds, 6.0)));
    let mut mask = Mask::new(res, false);
    let mut g = Generator::new(GeneratorKind::Directional);
    g.noise_amount = 0.35;
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    moss.mask = Some(mask);
    let moss = ts.insert_above(None, moss);
    (ts, moss)
}

fn with_level(ts: &TextureSet, moss: LayerId, level: f32) -> TextureSet {
    let mut ts = ts.clone();
    let mask = ts.find_mut(moss).unwrap().mask.as_mut().unwrap();
    let MaskEffectKind::Generator(g) = &mut Arc::make_mut(&mut mask.effects[0]).kind else { unreachable!() };
    g.level = level;
    ts
}

fn snapshot(ts: &TextureSet, geo: &Arc<TextureSetGeometry>) -> Snapshot {
    Snapshot {
        sets: vec![SetSnapshot { texture_set: ts.clone(), geometry: geo.clone(), mesh_maps: MeshMaps::new() }],
        resources: ResourceStore::default(),
        invalidate: BTreeSet::new(),
    }
}

fn full(ts: &TextureSet, geo: &TextureSetGeometry) -> Composite {
    let (res, maps) = (ResourceStore::default(), MeshMaps::new());
    composite_full(ts, &EvalContext { geometry: geo, resources: &res, mesh_maps: &maps })
}

fn worker() -> CompositeWorker {
    CompositeWorker::spawn(&mut HashMap::new()).expect("threads are available in tests")
}

fn published(w: &CompositeWorker, id: TextureSetId) -> Published {
    w.take().remove(&id).expect("the worker published this texture set")
}

/// What a GPU copy of `old` holds after uploading the reported changed tiles of `new`.
fn uploaded(old: &Composite, new: &Published) -> Composite {
    let mut gpu = old.clone();
    for (kind, img) in gpu.channels.iter_mut() {
        for &t in new.changed.get(kind).map_or(&[][..], Vec::as_slice) {
            img.set_slot(t, new.composite.channel(*kind).unwrap().tile(t).cloned());
        }
    }
    gpu
}

#[test]
fn worker_converges_exactly_and_reports_every_changed_tile() {
    let geo = Arc::new(TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 256));
    let (ts, moss) = stack(256);
    let w = worker();
    let a = with_level(&ts, moss, 0.3);
    w.submit(snapshot(&a, &geo));
    w.finish();
    assert!(w.idle());
    let first = published(&w, ts.id);
    assert_eq!(first.composite, full(&a, &geo));
    assert_eq!(first.tiles, (0..16).collect::<Vec<_>>());

    // Several edits in flight, then a synchronous finish: exact, and the reported tiles bring
    // an uploaded copy of the previous composite to the new one.
    for level in [0.4, 0.5, 0.6, 0.7] {
        w.submit(snapshot(&with_level(&ts, moss, level), &geo));
    }
    w.finish();
    let second = published(&w, ts.id);
    let b = with_level(&ts, moss, 0.7);
    assert_eq!(second.composite, full(&b, &geo));
    assert_eq!(uploaded(&first.composite, &second), second.composite);
    assert!(!second.changed.contains_key(&ChannelKind::Roughness), "the moss mask does not touch roughness");
    assert!(!w.has_published());

    // An unchanged snapshot re-evaluates nothing.
    w.submit(snapshot(&b, &geo));
    w.finish();
    assert!(w.take().is_empty());
}

#[test]
fn newest_snapshot_wins() {
    // Cold fields at 512² keep the worker busy while ten edits arrive.
    let geo = Arc::new(TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 512));
    let (ts, moss) = stack(512);
    let w = worker();
    for i in 0..10 {
        w.submit(snapshot(&with_level(&ts, moss, 0.3 + i as f32 * 0.05), &geo));
    }
    w.finish();
    let last = with_level(&ts, moss, 0.3 + 9.0 * 0.05);
    assert_eq!(published(&w, ts.id).composite, full(&last, &geo));
    let started = w.snapshots_started();
    assert!(started < 10, "replaced snapshots are skipped, started {started}");
}

#[test]
fn full_reevaluation_survives_a_replacing_snapshot() {
    let geo = Arc::new(TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 256));
    let (ts, _) = stack(256);
    let w = worker();
    w.submit(snapshot(&ts, &geo));
    w.finish();
    w.take();
    // Invalidate, then (possibly before the worker saw it) the same inputs without the flag.
    let mut invalidating = snapshot(&ts, &geo);
    invalidating.invalidate.insert(ts.id);
    w.submit(invalidating);
    w.submit(snapshot(&ts, &geo));
    w.finish();
    assert_eq!(published(&w, ts.id).tiles, (0..16).collect::<Vec<_>>());
}

#[test]
fn worker_continues_from_adopted_caches() {
    let geo = Arc::new(TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 256));
    let (ts, moss) = stack(256);
    let (res, maps) = (ResourceStore::default(), MeshMaps::new());
    let mut caches = HashMap::new();
    let cache: &mut subcraft_compositor::CompositeCache = caches.entry(ts.id).or_default();
    cache.update(&ts, &EvalContext { geometry: &geo, resources: &res, mesh_maps: &maps });
    let w = CompositeWorker::spawn(&mut caches).unwrap();
    assert!(caches.is_empty(), "the caches moved to the worker");
    // Only the moss mask changed: tiles the generator does not reach are not re-reported.
    let b = with_level(&ts, moss, 0.9);
    w.submit(snapshot(&b, &geo));
    w.finish();
    let p = published(&w, ts.id);
    assert_eq!(p.composite, full(&b, &geo));
    assert!(!p.changed.contains_key(&ChannelKind::Roughness));
}

#[test]
fn resolution_and_channel_changes_replace_pending_tile_layouts() {
    let geo = Arc::new(TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 256));
    let (mut ts, _) = stack(256);
    let w = worker();
    w.submit(snapshot(&ts, &geo));
    w.finish();
    // Leave the publication unread while changing resolution and removing a channel. Merging
    // publications must drop old tile indices rather than reporting them for the smaller image.
    ts.resolution = 128;
    ts.channels.retain(|c| c.kind != ChannelKind::Roughness);
    let smaller = Arc::new(TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 128));
    w.submit(snapshot(&ts, &smaller));
    w.finish();
    let p = published(&w, ts.id);
    assert_eq!(p.composite, full(&ts, &smaller));
    assert_eq!(p.tiles, vec![0, 1, 2, 3]);
    assert!(p.changed.values().flatten().all(|&t| t < 4));
    assert!(!p.composite.channels.contains_key(&ChannelKind::Roughness));
    w.submit(Snapshot::default());
    w.finish();
    assert!(w.idle());
}
