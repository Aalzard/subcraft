//! ID Color Mask (ADR-0041): selection by baked ID map color, tolerance, missing map, dirty tiles.

use std::sync::Arc;

use subcraft_compositor::{CompositeCache, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, LayerId, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, IdColorMask, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, ResourceStore, TextureSet};
use subcraft_image::TiledImage;
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;
const RED: [f32; 3] = [0.9, 0.3, 0.3];
const BLUE: [f32; 3] = [0.3, 0.3, 0.9];
/// 0.04 away from RED in green.
const NEAR_RED: [f32; 3] = [0.9, 0.34, 0.3];

/// Two ID regions: x < 64 red, x >= 64 blue, except a near-red strip at y >= 120 on the left.
fn id_map() -> TiledImage {
    let mut img = TiledImage::new(RES, RES, 3, [0.0; 4]);
    for y in 0..RES {
        for x in 0..RES {
            let c = if x >= 64 { BLUE } else if y >= 120 { NEAR_RED } else { RED };
            img.set(x, y, [c[0], c[1], c[2], 1.0]);
        }
    }
    img
}

struct Fixture {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fixture {
    fn new(with_id: bool) -> Self {
        let mut maps = MeshMaps::new();
        if with_id {
            maps.insert(MeshMapKind::Id, id_map());
        }
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps }
    }

    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

/// Roughness 1.0 fill masked by a black mask + an ID Color Mask (stack default roughness 0.3).
fn masked_fill(id: IdColorMask) -> (TextureSet, LayerId) {
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut layer = Layer::new_fill("Fill", &tc);
    let LayerContent::Fill(f) = &mut layer.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = *k == ChannelKind::Roughness;
        if *k == ChannelKind::Roughness {
            *ch = FillChannel::new(FillSource::uniform(ChannelValue::scalar(1.0)));
        }
    }
    let mut mask = Mask::new(RES, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::IdColor(id))));
    layer.mask = Some(mask);
    let lid = ts.insert_above(None, layer);
    (ts, lid)
}

fn picked(colors: &[[f32; 3]], tolerance: f32) -> IdColorMask {
    IdColorMask { colors: colors.to_vec(), tolerance, ..IdColorMask::default() }
}

/// Mask value recovered from roughness: 0.3 + 0.7·m.
fn mask_at(c: &subcraft_compositor::Composite, x: u32, y: u32) -> f32 {
    (c.channel(ChannelKind::Roughness).unwrap().get(x, y)[0] - 0.3) / 0.7
}

/// Counts texels whose mask is ~1 and ~0, failing on anything in between.
fn census(c: &subcraft_compositor::Composite, region: impl Fn(u32, u32) -> bool) -> (usize, usize) {
    let (mut inside_ok, mut outside_ok) = (0, 0);
    for y in 0..RES {
        for x in 0..RES {
            let m = mask_at(c, x, y);
            if region(x, y) {
                assert!((m - 1.0).abs() < 2e-3, "({x},{y}) should be selected: {m}");
                inside_ok += 1;
            } else {
                assert!(m.abs() < 2e-3, "({x},{y}) should be unselected: {m}");
                outside_ok += 1;
            }
        }
    }
    (inside_ok, outside_ok)
}

#[test]
fn picking_one_id_region_selects_exactly_it() {
    let fx = Fixture::new(true);
    let (ts, _) = masked_fill(picked(&[RED], 0.05));
    let c = composite_full(&ts, &fx.ctx());
    // The near-red strip (distance 0.04) is inside the default tolerance.
    assert_eq!(census(&c, |x, _| x < 64), (64 * 128, 64 * 128));

    // Picking both regions selects everything.
    let (ts, _) = masked_fill(picked(&[RED, BLUE], 0.05));
    let c = composite_full(&ts, &fx.ctx());
    assert_eq!(census(&c, |_, _| true), (128 * 128, 0));
}

#[test]
fn tolerance_includes_or_excludes_near_colors_and_invert_flips() {
    let fx = Fixture::new(true);
    let (ts, _) = masked_fill(picked(&[RED], 0.03));
    let c = composite_full(&ts, &fx.ctx());
    assert_eq!(census(&c, |x, y| x < 64 && y < 120), (64 * 120, 128 * 128 - 64 * 120));

    // Softness: d = 0.04, tolerance 0.03, softness 0.02 → halfway → smoothstep 0.5.
    let (ts, _) = masked_fill(IdColorMask { softness: 0.02, ..picked(&[RED], 0.03) });
    let c = composite_full(&ts, &fx.ctx());
    // The map is f16: 0.34 is stored as 0.33984, so d = 0.03986 and the falloff is at 0.49.
    assert!((mask_at(&c, 10, 125) - 0.5).abs() < 0.02, "{}", mask_at(&c, 10, 125));
    assert!((mask_at(&c, 10, 10) - 1.0).abs() < 2e-3);
    assert!(mask_at(&c, 100, 10).abs() < 2e-3);

    let (ts, _) = masked_fill(IdColorMask { invert: true, ..picked(&[RED], 0.05) });
    let c = composite_full(&ts, &fx.ctx());
    assert_eq!(census(&c, |x, _| x >= 64), (64 * 128, 64 * 128));
}

#[test]
fn without_a_baked_id_map_the_mask_is_black_even_inverted() {
    let fx = Fixture::new(false);
    for invert in [false, true] {
        let (ts, _) = masked_fill(IdColorMask { invert, ..picked(&[RED, [0.0; 3]], 0.05) });
        let c = composite_full(&ts, &fx.ctx());
        assert_eq!(census(&c, |_, _| false), (0, 128 * 128));
    }
}

#[test]
fn parameter_edits_dirty_every_tile_and_unchanged_documents_none() {
    let fx = Fixture::new(true);
    let (mut ts, lid) = masked_fill(picked(&[RED], 0.05));
    let mut cache = CompositeCache::new();
    assert_eq!(cache.update(&ts, &fx.ctx()).len(), 4, "first evaluation: all 2x2 tiles");
    assert!(cache.update(&ts, &fx.ctx()).is_empty(), "unchanged document: nothing dirty");

    let edit = |ts: &mut TextureSet, f: &dyn Fn(&mut IdColorMask)| {
        let mask = ts.find_mut(lid).unwrap().mask.as_mut().unwrap();
        let MaskEffectKind::IdColor(id) = &mut Arc::make_mut(&mut mask.effects[0]).kind else { unreachable!() };
        f(id);
    };
    edit(&mut ts, &|id| id.colors.push(BLUE));
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![0, 1, 2, 3], "adding a color re-evaluates every tile");
    let c = cache.composite().unwrap();
    assert_eq!(census(c, |_, _| true), (128 * 128, 0));
    assert_eq!(c, &composite_full(&ts, &fx.ctx()));

    edit(&mut ts, &|id| { id.colors.remove(0); id.tolerance = 0.01; });
    assert_eq!(cache.update(&ts, &fx.ctx()).len(), 4);
    assert_eq!(census(cache.composite().unwrap(), |x, _| x >= 64), (64 * 128, 64 * 128));
}
