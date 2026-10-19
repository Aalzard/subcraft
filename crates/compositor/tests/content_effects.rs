//! Content effect stacks: sources, adjustments, order, coverage, dirty tiles and exact undo.

use std::collections::BTreeMap;
use std::sync::Arc;

use subcraft_compositor::{CompositeCache, EvalContext, TileContext, composite_full, tile::preview_content};
use subcraft_core::{ChannelKind, ChannelValue, IRect, LayerId};
use subcraft_doc::{
    BlendMode, ContentEffect, ContentEffectKind, FillChannel, FillContent, FillSource, Layer, LayerContent, Levels, Mask,
    MaskEffect, MaskEffectKind, Projection, ResourceStore, TextureSet, empty_paint_image,
};
use subcraft_image::TiledImage;
use subcraft_materials::{MeshMaps, SampleContext, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;
const R: ChannelKind = ChannelKind::Roughness;
const C: ChannelKind = ChannelKind::BaseColor;

struct Fixture {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fixture {
    fn plane() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

fn get(ts: &TextureSet, fx: &Fixture, kind: ChannelKind, x: u32, y: u32) -> [f32; 4] {
    composite_full(ts, &fx.ctx()).channel(kind).unwrap().get(x, y)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

fn rect(img: &mut TiledImage, kind: ChannelKind, r: IRect, value: [f32; 3], alpha: f32) {
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            let v = if kind.components() == 1 { [value[0], alpha, 0.0, 0.0] } else { [value[0], value[1], value[2], alpha] };
            img.set(x as u32, y as u32, v);
        }
    }
}

fn paint_effect(ts: &TextureSet, kind: ChannelKind, r: IRect, value: [f32; 3]) -> ContentEffect {
    let mut img = empty_paint_image(kind, RES);
    rect(&mut img, kind, r, value, 1.0);
    ContentEffect::new(ContentEffectKind::Paint { channels: BTreeMap::from([(kind, img)]) }, ts)
}

fn roughness_fill(v: f32) -> FillContent {
    FillContent { projection: Projection::default(), channels: BTreeMap::from([(R, FillChannel::new(FillSource::uniform(ChannelValue::scalar(v))))]) }
}

fn fill_layer(ts: &TextureSet, roughness: f32) -> Layer {
    let mut l = Layer::new_fill("Fill", ts);
    l.content = LayerContent::Fill(roughness_fill(roughness));
    l
}

fn levels(out_low: f32) -> Levels {
    Levels { out_low, ..Levels::default() }
}

/// Paint effect on a fill layer: painted texels show over the fill, the rest is the fill; the
/// layer's own mask still hides content + effects; a hidden effect is bit-identical to none.
#[test]
fn paint_effect_paints_over_a_fill_layer_and_respects_the_layer_mask() {
    let fx = Fixture::plane();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let id = ts.insert_above(None, fill_layer(&tc, 0.2));
    let reference = composite_full(&ts, &fx.ctx());
    let effect = paint_effect(&tc, R, IRect::new(8, 8, 24, 24), [0.9; 3]);
    let eid = effect.id;
    ts.find_mut(id).unwrap().content_effects.push(Arc::new(effect));
    assert!(close(get(&ts, &fx, R, 10, 10)[0], 0.9));
    assert!(close(get(&ts, &fx, R, 60, 60)[0], 0.2));

    // Half-opacity effect lerps.
    let l = ts.find_mut(id).unwrap();
    Arc::make_mut(&mut l.content_effects[0]).blend.get_mut(&R).unwrap().opacity = 0.5;
    assert!(close(get(&ts, &fx, R, 10, 10)[0], 0.55));

    // Black layer mask hides content and effect alike: the root default (0.3) shows.
    ts.find_mut(id).unwrap().mask = Some(Mask::new(RES, false));
    assert!(close(get(&ts, &fx, R, 10, 10)[0], 0.3));
    ts.find_mut(id).unwrap().mask = None;

    // Hidden effect: exactly the original result.
    let l = ts.find_mut(id).unwrap();
    let e = Arc::make_mut(&mut l.content_effects[0]);
    assert_eq!(e.id, eid);
    e.visible = false;
    assert_eq!(composite_full(&ts, &fx.ctx()), reference);
}

/// Levels changes values but never coverage: transparent paint stays transparent, other
/// channels are untouched, and Normal is never adjusted.
#[test]
fn levels_adjusts_one_channel_and_keeps_coverage() {
    let fx = Fixture::plane();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut paint = Layer::new_paint("P", &tc);
    let LayerContent::Paint(p) = &mut paint.content else { unreachable!() };
    rect(p.channel_mut(R, RES), R, IRect::new(0, 0, 32, 32), [0.4; 3], 1.0);
    rect(p.channel_mut(C, RES), C, IRect::new(0, 0, 32, 32), [0.1, 0.2, 0.3], 1.0);
    let id = ts.insert_above(None, paint);
    ts.find_mut(id).unwrap().content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Levels { channel: R, levels: levels(0.5) }, &tc)));
    // 0.4 → 0.5 + 0.4·0.5 = 0.7 inside; outside is transparent paint → root default 0.3.
    assert!(close(get(&ts, &fx, R, 5, 5)[0], 0.7));
    assert!(close(get(&ts, &fx, R, 90, 90)[0], 0.3));
    let c = get(&ts, &fx, C, 5, 5);
    assert!(close(c[0], 0.1) && close(c[1], 0.2) && close(c[2], 0.3), "{c:?}");
    // The mismatched channel is a no-op even on the normal channel.
    let n0 = composite_full(&ts, &fx.ctx()).channel(ChannelKind::Normal).cloned();
    ts.find_mut(id).unwrap().content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Levels { channel: ChannelKind::Normal, levels: levels(0.9) }, &tc)));
    assert_eq!(composite_full(&ts, &fx.ctx()).channel(ChannelKind::Normal).cloned(), n0);
}

#[test]
fn point_filters_match_gradient_goldens_across_tiles_and_preserve_paint_coverage() {
    use subcraft_doc::Filter;
    let fx = Fixture::plane(); let mut ts = TextureSet::new("T", 0, RES); let tc = ts.clone();
    let mut layer = Layer::new_paint("Filter input", &tc);
    let LayerContent::Paint(paint) = &mut layer.content else { unreachable!() };
    for y in 0..RES/2 { for x in 0..RES {
        paint.channel_mut(C, RES).set(x, y, [x as f32 / 128.0, 0.0, 0.0, 0.5]);
    } }
    let original = layer.content.clone(); let id = ts.insert_above(None, layer);
    for filter in [Filter::Contrast { gain: 2.0, pivot: 0.5 }, Filter::Hsl { hue: 120.0, saturation: 1.0, lightness: 0.0 },
        Filter::Threshold { cutoff: 0.1 }, Filter::Clamp { low: 0.1, high: 0.6 }] {
        ts.find_mut(id).unwrap().content_effects = vec![Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C, filter }, &tc))];
        let image = composite_full(&ts, &fx.ctx());
        for y in [10, 63, 64, 100] { for x in 0..RES {
            let v = x as f32 / 128.0;
            let transformed = match filter {
                Filter::Contrast { .. } => [(2.0*v-0.5).clamp(0.0,1.0), 0.0, 0.0],
                Filter::Hsl { .. } => [0.0, v, 0.0],
                Filter::Threshold { .. } => [if v*0.2126 >= 0.1 {1.0} else {0.0}; 3],
                Filter::Clamp { .. } => [v.clamp(0.1,0.6), 0.1, 0.1],
                Filter::Blur { .. } | Filter::Sharpen { .. } | Filter::DirectionalBlur {..} | Filter::Highpass {..} | Filter::Bevel {..} | Filter::MaskOutline {..} => unreachable!("neighborhood goldens are in spatial.rs"),
            };
            let actual = image.channel(C).unwrap().get(x,y);
            for k in 0..3 { let expected = if y < RES/2 { 0.25 + 0.5*transformed[k] } else { 0.5 };
                assert!(close(actual[k], expected), "{filter:?} at {x},{y}: {actual:?} expected {expected}"); }
            assert!(close(image.channel(R).unwrap().get(x,y)[0], 0.3));
        } }
        assert_eq!(ts.find(id).unwrap().content, original, "filters never rewrite the input raster");
    }
}

#[test]
fn mask_filters_consume_the_previous_stage_and_obey_order_opacity_and_disable() {
    use subcraft_doc::Filter;
    let fx = Fixture::plane(); let mut ts = TextureSet::new("T", 0, RES); let tc = ts.clone();
    let mut layer = fill_layer(&tc, 1.0); let mut mask = Mask::new(RES, false);
    mask.base.set(63, 10, [0.4; 4]); mask.base.set(64, 10, [0.6; 4]);
    let base = mask.base.clone();
    let mut contrast = MaskEffect::new(MaskEffectKind::Filter(Filter::Contrast { gain: 2.0, pivot: 0.5 }));
    contrast.opacity = 0.5; mask.effects.push(Arc::new(contrast));
    layer.mask = Some(mask); let id = ts.insert_above(None, layer);
    assert!(close(get(&ts, &fx, R, 63,10)[0], 0.3+0.7*0.35));
    assert!(close(get(&ts, &fx, R, 64,10)[0], 0.3+0.7*0.65));
    let effect = Arc::new(MaskEffect::new(MaskEffectKind::Filter(Filter::Clamp { low: 0.2, high: 0.6 })));
    let threshold = Arc::new(MaskEffect::new(MaskEffectKind::Filter(Filter::Threshold { cutoff: 0.65 })));
    ts.find_mut(id).unwrap().mask.as_mut().unwrap().effects = vec![effect.clone(), threshold.clone()];
    assert!(close(get(&ts,&fx,R,64,10)[0],0.3));
    ts.find_mut(id).unwrap().mask.as_mut().unwrap().effects = vec![threshold, effect];
    assert!(close(get(&ts,&fx,R,64,10)[0],0.3+0.7*0.2));
    for e in &mut ts.find_mut(id).unwrap().mask.as_mut().unwrap().effects { Arc::make_mut(e).visible=false; }
    assert!(close(get(&ts,&fx,R,64,10)[0],0.3+0.7*0.6));
    assert_eq!(ts.find(id).unwrap().mask.as_ref().unwrap().base, base);
}

#[test]
fn folder_filters_adjust_isolated_children_or_passthrough_accumulation_before_opacity() {
    use subcraft_doc::Filter;
    let fx = Fixture::plane(); let mut ts = TextureSet::new("T", 0, RES); let tc = ts.clone();
    ts.insert_above(None, fill_layer(&tc,0.2));
    let mut folder = Layer::new_folder("Adjustment", &tc);
    folder.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel:R,
        filter:Filter::Contrast {gain:2.0,pivot:0.5} }, &tc)));
    folder.blend.get_mut(&R).unwrap().opacity=0.5;
    let id = ts.insert_above(None,folder);
    assert!(close(get(&ts,&fx,R,10,10)[0],0.1), "empty passthrough folder adjusts the accumulation below");
    ts.find_mut(id).unwrap().blend.get_mut(&R).unwrap().mode=BlendMode::Normal;
    ts.insert(Some(id),0,fill_layer(&tc,0.4)).unwrap();
    assert!(close(get(&ts,&fx,R,10,10)[0],0.25), "isolated child 0.4 -> 0.3, mixed halfway over 0.2");
}

/// Stack order is bottom to top: Fill then Levels differs from Levels then Fill.
#[test]
fn effect_order_is_bottom_to_top() {
    let fx = Fixture::plane();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let id = ts.insert_above(None, fill_layer(&tc, 0.2));
    let fill = Arc::new(ContentEffect::new(ContentEffectKind::Fill(roughness_fill(0.6)), &tc));
    let lv = Arc::new(ContentEffect::new(ContentEffectKind::Levels { channel: R, levels: levels(0.5) }, &tc));
    ts.find_mut(id).unwrap().content_effects = vec![fill.clone(), lv.clone()];
    assert!(close(get(&ts, &fx, R, 40, 40)[0], 0.8)); // levels(0.6)
    ts.find_mut(id).unwrap().content_effects = vec![lv, fill];
    assert!(close(get(&ts, &fx, R, 40, 40)[0], 0.6)); // fill covers levels(0.2)
}

/// Painting into a content or mask Paint effect refreshes only touched tiles; parameter changes
/// refresh everything; returning to an earlier document is bit-exact.
#[test]
fn effect_rasters_are_diffed_per_tile_and_undo_is_exact() {
    let fx = Fixture::plane();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let id = ts.insert_above(None, fill_layer(&tc, 0.2));
    let mut mask = Mask::new(RES, true);
    let mut mask_paint = TiledImage::new(RES, RES, 2, [0.0; 4]);
    mask_paint.set(1, 1, [0.0, 1.0, 0.0, 0.0]);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Paint { image: mask_paint })));
    let l = ts.find_mut(id).unwrap();
    l.mask = Some(mask);
    l.content_effects.push(Arc::new(paint_effect(&tc, R, IRect::new(0, 0, 1, 1), [0.9; 3])));
    l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: R,
        filter: subcraft_doc::Filter::Threshold { cutoff: 0.5 } }, &tc)));
    l.mask.as_mut().unwrap().effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(
        subcraft_doc::Filter::Clamp { low: 0.0, high: 1.0 }))));
    let mut cache = CompositeCache::new();
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![0, 1, 2, 3]);
    let before = ts.clone();
    let before_composite = cache.composite().unwrap().clone();

    // Content Paint effect: one tile.
    let l = ts.find_mut(id).unwrap();
    let ContentEffectKind::Paint { channels } = &mut Arc::make_mut(&mut l.content_effects[0]).kind else { unreachable!() };
    rect(channels.get_mut(&R).unwrap(), R, IRect::new(70, 70, 72, 72), [0.9; 3], 1.0);
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![3]);
    assert!(close(cache.composite().unwrap().channel(R).unwrap().get(71, 71)[0], 1.0));

    // First stroke into another channel of the effect is still incremental.
    let l = ts.find_mut(id).unwrap();
    let e = Arc::make_mut(&mut l.content_effects[0]);
    e.kind.paint_channel_mut(C, RES).unwrap().set(66, 3, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![1]);

    // Mask Paint effect: one tile (black dot hides the layer there).
    let m = ts.find_mut(id).unwrap().mask.as_mut().unwrap();
    let MaskEffectKind::Paint { image } = &mut Arc::make_mut(&mut m.effects[0]).kind else { unreachable!() };
    image.set(5, 70, [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![2]);
    assert!(close(cache.composite().unwrap().channel(R).unwrap().get(5, 70)[0], 0.3));

    // A parameter edit dirties every tile; renaming an effect dirties none.
    Arc::make_mut(&mut ts.find_mut(id).unwrap().content_effects[0]).name = "Renamed".into();
    assert!(cache.update(&ts, &fx.ctx()).is_empty());
    Arc::make_mut(&mut ts.find_mut(id).unwrap().content_effects[0]).blend.get_mut(&R).unwrap().mode = BlendMode::Multiply;
    assert_eq!(cache.update(&ts, &fx.ctx()).len(), 4);
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));

    // "Undo" to the earlier document: bit-identical composite.
    cache.update(&before, &fx.ctx());
    assert_eq!(cache.composite().unwrap(), &before_composite);
}

/// Content thumbnails include the content stack (before the layer's own mask).
#[test]
fn content_preview_includes_effects() {
    let fx = Fixture::plane();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let id: LayerId = ts.insert_above(None, fill_layer(&tc, 0.2));
    let l = ts.find_mut(id).unwrap();
    l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Levels { channel: R, levels: levels(0.5) }, &tc)));
    l.mask = Some(Mask::new(RES, false));
    let layer = ts.find(id).unwrap().clone();
    let ctx = fx.ctx();
    let geo = ctx.geometry.tile(0);
    let tile = TileContext { albedo_filter: ts.albedo_filter, geo: &geo, set_geometry: ctx.geometry, sample: SampleContext { resources: ctx.resources, mesh_maps: ctx.mesh_maps } };
    let px = preview_content(&layer, R, &tile);
    assert!(px.iter().all(|p| close(p[0], 0.6) && close(p[3], 1.0)));
}
