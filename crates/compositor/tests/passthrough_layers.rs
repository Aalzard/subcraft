//! Painter parity: Passthrough paint / fill layers (their effects process the stack below) and
//! filters that work on several channels.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{
    BlendMode, ChannelBlend, ContentEffect, ContentEffectKind, FillChannel, FillSource, Filter, Layer, LayerContent, Mask,
    ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

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

fn texture_set() -> TextureSet {
    let mut ts = TextureSet::new("T", 0, RES);
    // Roughness and Metallic are in the default set; make sure of it.
    for kind in [ChannelKind::Roughness, ChannelKind::Metallic] {
        if !ts.has_channel(kind) {
            ts.channels.push(subcraft_doc::ChannelDef::default_for(kind));
        }
    }
    ts
}

/// A fill layer setting these values (other channels off), blending Normal.
fn fill(ts: &TextureSet, values: &[(ChannelKind, ChannelValue)]) -> Layer {
    let mut l = Layer::new_fill("Fill", ts);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = false;
        if let Some((_, v)) = values.iter().find(|(kk, _)| kk == k) {
            *ch = FillChannel::new(FillSource::uniform(*v));
        }
    }
    for b in l.blend.values_mut() {
        *b = ChannelBlend::new(BlendMode::Normal);
    }
    l
}

fn filter_effect(ts: &TextureSet, channel: ChannelKind, extra: &[ChannelKind], filter: Filter) -> Arc<ContentEffect> {
    let mut e = ContentEffect::new(ContentEffectKind::Filter { channel, filter }, ts);
    e.extra_channels = extra.iter().copied().collect();
    Arc::new(e)
}

fn passthrough(mut l: Layer) -> Layer {
    for b in l.blend.values_mut() {
        b.mode = BlendMode::Passthrough;
    }
    l
}

fn px(ts: &TextureSet, fx: &Fixture, kind: ChannelKind, x: u32, y: u32) -> [f32; 4] {
    composite_full(ts, &fx.ctx()).channel(kind).unwrap().get(x, y)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

#[test]
fn a_passthrough_fill_layer_filters_the_stack_below_and_ignores_its_own_fill() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    ts.insert_above(None, fill(&tc, &[(ChannelKind::BaseColor, ChannelValue::rgb(0.2, 0.4, 0.6)), (ChannelKind::Roughness, ChannelValue::scalar(0.3))]));
    // Its own fill (pure red) must not show: in Passthrough the stack below is its content.
    let mut top = passthrough(fill(&tc, &[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]));
    top.content_effects.push(filter_effect(&tc, ChannelKind::BaseColor, &[], Filter::Invert));
    let top = ts.insert_above(None, top);
    let p = px(&ts, &fx, ChannelKind::BaseColor, 64, 64);
    assert!(close(p[0], 0.8) && close(p[1], 0.6) && close(p[2], 0.4), "{p:?}");
    assert!(close(px(&ts, &fx, ChannelKind::Roughness, 64, 64)[0], 0.3), "the filter's other channels are untouched");

    // Opacity mixes the filtered stack with the unfiltered one.
    ts.find_mut(top).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = 0.5;
    let p = px(&ts, &fx, ChannelKind::BaseColor, 64, 64);
    assert!(close(p[0], 0.5) && close(p[1], 0.5) && close(p[2], 0.5), "{p:?}");

    // A black mask hides the effect entirely.
    ts.find_mut(top).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().opacity = 1.0;
    ts.find_mut(top).unwrap().mask = Some(Mask::new(RES, false));
    let p = px(&ts, &fx, ChannelKind::BaseColor, 64, 64);
    assert!(close(p[0], 0.2) && close(p[1], 0.4) && close(p[2], 0.6), "{p:?}");

    // Without effects a passthrough layer changes nothing at all.
    ts.find_mut(top).unwrap().mask = None;
    ts.find_mut(top).unwrap().content_effects.clear();
    let p = px(&ts, &fx, ChannelKind::BaseColor, 64, 64);
    assert!(close(p[0], 0.2) && close(p[1], 0.4) && close(p[2], 0.6), "{p:?}");
}

/// A passthrough paint layer and an empty passthrough folder with the same effects give the
/// same result, for a per-texel filter (tile path) and a blur (stage graph).
#[test]
fn passthrough_paint_layers_match_an_empty_passthrough_folder_for_local_and_spatial_filters() {
    let fx = Fixture::plane();
    for filter in [Filter::Invert, Filter::ALL.into_iter().find(|f| matches!(f, Filter::Blur { .. })).unwrap()] {
        let build = |folder: bool| {
            let mut ts = texture_set();
            let tc = ts.clone();
            let mut base = fill(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(0.1))]);
            // Some structure for the blur to soften.
            let LayerContent::Fill(f) = &mut base.content else { unreachable!() };
            f.channels.insert(
                ChannelKind::Roughness,
                FillChannel::new(FillSource::Procedural {
                    params: subcraft_doc::ProceduralParams::new(subcraft_doc::ProceduralKind::Checker),
                    low: ChannelValue::scalar(0.1),
                    high: ChannelValue::scalar(0.9),
                }),
            );
            ts.insert_above(None, base);
            let mut l = if folder { Layer::new_folder("F", &tc) } else { Layer::new_paint("P", &tc) };
            l = passthrough(l);
            l.content_effects.push(filter_effect(&tc, ChannelKind::Roughness, &[], filter));
            ts.insert_above(None, l);
            ts
        };
        let (a, b) = (build(false), build(true));
        let (ca, cb) = (composite_full(&a, &fx.ctx()), composite_full(&b, &fx.ctx()));
        let (ra, rb) = (ca.channel(ChannelKind::Roughness).unwrap(), cb.channel(ChannelKind::Roughness).unwrap());
        for (x, y) in [(3, 3), (40, 17), (64, 64), (100, 90), (127, 0)] {
            let (pa, pb) = (ra.get(x, y), rb.get(x, y));
            assert!(close(pa[0], pb[0]), "{} at ({x},{y}): layer {pa:?} folder {pb:?}", filter.label());
        }
        // And the filter really ran (not just both unfiltered).
        let unfiltered = {
            let mut ts = a.clone();
            ts.layers.pop();
            composite_full(&ts, &fx.ctx()).channel(ChannelKind::Roughness).unwrap().get(64, 64)
        };
        assert!((unfiltered[0] - ra.get(64, 64)[0]).abs() > 1e-3 || filter != Filter::Invert, "{}", filter.label());
    }
}

#[test]
fn a_filter_processes_its_extra_channels_and_skips_the_ones_it_cannot() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    let mut l = fill(
        &tc,
        &[
            (ChannelKind::BaseColor, ChannelValue::rgb(0.2, 0.4, 0.6)),
            (ChannelKind::Roughness, ChannelValue::scalar(0.3)),
            (ChannelKind::Metallic, ChannelValue::scalar(0.25)),
        ],
    );
    l.content_effects.push(filter_effect(&tc, ChannelKind::BaseColor, &[ChannelKind::Roughness], Filter::Invert));
    let id = ts.insert_above(None, l);
    let base = px(&ts, &fx, ChannelKind::BaseColor, 64, 64);
    assert!(close(base[0], 0.8) && close(base[2], 0.4), "{base:?}");
    assert!(close(px(&ts, &fx, ChannelKind::Roughness, 64, 64)[0], 0.7), "extra channel inverted");
    assert!(close(px(&ts, &fx, ChannelKind::Metallic, 64, 64)[0], 0.25), "channels that are not toggled on stay");

    // A colour-only filter ignores a grey extra channel instead of failing.
    let e = Arc::make_mut(&mut ts.find_mut(id).unwrap().content_effects[0]);
    e.kind = ContentEffectKind::Filter { channel: ChannelKind::BaseColor, filter: Filter::Grayscale };
    assert!(close(px(&ts, &fx, ChannelKind::Roughness, 64, 64)[0], 0.3));
    let e = &ts.find(id).unwrap().content_effects[0];
    assert_eq!(e.filter_channels(), [ChannelKind::BaseColor]);
    assert!(e.applies_to(ChannelKind::BaseColor) && !e.applies_to(ChannelKind::Roughness));
}
