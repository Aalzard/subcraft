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

/// Adding a layer on top moves every planned boundary (they are spaced from the top), but the
/// boundaries stored before are still exact: every tile resumes from one of them instead of
/// re-blending the whole stack, and so does the first paint on the new layer.
#[test]
fn a_new_top_layer_resumes_from_the_boundaries_stored_before() {
    let fx = Fixture::new();
    let (mut ts, _) = stack(true);
    let mut cache = CompositeCache::new();
    // Tight: boundaries are spaced apart, so the plan for one more layer shares none of them.
    cache.set_layer_cache_budget(6 << 20);
    cache.update(&ts, &fx.ctx());
    let hits = cache.layer_cache_stats().hits;

    let tc = ts.clone();
    let top = ts.insert_above(None, Layer::new_paint("New", &tc));
    cache.update(&ts, &fx.ctx());
    assert!(cache.layer_cache_stats().hits >= hits + 16, "every tile resumed below the new layer");
    assert_exact(&cache, &ts, &fx, "a new top layer");

    let hits = cache.layer_cache_stats().hits;
    let LayerContent::Paint(pc) = &mut ts.find_mut(top).unwrap().content else { unreachable!() };
    let img = pc.channel_mut(ChannelKind::BaseColor, RES);
    for y in 100..140 {
        for x in 60..200 {
            img.set(x, y, [0.1, 0.8, 0.3, 1.0]);
        }
    }
    cache.update(&ts, &fx.ctx());
    assert!(cache.layer_cache_stats().hits > hits, "the stroke's tiles resumed from a cached boundary");
    assert_exact(&cache, &ts, &fx, "a stroke on the new top layer");
    assert!(cache.layer_cache_stats().bytes <= 6 << 20);

    // Removing it again, and an edit at the bottom (drops every boundary), stay exact.
    ts.remove(top).unwrap();
    cache.update(&ts, &fx.ctx());
    assert_exact(&cache, &ts, &fx, "the top layer removed");
}

/// An edit low in the stack changes what the layers above are blended onto, not what they are:
/// their filtered content (and masks) comes from the content cache instead of being evaluated
/// again, and the result is still exactly a full recomposite.
#[test]
fn an_edit_low_in_the_stack_reuses_the_content_of_the_layers_above() {
    let fx = Fixture::new();
    let (mut ts, ids) = stack(true);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    let (entries, _, hits) = cache.content_cache_stats();
    assert!(entries >= 16, "the filtered layers' content is stored ({entries} entries)");

    ts.find_mut(ids[0]).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = 0.4;
    cache.update(&ts, &fx.ctx());
    let (_, _, after) = cache.content_cache_stats();
    // Layers 1, 3, 5 and 7 carry a blur or sharpen: 16 tiles each.
    assert!(after >= hits + 4 * 16, "every filtered layer above was read from the cache ({hits} -> {after})");
    assert_exact(&cache, &ts, &fx, "a bottom-layer opacity change");

    // Editing a filtered layer itself replaces its content (and only its own).
    let layer = ts.find_mut(ids[3]).unwrap();
    let e = Arc::make_mut(&mut layer.content_effects[0]);
    e.kind = ContentEffectKind::Filter { channel: ChannelKind::BaseColor, filter: Filter::Blur { radius: 4.0 } };
    cache.update(&ts, &fx.ctx());
    assert_exact(&cache, &ts, &fx, "a filter parameter of a cached layer");
    ts.remove(ids[5]).unwrap();
    cache.update(&ts, &fx.ctx());
    assert_exact(&cache, &ts, &fx, "removing a cached layer");
}

/// A stack whose content does not fit the budget keeps what it holds (no eviction of entries the
/// same pass is about to read): every further edit still hits, within the budget, and exactly.
#[test]
fn content_cache_over_budget_keeps_hitting() {
    let fx = Fixture::new();
    let (mut ts, ids) = stack(true);
    let mut cache = CompositeCache::new();
    let budget = 1 << 20; // the four filtered layers need about 3 MB
    cache.set_layer_cache_budget(budget);
    cache.update(&ts, &fx.ctx());
    for (round, opacity) in [0.3f32, 0.6, 0.8].into_iter().enumerate() {
        let (_, bytes, hits) = cache.content_cache_stats();
        assert!(bytes > 0 && bytes <= budget, "{bytes} bytes");
        ts.find_mut(ids[0]).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = opacity;
        cache.update(&ts, &fx.ctx());
        let (_, _, after) = cache.content_cache_stats();
        assert!(after >= hits + 8, "edit {round}: stored content is still read ({hits} -> {after})");
        assert_exact(&cache, &ts, &fx, "a bottom edit over budget");
    }
}

/// Results that depend on other layers are never taken from the content cache: a passthrough
/// layer (its content is the stack below, blurred) and a masked layer stay exact when the layers
/// below them change.
#[test]
fn passthrough_and_masked_layers_stay_exact_with_cached_content() {
    use subcraft_doc::{Generator, GeneratorKind, Mask, MaskEffect, MaskEffectKind};
    let fx = Fixture::new();
    let (mut ts, ids) = stack(true);
    // Layer 5 (blur): passthrough, so the blur reads everything below it.
    ts.find_mut(ids[5]).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().mode = BlendMode::Passthrough;
    // Layer 6: masked by a generator.
    let mut mask = Mask::new(RES, false);
    let mut g = Generator::new(GeneratorKind::Directional);
    g.noise_amount = 0.35;
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    ts.find_mut(ids[6]).unwrap().mask = Some(mask);

    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    assert_exact(&cache, &ts, &fx, "the first composite");
    for (pos, opacity) in [(0usize, 0.45f32), (2, 0.7), (4, 0.2), (0, 0.9)] {
        ts.find_mut(ids[pos]).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = opacity;
        cache.update(&ts, &fx.ctx());
        assert_exact(&cache, &ts, &fx, &format!("opacity of layer {pos} under a passthrough blur"));
    }
    // The mask's generator level: only that layer's mask changes.
    let m = ts.find_mut(ids[6]).unwrap().mask.as_mut().unwrap();
    let MaskEffectKind::Generator(g) = &mut Arc::make_mut(&mut m.effects[0]).kind else { unreachable!() };
    g.level = 0.7;
    cache.update(&ts, &fx.ctx());
    assert_exact(&cache, &ts, &fx, "a mask generator level");
}
