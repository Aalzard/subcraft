//! Generator "micro effect": the generator mask also writes Normal / Height of the stack below.

use std::sync::Arc;

use subcraft_compositor::{Composite, CompositeCache, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{
    FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, MicroDirection,
    MicroEffect, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

struct Fx {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fx {
    fn cube() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

fn fill(ts: &TextureSet, values: &[(ChannelKind, ChannelValue)]) -> Layer {
    let mut l = Layer::new_fill("Fill", ts);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = false;
        if let Some((_, v)) = values.iter().find(|(kk, _)| kk == k) {
            *ch = FillChannel::new(FillSource::uniform(*v));
        }
    }
    l
}

/// Top faces of the cube: mask 1, everything else 0 (Directional +Y, no breakup, hard-ish edge).
fn top_faces(micro: Option<MicroEffect>) -> Generator {
    let mut g = Generator::new(GeneratorKind::Directional);
    g.level = 0.75;
    g.softness = 0.1;
    g.noise_amount = 0.0;
    g.micro = micro;
    g
}

/// base layer (given channels) + a colour-only layer masked by `g`.
fn scene(base: &[(ChannelKind, ChannelValue)], g: Generator) -> TextureSet {
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    if !base.is_empty() {
        ts.insert_above(None, fill(&tc, base));
    }
    let mut dirt = fill(&tc, &[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    let mut mask = Mask::new(RES, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    dirt.mask = Some(mask);
    ts.insert_above(None, dirt);
    ts
}

fn tilted() -> ChannelValue {
    ChannelValue::rgb(0.65, 0.4, 0.9664)
}

fn both() -> Vec<(ChannelKind, ChannelValue)> {
    vec![(ChannelKind::Normal, tilted()), (ChannelKind::Height, ChannelValue::scalar(0.2))]
}

fn on() -> MicroEffect {
    MicroEffect { enabled: true, amount: 0.8, direction: MicroDirection::Carve, grain_follows: true }
}

fn bits(c: &Composite, kind: ChannelKind) -> Vec<[f32; 4]> {
    let img = c.channel(kind).unwrap();
    (0..RES).flat_map(|y| (0..RES).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)).collect()
}

fn idx(x: u32, y: u32) -> usize {
    (y * RES + x) as usize
}

fn red(c: &Composite, x: u32, y: u32) -> bool {
    c.channel(ChannelKind::BaseColor).unwrap().get(x, y)[1] < 0.25
}

#[test]
fn off_is_bit_identical_to_a_generator_without_micro() {
    let fx = Fx::cube();
    let plain = composite_full(&scene(&both(), top_faces(None)), &fx.ctx());
    let off = MicroEffect { enabled: false, ..on() };
    let disabled = composite_full(&scene(&both(), top_faces(Some(off))), &fx.ctx());
    for kind in [ChannelKind::BaseColor, ChannelKind::Normal, ChannelKind::Height, ChannelKind::Roughness] {
        assert_eq!(bits(&plain, kind), bits(&disabled, kind), "{kind:?}");
    }
    // Amount 0 is also a no-op.
    let zero = composite_full(&scene(&both(), top_faces(Some(MicroEffect { amount: 0.0, ..on() }))), &fx.ctx());
    assert_eq!(bits(&plain, ChannelKind::Height), bits(&zero, ChannelKind::Height));
}

#[test]
fn on_changes_height_and_normal_only_where_the_mask_is() {
    let fx = Fx::cube();
    let off = composite_full(&scene(&both(), top_faces(None)), &fx.ctx());
    let ons = composite_full(&scene(&both(), top_faces(Some(on()))), &fx.ctx());
    assert_eq!(bits(&off, ChannelKind::BaseColor), bits(&ons, ChannelKind::BaseColor), "colour untouched");
    let (ho, hn) = (bits(&off, ChannelKind::Height), bits(&ons, ChannelKind::Height));
    let (no, nn) = (bits(&off, ChannelKind::Normal), bits(&ons, ChannelKind::Normal));
    let (mut lowered, mut dirty_normals) = (0, 0);
    for y in 0..RES {
        for x in 0..RES {
            let i = idx(x, y);
            if red(&off, x, y) {
                // Carve: lower by 0.7..1 of amount * MAX_HEIGHT (f16 storage tolerance).
                let d = hn[i][0] - ho[i][0];
                assert!(d < -0.8 * 0.1 * 0.7 + 2e-3 && d > -0.8 * 0.1 - 2e-3, "height delta {d} at ({x},{y})");
                lowered += 1;
            } else if !(0..9).any(|k| {
                // Not on the mask edge ring either: the normal reads one texel around.
                let (nx, ny) = ((x + k % 3).saturating_sub(1).min(RES - 1), (y + k / 3).saturating_sub(1).min(RES - 1));
                red(&off, nx, ny)
            }) {
                assert_eq!(ho[i], hn[i], "height outside the mask at ({x},{y})");
                assert_eq!(no[i], nn[i], "normal outside the mask at ({x},{y})");
            }
            if no[i] != nn[i] {
                dirty_normals += 1;
            }
        }
    }
    assert!(lowered > 500, "mask covers texels: {lowered}");
    assert!(dirty_normals > 20, "normals follow the height edge: {dirty_normals}");
}

#[test]
fn raise_goes_the_other_way_and_scales_with_amount() {
    let fx = Fx::cube();
    let h = |m: MicroEffect| bits(&composite_full(&scene(&both(), top_faces(Some(m))), &fx.ctx()), ChannelKind::Height);
    let (base, up, half) = (h(MicroEffect { amount: 0.0, ..on() }), h(MicroEffect { direction: MicroDirection::Raise, ..on() }), h(MicroEffect { direction: MicroDirection::Raise, amount: 0.4, ..on() }));
    let mut hit = 0;
    for i in 0..base.len() {
        if up[i][0] != base[i][0] {
            assert!(up[i][0] > base[i][0]);
            assert!((half[i][0] - base[i][0]) * 2.0 - (up[i][0] - base[i][0]) < 3e-3);
            hit += 1;
        }
    }
    assert!(hit > 500);
}

#[test]
fn normals_stay_unit_length_and_a_flat_base_stays_flat_outside() {
    let fx = Fx::cube();
    let flat = vec![(ChannelKind::Normal, ChannelValue::rgb(0.5, 0.5, 1.0)), (ChannelKind::Height, ChannelValue::scalar(0.0))];
    for base in [both(), flat.clone()] {
        let c = composite_full(&scene(&base, top_faces(Some(on()))), &fx.ctx());
        let mut changed = 0;
        for p in bits(&c, ChannelKind::Normal) {
            let n = [p[0] * 2.0 - 1.0, p[1] * 2.0 - 1.0, p[2] * 2.0 - 1.0];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 0.02, "length {len}");
            if (p[0] - 0.5).abs() > 1e-3 || (p[1] - 0.5).abs() > 1e-3 {
                changed += 1;
            }
        }
        if base == flat {
            assert!(changed > 20, "a flat map gets the micro slope: {changed}");
        }
    }
    // Flat base: every texel whose neighbourhood has no mask stays exactly flat.
    let off = composite_full(&scene(&flat, top_faces(None)), &fx.ctx());
    let ons = composite_full(&scene(&flat, top_faces(Some(on()))), &fx.ctx());
    let (a, b) = (bits(&off, ChannelKind::Normal), bits(&ons, ChannelKind::Normal));
    for y in 4..RES - 4 {
        for x in 4..RES - 4 {
            let near = (0..9).any(|k| red(&off, x + k % 3 - 1, y + k / 3 - 1));
            if !near {
                assert_eq!(a[idx(x, y)], b[idx(x, y)]);
            }
        }
    }
}

#[test]
fn a_stack_without_normal_or_height_gets_nothing() {
    let fx = Fx::cube();
    let colour_only = |m| composite_full(&scene(&[], top_faces(m)), &fx.ctx());
    let (off, ons) = (colour_only(None), colour_only(Some(on())));
    for kind in [ChannelKind::Normal, ChannelKind::Height] {
        assert_eq!(bits(&off, kind), bits(&ons, kind), "{kind:?}");
    }
    let flat = bits(&ons, ChannelKind::Height);
    assert!(flat.iter().all(|p| p[0] == 0.0), "no height channel content was created");
}

#[test]
fn normal_only_stack_changes_only_the_normal() {
    let fx = Fx::cube();
    let only_normal = vec![(ChannelKind::Normal, tilted())];
    let off = composite_full(&scene(&only_normal, top_faces(None)), &fx.ctx());
    let ons = composite_full(&scene(&only_normal, top_faces(Some(on()))), &fx.ctx());
    assert_eq!(bits(&off, ChannelKind::Height), bits(&ons, ChannelKind::Height));
    assert_ne!(bits(&off, ChannelKind::Normal), bits(&ons, ChannelKind::Normal));
}

#[test]
fn deterministic_and_the_incremental_cache_agrees() {
    let fx = Fx::cube();
    let ts = scene(&both(), top_faces(Some(on())));
    let a = composite_full(&ts, &fx.ctx());
    let b = composite_full(&ts, &fx.ctx());
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    for kind in [ChannelKind::Height, ChannelKind::Normal, ChannelKind::BaseColor] {
        assert_eq!(bits(&a, kind), bits(&b, kind), "{kind:?} deterministic");
        assert_eq!(bits(&a, kind), bits(cache.composite().unwrap(), kind), "{kind:?} cache");
    }
}

#[test]
fn grain_toggle_changes_the_micro_surface_not_the_mask() {
    let fx = Fx::cube();
    let smooth = composite_full(&scene(&both(), top_faces(Some(MicroEffect { grain_follows: false, ..on() }))), &fx.ctx());
    let grainy = composite_full(&scene(&both(), top_faces(Some(on()))), &fx.ctx());
    assert_eq!(bits(&smooth, ChannelKind::BaseColor), bits(&grainy, ChannelKind::BaseColor));
    assert_ne!(bits(&smooth, ChannelKind::Height), bits(&grainy, ChannelKind::Height));
    // Smooth: the full amount on the flat part of the mask.
    let h = bits(&smooth, ChannelKind::Height);
    assert!(h.iter().any(|p| (p[0] - (0.2 - 0.08)).abs() < 2e-3));
}
