//! Mask Builder and Fiber Glass Edge Wear generators through the compositor.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FiberSettings, FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskBuilder, MaskEffect, MaskEffectKind, ResourceStore, TextureSet};
use subcraft_image::TiledImage;
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

struct Fx {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

fn curvature(x: u32) -> f32 {
    x as f32 / (RES - 1) as f32
}
fn occlusion(y: u32) -> f32 {
    y as f32 / (RES - 1) as f32
}

impl Fx {
    /// A plane covering the whole texture: curvature rising left to right, occlusion rising
    /// top to bottom.
    fn plane() -> Self {
        let mut maps = MeshMaps::new();
        let mut map = |kind, f: &dyn Fn(u32, u32) -> f32| {
            let mut img = TiledImage::new(RES, RES, 1, [0.0; 4]);
            for y in 0..RES {
                for x in 0..RES {
                    img.set(x, y, [f(x, y), 0.0, 0.0, 0.0]);
                }
            }
            maps.insert(kind, img);
        };
        map(MeshMapKind::Curvature, &|x, _| curvature(x));
        map(MeshMapKind::AmbientOcclusion, &|_, y| occlusion(y));
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps }
    }
    /// A cube without baked maps: real edges for the wear to follow.
    fn cube() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    /// The mask `g` gives, read back from a layer it masks (NaN where the model has no texel).
    fn mask(&self, g: Generator) -> Vec<f32> {
        let mut ts = TextureSet::new("T", 0, RES);
        let mut l = Layer::new_fill("Fill", &ts);
        let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
        for (k, ch) in f.channels.iter_mut() {
            ch.enabled = *k == ChannelKind::Roughness;
            if ch.enabled {
                *ch = FillChannel::new(FillSource::uniform(ChannelValue::scalar(1.0)));
            }
        }
        let mut mask = Mask::new(RES, false);
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
        l.mask = Some(mask);
        ts.insert_above(None, l);
        let c = composite_full(&ts, &EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps });
        let img = c.channel(ChannelKind::Roughness).unwrap();
        // Roughness 1 over the 0.3 default: value = 0.3 + 0.7 * mask.
        (0..RES * RES).map(|i| (img.get(i % RES, i / RES)[0] - 0.3) / 0.7).collect()
    }
}

fn builder(edit: impl Fn(&mut MaskBuilder)) -> Generator {
    let mut g = Generator::new(GeneratorKind::MaskBuilder);
    let mut b = MaskBuilder { ao: 0.0, curvature: 0.0, ..MaskBuilder::default() };
    edit(&mut b);
    g.builder = Some(b);
    g
}

fn at(m: &[f32], x: u32, y: u32) -> f32 {
    m[(y * RES + x) as usize]
}

#[test]
fn each_contribution_is_its_map_times_its_slider_and_they_add() {
    let fx = Fx::plane();
    let ao = fx.mask(builder(|b| b.ao = 1.0));
    let convex = fx.mask(builder(|b| b.curvature = 1.0));
    let concave = fx.mask(builder(|b| { b.curvature = 1.0; b.convex_range = 0.0; b.concave_range = 1.0; }));
    let both = fx.mask(builder(|b| { b.ao = 0.5; b.curvature = 0.5; }));
    for (x, y) in [(5, 5), (40, 100), (100, 30), (120, 120), (64, 64)] {
        let (c, o) = (curvature(x), occlusion(y));
        let want_convex = ((c - 0.5) * 2.0).clamp(0.0, 1.0);
        assert!((at(&ao, x, y) - (1.0 - o)).abs() < 2e-3, "AO at {x},{y}: {}", at(&ao, x, y));
        assert!((at(&convex, x, y) - want_convex).abs() < 2e-3, "convex at {x},{y}");
        assert!((at(&concave, x, y) - ((0.5 - c) * 2.0).clamp(0.0, 1.0)).abs() < 2e-3, "concave at {x},{y}");
        assert!((at(&both, x, y) - (0.5 * (1.0 - o) + 0.5 * want_convex)).abs() < 3e-3, "sum at {x},{y}");
    }
    // Range keeps the top share of the map; Contrast 1 makes it a hard edge; Invert flips it.
    let half = fx.mask(builder(|b| { b.ao = 1.0; b.ao_range = 0.5; b.ao_contrast = 1.0; }));
    assert!((at(&half, 10, 20) - 1.0).abs() < 1e-3 && at(&half, 10, 100).abs() < 1e-3);
    let flipped = fx.mask(builder(|b| { b.ao = 1.0; b.ao_range = 0.5; b.ao_contrast = 1.0; b.ao_invert = true; }));
    assert!(at(&flipped, 10, 20).abs() < 1e-3 && (at(&flipped, 10, 100) - 1.0).abs() < 1e-3);
    // The global Level, Contrast and Invert act on the sum.
    let mut g = builder(|b| b.ao = 1.0);
    g.invert = true;
    let inverted = fx.mask(g);
    assert!((at(&inverted, 30, 40) - occlusion(40)).abs() < 2e-3);
    let mut g = builder(|b| b.ao = 1.0);
    g.softness = 0.0;
    let hard = fx.mask(g);
    assert!(hard.iter().all(|v| (*v - 0.0).abs() < 1e-3 || (*v - 1.0).abs() < 1e-3), "Contrast 1 is black and white");
}

#[test]
fn the_normal_scatter_scratches_and_grunge_contributions_behave() {
    let fx = Fx::plane();
    // Every side at full intensity: an axis-aligned plane faces exactly one of them.
    let facing = fx.mask(builder(|b| { b.world_normal = 1.0; b.normal = [1.0; 6]; }));
    assert!(facing.iter().all(|v| (*v - 1.0).abs() < 1e-3));
    let none = fx.mask(builder(|b| { b.world_normal = 1.0; b.normal = [0.0; 6]; }));
    assert!(none.iter().all(|v| v.abs() < 1e-3));
    // Scratches need Grunge, AO or Curvature above 0.
    let alone = fx.mask(builder(|b| { b.scratches = 1.0; b.scratches_amount = 1.0; }));
    assert!(alone.iter().all(|v| v.abs() < 1e-3), "scratches alone add nothing");
    let base = fx.mask(builder(|b| b.ao = 0.2));
    let scratched = fx.mask(builder(|b| { b.ao = 0.2; b.scratches = 1.0; b.scratches_amount = 1.0; }));
    assert!(scratched.iter().zip(&base).all(|(s, b)| *s >= *b - 1e-4), "scratches only add");
    assert!(scratched.iter().zip(&base).filter(|(s, b)| **s > **b + 0.2).count() > 50, "and they show");
    // Scatter: separate dots, a different set per seed, none at density 0.
    let dots = fx.mask(builder(|b| { b.scatter = 1.0; b.scatter_density = 0.6; b.scatter_size = 0.7; }));
    let covered = dots.iter().filter(|v| **v > 0.5).count() as f32 / dots.len() as f32;
    assert!((0.005..0.6).contains(&covered), "dots cover part of the surface: {covered}");
    let mut g = builder(|b| { b.scatter = 1.0; b.scatter_density = 0.6; b.scatter_size = 0.7; });
    g.noise.seed += 1;
    assert_ne!(fx.mask(g), dots);
    let empty = fx.mask(builder(|b| { b.scatter = 1.0; b.scatter_density = 0.0; }));
    assert!(empty.iter().all(|v| v.abs() < 1e-3));
    // Grunge: a varied pattern, mirrored by its Invert.
    let grunge = fx.mask(builder(|b| b.grunge = 1.0));
    let (lo, hi) = grunge.iter().fold((1.0f32, 0.0f32), |(l, h), v| (l.min(*v), h.max(*v)));
    assert!(hi - lo > 0.3, "grunge has contrast: {lo}..{hi}");
    let inverted = fx.mask(builder(|b| { b.grunge = 1.0; b.grunge_invert = true; }));
    assert!(grunge.iter().zip(&inverted).all(|(a, b)| (a + b - 1.0).abs() < 2e-3));
    // Out-of-range settings give an empty mask, never NaN.
    let broken = fx.mask(builder(|b| { b.ao = 1.0; b.ao_range = f32::NAN; }));
    assert!(broken.iter().all(|v| v.abs() < 1e-3));
}

#[test]
fn fiber_glass_edge_wear_is_edge_wear_plus_a_woven_fringe() {
    let fx = Fx::cube();
    let plain = fx.mask(Generator::new(GeneratorKind::Edges));
    let fiber = |edit: &dyn Fn(&mut Generator, &mut FiberSettings)| {
        let mut g = Generator::new(GeneratorKind::FiberGlass);
        let mut f = FiberSettings::default();
        edit(&mut g, &mut f);
        g.fiber = Some(f);
        fx.mask(g)
    };
    // Without fibres it is exactly the edge wear.
    let none = fiber(&|_, f| f.amount = 0.0);
    assert!(none.iter().zip(&plain).all(|(a, b)| (a - b).abs() < 1e-5));
    // Fibres only add wear, in a band next to it, and leave gaps (a weave, not a wider band).
    let frayed = fiber(&|_, f| { f.amount = 1.0; f.scale = 40.0; });
    assert!(frayed.iter().zip(&plain).all(|(a, b)| a.is_finite() && *a >= *b - 1e-5 && (0.0..=1.0 + 1e-5).contains(a)));
    let added = frayed.iter().zip(&plain).filter(|(a, b)| **a > **b + 0.5).count();
    let total = plain.len();
    assert!(added > total / 200, "strands show: {added}");
    assert!(added < total / 2, "the faces stay clean: {added}");
    let wider = {
        let mut g = Generator::new(GeneratorKind::Edges);
        g.edge_spread.as_mut().unwrap().distance *= FiberSettings::FRINGE * 1.45;
        fx.mask(g)
    };
    let band = wider.iter().zip(&plain).filter(|(w, p)| **w > 0.5 && **p < 0.5).count();
    assert!(added < band, "gaps stay open between the strands: {added} of {band}");
    // Scale, seed and UV / tri-planar change the weave; Invert flips the result.
    assert_ne!(fiber(&|_, f| { f.amount = 1.0; f.scale = 90.0; }), frayed);
    assert_ne!(fiber(&|_, f| { f.amount = 1.0; f.scale = 40.0; f.triplanar = false; }), frayed);
    let inverted = fiber(&|g, f| { g.invert = true; f.amount = 1.0; f.scale = 40.0; });
    // (Texels outside the UV islands hold no mask: 0 both ways.)
    assert!(inverted.iter().zip(&frayed).all(|(a, b)| (a + b - 1.0).abs() < 2e-3 || (a.abs() < 1e-3 && b.abs() < 1e-3)));
    assert!(inverted.iter().zip(&frayed).filter(|(a, b)| (*a + *b - 1.0).abs() < 2e-3).count() > total / 3);
    // The plain curvature threshold (no natural wear) frays its own falloff: never more wear.
    let old_plain = { let mut g = Generator::new(GeneratorKind::Edges); g.edge_spread = None; fx.mask(g) };
    let old = fiber(&|g, f| { g.edge_spread = None; f.amount = 1.0; });
    assert!(old.iter().zip(&old_plain).all(|(a, b)| *a <= *b + 1e-5));
}
