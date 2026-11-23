//! Blur Slope source / mode, Non-uniform Blur and Color Match through the real compositor
//! (128 px = 2 x 2 tiles, so every check crosses tile borders).
use std::sync::Arc;
use subcraft_compositor::{EvalContext, composite_full};
use std::collections::BTreeMap;
use subcraft_core::{ChannelKind as C, ChannelValue};
use subcraft_doc::{BlurMap, ContentEffect, ContentEffectKind, FillChannel, FillContent, FillSource, Filter, Layer, Projection, LayerContent, MatchSource, ResourceStore, SlopeMode, SlopeSource, TextureSet};
use subcraft_materials::{MeshMaps, TextureSetGeometry, filter::{linear_to_oklab, warp}};

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

/// A Roughness paint layer holding `value(x, y)`, with `filter` on it.
fn roughness(value: impl Fn(u32, u32) -> f32, filter: Filter) -> (TextureSet, Vec<f32>) {
    let mut ts = TextureSet::new("T", 0, N);
    let mut l = Layer::new_paint("P", &ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let image = p.channel_mut(C::Roughness, N);
    let mut data = Vec::new();
    for y in 0..N {
        for x in 0..N {
            let v = value(x, y);
            image.set(x, y, [v, 1.0, 0.0, 0.0]);
            data.push(v);
        }
    }
    l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::Roughness, filter }, &ts)));
    ts.insert_above(None, l);
    (ts, data)
}

fn result(ts: &TextureSet, fx: &Fixture) -> Vec<f32> {
    let c = composite_full(ts, &fx.ctx());
    let img = c.channel(C::Roughness).unwrap();
    (0..N).flat_map(|y| (0..N).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)[0]).collect()
}

fn speckle(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h % 1000) as f32 / 999.0
}

fn bilinear(data: &[f32], x: f32, y: f32) -> f32 {
    let n = N as i32;
    let at = |x: i32, y: i32| data[(y.clamp(0, n - 1) * n + x.clamp(0, n - 1)) as usize];
    let (sx, sy) = (x.clamp(0.0, (N - 1) as f32), y.clamp(0.0, (N - 1) as f32));
    let (x0, y0) = (sx.floor() as i32, sy.floor() as i32);
    let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
    let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

#[test]
fn blur_slope_modes_on_the_noise_source_match_a_dense_reference() {
    let fx = Fixture::new();
    let field = |x: u32, y: u32| (x as f32 / N as f32) * 0.5 + speckle(x, y) * 0.4;
    let (intensity, scale, seed, samples) = (6.0, 3, 5, 6);
    let reads = |data: &[f32], x: u32, y: u32, dense: bool| -> Vec<f32> {
        let [dx, dy] = warp::offset(x, y, N, intensity, scale, seed);
        let len = (dx * dx + dy * dy).sqrt();
        // Min and Max read at least once per pixel of the way, on a band 0.75 px to each side.
        let samples = if dense { samples.max(len.ceil() as u32 + 1) } else { samples };
        let mut out = Vec::new();
        for k in 0..samples {
            let t = k as f32 / (samples - 1) as f32;
            out.push(bilinear(data, x as f32 + dx * t, y as f32 + dy * t));
            if dense && len > 1e-6 && (dx * t != 0.0 || dy * t != 0.0) {
                for w in [0.75f32, -0.75] {
                    out.push(bilinear(data, x as f32 + dx * t - dy / len * w, y as f32 + dy * t + dx / len * w));
                }
            }
        }
        out
    };
    for mode in SlopeMode::ALL {
        // Blur on the noise source is the plain filter; the option variant must agree with it.
        let filter = Filter::BlurSlopeMode { intensity, scale, seed, samples, source: SlopeSource::Noise, mode };
        let (ts, data) = roughness(field, filter);
        let out = result(&ts, &fx);
        let mut moved = 0;
        for y in 0..N {
            for x in 0..N {
                let r = reads(&data, x, y, mode != SlopeMode::Blur);
                let want = match mode {
                    SlopeMode::Blur => r.iter().sum::<f32>() / r.len() as f32,
                    SlopeMode::Min => r.iter().copied().fold(f32::MAX, f32::min),
                    SlopeMode::Max => r.iter().copied().fold(f32::MIN, f32::max),
                };
                let got = out[(y * N + x) as usize];
                assert!((got - want).abs() < 0.0008, "{mode:?} at {x},{y}: {got} vs {want}");
                if (got - data[(y * N + x) as usize]).abs() > 0.02 { moved += 1; }
            }
        }
        assert!(moved > 2000, "{mode:?} changed real texels: {moved}");
    }
    assert_eq!(Filter::blur_slope(Filter::BlurSlopeMode { intensity, scale, seed, samples, source: SlopeSource::Noise, mode: SlopeMode::Blur }.slope_parts().unwrap()), Filter::BlurSlope { intensity, scale, seed, samples });
}

#[test]
fn blur_slope_on_the_previous_input_works_across_edges_and_leaves_flat_areas() {
    let fx = Fixture::new();
    // A bright disc on a dark ground: a closed edge in every direction, crossing the tile seams.
    let disc = |x: u32, y: u32| if ((x as f32 - 64.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt() < 30.0 { 0.9 } else { 0.1 };
    let run = |mode, intensity| {
        let (ts, data) = roughness(disc, Filter::BlurSlopeMode { intensity, scale: 4, seed: 1, samples: 12, source: SlopeSource::Input, mode });
        (result(&ts, &fx), data)
    };
    let bright = |v: &[f32]| v.iter().filter(|v| **v > 0.5).count();
    let (blur, data) = run(SlopeMode::Blur, 6.0);
    let (min, _) = run(SlopeMode::Min, 6.0);
    let (max, _) = run(SlopeMode::Max, 6.0);
    let (off, _) = run(SlopeMode::Blur, 0.0);
    let mut soft = 0;
    for y in 0..N {
        for x in 0..N {
            let i = (y * N + x) as usize;
            let d = (((x as f32 - 64.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt() - 30.0).abs();
            assert!((off[i] - data[i]).abs() < 1e-3, "no intensity is the identity");
            for out in [&blur, &min, &max] {
                assert!(out[i] > 0.099 && out[i] < 0.901, "stays inside the input range");
                // Flat areas have no slope to follow: untouched beyond the reach of the walk.
                if d > 9.0 { assert!((out[i] - data[i]).abs() < 1e-3, "at {x},{y}: {} vs {}", out[i], data[i]); }
            }
            assert!(min[i] <= data[i] + 1e-3 && max[i] >= data[i] - 1e-3 && min[i] <= blur[i] + 1e-3 && blur[i] <= max[i] + 1e-3);
            if blur[i] > 0.2 && blur[i] < 0.8 { soft += 1; }
        }
    }
    // Max grows the disc, Min shrinks it, by a ring a few pixels wide; Blur softens the edge.
    let (base, grown, shrunk) = (bright(&data), bright(&max), bright(&min));
    assert!(grown > base + 400 && shrunk + 400 < base, "{shrunk} < {base} < {grown}");
    assert!(soft > 300, "Blur leaves a soft edge: {soft}");
}

#[test]
fn non_uniform_blur_follows_its_map() {
    let fx = Fixture::new();
    // Left half dark (0..0.04), right half bright (0.95..1), both speckled.
    let field = |x: u32, y: u32| if x < N / 2 { speckle(x, y) * 0.04 } else { 0.95 + speckle(x, y) * 0.05 };
    let rough = |v: &[f32], x0: u32, x1: u32| {
        let mut sum = 0.0;
        for y in 8..N - 8 { for x in x0..x1 { sum += (v[(y * N + x + 1) as usize] - v[(y * N + x) as usize]).abs(); } }
        sum
    };
    let run = |map, radius| {
        let (ts, data) = roughness(field, Filter::NonUniformBlur { radius, map, scale: 4, seed: 1, samples: 32 });
        (result(&ts, &fx), data)
    };
    let (out, data) = run(BlurMap::Value, 8.0);
    // Dark side: the radius stays under half a pixel, so it is exactly the input.
    for y in 0..N { for x in 0..N / 2 { let i = (y * N + x) as usize; assert!((out[i] - data[i]).abs() < 1e-3, "at {x},{y}"); } }
    // Bright side: the speckle is smoothed away (well inside it, clear of the dark half).
    let (before, after) = (rough(&data, 80, 118), rough(&out, 80, 118));
    assert!(after < before * 0.25, "{after} vs {before}");
    for y in 8..N - 8 { for x in 80..118 { let v = out[(y * N + x) as usize]; assert!(v > 0.949 && v < 1.0, "a blur stays inside the range it averages: {v}"); } }
    // Inverted: the dark side is the smoothed one, the bright side is untouched.
    let (inv, _) = run(BlurMap::InvertedValue, 8.0);
    assert!(rough(&inv, 10, 48) < rough(&data, 10, 48) * 0.25);
    for y in 0..N { for x in N / 2 + 9..N { let i = (y * N + x) as usize; assert!((inv[i] - data[i]).abs() < 2e-3, "at {x},{y}: {} {}", inv[i], data[i]); } }
    // Noise: some texels blurred, some kept, and the seed moves the patches.
    let (noise, _) = run(BlurMap::Noise, 8.0);
    let kept = noise.iter().zip(&data).filter(|(a, b)| (**a - **b).abs() < 1e-3).count();
    assert!(kept > 300 && kept < (N * N) as usize - 3000, "{kept}");
    let (ts, _) = roughness(field, Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Noise, scale: 4, seed: 2, samples: 32 });
    assert!(result(&ts, &fx).iter().zip(&noise).filter(|(a, b)| (**a - **b).abs() > 1e-3).count() > 500);
    // Deterministic, and no radius is the identity.
    assert_eq!(run(BlurMap::Value, 8.0).0, out);
    let (zero, _) = run(BlurMap::Value, 0.0);
    for (a, b) in zero.iter().zip(&data) { assert!((a - b).abs() < 1e-3); }
}

fn color_layer(ts: &TextureSet, paint: impl Fn(u32, u32) -> Option<[f32; 3]>, filter: Filter) -> Layer {
    let mut l = Layer::new_paint("P", ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    for y in 0..N { for x in 0..N { if let Some(c) = paint(x, y) { p.channel_mut(C::BaseColor, N).set(x, y, [c[0], c[1], c[2], 1.0]); } } }
    l.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::BaseColor, filter }, ts)));
    l
}

#[test]
fn color_match_moves_the_average_to_the_target_and_keeps_the_variation() {
    let fx = Fixture::new();
    let (a, b) = ([0.30, 0.12, 0.08], [0.42, 0.20, 0.10]);
    let target = [0.10, 0.25, 0.40];
    // Two browns in stripes over three quarters of the texture; the rest is left unpainted.
    let paint = |x: u32, y: u32| (x < N * 3 / 4).then(|| if (x / 5 + y / 9) % 2 == 0 { a } else { b });
    let run = |filter| {
        let mut ts = TextureSet::new("T", 0, N);
        let l = color_layer(&ts, paint, filter);
        ts.insert_above(None, l);
        let c = composite_full(&ts, &fx.ctx());
        let img = c.channel(C::BaseColor).unwrap();
        (0..N).flat_map(|y| (0..N).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)).collect::<Vec<_>>()
    };
    let lab_mean = |px: &[[f32; 4]]| {
        let (mut sum, mut n) = ([0.0f32; 3], 0.0);
        for (_, p) in px.iter().enumerate().filter(|(i, _)| paint(*i as u32 % N, *i as u32 / N).is_some()) { let l = linear_to_oklab([p[0], p[1], p[2]]); for c in 0..3 { sum[c] += l[c]; } n += 1.0; }
        sum.map(|v| v / n)
    };
    let out = run(Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target, hue: 1.0, chroma: 1.0, luma: 1.0 });
    let plain = run(Filter::ColorMatch { source_mode: MatchSource::Parameter, source: [0.2; 3], target: [0.2; 3], hue: 1.0, chroma: 1.0, luma: 1.0 });
    // The identity leaves the paint and its coverage alone.
    for (x, p) in plain.iter().enumerate() {
        let (px, py) = (x as u32 % N, x as u32 / N);
        if let Some(c) = paint(px, py) { assert!((0..3).all(|i| (p[i] - c[i]).abs() < 2e-3), "{p:?}"); }
    }
    // Unpainted texels stay as they were.
    for (i, (p, q)) in out.iter().zip(&plain).enumerate() { if paint(i as u32 % N, i as u32 / N).is_none() { assert_eq!(p, q); } }
    // The two colours stay two colours, the same OKLab distance apart, and their mean is the
    // target moved by exactly the offset between the input's mean colour and its OKLab mean.
    let (oa, ob) = (out[0], out[5]);
    let dist = |p: [f32; 3], q: [f32; 3]| { let (p, q) = (linear_to_oklab(p), linear_to_oklab(q)); (0..3).map(|i| (p[i] - q[i]).powi(2)).sum::<f32>().sqrt() };
    assert!((dist([oa[0], oa[1], oa[2]], [ob[0], ob[1], ob[2]]) - dist(a, b)).abs() < 4e-3, "variation kept");
    let painted: Vec<_> = plain.iter().enumerate().filter(|(i, _)| paint(*i as u32 % N, *i as u32 / N).is_some()).map(|(_, p)| *p).collect();
    let mut mean = [0.0f32; 3];
    for p in &painted { for c in 0..3 { mean[c] += p[c] / painted.len() as f32; } }
    let (shift_from, shift_to) = (linear_to_oklab(mean), linear_to_oklab(target));
    let (before, after) = (lab_mean(&plain), lab_mean(&out));
    for c in 0..3 { assert!((after[c] - (before[c] + shift_to[c] - shift_from[c])).abs() < 4e-3, "{after:?} {before:?}"); }
    // The result is near the target colour (the average of two close colours moved onto it).
    let near = |p: [f32; 4]| dist([p[0], p[1], p[2]], target);
    assert!(near(oa) < 0.08 && near(ob) < 0.08 && dist(a, target) > 2.0 * near(oa), "{} {} {}", near(oa), near(ob), dist(a, target));

    // A flat fill has no variation: Average turns it into exactly the target.
    let mut ts = TextureSet::new("T", 0, N);
    let mut fill = Layer::new_fill("F", &ts);
    fill.content = LayerContent::Fill(FillContent { projection: Projection::default(), channels: BTreeMap::from([(C::BaseColor, FillChannel::new(FillSource::uniform(ChannelValue::rgb(a[0], a[1], a[2]))))]) });
    fill.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::BaseColor, filter: Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target, hue: 1.0, chroma: 1.0, luma: 1.0 } }, &ts)));
    ts.insert_above(None, fill);
    let c = composite_full(&ts, &fx.ctx());
    for (x, y) in [(0, 0), (63, 64), (127, 127)] {
        let p = c.channel(C::BaseColor).unwrap().get(x, y);
        assert!((0..3).all(|i| (p[i] - target[i]).abs() < 2e-3), "{p:?}");
    }
}
