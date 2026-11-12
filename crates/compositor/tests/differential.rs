//! Randomized differential tests of the compositor (filter audit, session 95):
//! (a) point path vs staged-graph path (a Blur radius-0 no-op forces the graph),
//! (b) incremental `CompositeCache` update after an edit vs `composite_full`.
use std::sync::Arc;
use subcraft_compositor::{CompositeCache, EvalContext, composite_full};
use subcraft_core::{ChannelKind as C, ChannelValue};
use subcraft_doc::{
    BlendMode, ContentEffect, ContentEffectKind, FillChannel, FillSource, Filter, Layer, LayerContent, Mask,
    MaskEffect, MaskEffectKind, OutlinePosition, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};

/// 16 tiles, so supports of 30 texels reach into neighboring tiles from every direction.
const N: u32 = 256;
const CHANNELS: [C; 3] = [C::BaseColor, C::Roughness, C::Metallic];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self) -> f32 { (self.next() >> 40) as f32 / (1u64 << 24) as f32 }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
    fn pick<T: Copy>(&mut self, items: &[T]) -> T { items[self.below(items.len())] }
}

struct Fixture { geo: TextureSetGeometry, res: ResourceStore, maps: MeshMaps }
impl Fixture {
    fn new() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 2)), 0, N), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> { EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps } }
}

fn blend_modes(folder: bool) -> Vec<BlendMode> {
    BlendMode::ALL.into_iter()
        .filter(|m| !matches!(m, BlendMode::NormalMapCombine | BlendMode::NormalMapDetail | BlendMode::NormalMapInverseDetail))
        .filter(|m| folder || *m != BlendMode::Passthrough).collect()
}

fn spatial_filter(r: &mut Rng) -> Filter {
    // Mostly small, sometimes reaching across a whole neighboring tile.
    let radius = (r.f() * r.pick(&[6.0, 6.0, 30.0])).round();
    match r.below(8) {
        7 => Filter::Warp { intensity: radius, scale: 1 + r.below(8) as u32, seed: r.below(100) as u32 },
        6 => Filter::Median { radius },
        0 => Filter::Blur { radius }, 1 => Filter::Sharpen { radius, amount: r.f() * 2.0 },
        2 => Filter::DirectionalBlur { radius, angle_deg: r.f() * 360.0 - 180.0 },
        3 => Filter::Highpass { radius, amount: r.f() * 2.0 },
        4 => Filter::Bevel { radius, amount: r.f() * 2.0 - 1.0, threshold: r.f() * 0.8 },
        _ => Filter::MaskOutline { radius, threshold: r.f() * 0.8, position: r.pick(&[OutlinePosition::Inner, OutlinePosition::Outer, OutlinePosition::Both]) },
    }
}
fn point_filter(r: &mut Rng) -> Filter {
    match r.below(5) {
        0 => Filter::Contrast { gain: r.f() * 2.0, pivot: r.f() },
        1 => Filter::Threshold { cutoff: r.f() },
        3 => Filter::HistogramScan { position: r.f(), contrast: r.f(), invert: r.below(2) == 0 },
        4 => Filter::HistogramRange { position: r.f(), range: r.f(), softness: r.f(), invert: r.below(2) == 0 },
        2 => {
            let (a, b) = (r.f() * 0.5, 0.5 + r.f() * 0.5);
            let st = |position, color| subcraft_doc::GradientStop { position, color };
            Filter::GradientMap { gradient: subcraft_doc::Gradient::new(&[st(0.0, [r.f(), r.f(), r.f()]), st(a, [r.f(), 0.0, r.f()]), st(b, [0.0, r.f(), r.f()]), st(1.0, [r.f(); 3])]).unwrap(), to_gray: r.below(2) == 0 }
        }
        _ => { let a = r.f() * 0.5; Filter::Clamp { low: a, high: a + 0.3 + r.f() * 0.2 } }
    }
}

fn randomize_blend(l: &mut Layer, r: &mut Rng, folder: bool) {
    let modes = blend_modes(folder);
    for ch in CHANNELS {
        if let Some(b) = l.blend.get_mut(&ch) { b.mode = r.pick(&modes); b.opacity = [1.0, 0.5, 0.8][r.below(3)]; }
    }
}

fn random_raster(l: &mut Layer, r: &mut Rng) {
    let LayerContent::Paint(p) = &mut l.content else { return };
    for ch in CHANNELS {
        if r.below(3) == 0 { continue; }
        let img = p.channel_mut(ch, N);
        let (sx, sy) = (3 + r.below(15) as u32, 3 + r.below(15) as u32);
        let (c, a) = (r.f(), if r.below(4) == 0 { 0.5 } else { 1.0 });
        for y in 0..N { for x in 0..N {
            if (x / sx + y / sy) % 2 == 0 { img.set(x, y, [c, 1.0 - c, c * 0.5, a]); }
        } }
    }
}

fn random_layer(ts: &TextureSet, r: &mut Rng, spatial: bool, depth: u32) -> Layer {
    let kind = if depth < 1 { r.below(5) } else { r.below(3) };
    let mut l = match kind {
        0 | 1 => { let mut l = Layer::new_paint("p", ts); random_raster(&mut l, r); l }
        2 => {
            let mut l = Layer::new_fill("f", ts);
            if let LayerContent::Fill(f) = &mut l.content {
                for ch in CHANNELS {
                    if r.below(2) == 0 { f.channels.insert(ch, FillChannel::new(FillSource::uniform(ChannelValue::rgb(r.f(), r.f(), r.f())))); }
                }
            }
            l
        }
        _ => {
            let mut l = Layer::new_folder("g", ts);
            let kids: Vec<_> = (0..1 + r.below(3)).map(|_| Arc::new(random_layer(ts, r, spatial, depth + 1))).collect();
            if let LayerContent::Group(g) = &mut l.content { g.children = kids; }
            l
        }
    };
    let folder = l.is_folder();
    randomize_blend(&mut l, r, folder);
    if r.below(3) == 0 {
        let mut m = Mask::new(N, r.below(2) == 0);
        for y in 0..N { for x in 0..N { if (x / 9 + y / 5) % 3 == 0 { m.base.set(x, y, [r.f(), 0.0, 0.0, 0.0]); } } }
        if r.below(2) == 0 { m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(point_filter(r))))); }
        if spatial && r.below(2) == 0 {
            let mut e = MaskEffect::new(MaskEffectKind::Filter(spatial_filter(r)));
            e.blend = r.pick(&[BlendMode::Normal, BlendMode::Multiply, BlendMode::Lighten]);
            e.opacity = [1.0, 0.6][r.below(2)];
            m.effects.push(Arc::new(e));
        }
        l.mask = Some(m);
    }
    for _ in 0..r.below(3) {
        let channel = r.pick(&CHANNELS);
        let filter = if spatial && r.below(2) == 0 { spatial_filter(r) } else { point_filter(r) };
        if !filter.supports_channel(channel) { continue; }
        let mut e = ContentEffect::new(ContentEffectKind::Filter { channel, filter }, ts);
        if let Some(b) = e.blend.get_mut(&channel) { b.mode = r.pick(&[BlendMode::Normal, BlendMode::Multiply, BlendMode::Replace]); b.opacity = [1.0, 0.7][r.below(2)]; }
        l.content_effects.push(Arc::new(e));
    }
    l
}

fn random_set(seed: u64, spatial: bool) -> TextureSet {
    let mut r = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    let mut ts = TextureSet::new("T", 0, N);
    for _ in 0..2 + r.below(4) {
        let l = random_layer(&ts, &mut r, spatial, 0);
        ts.insert_above(None, l);
    }
    ts
}

fn max_diff(a: &subcraft_compositor::Composite, b: &subcraft_compositor::Composite) -> Option<(C, u32, u32, f32)> {
    let mut worst: Option<(C, u32, u32, f32)> = None;
    for (&k, ia) in &a.channels {
        let ib = b.channel(k).unwrap();
        for y in 0..N { for x in 0..N {
            let d = ia.get(x, y).iter().zip(ib.get(x, y)).map(|(p, q)| (p - q).abs()).fold(0.0f32, f32::max);
            if d > 0.0 && worst.is_none_or(|w| d > w.3) { worst = Some((k, x, y, d)); }
        } }
    }
    worst
}

/// Adds a visible, active Blur radius-0 (switches evaluation to the graph) without changing which
/// layers take part: on a layer that really has Roughness content, or on a transparent bottom layer.
fn force_graph(ts: &mut TextureSet, r: &mut Rng) {
    let snapshot = ts.clone();
    let blur = || Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter: Filter::Blur { radius: 0.0 } }, &snapshot));
    let candidates: Vec<_> = ts.layers.iter().filter(|l| match &l.content {
        LayerContent::Paint(p) => p.channels.contains_key(&C::Roughness),
        LayerContent::Fill(f) => f.channels.get(&C::Roughness).is_some_and(|c| c.enabled),
        LayerContent::Group(_) => !l.content_effects.is_empty(),
    }).map(|l| l.id).collect();
    if !candidates.is_empty() && r.below(2) == 0 {
        let id = candidates[r.below(candidates.len())];
        ts.find_mut(id).unwrap().content_effects.push(blur());
    } else {
        let mut l = Layer::new_paint("noop", &snapshot);
        l.content_effects.push(blur());
        ts.insert(None, 0, l).unwrap();
    }
}

#[test]
fn random_stacks_without_spatial_filters_match_between_point_and_graph_paths() {
    let fx = Fixture::new();
    for seed in 1..=seed_count() {
        let mut ts = random_set(seed, false);
        let point = composite_full(&ts, &fx.ctx());
        let mut r = Rng(seed * 77 + 5);
        force_graph(&mut ts, &mut r);
        let graph = composite_full(&ts, &fx.ctx());
        if let Some((k, x, y, d)) = max_diff(&point, &graph) {
            panic!("seed {seed}: point and graph paths differ in {k:?} at ({x},{y}) by {d}");
        }
    }
}

fn edit_random_raster(ts: &mut TextureSet, r: &mut Rng) {
    // Pick a random paint layer or mask (anywhere in the tree) and overwrite a small block.
    fn collect(layers: &[Arc<Layer>], out: &mut Vec<subcraft_core::LayerId>) {
        for l in layers {
            out.push(l.id);
            if let LayerContent::Group(g) = &l.content { collect(&g.children, out); }
        }
    }
    let mut ids = Vec::new();
    collect(&ts.layers, &mut ids);
    let id = ids[r.below(ids.len())];
    let layer = ts.find_mut(id).unwrap();
    let (x0, y0, v) = (r.below(N as usize - 12) as u32, r.below(N as usize - 12) as u32, r.f());
    let paint_mask = layer.mask.is_some() && (r.below(2) == 0 || !matches!(layer.content, LayerContent::Paint(_)));
    if paint_mask {
        let m = layer.mask.as_mut().unwrap();
        for y in y0..y0 + 10 { for x in x0..x0 + 10 { m.base.set(x, y, [v, 0.0, 0.0, 0.0]); } }
    } else if let LayerContent::Paint(p) = &mut layer.content {
        let ch = r.pick(&CHANNELS);
        let img = p.channel_mut(ch, N);
        for y in y0..y0 + 10 { for x in x0..x0 + 10 { img.set(x, y, [v, 1.0 - v, v, 1.0]); } }
    }
}

#[test]
fn random_stacks_incremental_cache_matches_full_evaluation_after_paint_edits() {
    let fx = Fixture::new();
    for seed in 1..=seed_count() {
        let mut ts = random_set(seed + 1000, true);
        let mut cache = CompositeCache::new();
        cache.update(&ts, &fx.ctx());
        let mut r = Rng(seed * 31 + 3);
        for step in 0..3 {
            edit_random_raster(&mut ts, &mut r);
            cache.update(&ts, &fx.ctx());
            let full = composite_full(&ts, &fx.ctx());
            if let Some((k, x, y, d)) = max_diff(cache.composite().unwrap(), &full) {
                panic!("seed {seed} step {step}: incremental differs from full in {k:?} at ({x},{y}) by {d}");
            }
        }
    }
}

fn blur(ts: &TextureSet, radius: f32) -> Arc<ContentEffect> {
    Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter: Filter::Blur { radius } }, ts))
}

fn painted(ts: &TextureSet, name: &str, seed: u64) -> Layer {
    let mut l = Layer::new_paint(name, ts);
    random_raster(&mut l, &mut Rng(seed));
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    p.channel_mut(C::Roughness, N).set(0, 0, [0.5, 1.0, 0.0, 0.0]); // make sure Roughness exists
    l
}

fn dab(ts: &mut TextureSet, id: subcraft_core::LayerId, x: u32, y: u32) {
    let LayerContent::Paint(p) = &mut ts.find_mut(id).unwrap().content else { unreachable!() };
    p.channel_mut(C::Roughness, N).set(x, y, [0.9, 1.0, 0.0, 0.0]);
}

/// A change only travels through the filters that act on the changed layer's own pixels (its
/// effects, its folders', and folders above that let the accumulation through). Filters of other
/// layers act on those layers' content, so they must not widen the refresh: this is what keeps a
/// brush stroke on a big stack from re-evaluating a neighborhood of every dirty tile.
#[test]
fn refresh_grows_only_by_filters_that_carry_the_change_and_stays_exact() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let mut under = painted(&ts, "blurred below", 1);
    under.content_effects.push(blur(&ts, 30.0));
    under.content_effects.push(blur(&ts, 30.0));
    ts.insert_above(None, under);
    let top = ts.insert_above(None, painted(&ts, "top", 2));
    let blurred_id = ts.layers[0].id;
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());

    // (x, y) = (130, 130) is the middle of tile (2, 2) of 4x4; nothing above the layer filters.
    dab(&mut ts, top, 130, 130);
    let tiles = cache.update(&ts, &fx.ctx());
    assert_eq!(tiles, vec![2 * 4 + 2], "a dab on the top layer refreshes its own tile only");
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));

    // The blurred layer's own filters carry its change into the neighbors.
    dab(&mut ts, blurred_id, 130, 130);
    let tiles = cache.update(&ts, &fx.ctx());
    assert!(tiles.len() >= 9, "blurred layer: {tiles:?}");
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));

    // A passthrough folder above sees (and blurs) everything below it, so its filter counts.
    let mut folder = Layer::new_folder("above", &ts);
    folder.content_effects.push(blur(&ts, 30.0));
    for b in folder.blend.values_mut() { b.mode = BlendMode::Passthrough; }
    let child = painted(&ts, "child", 3);
    if let LayerContent::Group(g) = &mut folder.content { g.children = vec![Arc::new(child)]; }
    ts.insert_above(None, folder);
    cache.update(&ts, &fx.ctx());
    dab(&mut ts, top, 70, 70);
    let tiles = cache.update(&ts, &fx.ctx());
    assert!(tiles.len() >= 9, "passthrough folder above: {tiles:?}");
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));
}

/// 40 seeds by default; `DIFF_SEEDS=2000` hunts harder.
fn seed_count() -> u64 { std::env::var("DIFF_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(40) }

#[test]
fn an_effect_on_another_channel_does_not_make_an_empty_channel_replace_what_is_below() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let mut below = Layer::new_fill("below", &ts);
    if let LayerContent::Fill(f) = &mut below.content {
        f.channels.insert(C::BaseColor, FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.8, 0.2, 0.1))));
    }
    ts.insert_above(None, below);
    let mut top = Layer::new_paint("rough only", &ts);
    random_raster(&mut top, &mut Rng(9));
    let LayerContent::Paint(p) = &mut top.content else { unreachable!() };
    p.channels.remove(&C::BaseColor);
    top.blend.get_mut(&C::BaseColor).unwrap().mode = BlendMode::Replace;
    let before = composite_full(&ts, &fx.ctx()).channel(C::BaseColor).unwrap().get(10, 10);
    let effect = ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter: Filter::Clamp { low: 0.2, high: 0.6 } }, &ts);
    top.content_effects.push(Arc::new(effect));
    ts.insert_above(None, top);
    for spatial in [false, true] {
        if spatial {
            let l = Layer::new_paint("graph", &ts);
            let mut l = l;
            l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter: Filter::Blur { radius: 2.0 } }, &ts)));
            ts.insert(None, 0, l).unwrap();
        }
        let after = composite_full(&ts, &fx.ctx()).channel(C::BaseColor).unwrap().get(10, 10);
        assert_eq!(after, before, "Base Color below must survive (graph path: {spatial})");
    }
}

/// The new neighbourhood filters (Edge Detect, Glow, Directional Warp, Mosaic), chained on one
/// paint layer and also as a mask effect, refresh exactly the tiles a dab reaches: the
/// incremental cache must equal a full evaluation after every edit, near tile borders too.
#[test]
fn new_neighbourhood_filters_refresh_incrementally_and_match_full_evaluation() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let mut l = painted(&ts, "filtered", 4);
    for filter in [Filter::EdgeDetect { radius: 3.0, strength: 1.5 }, Filter::Glow { radius: 4.0, amount: 1.0, threshold: 0.3 },
        Filter::DirectionalWarp { intensity: 5.0, angle_deg: 40.0 }, Filter::Mosaic { size: 9 }] {
        l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter }, &ts)));
    }
    let mut mask = Mask::new(N, true);
    for (i, filter) in [Filter::Mosaic { size: 5 }, Filter::DirectionalWarp { intensity: 3.0, angle_deg: -20.0 }, Filter::EdgeDetect { radius: 2.0, strength: 1.0 }].into_iter().enumerate() {
        mask.base.set(30 + i as u32 * 20, 60, [0.2, 0.0, 0.0, 0.0]);
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(filter))));
    }
    l.mask = Some(mask);
    let id = ts.insert_above(None, l);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));
    for (x, y) in [(63, 63), (64, 10), (10, 64), (127, 127), (0, 0), (100, 70)] {
        dab(&mut ts, id, x, y);
        let tiles = cache.update(&ts, &fx.ctx());
        assert!(!tiles.is_empty());
        let full = composite_full(&ts, &fx.ctx());
        if let Some((k, px, py, d)) = max_diff(cache.composite().unwrap(), &full) {
            panic!("dab at ({x},{y}): incremental differs from full in {k:?} at ({px},{py}) by {d}");
        }
    }
}

/// QA 2026-11: a paint layer in Passthrough with a filter (a filter layer) filters the stack
/// below it, but a dab on a layer below only refreshed the dab's own tile: the blurred result in
/// the neighbouring tile kept its old value.
#[test]
fn a_dab_under_a_passthrough_filter_layer_refreshes_every_tile_the_filter_reaches() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, N);
    let below = ts.insert_above(None, painted(&ts, "below", 4));
    let mut filter_layer = Layer::new_paint("Blur everything below", &ts);
    for b in filter_layer.blend.values_mut() {
        b.mode = subcraft_doc::BlendMode::Passthrough;
    }
    filter_layer.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter: Filter::Blur { radius: 8.0 } }, &ts)));
    ts.insert_above(Some(below), filter_layer);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));
    for (x, y) in [(63, 10), (64, 64), (10, 127), (200, 191)] {
        dab(&mut ts, below, x, y);
        cache.update(&ts, &fx.ctx());
        let full = composite_full(&ts, &fx.ctx());
        if let Some((k, px, py, d)) = max_diff(cache.composite().unwrap(), &full) {
            panic!("dab at ({x},{y}): incremental differs from full in {k:?} at ({px},{py}) by {d}");
        }
    }
}
