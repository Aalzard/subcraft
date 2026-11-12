//! Mask-shape filters through the full composite: Drop Shadow, Glow, Height Adjust, FXAA, and the
//! Bevel / Mask Outline shape options.

use std::sync::Arc;

use subcraft_compositor::{Composite, EvalContext, composite_full};
use subcraft_core::ChannelKind as C;
use subcraft_doc::{BevelCorner, ContentEffect, ContentEffectKind, Filter, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, OutlinePosition, ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};

const N: u32 = 128;

struct Fixture {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fixture {
    fn new() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 2)), 0, N), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

fn square(x: u32, y: u32) -> bool {
    (40..60).contains(&x) && (40..60).contains(&y)
}

/// A paint layer: white Base Color and roughness 1 on the 20 x 20 square, transparent elsewhere,
/// with `filter` on `kind`.
fn painted(kind: C, filter: Filter) -> TextureSet {
    let mut ts = TextureSet::new("T", 0, N);
    let tc = ts.clone();
    let mut l = Layer::new_paint("P", &tc);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let color = p.channel_mut(C::BaseColor, N);
    for y in 0..N {
        for x in 0..N {
            if square(x, y) {
                color.set(x, y, [1.0, 1.0, 1.0, 1.0]);
            }
        }
    }
    let rough = p.channel_mut(C::Roughness, N);
    for y in 0..N {
        for x in 0..N {
            if square(x, y) {
                rough.set(x, y, [1.0, 1.0, 0.0, 0.0]);
            }
        }
    }
    l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: kind, filter }, &tc)));
    ts.insert_above(None, l);
    ts
}

fn rgb(c: &Composite, x: u32, y: u32) -> [f32; 4] {
    c.channel(C::BaseColor).unwrap().get(x, y)
}

#[test]
fn drop_shadow_lies_under_the_content_offset_along_its_angle() {
    let fx = Fixture::new();
    // Struct update syntax does not work on enum variants: build each variant from its parts.
    let drop = |angle_deg: f32, size: f32, opacity: f32, color: [f32; 3]| Filter::DropShadow { angle_deg, distance: 10.0, size, spread: 0.0, opacity, color };
    let shadow = drop(0.0, 0.0, 1.0, [0.0; 3]);
    let c = composite_full(&painted(C::BaseColor, shadow), &fx.ctx());
    // The square stays white; its shadow, 10 px to the right, is black on the 0.5 background.
    assert!((rgb(&c, 45, 50)[0] - 1.0).abs() < 2e-3);
    assert!(rgb(&c, 65, 50)[0].abs() < 2e-3, "{:?}", rgb(&c, 65, 50));
    assert!((rgb(&c, 75, 50)[0] - 0.5).abs() < 2e-3);
    assert!((rgb(&c, 35, 50)[0] - 0.5).abs() < 2e-3, "nothing on the other side");
    // Half opacity, red: half way from the background to red.
    let red = drop(0.0, 0.0, 0.5, [1.0, 0.0, 0.0]);
    let c = composite_full(&painted(C::BaseColor, red), &fx.ctx());
    let px = rgb(&c, 65, 50);
    assert!((px[0] - 0.75).abs() < 2e-3 && (px[1] - 0.25).abs() < 2e-3, "{px:?}");
    // Angle 90 points up: the shadow is above the square.
    let up = drop(90.0, 0.0, 1.0, [0.0; 3]);
    let c = composite_full(&painted(C::BaseColor, up), &fx.ctx());
    assert!(rgb(&c, 50, 35)[0].abs() < 2e-3 && (rgb(&c, 50, 65)[0] - 0.5).abs() < 2e-3);
    // Blurred: soft, symmetric about the shifted edge.
    let soft = drop(0.0, 4.0, 1.0, [0.0; 3]);
    let c = composite_full(&painted(C::BaseColor, soft), &fx.ctx());
    let edge = rgb(&c, 70, 50)[0];
    assert!(edge > 0.1 && edge < 0.4, "{edge}");
}

#[test]
fn glow_surrounds_the_content_and_brightens_masks() {
    let fx = Fixture::new();
    let outer = |color: [f32; 3], precise: bool| Filter::OuterGlow { width: 6.0, spread: 0.0, opacity: 1.0, color, precise };
    let glow = outer([0.0, 1.0, 0.0], false);
    let c = composite_full(&painted(C::BaseColor, glow), &fx.ctx());
    // Right outside the square: tinted green; far away: the background.
    let near = rgb(&c, 61, 50);
    assert!(near[1] > near[0] + 0.1, "{near:?}");
    assert!((rgb(&c, 100, 50)[1] - 0.5).abs() < 2e-3);
    assert!((rgb(&c, 50, 50)[0] - 1.0).abs() < 2e-3, "the content stays on top");
    // Precise falls off steeper: brighter right at the edge than Soft.
    let precise = composite_full(&painted(C::BaseColor, outer([0.0, 1.0, 0.0], true)), &fx.ctx());
    assert!(rgb(&precise, 61, 50)[1] > near[1]);
    // On a mask: a white glow brightens the black around a white square.
    let mut ts = TextureSet::new("T", 0, N);
    let tc = ts.clone();
    let mut l = Layer::new_fill("F", &tc);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = *k == C::Roughness;
        if *k == C::Roughness {
            *ch = subcraft_doc::FillChannel::new(subcraft_doc::FillSource::uniform(subcraft_core::ChannelValue::scalar(1.0)));
        }
    }
    let mut mask = Mask::new(N, false);
    for y in 0..N {
        for x in 0..N {
            if square(x, y) {
                mask.base.set(x, y, [1.0, 0.0, 0.0, 0.0]);
            }
        }
    }
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(outer([1.0; 3], false)))));
    l.mask = Some(mask);
    ts.insert_above(None, l);
    let c = composite_full(&ts, &fx.ctx());
    let m = |x| (c.channel(C::Roughness).unwrap().get(x, 50)[0] - 0.3) / 0.7;
    assert!((m(50) - 1.0).abs() < 2e-3 && m(62) > 0.05 && m(62) < 0.9 && m(100).abs() < 2e-3, "{} {} {}", m(50), m(62), m(100));
}

#[test]
fn height_adjust_and_fxaa_on_content() {
    let fx = Fixture::new();
    // Roughness 1 on the square: inverted then halved plus 0.2 = 0.2.
    let c = composite_full(&painted(C::Roughness, Filter::HeightAdjust { invert: true, offset: 0.2, multiply: 0.5 }), &fx.ctx());
    assert!((c.channel(C::Roughness).unwrap().get(50, 50)[0] - 0.2).abs() < 2e-3);
    // FXAA leaves the flat inside alone and softens the square's corner texel.
    let c = composite_full(&painted(C::BaseColor, Filter::Fxaa { alpha: true }), &fx.ctx());
    assert!((rgb(&c, 50, 50)[0] - 1.0).abs() < 2e-3);
    let corner = rgb(&c, 40, 40);
    assert!(corner[0] < 1.0 - 1e-3, "{corner:?}");
}

#[test]
fn angular_bevel_and_blurred_outline_change_the_contour_profile() {
    let fx = Fixture::new();
    let rounded = Filter::Bevel { radius: 8.0, amount: 1.0, threshold: 0.5 };
    let angular = Filter::BevelShape { radius: 8.0, amount: 1.0, threshold: 0.5, corner: BevelCorner::Angular, smoothing: 0.0 };
    let value = |f| composite_full(&painted(C::Roughness, f), &fx.ctx()).channel(C::Roughness).unwrap().get(44, 44)[0];
    // Near the square's corner the Chebyshev distance (4) exceeds the Euclidean one (about 4 too,
    // but to the corner region it is the larger axis): the angular ramp is at least as high.
    assert!(value(angular) >= value(rounded) - 1e-3);
    // At (42, 50): 2.5 px from the left edge either way; the ramp is the same.
    let along = |f| composite_full(&painted(C::Roughness, f), &fx.ctx()).channel(C::Roughness).unwrap().get(42, 50)[0];
    assert!((along(angular) - along(rounded)).abs() < 2e-3);
    // Mask Outline (inner, so the paint coverage is there): with Blur 4 the band reaches further
    // in. At x 45 (5.5 px inside): 0 without, (2.5 + 4 - 5.5) / 5 = 0.2 with.
    let sharp = Filter::MaskOutline { radius: 2.0, threshold: 0.5, position: OutlinePosition::Inner };
    let blurred = Filter::MaskOutlineShape { radius: 2.0, threshold: 0.5, position: OutlinePosition::Inner, start_black: false, blur: 4.0, curve: 0.0 };
    let at = |f, x| composite_full(&painted(C::Roughness, f), &fx.ctx()).channel(C::Roughness).unwrap().get(x, 50)[0];
    assert!(at(sharp, 45).abs() < 2e-3 && (at(blurred, 45) - 0.2).abs() < 2e-3, "{} {}", at(sharp, 45), at(blurred, 45));
}
