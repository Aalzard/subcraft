//! Compare Mask (operands from the layer stack, the mask and constants), per-component Levels
//! evaluated through the full composite.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{
    CompareMask, CompareOp, CompareOperand, ContentEffect, ContentEffectKind, FillChannel, FillSource, Layer, LayerContent, Levels, LevelsRange, Mask,
    MaskEffect, MaskEffectKind, ResourceStore, TextureSet,
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
    fn new() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

/// Height of the bottom layer: rises from 0 at the left edge to 1 at the right.
fn ramp(x: u32) -> f32 {
    x as f32 / (RES - 1) as f32
}

/// A paint layer with the height ramp, under a fill layer (roughness 1, height `top`) masked by
/// a black mask and `compare`.
fn stack(top: f32, compare: CompareMask) -> TextureSet {
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut bottom = Layer::new_paint("Ramp", &tc);
    let LayerContent::Paint(p) = &mut bottom.content else { unreachable!() };
    let height = p.channel_mut(ChannelKind::Height, RES);
    for y in 0..RES {
        for x in 0..RES {
            height.set(x, y, [ramp(x), 1.0, 0.0, 0.0]);
        }
    }
    ts.insert_above(None, bottom);
    let mut layer = Layer::new_fill("Top", &tc);
    let LayerContent::Fill(f) = &mut layer.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        *ch = match k {
            ChannelKind::Roughness => FillChannel::new(FillSource::uniform(ChannelValue::scalar(1.0))),
            ChannelKind::Height => FillChannel::new(FillSource::uniform(ChannelValue::scalar(top))),
            _ => FillChannel { enabled: false, ..ch.clone() },
        };
    }
    let mut mask = Mask::new(RES, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Compare(compare))));
    layer.mask = Some(mask);
    // As "Add mask with height combination" does: Height replaces instead of adding.
    layer.blend.insert(ChannelKind::Height, subcraft_doc::ChannelBlend::new(subcraft_doc::BlendMode::Normal));
    ts.insert_above(None, layer);
    ts
}

/// The top layer's mask, recovered from roughness (default 0.3, layer 1.0).
fn mask_at(c: &subcraft_compositor::Composite, x: u32, y: u32) -> f32 {
    (c.channel(ChannelKind::Roughness).unwrap().get(x, y)[0] - 0.3) / 0.7
}

#[test]
fn height_combination_shows_the_layer_where_it_rises_above_the_layers_below() {
    let fx = Fixture::new();
    let c = composite_full(&stack(0.5, CompareMask { hardness: 1.0, ..CompareMask::height_combination() }), &fx.ctx());
    for x in 0..RES {
        let m = mask_at(&c, x, 64);
        // Composites are f16: compare against the ramp as stored, away from the crossing.
        let expected = if ramp(x) < 0.49 { 1.0 } else if ramp(x) > 0.51 { 0.0 } else { continue };
        assert!((m - expected).abs() < 2e-3, "x {x}: mask {m}");
        // Height: the top layer's 0.5 where it shows, the ramp elsewhere, so the higher of the two.
        let h = c.channel(ChannelKind::Height).unwrap().get(x, 64)[0];
        assert!((h - ramp(x).max(0.5)).abs() < 2e-3, "x {x}: height {h}");
    }
    // Swapping the operands (Lesser than) selects the other side.
    let c = composite_full(&stack(0.5, CompareMask { hardness: 1.0, op: CompareOp::Lesser, ..CompareMask::height_combination() }), &fx.ctx());
    assert!(mask_at(&c, 10, 64).abs() < 2e-3 && (mask_at(&c, 120, 64) - 1.0).abs() < 2e-3);
}

#[test]
fn hardness_blends_across_the_crossing_and_within_keeps_a_band() {
    let fx = Fixture::new();
    // Hardness 0.6: linear over 0.1 of height, centred where the ramp crosses 0.5.
    let c = composite_full(&stack(0.5, CompareMask { hardness: 0.6, ..CompareMask::height_combination() }), &fx.ctx());
    for x in 0..RES {
        let expected = ((0.5 - ramp(x)) / 0.1 + 0.5).clamp(0.0, 1.0);
        let m = mask_at(&c, x, 30);
        assert!((m - expected).abs() < 4e-3, "x {x}: {m} vs {expected}");
    }
    // Within tolerance 0.1 of this layer's 0.3: the band 0.2..0.4 of the ramp.
    let within = CompareMask { op: CompareOp::Within, tolerance: 0.1, hardness: 1.0, ..CompareMask::height_combination() };
    let c = composite_full(&stack(0.3, within), &fx.ctx());
    for x in 0..RES {
        let d = (ramp(x) - 0.3).abs();
        if (d - 0.1).abs() < 0.01 {
            continue;
        }
        let expected = if d < 0.1 { 1.0 } else { 0.0 };
        assert!((mask_at(&c, x, 90) - expected).abs() < 2e-3, "x {x}");
    }
}

#[test]
fn mask_and_constant_operands_need_no_layers() {
    let fx = Fixture::new();
    // This Mask (the black base, 0) lesser than Constant 0.5 holds everywhere.
    let c = composite_full(
        &stack(0.5, CompareMask { left: CompareOperand::ThisMask, op: CompareOp::Lesser, right: CompareOperand::Constant, hardness: 1.0, ..CompareMask::default() }),
        &fx.ctx(),
    );
    assert!((0..RES).all(|x| (mask_at(&c, x, 5) - 1.0).abs() < 2e-3));
    // Constant 0.5 greater than Layers Below in another channel (Roughness: the default 0.3).
    let c = composite_full(
        &stack(0.5, CompareMask { channel: ChannelKind::Roughness, left: CompareOperand::Constant, right: CompareOperand::LayersBelow, hardness: 1.0, ..CompareMask::default() }),
        &fx.ctx(),
    );
    assert!((0..RES).all(|x| (mask_at(&c, x, 5) - 1.0).abs() < 2e-3));
}

#[test]
fn levels_per_component_and_unclamped_on_a_colour_channel() {
    let fx = Fixture::new();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut layer = Layer::new_fill("Color", &tc);
    let LayerContent::Fill(f) = &mut layer.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        if *k == ChannelKind::BaseColor {
            *ch = FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.2, 0.4, 0.6)));
        } else {
            ch.enabled = false;
        }
    }
    let mut levels = Levels::default();
    levels.set_component(Some(0), LevelsRange { out_high: 0.5, ..LevelsRange::default() });
    levels.set_component(Some(2), LevelsRange { in_low: 0.1, in_high: 0.5, ..LevelsRange::default() });
    layer.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Levels { channel: ChannelKind::BaseColor, levels }, &tc)));
    ts.insert_above(None, layer);
    let c = composite_full(&ts, &fx.ctx());
    let px = c.channel(ChannelKind::BaseColor).unwrap().get(40, 40);
    // Red halved, green untouched, blue (0.6) past its input high: clamped to 1. (Unclamped
    // levels are checked numerically in subcraft-doc.)
    for (got, want) in px.iter().zip([0.1, 0.4, 1.0]) {
        assert!((got - want).abs() < 2e-3, "{px:?}");
    }
}
