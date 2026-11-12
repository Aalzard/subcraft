//! Transparent texture sets (issue #145): the layer stack starts at alpha 0 instead of the opaque
//! channel default, so unpainted texels stay transparent. Numeric checks of the composite alpha
//! across layers and blend modes, of opaque sets staying as they were, and of the cache
//! re-evaluating when a set switches between the two.

use std::sync::Arc;

use subcraft_compositor::{CompositeCache, EvalContext, composite_full};
use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, ChannelBlend, ChannelDef, Layer, LayerContent, ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;
const C: ChannelKind = ChannelKind::BaseColor;
const O: ChannelKind = ChannelKind::Opacity;

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

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 2e-3)
}

fn set(transparent: bool) -> TextureSet {
    let mut ts = TextureSet::new("T", 0, RES);
    ts.channels.retain(|c| matches!(c.kind, ChannelKind::BaseColor | ChannelKind::Roughness));
    ts.channels.push(ChannelDef::default_for(O));
    ts.transparent = transparent;
    ts
}

/// A paint layer with `rgba` Base Color on the left half (x < 64).
fn paint(ts: &TextureSet, name: &str, rgba: [f32; 4], mode: BlendMode) -> Layer {
    let mut l = Layer::new_paint(name, ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let img = p.channel_mut(C, RES);
    for y in 0..RES {
        for x in 0..64 {
            img.set(x, y, rgba);
        }
    }
    l.blend.insert(C, ChannelBlend::new(mode));
    l
}

#[test]
fn opaque_sets_start_at_the_default_with_alpha_one() {
    let fx = Fixture::plane();
    let c = composite_full(&set(false), &fx.ctx());
    let d = C.default_value().0;
    assert!(close(c.channel(C).unwrap().get(100, 10), [d[0], d[1], d[2], 1.0]));
    assert_eq!(c.channel(C).unwrap().fill()[3], 1.0);
}

#[test]
fn transparent_sets_start_empty_and_show_only_what_was_painted() {
    let fx = Fixture::plane();
    let mut ts = set(true);
    let c = composite_full(&ts, &fx.ctx());
    let base = c.channel(C).unwrap();
    assert_eq!(base.get(100, 10)[3], 0.0, "unpainted: alpha 0");
    assert_eq!(base.fill()[3], 0.0, "unevaluated tiles read transparent too");
    // An opaque stroke: its colour with alpha 1; elsewhere still transparent.
    let t = ts.clone();
    ts.insert_above(None, paint(&t, "Red", [1.0, 0.0, 0.0, 1.0], BlendMode::Normal));
    let c = composite_full(&ts, &fx.ctx());
    assert!(close(c.channel(C).unwrap().get(10, 10), [1.0, 0.0, 0.0, 1.0]));
    assert_eq!(c.channel(C).unwrap().get(100, 10)[3], 0.0);
    // The same set opaque: the stroke covers the default grey, the rest is grey at alpha 1.
    ts.transparent = false;
    let c = composite_full(&ts, &fx.ctx());
    assert!(close(c.channel(C).unwrap().get(10, 10), [1.0, 0.0, 0.0, 1.0]));
    assert_eq!(c.channel(C).unwrap().get(100, 10)[3], 1.0);
}

#[test]
fn half_covered_layers_union_their_alpha_with_straight_colour() {
    let fx = Fixture::plane();
    let mut ts = set(true);
    let t = ts.clone();
    let red = ts.insert_above(None, paint(&t, "Red", [1.0, 0.0, 0.0, 0.5], BlendMode::Normal));
    let c = composite_full(&ts, &fx.ctx());
    assert!(close(c.channel(C).unwrap().get(10, 10), [1.0, 0.0, 0.0, 0.5]), "straight colour over nothing keeps its hue");
    ts.insert_above(Some(red), paint(&t, "Blue", [0.0, 0.0, 1.0, 0.5], BlendMode::Normal));
    let c = composite_full(&ts, &fx.ctx());
    // a = 0.5 + 0.5 (1 - 0.5) = 0.75; red weight 0.25, blue 0.5.
    assert!(close(c.channel(C).unwrap().get(10, 10), [1.0 / 3.0, 0.0, 2.0 / 3.0, 0.75]), "{:?}", c.channel(C).unwrap().get(10, 10));
}

#[test]
fn blend_modes_act_on_covered_texels_and_pass_through_over_empty_ones() {
    let fx = Fixture::plane();
    for (mode, over_red) in [
        (BlendMode::Multiply, [0.5, 0.25, 0.25, 1.0]),
        (BlendMode::Screen, [0.75, 0.5, 0.5, 1.0]),
    ] {
        let mut ts = set(true);
        let t = ts.clone();
        let red = ts.insert_above(None, paint(&t, "Red", [1.0, 0.0, 0.0, 0.5], BlendMode::Normal));
        // A grey layer everywhere (also over the empty right half).
        let mut grey = Layer::new_paint("Grey", &t);
        let LayerContent::Paint(p) = &mut grey.content else { unreachable!() };
        let img = p.channel_mut(C, RES);
        for y in 0..RES {
            for x in 0..RES {
                img.set(x, y, [0.5, 0.5, 0.5, 1.0]);
            }
        }
        grey.blend.insert(C, ChannelBlend::new(mode));
        ts.insert_above(Some(red), grey);
        let c = composite_full(&ts, &fx.ctx());
        let img = c.channel(C).unwrap();
        // Over half-covered red: W3C compositing, (1 - ab) cs + ab B(cb, cs), then source-over.
        assert!(close(img.get(10, 10), over_red), "{mode:?}: {:?}", img.get(10, 10));
        // Over nothing the blend function has no backdrop: the grey itself, fully covering.
        assert!(close(img.get(100, 10), [0.5, 0.5, 0.5, 1.0]), "{mode:?}: {:?}", img.get(100, 10));
    }
}

#[test]
fn opacity_is_paintable_like_any_channel() {
    let fx = Fixture::plane();
    let mut ts = set(true);
    let t = ts.clone();
    let mut l = Layer::new_paint("Holes", &t);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let img = p.channel_mut(O, RES);
    for y in 0..RES {
        for x in 0..64 {
            img.set(x, y, [0.25, 1.0, 0.0, 0.0]);
        }
    }
    ts.insert_above(None, l);
    let c = composite_full(&ts, &fx.ctx());
    let o = c.channel(O).unwrap();
    assert_eq!(o.comps(), 1, "scalar channels store the value only");
    assert!((o.get(10, 10)[0] - 0.25).abs() < 2e-3);
    assert!((o.get(100, 10)[0] - 1.0).abs() < 2e-3, "unpainted opacity reads its default 1");
}

#[test]
fn the_cache_re_evaluates_everything_when_a_set_becomes_transparent() {
    let fx = Fixture::plane();
    let mut ts = set(false);
    let t = ts.clone();
    ts.insert_above(None, paint(&t, "Red", [1.0, 0.0, 0.0, 1.0], BlendMode::Normal));
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    assert_eq!(cache.composite().unwrap().channel(C).unwrap().get(100, 10)[3], 1.0);
    ts.transparent = true;
    assert!(!cache.stale_tiles(&ts).is_empty());
    cache.update(&ts, &fx.ctx());
    let img = cache.composite().unwrap().channel(C).unwrap();
    assert_eq!(img.get(100, 10)[3], 0.0);
    assert!(close(img.get(10, 10), [1.0, 0.0, 0.0, 1.0]));
    assert_eq!(*cache.composite().unwrap(), composite_full(&ts, &fx.ctx()), "same as a fresh evaluation");
}
