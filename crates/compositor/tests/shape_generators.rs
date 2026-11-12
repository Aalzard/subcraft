//! Position, UV and conversion generators (3D Distance, 3D Linear Gradient, UV Random Color,
//! Grayscale Conversion) and the Light's angles and attenuation, read back from a masked layer.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, PartKind, ResourceId};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, GrayscaleType, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, Resource, ResourceKind, ResourceStore, ShapeSettings, TextureSet};
use subcraft_image::{Bitmap, TILE_SIZE};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

struct Fx {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fx {
    fn plane() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
    /// The mask `g` gives, read back from a roughness-1 fill it masks (over the 0.3 default).
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
        let c = composite_full(&ts, &self.ctx());
        let img = c.channel(ChannelKind::Roughness).unwrap();
        (0..RES * RES).map(|i| (img.get(i % RES, i / RES)[0] - 0.3) / 0.7).collect()
    }
    /// Per texel (row by row): the model position in box coordinates (0..1 per axis, a flat axis
    /// 0.5) and in units of the longest side, and the UV island, where the mesh covers the texel.
    fn texels(&self) -> Vec<Option<([f32; 3], [f32; 3], Option<u32>)>> {
        let ext = self.geo.bounds.extent().to_array();
        let max = self.geo.max_extent;
        let mut out = vec![None; (RES * RES) as usize];
        let tiles = RES.div_ceil(TILE_SIZE);
        for t in 0..(tiles * tiles) as usize {
            let g = self.geo.tile(t);
            for i in 0..(TILE_SIZE * TILE_SIZE) as usize {
                let (x, y) = g.texel_xy(i);
                if x >= RES || y >= RES || g.coverage(i) <= 0.0 {
                    continue;
                }
                let p = g.position(i);
                let boxed = std::array::from_fn(|a| if ext[a] > max * 1e-6 { p[a] * max / ext[a] } else { 0.5 });
                out[(y * RES + x) as usize] = Some((boxed, p, g.region(i, PartKind::UvIsland)));
            }
        }
        out
    }
}

fn shaped(kind: GeneratorKind, s: ShapeSettings) -> Generator {
    let mut g = Generator::new(kind);
    g.shape = Some(s);
    g
}

#[test]
fn linear_gradient_follows_the_model_between_its_two_points() {
    let fx = Fx::plane();
    let s = ShapeSettings { start: [0.0, 0.5, 0.5], end: [1.0, 0.5, 0.5], ..ShapeSettings::default() };
    let m = fx.mask(shaped(GeneratorKind::LinearGradient3D, s));
    let mut seen = (1.0f32, 0.0f32);
    for (i, t) in fx.texels().into_iter().enumerate() {
        let Some((boxed, _, _)) = t else { continue };
        // Along X: the box coordinate itself (Balance 0.5, Contrast 0 change nothing).
        assert!((m[i] - boxed[0].clamp(0.0, 1.0)).abs() < 3e-3, "texel {i}: {} vs {}", m[i], boxed[0]);
        seen = (seen.0.min(m[i]), seen.1.max(m[i]));
    }
    assert!(seen.0 < 0.05 && seen.1 > 0.95, "{seen:?}");
    // Invert and a hard Contrast: a step at the middle.
    let mut g = shaped(GeneratorKind::LinearGradient3D, ShapeSettings { contrast: 1.0, ..s });
    g.invert = true;
    let m = fx.mask(g);
    for (i, t) in fx.texels().into_iter().enumerate() {
        let Some((boxed, _, _)) = t else { continue };
        if (boxed[0] - 0.5).abs() > 0.02 {
            assert_eq!(m[i].round(), if boxed[0] >= 0.5 { 0.0 } else { 1.0 }, "texel {i}");
        }
    }
}

#[test]
fn distance_3d_is_white_at_its_point_and_fades_over_its_radius() {
    let fx = Fx::plane();
    let s = ShapeSettings { point: [0.5, 0.5, 0.5], radius: 0.3, offset: 0.1, ..ShapeSettings::default() };
    let m = fx.mask(shaped(GeneratorKind::Distance3D, s));
    let ext = fx.geo.bounds.extent().to_array();
    let c: [f32; 3] = std::array::from_fn(|a| 0.5 * ext[a] / fx.geo.max_extent);
    let (mut inside, mut outside) = (0, 0);
    for (i, t) in fx.texels().into_iter().enumerate() {
        let Some((_, p, _)) = t else { continue };
        let d = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt();
        let expected = (1.0 - (d - 0.1) / 0.3).clamp(0.0, 1.0);
        assert!((m[i] - expected).abs() < 3e-3, "texel {i}: {} vs {expected}", m[i]);
        if d < 0.1 { inside += 1 }
        if d > 0.4 { outside += 1 }
    }
    assert!(inside > 100 && outside > 100, "{inside} {outside}");
}

#[test]
fn uv_random_color_is_one_value_per_island_and_follows_the_seed() {
    let fx = Fx::plane();
    let s = ShapeSettings::default();
    let a = fx.mask(shaped(GeneratorKind::UvRandomColor, s));
    let b = fx.mask(shaped(GeneratorKind::UvRandomColor, ShapeSettings { seed: 7, ..s }));
    let texels = fx.texels();
    for (i, t) in texels.iter().enumerate() {
        let Some((_, _, island)) = t else { continue };
        let island = island.expect("the plane has a UV island");
        assert!((a[i] - s.island_value(island)).abs() < 2e-3);
        assert!((b[i] - ShapeSettings { seed: 7, ..s }.island_value(island)).abs() < 2e-3);
    }
}

#[test]
fn grayscale_conversion_reads_its_image_through_tile_and_type() {
    let mut fx = Fx::plane();
    let id = ResourceId::new();
    // Left half dark red (0.4, 0, 0), right half white.
    fx.res.insert(Resource { id, name: "t.png".into(), kind: ResourceKind::Texture, bitmap: Arc::new(Bitmap::new_u8(4, 1, 3, vec![102, 0, 0, 102, 0, 0, 255, 255, 255, 255, 255, 255])), source_path: None, encoded: None });
    let s = ShapeSettings { texture: Some(id), grayscale: GrayscaleType::Red, ..ShapeSettings::default() };
    let m = fx.mask(shaped(GeneratorKind::GrayscaleConversion, s));
    let at = |x: u32| m[(64 * RES + x) as usize];
    assert!((at(20) - 0.4).abs() < 3e-3 && (at(110) - 1.0).abs() < 3e-3, "{} {}", at(20), at(110));
    // Green channel: 0 on the red half.
    let m = fx.mask(shaped(GeneratorKind::GrayscaleConversion, ShapeSettings { grayscale: GrayscaleType::Green, ..s }));
    assert!(m[(64 * RES + 20) as usize].abs() < 3e-3);
    // Without an image: black (inverted: white).
    let mut g = shaped(GeneratorKind::GrayscaleConversion, ShapeSettings { texture: None, ..s });
    assert!(fx.mask(g).iter().all(|v| v.abs() < 3e-3));
    g.invert = true;
    assert!(fx.mask(g).iter().all(|v| (v - 1.0).abs() < 3e-3));
}

#[test]
fn light_attenuation_fades_toward_the_far_side() {
    let fx = Fx::plane();
    // Light from +X, level 0.5: an upward-facing plane is half lit everywhere ...
    let mut g = Generator::new(GeneratorKind::Directional);
    g.direction = subcraft_doc::light_direction(90.0, 0.0);
    let flat = fx.mask(g);
    // ... and with full attenuation, darker toward -X.
    g.shape = Some(ShapeSettings { attenuation: 1.0, ..ShapeSettings::default() });
    let faded = fx.mask(g);
    for (i, t) in fx.texels().into_iter().enumerate() {
        let Some((boxed, _, _)) = t else { continue };
        let t = ((boxed[0] - 0.5) / 0.866_025_4 * 0.5 + 0.5).clamp(0.0, 1.0);
        assert!((faded[i] - flat[i] * t).abs() < 3e-3, "texel {i}: {} vs {}", faded[i], flat[i] * t);
    }
}
