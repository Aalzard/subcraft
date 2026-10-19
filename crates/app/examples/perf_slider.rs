//! Measures what a mask-generator slider drag costs: the demo layer stack (wood + moss fills,
//! directional generator with noise breakup, painted layer) on the sample crate.
//!
//! `cargo run --release -p subcraft-app --example perf_slider -- [resolution]`
#![allow(unsafe_code)]
use std::time::Instant;

/// Opt out of Windows power throttling (EcoQoS) for repeatable benchmark numbers.
#[cfg(windows)]
fn no_throttle() {
    #[repr(C)]
    struct State { version: u32, control: u32, state: u32 }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn SetProcessInformation(h: isize, class: i32, info: *const core::ffi::c_void, size: u32) -> i32;
    }
    let s = State { version: 1, control: 1, state: 0 };
    let ok = unsafe { SetProcessInformation(GetCurrentProcess(), 4, &s as *const State as *const _, 12) };
    assert_eq!(ok, 1);
}

#[cfg(not(windows))]
fn no_throttle() {}

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, LayerContent, MaskEffectKind, ProceduralKind, ProceduralParams};

fn demo_session(res: u32) -> (Session, LayerId, LayerId) {
    let mut s = Session::new(subcraft_mesh::primitives::sample_crate(), res);
    let paint = s.selection().layer.unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let base = s.selection().layer.unwrap();
    s.execute(Command::MoveDown(base)).unwrap();
    let wood = FillSource::Procedural {
        params: ProceduralParams { scale: 3.0, ..ProceduralParams::new(ProceduralKind::WoodRings) },
        low: ChannelValue::rgb(0.18, 0.09, 0.04),
        high: ChannelValue::rgb(0.45, 0.25, 0.11),
    };
    s.execute(Command::SetFillChannel { layer: base, channel: ChannelKind::BaseColor, fill: FillChannel::new(wood) }).unwrap();
    let rough = FillSource::Procedural {
        params: ProceduralParams { scale: 8.0, ..ProceduralParams::new(ProceduralKind::Grunge) },
        low: ChannelValue::scalar(0.45),
        high: ChannelValue::scalar(0.8),
    };
    s.execute(Command::SetFillChannel { layer: base, channel: ChannelKind::Roughness, fill: FillChannel::new(rough) }).unwrap();
    s.execute(Command::SelectLayer(Some(paint))).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let moss = s.selection().layer.unwrap();
    let moss_color = FillSource::Procedural {
        params: ProceduralParams { scale: 6.0, ..ProceduralParams::new(ProceduralKind::Clouds) },
        low: ChannelValue::rgb(0.06, 0.12, 0.03),
        high: ChannelValue::rgb(0.2, 0.35, 0.08),
    };
    s.execute(Command::SetFillChannel { layer: moss, channel: ChannelKind::BaseColor, fill: FillChannel::new(moss_color) }).unwrap();
    if let Some(LayerContent::Fill(f)) = s.selected_layer().map(|l| l.content.clone()) {
        for (k, ch) in f.channels {
            if k != ChannelKind::BaseColor {
                s.execute(Command::SetFillChannel { layer: moss, channel: k, fill: FillChannel { enabled: k == ChannelKind::Roughness, ..ch } }).unwrap();
            }
        }
    }
    s.execute(Command::AddMask { layer: moss, white: false }).unwrap();
    let mut g = Generator::new(GeneratorKind::Directional);
    g.level = 0.85;
    g.softness = 0.2;
    g.noise_amount = 0.35;
    s.execute(Command::AddMaskEffect { layer: moss, kind: MaskEffectKind::Generator(g) }).unwrap();
    (s, base, moss)
}

/// p50 / p95 / max of per-frame UI thread times (ms).
fn stats(mut v: Vec<f64>) -> String {
    v.sort_by(f64::total_cmp);
    format!("p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms", v[v.len() / 2], v[v.len() * 95 / 100], v[v.len() - 1])
}

/// 60 Hz frames (`edit` runs at the start of each of the first `edits`) until the composite has
/// converged: the UI thread's time per frame and the total wall time (ms).
fn interactive_frames(s: &mut Session, edits: usize, mut edit: impl FnMut(&mut Session, usize)) -> (Vec<f64>, f64) {
    let start = Instant::now();
    let mut frames = Vec::new();
    for i in 0.. {
        let t = Instant::now();
        if i < edits {
            edit(s, i);
        }
        s.update_composites_within(Some(std::time::Duration::from_millis(10)));
        frames.push(t.elapsed().as_secs_f64() * 1e3);
        if i >= edits && !s.composites_pending() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(16_667).saturating_sub(t.elapsed()));
    }
    (frames, start.elapsed().as_secs_f64() * 1e3)
}

fn main() {
    no_throttle();
    let res: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(1024);
    let (mut s, base, moss) = demo_session(res);

    coverage_report(&s);
    let t = Instant::now();
    s.update_composites();
    println!("{res}²: first full composite {:.1} ms, field cache {} MB", t.elapsed().as_secs_f64() * 1e3, subcraft_materials::sample::field::cached_bytes() >> 20);

    let effect = s.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].clone();
    let mut times = Vec::new();
    for i in 0..30 {
        let mut e = (*effect).clone();
        let MaskEffectKind::Generator(g) = &mut e.kind else { unreachable!() };
        g.level = 0.5 + i as f32 * 0.01;
        if std::env::var_os("NO_NOISE").is_some() { g.noise_amount = 0.0; }
        let t = Instant::now();
        s.execute(Command::SetMaskEffect { layer: moss, effect: e }).unwrap();
        let cmd = t.elapsed();
        let updates = s.update_composites();
        let total = t.elapsed();
        let tiles: usize = updates.iter().map(|u| u.tiles.len()).sum();
        times.push((total.as_secs_f64() * 1e3, cmd.as_secs_f64() * 1e3, tiles));
        if i < 3 { println!("  step {i}: {:.1} ms, field cache {} MB", total.as_secs_f64() * 1e3, subcraft_materials::sample::field::cached_bytes() >> 20); }
    }
    times.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (p50, p95) = (times[times.len() / 2], times[times.len() * 95 / 100]);
    println!("generator level step: p50 {:.1} ms (command {:.2} ms, {} tiles), p95 {:.1} ms", p50.0, p50.1, p50.2, p95.0);
    breakdown(&s);
    // A pattern parameter drag: the wood fill's procedural balance changes every step (cold field).
    let mut cold = Vec::new();
    for i in 0..10 {
        let t = Instant::now();
        set_wood(&mut s, base, |p| p.balance = 0.4 + i as f32 * 0.01);
        s.update_composites();
        cold.push(t.elapsed().as_secs_f64() * 1e3);
    }
    cold.sort_by(f64::total_cmp);
    println!("wood pattern balance step (cold field): p50 {:.1} ms", cold[cold.len() / 2]);
    // Interactive frames (what the UI does): the level changes every frame, fields are warm.
    let (frames, total) = interactive_frames(&mut s, 40, |s, i| {
        let mut e = (*effect).clone();
        let MaskEffectKind::Generator(g) = &mut e.kind else { unreachable!() };
        g.level = 0.4 + i as f32 * 0.01;
        s.execute(Command::SetMaskEffect { layer: moss, effect: e }).unwrap();
    });
    println!("interactive generator drag: {} frames, converged after {total:.0} ms, UI thread per frame {}", frames.len(), stats(frames));
    drop(s);
    // Interactive warm-up of a fresh stack (every field cold), as the app's frames see it. Last:
    // Windows may throttle a console process whose main thread mostly sleeps (EcoQoS), which
    // would slow the synchronous measurements above.
    {
        let (mut s, base, _) = demo_session(res);
        let (frames, total) = interactive_frames(&mut s, 0, |_, _| {});
        println!("{res}²: interactive warm-up: {} frames, converged after {total:.0} ms, UI thread per frame {}", frames.len(), stats(frames));
        // Pattern scale drag: a new cold field every frame for 40 frames.
        let (frames, total) = interactive_frames(&mut s, 40, |s, i| set_wood(s, base, |p| p.scale = 3.0 + i as f32 * 0.02));
        println!("  pattern scale drag: {} frames, converged after {total:.0} ms, UI thread per frame {}", frames.len(), stats(frames));
    }
}

/// Change the wood fill's base color pattern parameters.
fn set_wood(s: &mut Session, base: LayerId, change: impl FnOnce(&mut ProceduralParams)) {
    let wood = s.document().texture_sets[0].layers.iter().find(|l| l.id == base).unwrap().clone();
    let LayerContent::Fill(f) = &wood.content else { unreachable!() };
    let mut ch = f.channels[&ChannelKind::BaseColor].clone();
    if let FillSource::Procedural { params, .. } = &mut ch.source {
        change(params);
    }
    s.execute(Command::SetFillChannel { layer: base, channel: ChannelKind::BaseColor, fill: ch }).unwrap();
}

/// Composite time with subsets of the stack (fields already cached).
fn breakdown(s: &Session) {
    use subcraft_compositor::{EvalContext, composite_full};
    let ts = s.selected_texture_set().clone();
    let geo = s.scene().geometry.get(ts.material_slot, ts.resolution);
    let maps = subcraft_materials::MeshMaps::new();
    let ctx = EvalContext { geometry: &geo, resources: s.resources(), mesh_maps: &maps };
    let time = |label: &str, t: &subcraft_doc::TextureSet| {
        composite_full(t, &ctx);
        let start = Instant::now();
        for _ in 0..5 { composite_full(t, &ctx); }
        println!("  {label}: {:.1} ms", start.elapsed().as_secs_f64() * 1e3 / 5.0);
    };
    time("all layers", &ts);
    for (i, l) in ts.layers.iter().enumerate() {
        let mut one = ts.clone();
        one.layers = vec![ts.layers[i].clone()];
        time(&format!("only {} ({:?}, mask {})", l.name, l.kind(), l.mask.is_some()), &one);
    }
    let mut none = ts.clone();
    none.layers.clear();
    time("no layers", &none);
    use rayon::prelude::*;
    use subcraft_compositor::{Composite, TileContext, evaluate_tile};
    let tiles: Vec<usize> = (0..(ts.resolution / 64).pow(2) as usize).collect();
    for label in ["empty", "all"] {
        let t_set = if label == "empty" { &none } else { &ts };
        let start = Instant::now();
        let results: Vec<_> = tiles.par_iter().map(|&t| {
            let g = geo.tile(t);
            let tc = TileContext { albedo_filter: ts.albedo_filter, geo: &g, set_geometry: &geo, sample: subcraft_materials::SampleContext { resources: s.resources(), mesh_maps: &maps } };
            (t, evaluate_tile(t_set, &tc))
        }).collect();
        let eval = start.elapsed();
        let mut c = Composite::new(t_set);
        let start = Instant::now();
        let mut buf = Vec::new();
        for (t, channels) in results {
            for (kind, px) in channels {
                let img = c.channels.get_mut(&kind).unwrap();
                let comps = img.comps();
                buf.clear();
                for p in &px { buf.extend_from_slice(&p[..comps]); }
                img.write_tile(t, &buf);
            }
        }
        println!("  {label}: eval {:.1} ms, serial write {:.1} ms", eval.as_secs_f64() * 1e3, start.elapsed().as_secs_f64() * 1e3);
    }
}

#[allow(dead_code)]
fn coverage_report(s: &Session) {
    let ts = s.selected_texture_set();
    let geo = s.scene().geometry.get(ts.material_slot, ts.resolution);
    let tiles = (ts.resolution / 64).pow(2) as usize;
    let (mut empty, mut covered) = (0, 0usize);
    for t in 0..tiles {
        let g = geo.tile(t);
        if g.is_empty() { empty += 1; continue; }
        covered += (0..4096).filter(|&i| g.coverage(i) > 0.0).count();
    }
    println!("tiles {tiles}, empty {empty}, covered texels {:.1}%", covered as f64 / (tiles * 4096) as f64 * 100.0);
}
