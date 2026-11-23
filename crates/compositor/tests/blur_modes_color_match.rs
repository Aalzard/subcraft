//! Blur Slope source / mode, Non-uniform Blur and Color Match through the real compositor
//! (128 px = 2 x 2 tiles, so every check crosses tile borders).
use std::sync::Arc;
use subcraft_compositor::{EvalContext, composite_full};
use std::collections::BTreeMap;
use subcraft_core::{ChannelKind as C, ChannelValue, ResourceId};
use subcraft_doc::{BlurMap, ContentEffect, ContentEffectKind, FillChannel, FillContent, FillSource, Filter, ImageSource, Layer, Projection, LayerContent, MatchSource, ResourceStore, SlopeMode, SlopeSource, TextureSet};
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
    fn image(&mut self, w: u32, h: u32, comps: u8, data: Vec<u8>) -> ResourceId {
        let id = ResourceId::new();
        self.res.insert(subcraft_doc::Resource { id, name: "i.png".into(), kind: subcraft_doc::ResourceKind::Texture, bitmap: Arc::new(subcraft_image::Bitmap::new_u8(w, h, comps, data)), source_path: None, encoded: None });
        id
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
        // Min and Max take whole texels: at each stop (one per pixel of the way at least) the
        // texels a bilinear read there would mix.
        let samples = if dense { samples.max(len.ceil() as u32 + 1) } else { samples };
        let n = N as i32;
        let mut out = Vec::new();
        for k in 0..samples {
            let t = k as f32 / (samples - 1) as f32;
            let (ox, oy) = (dx * t, dy * t);
            if !dense {
                out.push(bilinear(data, x as f32 + ox, y as f32 + oy));
                continue;
            }
            let (x0, y0) = (ox.floor(), oy.floor());
            for ty in [Some(y0 as i32), (oy > y0).then_some(y0 as i32 + 1)].into_iter().flatten() {
                for tx in [Some(x0 as i32), (ox > x0).then_some(x0 as i32 + 1)].into_iter().flatten() {
                    out.push(data[((y as i32 + ty).clamp(0, n - 1) * n + (x as i32 + tx).clamp(0, n - 1)) as usize]);
                }
            }
        }
        out
    };
    for mode in SlopeMode::ALL {
        // Blur on the noise source is the plain filter; the option variant must agree with it.
        let filter = Filter::BlurSlopeMode { intensity, scale, seed, samples, source: SlopeSource::Noise, mode, image: None };
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
    assert_eq!(Filter::blur_slope(Filter::BlurSlopeMode { intensity, scale, seed, samples, source: SlopeSource::Noise, mode: SlopeMode::Blur, image: None }.slope_parts().unwrap()), Filter::BlurSlope { intensity, scale, seed, samples });
}

#[test]
fn blur_slope_on_the_previous_input_works_across_edges_and_leaves_flat_areas() {
    let fx = Fixture::new();
    // A bright disc on a dark ground: a closed edge in every direction, crossing the tile seams.
    let disc = |x: u32, y: u32| if ((x as f32 - 64.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt() < 30.0 { 0.9 } else { 0.1 };
    let run = |mode, intensity| {
        let (ts, data) = roughness(disc, Filter::BlurSlopeMode { intensity, scale: 4, seed: 1, samples: 12, source: SlopeSource::Input, mode, image: None });
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
        let (ts, data) = roughness(field, Filter::NonUniformBlur { radius, map, scale: 4, seed: 1, samples: 32, image: None });
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
    let (ts, _) = roughness(field, Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Noise, scale: 4, seed: 2, samples: 32, image: None });
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
    let out = run(Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target, hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None });
    let plain = run(Filter::ColorMatch { source_mode: MatchSource::Parameter, source: [0.2; 3], target: [0.2; 3], hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None });
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
    fill.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: C::BaseColor, filter: Filter::ColorMatch { source_mode: MatchSource::Average, source: [0.5; 3], target, hue: 1.0, chroma: 1.0, luma: 1.0, source_image: None, target_image: None } }, &ts)));
    ts.insert_above(None, fill);
    let c = composite_full(&ts, &fx.ctx());
    for (x, y) in [(0, 0), (63, 64), (127, 127)] {
        let p = c.channel(C::BaseColor).unwrap().get(x, y);
        assert!((0..3).all(|i| (p[i] - target[i]).abs() < 2e-3), "{p:?}");
    }
}

/// Sum of the differences between horizontal neighbours over a block: how rough it is.
fn rough(v: &[f32], xs: std::ops::Range<u32>) -> f32 {
    let mut sum = 0.0;
    for y in 8..N - 8 { for x in xs.clone() { sum += (v[(y * N + x + 1) as usize] - v[(y * N + x) as usize]).abs(); } }
    sum
}

#[test]
fn non_uniform_blur_reads_its_map_from_an_image() {
    let mut fx = Fixture::new();
    // Black over texels 16..48, white over 80..112 (texel centres of a 4 px image, clear of
    // the bilinear ramps between them).
    let map = fx.image(4, 1, 1, vec![0, 0, 255, 255]);
    let field = |x: u32, y: u32| 0.3 + speckle(x, y) * 0.4;
    let run = |fx: &Fixture, image: Option<ResourceId>| {
        let (ts, data) = roughness(field, Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Image, scale: 4, seed: 1, samples: 32, image: image.map(ImageSource::Image) });
        (result(&ts, fx), data)
    };
    let (out, data) = run(&fx, Some(map));
    for y in 0..N { for x in 16..48 { let i = (y * N + x) as usize; assert!((out[i] - data[i]).abs() < 1e-3, "black keeps {x},{y}"); } }
    assert!(rough(&out, 84..108) < rough(&data, 84..108) * 0.25, "white blurs");
    // No image picked, or one that is gone: nothing is blurred.
    for image in [None, Some(ResourceId::new())] {
        let (none, _) = run(&fx, image);
        for (a, b) in none.iter().zip(&data) { assert!((a - b).abs() < 1e-3); }
    }
    // The image is ignored (but kept) in the other map modes.
    let value = |image: Option<ResourceId>| result(&roughness(field, Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Value, scale: 4, seed: 1, samples: 32, image: image.map(ImageSource::Image) }).0, &fx);
    assert_eq!(value(Some(map)), value(None));
}

#[test]
fn blur_slope_follows_the_slopes_of_a_custom_noise_image() {
    let mut fx = Fixture::new();
    // Ramps along x and along y: their slopes point along one axis only.
    let ramp: Vec<u8> = (0..8).map(|i| i * 36).collect();
    let (across, down) = (fx.image(8, 1, 1, ramp.clone()), fx.image(1, 8, 1, ramp));
    let columns = |x: u32, _: u32| if (x / 6) % 2 == 0 { 0.2 } else { 0.8 };
    let rows = |_: u32, y: u32| if (y / 6) % 2 == 0 { 0.2 } else { 0.8 };
    let run = |fx: &Fixture, field: fn(u32, u32) -> f32, image: Option<ResourceId>, mode| {
        let (ts, data) = roughness(field, Filter::BlurSlopeMode { intensity: 8.0, scale: 4, seed: 1, samples: 12, source: SlopeSource::Custom, mode, image: image.map(ImageSource::Image) });
        let out = result(&ts, fx);
        let moved = out.iter().zip(&data).filter(|(a, b)| (**a - **b).abs() > 0.02).count();
        (out, data, moved)
    };
    for mode in SlopeMode::ALL {
        // Reading along x: columns smear, rows (constant along x) are left exactly as they are.
        assert!(run(&fx, columns, Some(across), mode).2 > 2500, "{mode:?}");
        assert_eq!(run(&fx, rows, Some(across), mode).2, 0, "{mode:?}");
        // Reading along y: the other way round.
        assert!(run(&fx, rows, Some(down), mode).2 > 2500, "{mode:?}");
        assert_eq!(run(&fx, columns, Some(down), mode).2, 0, "{mode:?}");
        // No image picked: nothing to follow.
        assert_eq!(run(&fx, columns, None, mode).2, 0, "{mode:?}");
    }
    let (min, data, _) = run(&fx, columns, Some(across), SlopeMode::Min);
    let (max, _, _) = run(&fx, columns, Some(across), SlopeMode::Max);
    let (blur, _, _) = run(&fx, columns, Some(across), SlopeMode::Blur);
    for i in 0..data.len() {
        assert!(min[i] <= data[i] + 1e-3 && max[i] >= data[i] - 1e-3 && min[i] <= blur[i] + 1e-3 && blur[i] <= max[i] + 1e-3);
        // Min and Max pick whole texels: only the two input values exist.
        for v in [min[i], max[i]] { assert!((v - 0.2).abs() < 1e-3 || (v - 0.8).abs() < 1e-3, "{v}"); }
    }
}

#[test]
fn blur_slope_min_leaves_no_ribs_on_a_thin_diagonal_line() {
    let fx = Fixture::new();
    // A one pixel bright diagonal on black, widened by Max; a dark one on white, by Min. With
    // whole-texel reads the widened line holds only the two input values, never a mix.
    for (mode, line, ground) in [(SlopeMode::Max, 0.9, 0.1), (SlopeMode::Min, 0.1, 0.9)] {
        let field = move |x: u32, y: u32| if x == y { line } else { ground };
        let (ts, data) = roughness(field, Filter::BlurSlopeMode { intensity: 6.0, scale: 3, seed: 5, samples: 6, source: SlopeSource::Noise, mode, image: None });
        let out = result(&ts, &fx);
        let grown = out.iter().filter(|v| (**v - line).abs() < 1e-3).count();
        assert!(grown > data.iter().filter(|v| **v == line).count() * 2, "{mode:?} widens the line: {grown}");
        for v in &out { assert!((v - line).abs() < 1e-3 || (v - ground).abs() < 1e-3, "{mode:?}: a mixed value {v}"); }
    }
}

fn srgb(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

#[test]
fn color_match_reads_its_source_and_target_from_images() {
    let mut fx = Fixture::new();
    // Two texels: the left one covers x < 32 unmixed, the right one x >= 96.
    let (left, right) = ([200u8, 60, 40], [40u8, 90, 180]);
    let two = fx.image(2, 1, 3, [left, right].concat());
    let (lin_l, lin_r) = (left.map(srgb), right.map(srgb));
    let a = [0.30, 0.12, 0.08];
    let target = [0.10, 0.25, 0.40];
    let run = |fx: &Fixture, paint: &dyn Fn(u32, u32) -> Option<[f32; 3]>, filter| {
        let mut ts = TextureSet::new("T", 0, N);
        let l = color_layer(&ts, paint, filter);
        ts.insert_above(None, l);
        let c = composite_full(&ts, &fx.ctx());
        let img = c.channel(C::BaseColor).unwrap();
        (0..N).flat_map(|y| (0..N).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)).collect::<Vec<_>>()
    };
    let near = |p: [f32; 4], want: [f32; 3]| (0..3).all(|i| (p[i] - want[i]).abs() < 3e-3);
    let flat = |_: u32, _: u32| Some(a);
    let cm = |source_mode, source, source_image: Option<ResourceId>, target_image: Option<ResourceId>| Filter::ColorMatch { source_mode, source, target, hue: 1.0, chroma: 1.0, luma: 1.0, source_image: source_image.map(ImageSource::Image), target_image: target_image.map(ImageSource::Image) };
    // Target image: the picked source colour (Parameter) and the mean colour (Average, the
    // staged path) both land on the image's colour at each texel.
    for mode in [MatchSource::Parameter, MatchSource::Average] {
        let out = run(&fx, &flat, cm(mode, a, None, Some(two)));
        for y in [0, 63, 64, 127] {
            assert!(near(out[(y * N + 10) as usize], lin_l), "{mode:?} {:?} vs {lin_l:?}", out[(y * N + 10) as usize]);
            assert!(near(out[(y * N + 120) as usize], lin_r), "{mode:?} {:?} vs {lin_r:?}", out[(y * N + 120) as usize]);
        }
    }
    // Source image: paint that equals the image at each texel becomes the target everywhere,
    // which no single source colour does.
    let halves = |x: u32, _: u32| Some(if x < N / 2 { lin_l } else { lin_r });
    let out = run(&fx, &halves, cm(MatchSource::Image, a, Some(two), None));
    let single = run(&fx, &halves, cm(MatchSource::Parameter, lin_l, None, None));
    for y in [0, 64, 127] {
        for x in [5, 30, 97, 125] { assert!(near(out[(y * N + x) as usize], target), "{:?}", out[(y * N + x) as usize]); }
        assert!(near(single[(y * N + 5) as usize], target) && !near(single[(y * N + 120) as usize], target));
    }
    // Both images at once: the image maps onto itself, so the paint is unchanged.
    let out = run(&fx, &halves, cm(MatchSource::Image, a, Some(two), Some(two)));
    for x in [5u32, 30, 97, 125] { assert!(near(out[(64 * N + x) as usize], halves(x, 0).unwrap())); }
    // A missing source image falls back to the source colour; a missing target image to the
    // target colour. The source image is kept but unused outside the Input mode.
    let gone = Some(ResourceId::new());
    let plain = run(&fx, &halves, cm(MatchSource::Parameter, a, None, None));
    assert_eq!(run(&fx, &halves, cm(MatchSource::Image, a, None, None)), plain);
    assert_eq!(run(&fx, &halves, cm(MatchSource::Image, a, gone, gone)), plain);
    assert_eq!(run(&fx, &halves, cm(MatchSource::Parameter, a, Some(two), None)), plain);
}

// ---- Anchor points in the image slots (ADR-0045) ----

use subcraft_doc::{AnchorIndex, AnchorInput, Mask, MaskEffect, MaskEffectKind, RefSite, RefStatus};

/// A paint layer holding `rough(x, y)` in Roughness, `height(x, y)` in Height and `color(x, y)`
/// in Base Color, every texel covered.
fn painted(ts: &TextureSet, name: &str, rough: impl Fn(u32, u32) -> f32, height: impl Fn(u32, u32) -> f32, color: impl Fn(u32, u32) -> [f32; 3]) -> Layer {
    let mut l = Layer::new_paint(name, ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    for y in 0..N {
        for x in 0..N {
            p.channel_mut(C::Roughness, N).set(x, y, [rough(x, y), 1.0, 0.0, 0.0]);
            p.channel_mut(C::Height, N).set(x, y, [height(x, y), 1.0, 0.0, 0.0]);
            let c = color(x, y);
            p.channel_mut(C::BaseColor, N).set(x, y, [c[0], c[1], c[2], 1.0]);
        }
    }
    l
}

fn anchor(l: &mut Layer, ts: &TextureSet) -> subcraft_core::EffectId {
    let e = ContentEffect::new(ContentEffectKind::Anchor, ts);
    let id = e.id;
    l.content_effects.push(Arc::new(e));
    id
}

fn filter_on(l: &mut Layer, ts: &TextureSet, channel: C, filter: Filter) -> subcraft_core::EffectId {
    let e = ContentEffect::new(ContentEffectKind::Filter { channel, filter }, ts);
    let id = e.id;
    l.content_effects.push(Arc::new(e));
    id
}

fn channel(ts: &TextureSet, fx: &Fixture, kind: C) -> Vec<[f32; 4]> {
    let c = composite_full(ts, &fx.ctx());
    let img = c.channel(kind).unwrap();
    (0..N).flat_map(|y| (0..N).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)).collect()
}

fn gray(v: Vec<[f32; 4]>) -> Vec<f32> {
    v.into_iter().map(|p| p[0]).collect()
}

fn halves(x: u32, _: u32) -> f32 {
    if x < N / 2 { 0.0 } else { 1.0 }
}

fn noisy(x: u32, y: u32) -> f32 {
    0.3 + speckle(x, y) * 0.4
}

fn nub(image: Option<ImageSource>) -> Filter {
    Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Image, scale: 4, seed: 1, samples: 32, image }
}

#[test]
fn non_uniform_blur_reads_its_map_from_an_anchor_of_a_lower_layer() {
    let fx = Fixture::new();
    let build = |source: &dyn Fn(subcraft_core::EffectId) -> Option<ImageSource>, anchor_on_top: bool| {
        let mut ts = TextureSet::new("T", 0, N);
        // The map: Roughness and Base Color black on the left, white on the right; Height the other way.
        let mut low = painted(&ts, "map", halves, |x, y| 1.0 - halves(x, y), |x, y| [halves(x, y); 3]);
        let id = anchor(&mut low, &ts);
        let mut top = painted(&ts, "top", noisy, |_, _| 0.5, |_, _| [0.5; 3]);
        let effect = filter_on(&mut top, &ts, C::Roughness, nub(source(id)));
        let top_id = top.id;
        if anchor_on_top {
            ts.insert_above(None, top);
            ts.insert_above(None, low);
        } else {
            ts.insert_above(None, low);
            ts.insert_above(None, top);
        }
        (ts, RefSite::Content { layer: top_id, effect, slot: 0 })
    };
    let data: Vec<f32> = (0..N).flat_map(|y| (0..N).map(move |x| noisy(x, y))).collect();
    let same = |out: &[f32], xs: std::ops::Range<u32>| (0..N).all(|y| xs.clone().all(|x| (out[(y * N + x) as usize] - data[(y * N + x) as usize]).abs() < 1e-3));
    // The anchor's Roughness: the left keeps, the right blurs (clear of the seam's reach).
    let (ts, site) = build(&|id| Some(ImageSource::Anchor(AnchorInput::new(id, C::Roughness))), false);
    assert_eq!(AnchorIndex::build(&ts.layers).reference(site).map(|r| r.status), Some(RefStatus::Ok));
    let out = gray(channel(&ts, &fx, C::Roughness));
    assert!(same(&out, 0..N / 2), "black keeps");
    assert!(rough(&out, 76..118) < rough(&data, 76..118) * 0.25, "white blurs");
    // Its Height is the other way round, and its Base Color is read as gray.
    let (ts, _) = build(&|id| Some(ImageSource::Anchor(AnchorInput::new(id, C::Height))), false);
    let out = gray(channel(&ts, &fx, C::Roughness));
    assert!(same(&out, N / 2..N) && rough(&out, 10..52) < rough(&data, 10..52) * 0.25, "the Height channel of the same anchor");
    let (ts, _) = build(&|id| Some(ImageSource::Anchor(AnchorInput::new(id, C::BaseColor))), false);
    let out = gray(channel(&ts, &fx, C::Roughness));
    assert!(same(&out, 0..N / 2) && rough(&out, 76..118) < rough(&data, 76..118) * 0.25, "a colour anchor reads as gray");
    // An anchor that is not below, or that is gone: reported, and nothing is blurred.
    let (ts, site) = build(&|id| Some(ImageSource::Anchor(AnchorInput::new(id, C::Roughness))), true);
    assert_eq!(AnchorIndex::build(&ts.layers).reference(site).map(|r| r.status), Some(RefStatus::NotBelow));
    // (The map layer is on top here: read the filtered layer alone.)
    let mut alone = ts.clone();
    alone.layers.retain(|l| l.name == "top");
    assert!(same(&gray(channel(&alone, &fx, C::Roughness)), 0..N));
    let (ts, site) = build(&|_| Some(ImageSource::Anchor(AnchorInput::new(subcraft_core::EffectId::new(), C::Roughness))), false);
    assert_eq!(AnchorIndex::build(&ts.layers).reference(site).map(|r| r.status), Some(RefStatus::Missing));
    assert!(same(&gray(channel(&ts, &fx, C::Roughness)), 0..N));
}

#[test]
fn a_filter_reads_an_anchor_lower_in_its_own_stack() {
    let fx = Fixture::new();
    // Dark and speckled on the left, bright and speckled on the right.
    let field = |x: u32, y: u32| if x < N / 2 { speckle(x, y) * 0.04 } else { 0.95 + speckle(x, y) * 0.05 };
    let run = |effects: &dyn Fn(&mut Layer, &TextureSet)| {
        let mut ts = TextureSet::new("T", 0, N);
        let mut l = painted(&ts, "P", field, |x, y| 1.0 - halves(x, y), |_, _| [0.5; 3]);
        effects(&mut l, &ts);
        ts.insert_above(None, l);
        (gray(channel(&ts, &fx, C::Roughness)), gray(channel(&ts, &fx, C::Height)))
    };
    // The layer's own Roughness as the map is the Value map.
    let value = run(&|l, ts| { filter_on(l, ts, C::Roughness, Filter::NonUniformBlur { radius: 8.0, map: BlurMap::Value, scale: 4, seed: 1, samples: 32, image: None }); }).0;
    let own = run(&|l, ts| {
        let id = anchor(l, ts);
        filter_on(l, ts, C::Roughness, nub(Some(ImageSource::Anchor(AnchorInput::new(id, C::Roughness)))));
    }).0;
    for (i, (a, b)) in own.iter().zip(&value).enumerate() { assert!((a - b).abs() < 1e-4, "texel {i}: {a} vs {b}"); }
    // Another channel of the same anchor (Height: white on the left), read by the Roughness filter.
    let data: Vec<f32> = (0..N).flat_map(|y| (0..N).map(move |x| field(x, y))).collect();
    let cross = run(&|l, ts| {
        let id = anchor(l, ts);
        filter_on(l, ts, C::Roughness, nub(Some(ImageSource::Anchor(AnchorInput::new(id, C::Height)))));
    }).0;
    assert!(rough(&cross, 10..52) < rough(&data, 10..52) * 0.25, "the left blurs");
    for y in 0..N { for x in N / 2 + 9..N { let i = (y * N + x) as usize; assert!((cross[i] - data[i]).abs() < 2e-3, "the right keeps"); } }
    // Two channels reading each other's anchor: both resolve (and the build ends).
    let (r, h) = run(&|l, ts| {
        let id = anchor(l, ts);
        filter_on(l, ts, C::Roughness, nub(Some(ImageSource::Anchor(AnchorInput::new(id, C::Height)))));
        filter_on(l, ts, C::Height, nub(Some(ImageSource::Anchor(AnchorInput::new(id, C::Roughness)))));
    });
    for (a, b) in r.iter().zip(&cross) { assert!((a - b).abs() < 1e-4); }
    // The Height is two flat halves: a blur only softens the seam, on the side the map is white.
    assert!(h.iter().all(|v| (-1e-3..=1.001).contains(v)) && (h[(64 * N + 10) as usize] - 1.0).abs() < 1e-3 && h[(64 * N + 120) as usize].abs() < 1e-3);
    // An anchor above the filter in the same stack is not below it: ignored.
    let above = run(&|l, ts| {
        let slot = filter_on(l, ts, C::Roughness, nub(None));
        let id = anchor(l, ts);
        let e = Arc::make_mut(l.content_effects.iter_mut().find(|e| e.id == slot).unwrap());
        let ContentEffectKind::Filter { filter, .. } = &mut e.kind else { unreachable!() };
        assert!(filter.set_anchor(0, AnchorInput::new(id, C::Roughness)));
    }).0;
    for (a, b) in above.iter().zip(&data) { assert!((a - b).abs() < 1e-3); }
}

#[test]
fn blur_slope_follows_an_anchor_as_its_custom_noise() {
    let fx = Fixture::new();
    let columns = |x: u32, _: u32| if (x / 6) % 2 == 0 { 0.2 } else { 0.8 };
    let rows = |_: u32, y: u32| if (y / 6) % 2 == 0 { 0.2 } else { 0.8 };
    let run = |field: fn(u32, u32) -> f32, along_x: bool, mode| {
        let mut ts = TextureSet::new("T", 0, N);
        // Saw teeth 32 texels long: a slope of 1/32 per texel along one axis only.
        let teeth = move |x: u32, y: u32| ((if along_x { x } else { y }) % 32) as f32 / 32.0;
        let mut low = painted(&ts, "noise", teeth, |_, _| 0.0, |_, _| [0.5; 3]);
        let id = anchor(&mut low, &ts);
        let mut top = painted(&ts, "top", field, |_, _| 0.5, |_, _| [0.5; 3]);
        filter_on(&mut top, &ts, C::Roughness, Filter::BlurSlopeMode { intensity: 8.0, scale: 4, seed: 1, samples: 12, source: SlopeSource::Custom, mode, image: Some(ImageSource::Anchor(AnchorInput::new(id, C::Roughness))) });
        ts.insert_above(None, low);
        ts.insert_above(None, top);
        let out = gray(channel(&ts, &fx, C::Roughness));
        (0..N * N).filter(|i| (out[*i as usize] - field(i % N, i / N)).abs() > 0.02).count()
    };
    for mode in SlopeMode::ALL {
        assert!(run(columns, true, mode) > 2500, "{mode:?}");
        assert_eq!(run(rows, true, mode), 0, "{mode:?}");
        assert!(run(rows, false, mode) > 2500, "{mode:?}");
        assert_eq!(run(columns, false, mode), 0, "{mode:?}");
    }
}

#[test]
fn color_match_reads_its_source_and_target_from_anchors() {
    let fx = Fixture::new();
    let (left, right) = ([0.55, 0.05, 0.02], [0.02, 0.10, 0.45]);
    let two = move |x: u32, _: u32| if x < N / 2 { left } else { right };
    let a = [0.30, 0.12, 0.08];
    let target = [0.10, 0.25, 0.40];
    let near = |p: [f32; 4], want: [f32; 3]| (0..3).all(|i| (p[i] - want[i]).abs() < 3e-3);
    let run = |paint: &dyn Fn(u32, u32) -> [f32; 3], filter: &dyn Fn(subcraft_core::EffectId) -> Filter| {
        let mut ts = TextureSet::new("T", 0, N);
        let mut low = painted(&ts, "colours", |_, _| 0.5, |_, _| 0.0, two);
        let id = anchor(&mut low, &ts);
        let mut top = painted(&ts, "top", |_, _| 0.5, |_, _| 0.0, paint);
        filter_on(&mut top, &ts, C::BaseColor, filter(id));
        ts.insert_above(None, low);
        ts.insert_above(None, top);
        channel(&ts, &fx, C::BaseColor)
    };
    let cm = |source_mode, source, source_image, target_image| Filter::ColorMatch { source_mode, source, target, hue: 1.0, chroma: 1.0, luma: 1.0, source_image, target_image };
    let from = |id| Some(ImageSource::Anchor(AnchorInput::new(id, C::BaseColor)));
    // Target anchor: flat paint lands on the anchor's colour at each texel, with a picked source
    // colour and with the average.
    for mode in [MatchSource::Parameter, MatchSource::Average] {
        let out = run(&|_, _| a, &|id| cm(mode, a, None, from(id)));
        for y in [0, 63, 64, 127] {
            for (x, want) in [(0, left), (63, left), (64, right), (127, right)] { assert!(near(out[(y * N + x) as usize], want), "{mode:?} {x},{y}: {:?}", out[(y * N + x) as usize]); }
        }
    }
    // Source anchor: paint equal to the anchor becomes the target everywhere.
    let out = run(&two, &|id| cm(MatchSource::Image, a, from(id), None));
    for i in [0usize, 63, 64, 127, (N * N - 1) as usize] { assert!(near(out[i], target), "{:?}", out[i]); }
    // Source and target on the same anchor: unchanged.
    let out = run(&two, &|id| cm(MatchSource::Image, a, from(id), from(id)));
    for x in [0u32, 63, 64, 127] { assert!(near(out[(70 * N + x) as usize], two(x, 0))); }
    // A source anchor outside the Input mode is kept but not read.
    let plain = run(&two, &|_| cm(MatchSource::Parameter, a, None, None));
    assert_eq!(run(&two, &|id| cm(MatchSource::Parameter, a, from(id), None)), plain);
}

#[test]
fn a_mask_filter_reads_an_anchor_in_its_image_slot() {
    let fx = Fixture::new();
    let build = |filter: Option<&dyn Fn(subcraft_core::EffectId) -> Filter>| {
        let mut ts = TextureSet::new("T", 0, N);
        // The map is the lower layer's Height; its Roughness is 0 everywhere.
        let mut low = painted(&ts, "map", |_, _| 0.0, halves, |_, _| [0.5; 3]);
        let id = anchor(&mut low, &ts);
        // Roughness 1 over the map layer, through a speckled mask.
        let mut top = painted(&ts, "top", |_, _| 1.0, |_, _| 0.0, |_, _| [0.5; 3]);
        let mut m = Mask::new(N, false);
        for y in 0..N { for x in 0..N { m.base.set(x, y, [noisy(x, y), 0.0, 0.0, 0.0]); } }
        if let Some(f) = filter { m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(f(id))))); }
        top.mask = Some(m);
        ts.insert_above(None, low);
        ts.insert_above(None, top);
        gray(channel(&ts, &fx, C::Roughness))
    };
    let plain = build(None);
    let out = build(Some(&|id| nub(Some(ImageSource::Anchor(AnchorInput::new(id, C::Height))))));
    // Left: the map is black, the mask is untouched. Right: the mask is blurred.
    for y in 0..N { for x in 0..N / 2 { let i = (y * N + x) as usize; assert!((out[i] - plain[i]).abs() < 1e-3, "at {x},{y}"); } }
    assert!(rough(&out, 76..118) < rough(&plain, 76..118) * 0.25);
    // A missing anchor: the filter changes nothing.
    let gone = build(Some(&|_| nub(Some(ImageSource::Anchor(AnchorInput::new(subcraft_core::EffectId::new(), C::Roughness))))));
    for (a, b) in gone.iter().zip(&plain) { assert!((a - b).abs() < 1e-3); }
}
