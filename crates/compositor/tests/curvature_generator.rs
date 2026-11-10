//! Curvature generator modes and detail sizes: defaults are the baked map bit for bit, modes
//! keep one side, and the medium / coarse sizes average fine detail away.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{CurvatureDetail, CurvatureMode, FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, ResourceStore, TextureSet};
use subcraft_image::TiledImage;
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 256;

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The new Curvature generator (Balance 0.5, Contrast 0: smoothstep over 0..1 of the source).
fn curvature(detail: Option<CurvatureDetail>) -> Generator {
    let mut g = Generator::new(GeneratorKind::MeshMap(MeshMapKind::Curvature));
    g.curvature = detail;
    g
}

/// Mask values (recovered from roughness 0.3 + 0.7 * mask) of a plane over `map`.
fn mask_of(g: Generator, map: TiledImage) -> Vec<f32> {
    let geo = TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES);
    let mut maps = MeshMaps::new();
    maps.insert(MeshMapKind::Curvature, map);
    let res = ResourceStore::default();
    let ctx = EvalContext { geometry: &geo, resources: &res, mesh_maps: &maps };
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut layer = Layer::new_fill("Fill", &tc);
    if let LayerContent::Fill(f) = &mut layer.content {
        f.channels.insert(ChannelKind::Roughness, FillChannel::new(FillSource::uniform(ChannelValue::scalar(1.0))));
    }
    let mut mask = Mask::new(RES, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    layer.mask = Some(mask);
    ts.insert_above(None, layer);
    let c = composite_full(&ts, &ctx);
    c.channel(ChannelKind::Roughness).unwrap().to_dense().iter().map(|r| (r - 0.3) / 0.7).collect()
}

fn dense(f: impl Fn(u32, u32) -> f32) -> TiledImage {
    let d: Vec<f32> = (0..RES * RES).map(|i| f(i % RES, i / RES)).collect();
    TiledImage::from_dense(RES, RES, 1, [0.5; 4], &d)
}

/// Smooth bumps and creases in both directions.
fn varied() -> TiledImage {
    dense(|x, y| (0.5 + 0.45 * ((x as f32 * 0.07).sin() * (y as f32 * 0.05).cos())).clamp(0.0, 1.0))
}

#[test]
fn default_detail_is_the_baked_map_bit_for_bit() {
    let none = mask_of(curvature(None), varied());
    let default = mask_of(curvature(Some(CurvatureDetail::default())), varied());
    assert_eq!(none, default, "Some(default) changes nothing");
    // And it is the documented Balance / Contrast remap of the map itself.
    let map = varied().to_dense();
    for i in (0..none.len()).step_by(97) {
        assert!((none[i] - smoothstep(0.0, 1.0, map[i])).abs() < 2e-3, "texel {i}: {} vs {}", none[i], map[i]);
    }
}

#[test]
fn edges_only_keeps_convex_and_cavities_only_keeps_concave() {
    let edges = |c: f32| mask_of(curvature(Some(CurvatureDetail { mode: CurvatureMode::Edges, ..CurvatureDetail::default() })), dense(|_, _| c))[RES as usize * 128 + 128];
    let cavities = |c: f32| mask_of(curvature(Some(CurvatureDetail { mode: CurvatureMode::Cavities, ..CurvatureDetail::default() })), dense(|_, _| c))[RES as usize * 128 + 128];
    // Convex 0.8: deviation 0.3 -> source 0.6 for edges, 0 for cavities; concave mirrors it.
    assert!((edges(0.8) - smoothstep(0.0, 1.0, 0.6)).abs() < 2e-3, "{}", edges(0.8));
    assert!(edges(0.2).abs() < 1e-3 && edges(0.5).abs() < 1e-3, "flat and concave are black");
    assert!((cavities(0.2) - smoothstep(0.0, 1.0, 0.6)).abs() < 2e-3, "{}", cavities(0.2));
    assert!(cavities(0.8).abs() < 1e-3 && cavities(0.5).abs() < 1e-3);
    // The strongest curvature is white in its mode.
    assert!(edges(1.0) > 0.999 && cavities(0.0) > 0.999);
}

#[test]
fn coarse_detail_averages_fine_texel_noise_away_and_medium_keeps_broad_forms() {
    // One-texel checker: pure fine detail.
    let checker = || dense(|x, y| if (x + y) % 2 == 0 { 0.8 } else { 0.2 });
    let fine = mask_of(curvature(None), checker());
    let spread = |v: &[f32]| v.iter().fold((1.0f32, 0.0f32), |(l, h), x| (l.min(*x), h.max(*x)));
    let (lo, hi) = spread(&fine);
    assert!(hi - lo > 0.5, "the fine map is all detail: {lo}..{hi}");
    for (fine_w, medium, coarse) in [(0.0, 1.0, 0.0), (0.0, 0.0, 1.0), (0.0, 1.0, 1.0)] {
        let d = CurvatureDetail { fine: fine_w, medium, coarse, ..CurvatureDetail::default() };
        let (lo, hi) = spread(&mask_of(curvature(Some(d)), checker()));
        assert!((lo - 0.5).abs() < 2e-3 && (hi - 0.5).abs() < 2e-3, "{d:?}: {lo}..{hi} should be flat");
    }
    // A broad convex half (left) on a flat right: medium keeps it, boosted back towards the fine contrast.
    let half = || dense(|x, _| if x < RES / 2 { 0.7 } else { 0.5 });
    let d = CurvatureDetail { fine: 0.0, medium: 1.0, ..CurvatureDetail::default() };
    let m = mask_of(curvature(Some(d)), half());
    let at = |x: u32, y: u32| m[(y * RES + x) as usize];
    // Deep inside the left half: 0.5 + 2 x 0.2 = 0.9 before the remap.
    assert!((at(40, 128) - smoothstep(0.0, 1.0, 0.9)).abs() < 5e-3, "{}", at(40, 128));
    assert!((at(220, 128) - 0.5).abs() < 2e-3, "{}", at(220, 128));
    // ...with a soft transition, not the fine map's hard step.
    let ramp: Vec<f32> = (120..136).map(|x| at(x, 128)).collect();
    assert!(ramp.windows(2).all(|w| w[1] <= w[0] + 1e-6) && ramp.iter().filter(|v| **v > 0.52 && **v < 0.95).count() >= 4, "{ramp:?}");
}

#[test]
fn weights_are_bounded_and_garbage_values_stay_finite() {
    for d in [
        CurvatureDetail { fine: f32::NAN, medium: f32::INFINITY, coarse: -3.0, mode: CurvatureMode::Edges },
        CurvatureDetail { fine: 100.0, medium: 100.0, coarse: 100.0, mode: CurvatureMode::Both },
    ] {
        let m = mask_of(curvature(Some(d)), varied());
        assert!(m.iter().all(|v| v.is_finite() && (-1e-4..=1.0001).contains(v)), "{d:?}");
    }
}
