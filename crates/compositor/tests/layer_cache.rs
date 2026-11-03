//! The per-layer result cache is derived data: whatever the edit, the cached recomposite must be
//! bit-for-bit the same as a full recomposite, and edits above a cached boundary must reuse it.

use std::sync::Arc;

use subcraft_compositor::{CompositeCache, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{
    BlendMode, ContentEffect, ContentEffectKind, FillChannel, FillSource, Filter, Layer, LayerContent, ProceduralKind, ProceduralParams,
    ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 256; // 16 tiles
const LAYERS: usize = 8;

struct Fixture {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fixture {
    fn new() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

fn procedural(kind: ProceduralKind, scale: f32, t: f32) -> FillSource {
    FillSource::Procedural {
        params: ProceduralParams { scale, ..ProceduralParams::new(kind) },
        low: ChannelValue::rgb(0.1 + t, 0.2, 0.3),
        high: ChannelValue::rgb(0.8, 0.6 - t, 0.4),
    }
}

/// Bottom to top: fills with different patterns and blends; every third layer is a paint layer
/// with one stroke; with `filters` some layers carry blur / sharpen content effects.
fn stack(filters: bool) -> (TextureSet, Vec<LayerId>) {
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let kinds = [ProceduralKind::Clouds, ProceduralKind::Cells, ProceduralKind::Grunge, ProceduralKind::Scratches];
    let modes = [BlendMode::Normal, BlendMode::Multiply, BlendMode::Overlay, BlendMode::Screen];
    let mut ids = Vec::new();
    for i in 0..LAYERS {
        let mut l = if i % 3 == 2 {
            let mut p = Layer::new_paint(format!("P{i}"), &tc);
            let LayerContent::Paint(pc) = &mut p.content else { unreachable!() };
            let img = pc.channel_mut(ChannelKind::BaseColor, RES);
            for y in 20..60 {
                for x in 30..90 + 10 * i as u32 {
                    img.set(x, y, [0.9, 0.2, 0.1, 1.0]);
                }
            }
            p
        } else {
            let mut f = Layer::new_fill(format!("F{i}"), &tc);
            let LayerContent::Fill(fc) = &mut f.content else { unreachable!() };
            fc.channels.insert(ChannelKind::BaseColor, FillChannel::new(procedural(kinds[i % 4], 3.0 + i as f32, 0.05 * i as f32)));
            fc.channels.insert(ChannelKind::Roughness, FillChannel::new(procedural(kinds[(i + 1) % 4], 5.0, 0.0)));
            f
        };
        if i > 0 {
            l.blend.get_mut(&ChannelKind::BaseColor).unwrap().mode = modes[i % 4];
        }
        if filters && i % 2 == 1 {
            let filter = if i % 4 == 1 { Filter::Blur { radius: 3.0 } } else { Filter::Sharpen { radius: 2.0, amount: 1.0 } };
            l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: ChannelKind::BaseColor, filter }, &tc)));
        }
        ids.push(ts.insert_above(None, l));
    }
    (ts, ids)
}

fn assert_exact(cache: &CompositeCache, ts: &TextureSet, fx: &Fixture, what: &str) {
    assert_eq!(cache.composite().unwrap(), &composite_full(ts, &fx.ctx()), "cached result differs from a full recomposite after {what}");
}

fn edits(filters: bool, budget: usize) {
    let fx = Fixture::new();
    let (mut ts, ids) = stack(filters);
    let mut cache = CompositeCache::new();
    cache.set_layer_cache_budget(budget);
    cache.update(&ts, &fx.ctx());
    assert_exact(&cache, &ts, &fx, "the first composite");
    if budget > 0 {
        assert!(cache.layer_cache_stats().entries > 0, "boundaries are stored");
        assert!(cache.layer_cache_stats().bytes <= budget);
    }

    for pos in [0usize, 3, 5, LAYERS - 1] {
        let id = ids[pos];
        let before = cache.layer_cache_stats().hits;
        ts.find_mut(id).unwrap().visible = false;
        cache.update(&ts, &fx.ctx());
        assert_exact(&cache, &ts, &fx, &format!("hiding layer {pos}"));
        ts.find_mut(id).unwrap().visible = true;
        cache.update(&ts, &fx.ctx());
        assert_exact(&cache, &ts, &fx, &format!("showing layer {pos}"));

        ts.find_mut(id).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = 0.37;
        cache.update(&ts, &fx.ctx());
        assert_exact(&cache, &ts, &fx, &format!("opacity of layer {pos}"));

        ts.find_mut(id).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().mode = BlendMode::Difference;
        cache.update(&ts, &fx.ctx());
        assert_exact(&cache, &ts, &fx, &format!("blend mode of layer {pos}"));

        if budget >= 6 << 20 && pos > 3 {
            assert!(cache.layer_cache_stats().hits > before, "an edit of layer {pos} reuses cached layers below it");
        }
    }

    // A filter parameter.
    if filters {
        let layer = ts.find_mut(ids[5]).unwrap();
        let e = Arc::make_mut(&mut layer.content_effects[0]);
        e.kind = ContentEffectKind::Filter { channel: ChannelKind::BaseColor, filter: Filter::Blur { radius: 5.0 } };
        cache.update(&ts, &fx.ctx());
        assert_exact(&cache, &ts, &fx, "a filter parameter");
    }

    // Raster edits (a stroke on a paint layer): only the touched tiles are re-evaluated, and the
    // stored boundaries above stay valid elsewhere.
    for round in 0..3u32 {
        let id = ids[2 + 3 * (round as usize % 2)];
        let layer = ts.find_mut(id).unwrap();
        let LayerContent::Paint(pc) = &mut layer.content else { unreachable!() };
        let img = pc.channel_mut(ChannelKind::BaseColor, RES);
        for y in 130..150 {
            for x in 140..170 + round {
                img.set(x, y, [0.1, 0.8, 0.3, 1.0]);
            }
        }
        let tiles = cache.update(&ts, &fx.ctx());
        assert!(tiles.len() < 16, "a raster edit does not dirty every tile ({})", tiles.len());
        assert_exact(&cache, &ts, &fx, "a raster edit");
    }

    // Removing a layer, then an explicit invalidation.
    ts.remove(ids[4]).unwrap();
    cache.update(&ts, &fx.ctx());
    assert_exact(&cache, &ts, &fx, "removing a layer");
    cache.invalidate();
    cache.update(&ts, &fx.ctx());
    assert_exact(&cache, &ts, &fx, "an explicit invalidation");
}

#[test]
fn cached_recomposite_is_bit_exact_local_path() {
    edits(false, 64 << 20);
}

#[test]
fn cached_recomposite_is_bit_exact_neighborhood_path() {
    edits(true, 64 << 20);
}

#[test]
fn cached_recomposite_is_bit_exact_under_a_tight_budget() {
    // Roughly two boundaries' worth: LRU eviction and sparse planning must not change a bit.
    edits(false, 6 << 20);
    edits(true, 6 << 20);
}

#[test]
fn disabled_cache_is_identical() {
    edits(false, 0);
    edits(true, 0);
}

#[test]
fn top_edit_reuses_cache_and_budget_is_respected() {
    let fx = Fixture::new();
    let (mut ts, ids) = stack(true);
    let mut cache = CompositeCache::new();
    cache.set_layer_cache_budget(6 << 20);
    cache.update(&ts, &fx.ctx());
    let stats = cache.layer_cache_stats();
    assert!(stats.entries > 0 && stats.bytes <= 6 << 20, "{stats:?}");
    let hits = stats.hits;
    ts.find_mut(*ids.last().unwrap()).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = 0.5;
    cache.update(&ts, &fx.ctx());
    assert!(cache.layer_cache_stats().hits >= hits + 16, "every tile resumed from a cached boundary");
    assert_exact(&cache, &ts, &fx, "a top-layer opacity change");
}
