//! GPU compositing equivalence (ADR-0044): the same texture set composited by the CPU reference
//! and by the GPU backend must agree within the documented tolerance. Skips without an adapter.
//!
//! Tolerance: both paths compute in f32 and round to f16 at the end; the GPU differs in the last
//! bits (fused multiply-add, pow / sqrt / division rounding), so a texel may land on the
//! neighbouring f16 value. Accepted: |gpu - cpu| <= 2^-9 * max(1, |value|) (two f16 steps near
//! 1.0), alpha included, with no NaN. Texels that differ by more than one step must be rare.

use std::sync::Arc;

use subcraft_compositor::gpu::{self, take_gpu_stats};
use subcraft_compositor::{Composite, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, IRect};
use subcraft_doc::{
    BlendMode, ContentEffect, ContentEffectKind, FillChannel, FillSource, Filter, Layer, LayerContent, Levels, Mask, MaskEffect,
    MaskEffectKind, ProceduralKind, ProceduralParams, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 256;

/// The GPU switch is process-wide: the tests take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn adapter_ok() -> bool {
    if subcraft_render::gpu_composite::create_any_backend().is_none() {
        eprintln!("no GPU adapter: skipping");
        return false;
    }
    gpu::register_gpu_compositor(|| subcraft_render::gpu_composite::create_any_backend().map(|b| b as Arc<dyn gpu::GpuCompositeBackend>));
    gpu::set_gpu_min_tiles(1);
    gpu::set_gpu_min_resolution(0);
    true
}

fn fill(ts: &TextureSet, values: &[(ChannelKind, ChannelValue)]) -> Layer {
    let mut l = Layer::new_fill("Fill", ts);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = false;
        if let Some((_, v)) = values.iter().find(|(kk, _)| kk == k) {
            *ch = FillChannel::new(FillSource::uniform(*v));
        }
    }
    l
}

fn paint(ts: &TextureSet, kind: ChannelKind, rect: IRect, value: [f32; 3], alpha: f32) -> Layer {
    let mut l = Layer::new_paint("Paint", ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let img = p.channel_mut(kind, RES);
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let a = alpha * (0.5 + 0.5 * ((x + y) as f32 * 0.05).sin());
            let v = if kind.components() == 1 { [value[0], a, 0.0, 0.0] } else { [value[0], value[1], value[2], a] };
            img.set(x as u32, y as u32, v);
        }
    }
    l
}

fn set_blend(l: &mut Layer, mode: BlendMode, opacity: f32) {
    for b in l.blend.values_mut() {
        b.mode = mode;
        b.opacity = opacity;
    }
}

fn mask_with_rect(white: bool, rect: IRect) -> Mask {
    let mut m = Mask::new(RES, white);
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            m.base.set(x as u32, y as u32, [if white { 0.2 } else { 0.9 }; 4]);
        }
    }
    m
}

/// A set exercising every supported feature.
fn rich_set() -> TextureSet {
    let mut ts = TextureSet::new("T", 0, RES);
    let base = ts.clone();
    let color = |r, g, b| (ChannelKind::BaseColor, ChannelValue::rgb(r, g, b));
    ts.insert_above(None, fill(&base, &[color(0.2, 0.5, 0.3), (ChannelKind::Roughness, ChannelValue::scalar(0.6)), (ChannelKind::Metallic, ChannelValue::scalar(0.1))]));
    // One layer per blend mode, alternating sources, masks and opacities.
    for (i, mode) in BlendMode::ALL.into_iter().filter(|m| !matches!(m, BlendMode::Disable | BlendMode::Passthrough)).enumerate() {
        let f = i as f32 / 32.0;
        let mut l = if i % 3 == 0 {
            paint(&base, ChannelKind::BaseColor, IRect::new(10 + i as i32 * 3, 20, 180 + i as i32, 230), [0.9 - f * 0.5, 0.2 + f, 0.4], 0.9)
        } else {
            fill(&base, &[color(0.8 - f * 0.4, 0.3 + f * 0.5, 0.6), (ChannelKind::Roughness, ChannelValue::scalar(0.2 + f))])
        };
        set_blend(&mut l, mode, 0.35 + 0.65 * ((i * 7 % 10) as f32 / 10.0));
        if i % 4 == 1 {
            l.mask = Some(mask_with_rect(i % 8 == 1, IRect::new(40, 40, 200, 200)));
        }
        ts.insert_above(None, l);
    }
    // Paint on scalar and normal channels, height, with a normal-map mode.
    ts.insert_above(None, paint(&base, ChannelKind::Roughness, IRect::new(0, 0, 200, 120), [0.9; 3], 1.0));
    let mut n = paint(&base, ChannelKind::Normal, IRect::new(30, 30, 220, 220), [0.7, 0.4, 0.8], 0.8);
    set_blend(&mut n, BlendMode::NormalMapCombine, 0.7);
    ts.insert_above(None, n);
    let mut n2 = paint(&base, ChannelKind::Normal, IRect::new(60, 10, 250, 190), [0.3, 0.6, 0.85], 1.0);
    set_blend(&mut n2, BlendMode::NormalMapDetail, 1.0);
    ts.insert_above(None, n2);
    let mut h = fill(&base, &[(ChannelKind::Height, ChannelValue::scalar(0.4))]);
    set_blend(&mut h, BlendMode::LinearDodge, 0.8);
    ts.insert_above(None, h);
    // Procedural fill (CPU evaluated, uploaded as a block) with a generator-free Levels mask.
    let mut proc = Layer::new_fill("Proc", &base);
    let LayerContent::Fill(f) = &mut proc.content else { unreachable!() };
    f.channels.insert(
        ChannelKind::BaseColor,
        FillChannel::new(FillSource::Procedural {
            params: ProceduralParams { scale: 6.0, ..ProceduralParams::new(ProceduralKind::Clouds) },
            low: ChannelValue::rgb(0.1, 0.1, 0.1),
            high: ChannelValue::rgb(0.9, 0.7, 0.5),
        }),
    );
    let mut m = mask_with_rect(false, IRect::new(0, 0, 256, 256));
    m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(Levels { gamma: 2.0, out_low: 0.1, ..Levels::default() }))));
    proc.mask = Some(m);
    set_blend(&mut proc, BlendMode::Overlay, 0.9);
    ts.insert_above(None, proc);
    // Content effects: GPU-side (Levels, Contrast, Threshold, HistogramScan) and CPU-side (HSL).
    for (i, kind) in [
        ContentEffectKind::Levels { channel: ChannelKind::BaseColor, levels: Levels { in_low: 0.1, in_high: 0.8, gamma: 1.4, out_low: 0.05, out_high: 0.95 } },
        ContentEffectKind::Filter { channel: ChannelKind::BaseColor, filter: Filter::Contrast { gain: 1.6, pivot: 0.5 } },
        ContentEffectKind::Filter { channel: ChannelKind::Roughness, filter: Filter::Threshold { cutoff: 0.5 } },
        ContentEffectKind::Filter { channel: ChannelKind::Roughness, filter: Filter::HistogramScan { position: 0.5, contrast: 0.3, invert: true } },
        ContentEffectKind::Filter { channel: ChannelKind::BaseColor, filter: Filter::Hsl { hue: 40.0, saturation: 1.2, lightness: 0.05 } },
    ]
    .into_iter()
    .enumerate()
    {
        let mut l = if i % 2 == 0 { paint(&base, ChannelKind::BaseColor, IRect::new(5, 5, 240, 240), [0.6, 0.3, 0.2], 1.0) } else { fill(&base, &[color(0.4, 0.7, 0.5), (ChannelKind::Roughness, ChannelValue::scalar(0.7))]) };
        l.content_effects.push(Arc::new(ContentEffect::new(kind, &base)));
        set_blend(&mut l, BlendMode::Normal, 0.8);
        ts.insert_above(None, l);
    }
    // Folders: isolated Multiply folder with children, passthrough folder with a mask, nested.
    let iso = ts.insert_above(None, Layer::new_folder("Iso", &base));
    for (j, mode) in [BlendMode::Normal, BlendMode::Screen, BlendMode::SoftLight].into_iter().enumerate() {
        let mut c = paint(&base, ChannelKind::BaseColor, IRect::new(20 * j as i32, 40, 150 + 20 * j as i32, 200), [0.3 + 0.2 * j as f32, 0.6, 0.9], 0.8);
        set_blend(&mut c, mode, 0.9);
        ts.insert(Some(iso), 0, c).unwrap();
    }
    set_blend(ts.find_mut(iso).unwrap(), BlendMode::Multiply, 0.75);
    let pass = ts.insert_above(None, Layer::new_folder("Pass", &base));
    ts.find_mut(pass).unwrap().mask = Some(mask_with_rect(true, IRect::new(60, 60, 140, 140)));
    for b in ts.find_mut(pass).unwrap().blend.values_mut() {
        b.mode = BlendMode::Passthrough;
        b.opacity = 0.6;
    }
    let inner = ts.insert(Some(pass), 0, Layer::new_folder("Inner", &base)).unwrap();
    set_blend(ts.find_mut(inner).unwrap(), BlendMode::Difference, 0.9);
    ts.insert(Some(inner), 0, fill(&base, &[color(0.9, 0.2, 0.1), (ChannelKind::Roughness, ChannelValue::scalar(0.95))])).unwrap();
    let mut pc = paint(&base, ChannelKind::BaseColor, IRect::new(50, 50, 180, 180), [0.1, 0.8, 0.4], 1.0);
    set_blend(&mut pc, BlendMode::Multiply, 1.0);
    ts.insert(Some(pass), 0, pc).unwrap();
    // A group with a Levels effect on its result.
    let eff = ts.insert_above(None, Layer::new_folder("Eff", &base));
    ts.insert(Some(eff), 0, paint(&base, ChannelKind::Roughness, IRect::new(100, 100, 250, 250), [0.8; 3], 1.0)).unwrap();
    ts.find_mut(eff).unwrap().content_effects.push(Arc::new(ContentEffect::new(
        ContentEffectKind::Levels { channel: ChannelKind::Roughness, levels: Levels { gamma: 0.7, ..Levels::default() } },
        &base,
    )));
    ts
}

struct Fixture {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

fn run(ts: &TextureSet, fx: &Fixture, gpu_on: bool) -> Composite {
    gpu::set_gpu_compositing_enabled(gpu_on);
    let ctx = EvalContext { geometry: &fx.geo, resources: &fx.res, mesh_maps: &fx.maps };
    let c = composite_full(ts, &ctx);
    gpu::set_gpu_compositing_enabled(true);
    c
}

/// (max error in f16 steps of tolerance units, texels beyond one tolerance unit, total texels)
fn compare(a: &Composite, b: &Composite) -> (f32, usize, usize) {
    let (mut worst, mut over, mut total) = (0.0f32, 0usize, 0usize);
    for (kind, ia) in &a.channels {
        let ib = b.channel(*kind).unwrap();
        for y in 0..a.resolution {
            for x in 0..a.resolution {
                let (p, q) = (ia.get(x, y), ib.get(x, y));
                for c in 0..ia.comps() {
                    assert!(q[c].is_finite(), "{kind:?} ({x},{y}) not finite: {q:?}");
                    let tol = 2f32.powi(-9) * p[c].abs().max(1.0);
                    let e = (p[c] - q[c]).abs() / tol;
                    worst = worst.max(e);
                    over += usize::from(e > 1.0);
                    total += 1;
                }
            }
        }
    }
    (worst, over, total)
}

#[test]
fn gpu_matches_cpu_on_a_rich_stack() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    if !adapter_ok() {
        return;
    }
    let fx = Fixture { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() };
    let ts = rich_set();
    let cpu = run(&ts, &fx, false);
    let _ = take_gpu_stats();
    let on_gpu = run(&ts, &fx, true);
    let stats = take_gpu_stats();
    assert!(stats.tiles > 0, "the GPU path did not run: {stats:?}");
    assert_eq!(stats.errors, 0);
    let (worst, over, total) = compare(&cpu, &on_gpu);
    println!("rich stack: worst error {worst:.2} tolerance units, {over} of {total} values beyond 1 unit; {stats:?}");
    assert!(worst <= 8.0, "worst error {worst} tolerance units");
    assert!(over * 1000 <= total, "{over} of {total} values beyond tolerance");
}

#[test]
fn each_blend_mode_matches_on_every_channel_kind() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    if !adapter_ok() {
        return;
    }
    let fx = Fixture { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() };
    let mut worst_all = 0.0f32;
    for mode in BlendMode::ALL.into_iter().filter(|m| !matches!(m, BlendMode::Disable | BlendMode::Passthrough)) {
        let mut ts = TextureSet::new("T", 0, RES);
        let base = ts.clone();
        let channels = [
            (ChannelKind::BaseColor, [0.7, 0.3, 0.5]),
            (ChannelKind::Normal, [0.62, 0.41, 0.8]),
            (ChannelKind::Roughness, [0.8, 0.0, 0.0]),
            (ChannelKind::Height, [0.3, 0.0, 0.0]),
        ];
        ts.insert_above(None, paint(&base, ChannelKind::BaseColor, IRect::new(0, 0, 256, 256), [0.25, 0.6, 0.35], 1.0));
        for (kind, v) in channels {
            ts.insert_above(None, paint(&base, kind, IRect::new(0, 0, 256, 256), [v[0] * 0.5, v[1] * 0.7, v[2] * 0.9], 0.9));
        }
        for (kind, v) in channels {
            let mut l = paint(&base, kind, IRect::new(20, 20, 230, 230), v, 0.85);
            set_blend(&mut l, mode, 0.7);
            ts.insert_above(None, l);
        }
        let cpu = run(&ts, &fx, false);
        let gpu_c = run(&ts, &fx, true);
        let (worst, over, total) = compare(&cpu, &gpu_c);
        println!("{mode:?}: worst {worst:.2} units, {over}/{total} beyond");
        worst_all = worst_all.max(worst);
        assert!(worst <= 8.0, "{mode:?}: worst error {worst} tolerance units");
        assert!(over * 1000 <= total, "{mode:?}: {over} of {total} beyond tolerance");
    }
    println!("all modes: worst {worst_all:.2} tolerance units");
}

#[test]
fn unsupported_sets_use_the_cpu_and_stay_identical() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    if !adapter_ok() {
        return;
    }
    let fx = Fixture { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() };
    let mut ts = rich_set();
    let base = ts.clone();
    let mut blur = fill(&base, &[(ChannelKind::BaseColor, ChannelValue::rgb(0.9, 0.9, 0.2))]);
    blur.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Filter { channel: ChannelKind::BaseColor, filter: Filter::Blur { radius: 3.0 } }, &base)));
    ts.insert_above(None, blur);
    let cpu = run(&ts, &fx, false);
    let _ = take_gpu_stats();
    let gpu_c = run(&ts, &fx, true);
    assert_eq!(take_gpu_stats().tiles, 0, "a set with neighborhood filters must not use the GPU");
    assert_eq!(cpu, gpu_c);
}

/// With the default thresholds a set below 2048² stays on the CPU (the GPU loses there).
#[test]
fn small_sets_stay_on_the_cpu_by_default() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    if !adapter_ok() {
        return;
    }
    gpu::set_gpu_min_tiles(gpu::DEFAULT_MIN_TILES);
    gpu::set_gpu_min_resolution(gpu::DEFAULT_MIN_RESOLUTION);
    let fx = Fixture { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() };
    let ts = rich_set();
    let cpu = run(&ts, &fx, false);
    let _ = take_gpu_stats();
    let gpu_c = run(&ts, &fx, true);
    let stats = take_gpu_stats();
    gpu::set_gpu_min_tiles(1);
    gpu::set_gpu_min_resolution(0);
    assert_eq!(stats.tiles, 0, "a {RES}² set must stay on the CPU by default");
    assert_eq!(cpu, gpu_c);
}

#[test]
fn gpu_results_are_repeatable_and_cache_hits_on_a_second_run() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    if !adapter_ok() {
        return;
    }
    let fx = Fixture { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() };
    let ts = rich_set();
    let _ = take_gpu_stats();
    let first = run(&ts, &fx, true);
    let s1 = take_gpu_stats();
    let second = run(&ts, &fx, true);
    let s2 = take_gpu_stats();
    assert_eq!(first, second, "same device, same input: identical output");
    assert!(s2.timing.cache_hits > 0 && s2.timing.cache_misses == 0, "second run should hit the tile cache: {s1:?} {s2:?}");
}
