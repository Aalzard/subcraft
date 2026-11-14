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
        sets: vec![SetSnapshot { texture_set: ts.clone(), geometry: geo.clone(), mesh_maps: MeshMaps::new(), preview_geometry: None }],
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

/// Wait (bounded) until the worker has made more than `before` preview baselines.
fn wait_for_baseline(w: &CompositeWorker, before: u64) {
    let start = std::time::Instant::now();
    while w.baselines_made() <= before {
        assert!(start.elapsed() < std::time::Duration::from_secs(60), "no preview baseline was made");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// A quick preview is the low-resolution composite stretched by nearest texel, value for value.
#[test]
fn stretched_preview_repeats_each_low_resolution_texel() {
    let mesh = Arc::new(primitives::cube(2.0));
    let low_geo = TextureSetGeometry::new(mesh, 0, 512);
    let (ts, _) = stack(1024);
    let mut low_ts = ts.clone();
    low_ts.resolution = 512;
    let low = full(&low_ts, &low_geo);
    let mut shown = Composite::new(&ts);
    let all: Vec<usize> = (0..shown.tile_count()).collect();
    shown.overlay_preview(&low, &all);
    for (kind, img) in &shown.channels {
        let src = low.channel(*kind).unwrap();
        for y in (0..1024).step_by(7) {
            for x in (0..1024).step_by(5) {
                assert_eq!(img.get(x, y), src.get(x / 2, y / 2), "{kind:?} at {x},{y}");
            }
        }
    }
}

/// After the worker went idle, a preview only replaces the tiles whose low-resolution texels
/// changed: a generator edit on a layer that only writes Base Color reports nothing to upload for
/// Roughness, the rest shows the exact tiles, and the result converges exactly.
#[test]
fn previews_after_an_idle_baseline_only_cover_what_changed() {
    let mesh = Arc::new(primitives::cube(2.0));
    let geo = Arc::new(TextureSetGeometry::new(mesh.clone(), 0, 1024));
    let low = Arc::new(TextureSetGeometry::new(mesh, 0, 512));
    let (ts, moss) = stack(1024);
    let with_preview = |ts: &TextureSet| {
        let mut s = snapshot(ts, &geo);
        s.sets[0].preview_geometry = Some(low.clone());
        s
    };
    let w = worker();
    let a = with_level(&ts, moss, 0.4);
    let made = w.baselines_made();
    w.submit(with_preview(&a));
    w.finish();
    let first = published(&w, ts.id);
    wait_for_baseline(&w, made);

    let previews = w.previews_published();
    let b = with_level(&ts, moss, 0.6);
    w.submit(with_preview(&b));
    w.finish();
    assert!(w.previews_published() > previews, "a whole-texture edit still starts with a preview");
    let second = published(&w, ts.id);
    assert_eq!(second.composite, full(&b, &geo));
    assert_eq!(uploaded(&first.composite, &second), second.composite);
    assert!(second.changed.get(&ChannelKind::Roughness).is_none_or(Vec::is_empty), "Roughness does not change: nothing to upload");
    let color = second.changed.get(&ChannelKind::BaseColor).map_or(0, Vec::len);
    assert!(color > 0 && color < second.tiles.len(), "only the Base Color tiles the level moved: {color} of {}", second.tiles.len());
}

#[test]
fn quick_preview_precedes_and_exact_result_replaces_it() {
    let mesh = Arc::new(primitives::cube(2.0));
    let geo = Arc::new(TextureSetGeometry::new(mesh.clone(), 0, 1024));
    let low = Arc::new(TextureSetGeometry::new(mesh, 0, 512));
    let (ts, moss) = stack(1024);
    let with_preview = |ts: &TextureSet| {
        let mut s = snapshot(ts, &geo);
        s.sets[0].preview_geometry = Some(low.clone());
        s
    };
    assert_eq!(subcraft_compositor::preview_resolution(1024), Some(512));
    assert_eq!(subcraft_compositor::preview_resolution(4096), Some(512));
    assert_eq!(subcraft_compositor::preview_resolution(512), None);

    let w = worker();
    let a = with_level(&ts, moss, 0.3);
    w.submit(with_preview(&a));
    w.finish();
    assert!(w.previews_published() >= 1, "the first composite starts with a quick preview");
    let first = published(&w, ts.id);
    assert_eq!(first.composite, full(&a, &geo), "the final composite is the exact one");
    assert_eq!(uploaded(&Composite::new(&a), &first), first.composite, "every previewed tile is reported for upload");

    let b = with_level(&ts, moss, 0.7);
    w.submit(with_preview(&b));
    w.finish();
    assert!(w.previews_published() >= 2);
    let second = published(&w, ts.id);
    assert_eq!(second.composite, full(&b, &geo));
    assert_eq!(uploaded(&first.composite, &second), second.composite);
    assert!(w.idle() && !w.has_published());
}

/// A stroke-sized raster edit (more than the old fixed 128-tile threshold, under a quarter of the
/// set) goes straight to the exact tiles: no quick preview, exact result, only its tiles reported.
/// An edit of the whole texture still starts with a preview.
#[test]
fn a_stroke_sized_edit_skips_the_quick_preview() {
    assert_eq!(subcraft_compositor::worker::preview_min_tiles(256), 128, "1024²: unchanged");
    assert_eq!(subcraft_compositor::worker::preview_min_tiles(1024), 256);
    assert_eq!(subcraft_compositor::worker::preview_min_tiles(4096), 1024);

    let mesh = Arc::new(primitives::cube(2.0));
    let geo = Arc::new(TextureSetGeometry::new(mesh.clone(), 0, 2048));
    let low = Arc::new(TextureSetGeometry::new(mesh, 0, 512));
    let (mut ts, moss) = stack(2048);
    let tc = ts.clone();
    let paint = ts.insert_above(None, Layer::new_paint("Paint", &tc));
    // The layer already holds paint, so the stroke below is a raster-only change.
    let LayerContent::Paint(content) = &mut ts.find_mut(paint).unwrap().content else { unreachable!() };
    content.channel_mut(ChannelKind::BaseColor, 2048).set(1, 1, [0.0, 1.0, 0.0, 1.0]);
    let with_preview = |ts: &TextureSet| {
        let mut s = snapshot(ts, &geo);
        s.sets[0].preview_geometry = Some(low.clone());
        s
    };
    let w = worker();
    let made = w.baselines_made();
    w.submit(with_preview(&ts));
    w.finish();
    let first = published(&w, ts.id);
    wait_for_baseline(&w, made);

    // One texel in each tile of a 13 x 13 block: 169 of 1024 tiles.
    let mut painted = ts.clone();
    let LayerContent::Paint(content) = &mut painted.find_mut(paint).unwrap().content else { unreachable!() };
    let img = content.channel_mut(ChannelKind::BaseColor, 2048);
    for ty in 3..16u32 {
        for tx in 5..18u32 {
            img.set(tx * 64 + 7, ty * 64 + 9, [1.0, 0.0, 0.0, 1.0]);
        }
    }
    let previews = w.previews_published();
    w.submit(with_preview(&painted));
    w.finish();
    assert_eq!(w.previews_published(), previews, "169 stale tiles of 1024: no preview");
    let second = published(&w, ts.id);
    assert_eq!(second.composite, full(&painted, &geo));
    assert!((169..256).contains(&second.tiles.len()), "only the painted tiles were evaluated: {}", second.tiles.len());
    assert_eq!(uploaded(&first.composite, &second), second.composite);

    // The whole texture: previewed first, as before.
    w.submit(with_preview(&with_level(&painted, moss, 0.7)));
    w.finish();
    assert!(w.previews_published() > previews, "a whole-texture edit still starts with a preview");
    assert_eq!(published(&w, ts.id).composite, full(&with_level(&painted, moss, 0.7), &geo));
}
