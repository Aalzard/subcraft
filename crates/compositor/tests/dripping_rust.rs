//! The Dripping Rust generator through the compositor: rust on a cube's edges and the streaks
//! that run down its sides.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::ChannelKind;
use subcraft_core::ChannelValue;
use subcraft_doc::{DripSettings, FillChannel, FillSource, Generator, GeneratorKind, GradientAxis, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, ResourceStore, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 256;

struct Fx {
    geo: TextureSetGeometry,
    /// Per texel of the texture: position and normal where the cube has surface.
    surface: Vec<Option<([f32; 3], [f32; 3])>>,
}

impl Fx {
    fn cube() -> Self {
        let geo = TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, RES);
        let mut surface = vec![None; (RES * RES) as usize];
        for index in 0..geo.tile_count() {
            let tile = geo.tile(index);
            if tile.is_empty() {
                continue;
            }
            for i in 0..subcraft_image::TILE_TEXELS {
                let (x, y) = tile.texel_xy(i);
                if tile.coverage(i) > 0.0 && x < RES && y < RES {
                    surface[(y * RES + x) as usize] = Some((tile.position(i), tile.normal(i)));
                }
            }
        }
        Self { geo, surface }
    }

    /// The mask `g` gives, read back from a layer it masks.
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
        let (res, maps) = (ResourceStore::default(), MeshMaps::new());
        let c = composite_full(&ts, &EvalContext { geometry: &self.geo, resources: &res, mesh_maps: &maps });
        let img = c.channel(ChannelKind::Roughness).unwrap();
        // Roughness 1 over the 0.3 default: value = 0.3 + 0.7 * mask (stored in 16 bits: a mask of 0
        // reads back within 1e-4 of it).
        (0..RES * RES).map(|i| (img.get(i % RES, i / RES)[0] - 0.3) / 0.7).collect()
    }

    /// Sum of `values` over the surface texels `keep` accepts (position, normal).
    fn sum(&self, values: &[f32], keep: impl Fn([f32; 3], [f32; 3]) -> bool) -> f32 {
        self.surface.iter().zip(values).filter_map(|(s, v)| s.filter(|(p, n)| keep(*p, *n)).map(|_| *v)).sum()
    }
}

fn rust(edit: impl Fn(&mut Generator, &mut DripSettings)) -> Generator {
    let mut g = Generator::new(GeneratorKind::DrippingRust);
    let mut d = g.drip_settings();
    edit(&mut g, &mut d);
    g.drip = Some(d);
    g
}

fn added(with: &[f32], without: &[f32]) -> Vec<f32> {
    with.iter().zip(without).map(|(a, b)| a - b).collect()
}

#[test]
fn rust_sits_on_the_edges_and_drips_run_down_the_sides_only() {
    let fx = Fx::cube();
    let plain = fx.mask(rust(|_, d| d.intensity = 0.0));
    let dripping = fx.mask(rust(|_, _| {}));
    let covered: Vec<bool> = fx.surface.iter().map(Option::is_some).collect();
    for (i, on) in covered.iter().enumerate() {
        if *on {
            assert!((-1e-4..=1.0 + 1e-4).contains(&plain[i]) && (-1e-4..=1.0 + 1e-4).contains(&dripping[i]), "texel {i}: {} {}", plain[i], dripping[i]);
            // Drips only add to the rust.
            assert!(dripping[i] >= plain[i] - 1e-5, "texel {i}: {} < {}", dripping[i], plain[i]);
        }
    }
    // There is rust, it is not everywhere, and it is denser by the edges than mid-face.
    let total = fx.sum(&vec![1.0; plain.len()], |_, _| true);
    let share = fx.sum(&plain, |_, _| true) / total;
    assert!((0.01..0.6).contains(&share), "rust covers {share}");
    let (ys, _): (Vec<f32>, Vec<f32>) = fx.surface.iter().flatten().map(|(p, _)| (p[1], 0.0)).unzip();
    let (low, high) = (ys.iter().copied().fold(f32::INFINITY, f32::min), ys.iter().copied().fold(f32::NEG_INFINITY, f32::max));
    let (mid, half) = ((low + high) * 0.5, (high - low) * 0.5);
    assert!(half > 0.1, "{low}..{high}");
    // The drips: on the sides, under the top edge; none on the level top face.
    let extra = added(&dripping, &plain);
    let top = |_: [f32; 3], n: [f32; 3]| n[1] > 0.9;
    let side = |n: [f32; 3]| n[1].abs() < 0.1;
    assert!(fx.sum(&extra, top).abs() < 1e-3, "drips on the top face: {}", fx.sum(&extra, top));
    let upper = fx.sum(&extra, |p, n| side(n) && p[1] > mid + 0.1 * half);
    let lower = fx.sum(&extra, |p, n| side(n) && p[1] < mid - 0.1 * half);
    assert!(upper > 20.0, "drips under the top edge: {upper}");
    assert!(upper > 3.0 * lower, "drips run down from the top edge, not up from the bottom one: {upper} vs {lower}");
    // No length is no drips; a longer drip reaches further.
    assert_eq!(fx.mask(rust(|_, d| d.length = 0.0)), plain);
    let long = added(&fx.mask(rust(|_, d| d.length = 0.8)), &plain);
    assert!(fx.sum(&long, |p, n| side(n) && p[1] < mid) > 2.0 * fx.sum(&extra, |p, n| side(n) && p[1] < mid) + 1.0);
}

#[test]
fn the_axis_turns_the_drips_and_the_settings_shape_them() {
    let fx = Fx::cube();
    let plain = fx.mask(rust(|_, d| d.intensity = 0.0));
    // Up is X: the faces across X are level now, the old top face is a side.
    let sideways = added(&fx.mask(rust(|g, _| g.axis = GradientAxis::X)), &plain);
    assert!(fx.sum(&sideways, |_, n| n[0] > 0.9).abs() < 1e-3);
    assert!(fx.sum(&sideways, |_, n| n[1] > 0.9) > 5.0, "{}", fx.sum(&sideways, |_, n| n[1] > 0.9));
    // More intensity, more drips; smoothness spreads them wider.
    let cover = |v: &[f32]| fx.surface.iter().zip(v).filter(|(s, v)| s.is_some() && **v > 0.05).count();
    let weak = added(&fx.mask(rust(|_, d| d.intensity = 0.3)), &plain);
    let strong = added(&fx.mask(rust(|_, d| d.intensity = 1.0)), &plain);
    assert!(fx.sum(&strong, |_, _| true) > 1.5 * fx.sum(&weak, |_, _| true));
    let thin = added(&fx.mask(rust(|_, d| d.smoothness = 0.0)), &plain);
    let wide = added(&fx.mask(rust(|_, d| d.smoothness = 1.0)), &plain);
    // (Merged streaks: less change from one texel to the next for the same amount of stain.)
    let busy = |v: &[f32]| v.windows(2).zip(fx.surface.windows(2)).filter(|(_, s)| s[0].is_some() && s[1].is_some()).map(|(v, _)| (v[0] - v[1]).abs()).sum::<f32>() / fx.sum(v, |_, _| true).max(1e-6);
    assert!(cover(&thin) > 100 && cover(&wide) > 100 && busy(&wide) < 0.8 * busy(&thin), "{} vs {}", busy(&wide), busy(&thin));
    // Rust Spreading: none at 0, more at 0.8 than at the default.
    assert!(fx.surface.iter().zip(&fx.mask(rust(|g, _| g.level = 0.0))).all(|(s, v)| s.is_none() || v.abs() < 1e-3));
    assert!(fx.sum(&fx.mask(rust(|g, d| { g.level = 0.8; d.intensity = 0.0; })), |_, _| true) > 1.5 * fx.sum(&plain, |_, _| true));
    // Rust Contrast hardens the border: fewer half-covered texels.
    let soft_texels = |v: &[f32]| fx.surface.iter().zip(v).filter(|(s, v)| s.is_some() && (0.2..0.8).contains(*v)).count();
    let hard = fx.mask(rust(|_, d| { d.contrast = 1.0; d.intensity = 0.0; }));
    let soft = fx.mask(rust(|_, d| { d.contrast = 0.0; d.intensity = 0.0; }));
    assert!(soft_texels(&hard) < soft_texels(&soft), "{} vs {}", soft_texels(&hard), soft_texels(&soft));
}

#[test]
fn it_is_repeatable_inverts_and_survives_bad_settings() {
    let fx = Fx::cube();
    let a = fx.mask(rust(|_, _| {}));
    // The second read takes the drips kept with the geometry.
    assert_eq!(fx.mask(rust(|_, _| {})), a);
    let inverted = fx.mask(rust(|g, _| g.invert = true));
    for (i, s) in fx.surface.iter().enumerate() {
        if s.is_some() {
            assert!((inverted[i] - (1.0 - a[i])).abs() < 5e-3, "texel {i} {} {} {:?}", inverted[i], a[i], s);
        }
    }
    // Another seed, another pattern.
    assert_ne!(fx.mask(rust(|g, _| g.noise.seed += 1)), a);
    // Settings out of range (a damaged file): no mask, no NaN.
    for bad in [f32::NAN, f32::INFINITY, -1.0, 4.0] {
        let m = fx.mask(rust(|_, d| d.length = bad));
        assert!(fx.surface.iter().zip(&m).all(|(s, v)| s.is_none() || v.abs() < 1e-3), "length {bad}");
    }
    let nan_level = fx.mask(rust(|g, _| { g.level = f32::NAN; g.softness = f32::NAN; g.noise_amount = f32::NAN; g.offset = [f32::NAN; 3]; }));
    assert!(nan_level.iter().all(|v| v.is_finite()));
}

#[test]
fn what_a_set_keeps_for_its_generators_is_shared_and_bounded() {
    let fx = Fx::cube();
    let made = std::sync::atomic::AtomicU32::new(0);
    let get = |key: u32| fx.geo.shared(|v: &u32| *v == key, || { made.fetch_add(1, std::sync::atomic::Ordering::Relaxed); key });
    let (a, b) = (get(7), get(7));
    assert!(Arc::ptr_eq(&a, &b) && made.load(std::sync::atomic::Ordering::Relaxed) == 1);
    // Another type with a matching test is another entry.
    let text = fx.geo.shared(|v: &String| v == "7", || "7".to_string());
    assert_eq!(*text, "7");
    // Old entries make room: after many others, 7 is built again.
    for key in 100..110 {
        assert_eq!(*get(key), key);
    }
    let before = made.load(std::sync::atomic::Ordering::Relaxed);
    assert!(*get(7) == 7 && made.load(std::sync::atomic::Ordering::Relaxed) == before + 1);
}
