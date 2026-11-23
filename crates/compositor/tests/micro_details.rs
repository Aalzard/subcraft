//! Generator micro details (Painter's Use Micro Details): Dirt / Edge wear / AO / curvature read
//! the Normal and Height of the layers below as extra occlusion and curvature.

use std::sync::Arc;

use subcraft_compositor::{Composite, CompositeCache, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{
    Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, MicroDetails, ResourceStore, TextureSet,
};
use subcraft_image::TiledImage;
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

/// Micro details scale with the resolution (a fixed UV feature spans more texels at 4K), so the
/// test texture is large enough for the default radius to reach a few texels.
const RES: u32 = 512;
/// The groove: texel columns GROOVE.0..GROOVE.1 sit lower than the rest.
const GROOVE: (u32, u32) = (254, 257);
/// The ridge: texel rows RIDGE.0..RIDGE.1 sit higher than the rest.
const RIDGE: (u32, u32) = (120, 125);

struct Fx {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fx {
    fn plane() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 1)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

/// A paint layer holding a Height map: 0 everywhere, -0.2 in the groove, +0.2 on the ridge.
fn relief(ts: &TextureSet) -> Layer {
    let mut l = Layer::new_paint("Relief", ts);
    let mut h = TiledImage::new(RES, RES, 2, [0.0, 1.0, 0.0, 0.0]);
    for y in 0..RES {
        for x in 0..RES {
            let v = if (GROOVE.0..GROOVE.1).contains(&x) { -0.2 } else if (RIDGE.0..RIDGE.1).contains(&y) { 0.2 } else { 0.0 };
            h.set(x, y, [v, 1.0, 0.0, 0.0]);
        }
    }
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    p.channels.insert(ChannelKind::Height, h);
    l
}

/// A colour-only layer (pure red) masked by `g`.
fn masked(ts: &TextureSet, g: Generator) -> Layer {
    let mut l = Layer::new_fill("Masked", ts);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = *k == ChannelKind::BaseColor;
        if *k == ChannelKind::BaseColor {
            *ch = subcraft_doc::FillChannel::new(subcraft_doc::FillSource::uniform(ChannelValue::rgb(1.0, 0.0, 0.0)));
        }
    }
    let mut mask = Mask::new(RES, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    l.mask = Some(mask);
    l
}

/// Generator of `kind` without breakup, so the mask is a pure function of the maps it reads.
fn generator(kind: GeneratorKind, details: Option<MicroDetails>) -> Generator {
    let mut g = Generator::new(kind);
    g.noise_amount = 0.0;
    if let Some(p) = g.painter.as_mut() {
        p.grunge_floor = 0.0;
        p.grain = 0.0;
    }
    g.micro_details = details;
    g
}

fn on() -> MicroDetails {
    MicroDetails { enabled: true, ..MicroDetails::default() }
}

/// relief below + the generator layer on top.
fn scene(g: Generator) -> TextureSet {
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    ts.insert_above(None, relief(&tc));
    ts.insert_above(None, masked(&tc, g));
    ts
}

/// The mask of the red layer, read from the composited base colour (red over the relief's grey).
fn mask_of(c: &Composite) -> Vec<f32> {
    let img = c.channel(ChannelKind::BaseColor).unwrap();
    let base = ChannelKind::BaseColor.default_value().0;
    (0..RES).flat_map(|y| (0..RES).map(move |x| (x, y))).map(|(x, y)| {
        let g = img.get(x, y)[1];
        // green goes from the base grey (mask 0) to 0 (mask 1).
        1.0 - g / base[1]
    }).collect()
}

fn bits(c: &Composite, kind: ChannelKind) -> Vec<[f32; 4]> {
    let img = c.channel(kind).unwrap();
    (0..RES).flat_map(|y| (0..RES).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)).collect()
}

fn at(x: u32, y: u32) -> usize {
    (y * RES + x) as usize
}

#[test]
fn off_and_zero_amount_are_bit_identical_to_no_micro_details() {
    let fx = Fx::plane();
    for kind in [GeneratorKind::Cavity, GeneratorKind::Edges, GeneratorKind::AmbientOcclusion] {
        let plain = composite_full(&scene(generator(kind, None)), &fx.ctx());
        for d in [MicroDetails { enabled: false, ..on() }, MicroDetails { amount: 0.0, ..on() }, MicroDetails { micro_normal: false, micro_height: false, ..on() }] {
            let other = composite_full(&scene(generator(kind, Some(d))), &fx.ctx());
            for ch in [ChannelKind::BaseColor, ChannelKind::Height, ChannelKind::Normal] {
                assert_eq!(bits(&plain, ch), bits(&other, ch), "{kind:?} {d:?} {ch:?}");
            }
        }
    }
}

#[test]
fn dirt_settles_in_the_groove_of_the_height_below() {
    let fx = Fx::plane();
    let off = mask_of(&composite_full(&scene(generator(GeneratorKind::Cavity, None)), &fx.ctx()));
    let on = mask_of(&composite_full(&scene(generator(GeneratorKind::Cavity, Some(on()))), &fx.ctx()));
    // Without baked maps (neutral AO and curvature) and without breakup the dirt is uniform.
    let first = off[0];
    assert!(off.iter().all(|v| (v - first).abs() < 1e-3), "flat without micro details");
    // Inside the groove (its middle, away from the ridge rows) the dirt is far stronger.
    let groove = on[at((GROOVE.0 + GROOVE.1) / 2, 300)];
    let flat = on[at(20, 300)];
    assert!(groove > first + 0.4, "groove {groove} vs off {first}");
    assert!((flat - first).abs() < 0.02, "flat open surface unchanged: {flat} vs {first}");
    // The ridge top is not a cavity.
    let ridge = on[at(20, (RIDGE.0 + RIDGE.1) / 2)];
    let _ = ridge;
    assert!(on[at(20, (RIDGE.0 + RIDGE.1) / 2)] < groove - 0.3, "ridge top vs groove {groove}");
}

#[test]
fn edge_wear_catches_the_rims_of_the_ridge_below() {
    let fx = Fx::plane();
    let off = mask_of(&composite_full(&scene(generator(GeneratorKind::Edges, None)), &fx.ctx()));
    // At 512 the curvature is a quarter of its 2048 strength: full intensity makes up for it.
    let on = mask_of(&composite_full(&scene(generator(GeneratorKind::Edges, Some(MicroDetails { curvature_intensity: 4.0, ..on() }))), &fx.ctx()));
    // A flat plane has no geometric edge near the middle: no wear without micro details.
    assert!(off[at(20, 122)] < 0.05 && off[at(20, 300)] < 0.05);
    // With them, the ridge's rims are worn and the open surface stays clean.
    let rim = (RIDGE.0..RIDGE.1).map(|y| on[at(20, y)]).fold(0.0f32, f32::max);
    assert!(rim > 0.5, "ridge rim wear {rim}");
    assert!(on[at(20, 300)] < 0.05, "open surface {}", on[at(20, 300)]);
    // The groove floor is concave: no wear there.
    assert!(on[at((GROOVE.0 + GROOVE.1) / 2, 300)] < 0.05);
}

#[test]
fn amount_scales_the_effect_monotonically() {
    let fx = Fx::plane();
    let groove = |amount: f32| {
        let m = mask_of(&composite_full(&scene(generator(GeneratorKind::AmbientOcclusion, Some(MicroDetails { amount, ..on() }))), &fx.ctx()));
        m[at((GROOVE.0 + GROOVE.1) / 2, 300)]
    };
    let (a, b, c) = (groove(0.0), groove(0.4), groove(1.0));
    assert!(a < b && b < c, "{a} < {b} < {c}");
}

#[test]
fn works_from_inside_a_folder_over_the_relief_outside_it() {
    let fx = Fx::plane();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    ts.insert_above(None, relief(&tc));
    let mut folder = Layer::new_folder("Folder", &tc);
    let LayerContent::Group(g) = &mut folder.content else { unreachable!() };
    g.children.push(Arc::new(masked(&tc, generator(GeneratorKind::Cavity, Some(on())))));
    ts.insert_above(None, folder);
    let m = mask_of(&composite_full(&ts, &fx.ctx()));
    let reference = mask_of(&composite_full(&scene(generator(GeneratorKind::Cavity, Some(on()))), &fx.ctx()));
    let i = at((GROOVE.0 + GROOVE.1) / 2, 300);
    assert!((m[i] - reference[i]).abs() < 0.02, "folder {} vs top level {}", m[i], reference[i]);
}

#[test]
fn incremental_cache_follows_an_edit_of_the_height_below() {
    let fx = Fx::plane();
    // Edge wear: the convex lip of a pit lies outside the pit, so a pit at the start of one tile
    // wears a texel of the tile before it, which the raster edit itself never touched.
    let mut ts = scene(generator(GeneratorKind::Edges, Some(MicroDetails { curvature_intensity: 4.0, ..on() })));
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    assert_eq!(bits(cache.composite().unwrap(), ChannelKind::BaseColor), bits(&composite_full(&ts, &fx.ctx()), ChannelKind::BaseColor));
    let mut layer = ts.layers[0].as_ref().clone();
    let LayerContent::Paint(p) = &mut layer.content else { unreachable!() };
    let h = p.channels.get_mut(&ChannelKind::Height).unwrap();
    // A pit in tile 64..128 only (x 64 is a tile border).
    for y in 400..406 {
        for x in 64..70 {
            h.set(x, y, [-0.2, 1.0, 0.0, 0.0]);
        }
    }
    ts.layers[0] = Arc::new(layer);
    cache.update(&ts, &fx.ctx());
    let full = composite_full(&ts, &fx.ctx());
    let m = mask_of(&full);
    assert!((400..406).any(|y| m[at(63, y)] > 0.3), "the pit's lip reaches into the tile before it");
    assert_eq!(bits(cache.composite().unwrap(), ChannelKind::BaseColor), bits(&full, ChannelKind::BaseColor), "incremental == full");
}

#[test]
fn deterministic() {
    let fx = Fx::plane();
    let ts = scene(generator(GeneratorKind::Edges, Some(on())));
    assert_eq!(bits(&composite_full(&ts, &fx.ctx()), ChannelKind::BaseColor), bits(&composite_full(&ts, &fx.ctx()), ChannelKind::BaseColor));
}

/// Owner report 2026-11-22: default Edge wear spread in blotches far beyond the rim. The broad,
/// mild convexity a baked curvature has around a rounded edge must stay clean at the default
/// Wear Level: Breakup and Variation only erode wear, they never reach below the threshold.
#[test]
fn breakup_never_spreads_baked_wear_onto_mildly_convex_surface() {
    let mut fx = Fx::plane();
    // Convexity 0.5 everywhere (curvature 0.75): below the 0.7 threshold of Wear Level 0.3.
    fx.maps.insert(subcraft_core::MeshMapKind::Curvature, TiledImage::new(RES, RES, 1, [0.75, 0.0, 0.0, 0.0]));
    let mut g = Generator::new(GeneratorKind::Edges);
    g.noise_amount = 0.6;
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    ts.insert_above(None, masked(&tc, g));
    let m = mask_of(&composite_full(&ts, &fx.ctx()));
    let worn = m.iter().filter(|&&v| v > 0.1).count();
    assert_eq!(worn, 0, "{worn} texels worn on a surface below the wear threshold");
    // A sharp apex (convexity 1) is worn with the same breakup.
    fx.maps.insert(subcraft_core::MeshMapKind::Curvature, TiledImage::new(RES, RES, 1, [1.0, 0.0, 0.0, 0.0]));
    let m = mask_of(&composite_full(&ts, &fx.ctx()));
    let worn = m.iter().filter(|&&v| v > 0.5).count();
    assert!(worn > m.len() / 3, "apex wear {worn} of {}", m.len());
}

/// QA fuzz seed 424: a Height blend mode change on a layer under an HBAO material filter must
/// also redraw the Base Color the filter darkens (the cache once confined it to Height).
#[test]
fn incremental_cache_follows_a_height_blend_change_under_hbao() {
    use subcraft_doc::{BlendMode, ChannelBlend, ContentEffect, ContentEffectKind, MaterialFilter};
    let fx = Fx::plane();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let relief_id = ts.insert_above(None, relief(&tc));
    let mut top = Layer::new_fill("Top", &tc);
    let hbao = MaterialFilter::ALL.into_iter().find(|f| matches!(f, MaterialFilter::Hbao { .. })).unwrap();
    top.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Material(hbao), &tc)));
    ts.insert_above(None, top);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    let before = bits(cache.composite().unwrap(), ChannelKind::BaseColor);
    assert_eq!(before, bits(&composite_full(&ts, &fx.ctx()), ChannelKind::BaseColor));
    ts.find_mut(relief_id).unwrap().blend.insert(ChannelKind::Height, ChannelBlend::new(BlendMode::Difference));
    cache.update(&ts, &fx.ctx());
    let full = composite_full(&ts, &fx.ctx());
    assert_ne!(before, bits(&full, ChannelKind::BaseColor), "the blend change moves the occlusion");
    assert_eq!(bits(cache.composite().unwrap(), ChannelKind::BaseColor), bits(&full, ChannelKind::BaseColor), "incremental == full");
}
