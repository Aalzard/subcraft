//! What a generator slider costs on a big texture set: a smart-material-like stack on the sample
//! crate (base metal, paint, and one fill layer per generator: Dirt, Edges with natural wear,
//! Concave, Directional, Position Gradient, the Dirt one with the micro effect when asked) with
//! baked curvature + AO, then for every generator:
//!
//! * `sync`: one parameter step brought exactly up to date (`update_composites`, blocking);
//! * `drag`: 30 steps at 60 Hz through the background worker as the UI drives it, then idle:
//!   time from each step to the first published result that contains it (the visible update),
//!   the longest UI-thread call, and the time from the last step until the exact result.
//!
//! `cargo run --release -p subcraft-app --example generator_4k -- [resolution] [generators]`
//!
//! `generators` is a list of letters (default `decdpm`): `d` Dirt, `e` Edges, `c` Concave,
//! `r` Directional, `p` Position Gradient, `m` Dirt with the micro effect. Mesh maps are baked at
//! `SUBCRAFT_BENCH_BAKE` (default 1024; generators read them bilinearly at the set resolution).
//! `SUBCRAFT_COMPOSITE_PROFILE=1` prints graph node time per step. `SUBCRAFT_BENCH_HASH=1`
//! prints a hash of every composite texel bit after the steps (exactness across builds).
#![allow(unsafe_code)]
use std::time::{Duration, Instant};

use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue, LayerId, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind, ProceduralKind, ProceduralParams};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(windows)]
fn no_throttle() {
    #[repr(C)]
    struct State { version: u32, control: u32, state: u32 }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn SetProcessInformation(h: isize, class: i32, info: *const core::ffi::c_void, size: u32) -> i32;
    }
    let s = State { version: 1, control: 1, state: 0 };
    unsafe { SetProcessInformation(GetCurrentProcess(), 4, &s as *const State as *const _, 12) };
}
#[cfg(not(windows))]
fn no_throttle() {}

fn ms(d: Duration) -> f64 { d.as_secs_f64() * 1e3 }

fn run(s: &mut Session, cmd: Command) { s.execute(cmd).unwrap_or_else(|e| panic!("{e:?}")); }

fn uniform(v: ChannelValue) -> FillChannel { FillChannel::new(FillSource::uniform(v)) }

fn procedural(kind: ProceduralKind, scale: f32, low: ChannelValue, high: ChannelValue) -> FillChannel {
    FillChannel::new(FillSource::Procedural { params: ProceduralParams { scale, ..ProceduralParams::new(kind) }, low, high })
}

/// A fill layer on top with these channels; returns its id.
fn fill(s: &mut Session, channels: Vec<(ChannelKind, FillChannel)>) -> LayerId {
    run(s, Command::SelectLayer(None));
    run(s, Command::AddLayer(NewLayer::Fill));
    let id = s.selection().layer.unwrap();
    for (channel, f) in channels {
        run(s, Command::SetFillChannel { layer: id, channel, fill: f });
    }
    id
}

fn masked(s: &mut Session, color: [f32; 3], rough: f32, height: f32, g: Generator) -> LayerId {
    let id = fill(s, vec![
        (ChannelKind::BaseColor, uniform(ChannelValue::rgb(color[0], color[1], color[2]))),
        (ChannelKind::Roughness, uniform(ChannelValue::scalar(rough))),
        (ChannelKind::Height, uniform(ChannelValue::scalar(height))),
    ]);
    run(s, Command::AddMask { layer: id, white: false });
    run(s, Command::AddMaskEffect { layer: id, kind: MaskEffectKind::Generator(g) });
    id
}

fn generator_of(s: &Session, layer: LayerId) -> std::sync::Arc<subcraft_doc::MaskEffect> {
    let l = s.document().texture_sets[0].find(layer).unwrap();
    l.mask.as_ref().unwrap().effects.iter().find(|e| matches!(e.kind, MaskEffectKind::Generator(_))).unwrap().clone()
}

/// Step `i` of a Level drag.
fn step(s: &mut Session, layer: LayerId, i: usize) {
    let mut e = (*generator_of(s, layer)).clone();
    if let MaskEffectKind::Generator(g) = &mut e.kind {
        g.level = 0.35 + 0.01 * (i % 40) as f32;
    }
    run(s, Command::SetMaskEffect { layer, effect: e });
}

fn percentile(v: &[f64], p: usize) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v[(v.len() * p / 100).min(v.len() - 1)]
}

fn profile_dump() {
    let rows = subcraft_compositor::profile::take();
    for (k, n, t) in rows.iter().take(6) {
        println!("        {k:<18} {n:>7} evals {t:>8.0} ms (thread-sum)");
    }
}

/// FNV-1a over every texel bit of every channel.
fn composite_hash(s: &Session) -> u64 {
    let ts = s.document().texture_sets[0].id;
    let c = s.composite(ts).unwrap();
    let mut h = 0xcbf29ce484222325u64;
    for (kind, img) in &c.channels {
        h = (h ^ *kind as u64).wrapping_mul(0x100000001b3);
        for y in 0..c.resolution {
            for x in 0..c.resolution {
                for v in img.get(x, y) {
                    h = (h ^ v.to_bits() as u64).wrapping_mul(0x100000001b3);
                }
            }
        }
    }
    h
}

/// 60 Hz drag through the worker. Returns (visible-update latencies, max UI call, settle after
/// the last step, publications, channel tiles the UI would upload).
fn drag(s: &mut Session, layer: LayerId, steps: usize) -> (Vec<f64>, f64, f64, usize, usize) {
    let frame = Duration::from_micros(16_667);
    let mut pending_since: Vec<Instant> = Vec::new();
    let mut latencies = Vec::new();
    let mut max_call = 0.0f64;
    let tiles_total = (s.document().texture_sets[0].resolution as usize / 64).pow(2);
    let mut pubs = 0;
    let mut uploads = 0;
    let mut last_edit = Instant::now();
    let mut i = 0;
    loop {
        let t = Instant::now();
        if i < steps {
            step(s, layer, i + 100);
            pending_since.push(t);
            last_edit = t;
        }
        let c = Instant::now();
        let ups = s.update_composites_within(Some(Duration::from_millis(1)));
        max_call = max_call.max(ms(c.elapsed()));
        if !ups.is_empty() {
            pubs += 1;
            uploads += ups.iter().flat_map(|u| u.changed.values()).map(Vec::len).sum::<usize>();
        }
        // An edit is visible once a publication covers the whole image (a quick preview lists every
        // stale tile) or the composite is exact; a slice of exact tiles alone does not count. The
        // worker always evaluates the newest snapshot, so this covers every earlier edit too.
        let whole = ups.iter().any(|u| u.tiles.len() * 2 >= tiles_total);
        if whole || !s.composites_pending() {
            let now = Instant::now();
            for p in pending_since.drain(..) {
                latencies.push(ms(now - p));
            }
        }
        i += 1;
        if i > steps && !s.composites_pending() {
            break;
        }
        std::thread::sleep(frame.saturating_sub(t.elapsed()));
    }
    (latencies, max_call, ms(last_edit.elapsed()), pubs, uploads)
}

fn main() {
    no_throttle();
    let mut args = std::env::args().skip(1);
    let res: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(4096);
    let which = args.next().unwrap_or_else(|| "decrpm".into());
    println!("== generator_4k: {res}², generators `{which}`, {} threads", rayon::current_num_threads());
    let mut s = Session::new(subcraft_mesh::primitives::sample_crate(), res);
    {
        let size: u32 = std::env::var("SUBCRAFT_BENCH_BAKE").ok().and_then(|v| v.parse().ok()).unwrap_or(1024);
        let ts = s.document().texture_sets[0].clone();
        let settings = subcraft_bake::BakeSettings { size, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], ..Default::default() };
        let t = Instant::now();
        let mut job = s.start_bake(subcraft_app::BakeRequest { settings, high_poly: None, texture_sets: vec![(ts.id, ts.material_slot)] });
        while job.poll(&mut s).is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("bake curvature + AO at {size}²: {:.1} s", t.elapsed().as_secs_f64());
    }
    // Base metal + paint, like a smart material.
    fill(&mut s, vec![
        (ChannelKind::BaseColor, procedural(ProceduralKind::Clouds, 9.0, ChannelValue::rgb(0.42, 0.42, 0.43), ChannelValue::rgb(0.58, 0.58, 0.59))),
        (ChannelKind::Roughness, procedural(ProceduralKind::Grunge, 8.0, ChannelValue::scalar(0.35), ChannelValue::scalar(0.65))),
        (ChannelKind::Metallic, uniform(ChannelValue::scalar(1.0))),
        (ChannelKind::Height, uniform(ChannelValue::scalar(0.0))),
    ]);
    fill(&mut s, vec![
        (ChannelKind::BaseColor, uniform(ChannelValue::rgb(0.5, 0.07, 0.05))),
        (ChannelKind::Roughness, uniform(ChannelValue::scalar(0.42))),
        (ChannelKind::Metallic, uniform(ChannelValue::scalar(0.0))),
    ]);
    let mut cases: Vec<(&str, LayerId)> = Vec::new();
    for c in which.chars() {
        let (name, mut g) = match c {
            'd' => ("Dirt", Generator::new(GeneratorKind::Cavity)),
            'e' => ("Edges (natural wear)", Generator::new(GeneratorKind::Edges)),
            'c' => ("Concave", { let mut g = Generator::new(GeneratorKind::Concave); g.noise_amount = 0.3; g }),
            'r' => ("Directional", { let mut g = Generator::new(GeneratorKind::Directional); g.noise_amount = 0.35; g }),
            'p' => ("Position Gradient", { let mut g = Generator::new(GeneratorKind::PositionGradient); g.noise_amount = 0.3; g }),
            'm' => ("Dirt + micro effect", { let mut g = Generator::new(GeneratorKind::Cavity); g.set_micro_enabled(true); g }),
            _ => continue,
        };
        // Every layer of a real stack has its own breakup pattern.
        g.noise.seed = 11 + cases.len() as u32;
        // A/B aid: SUBCRAFT_BENCH_ORGANIC=1 breaks up with the organic noise instead of the grunge map.
        if std::env::var_os("SUBCRAFT_BENCH_ORGANIC").is_some() && let Some(p) = g.painter.as_mut() { p.grunge_map = false; }
        let id = masked(&mut s, [0.08, 0.06, 0.05], 0.85, 0.02, g);
        cases.push((name, id));
    }
    let t = Instant::now();
    s.update_composites();
    println!("first full composite: {:.0} ms (field cache {} MB)", ms(t.elapsed()), subcraft_materials::sample::field::cached_bytes() >> 20);
    let t = Instant::now();
    s.update_composites();
    println!("no-change recomposite: {:.1} ms", ms(t.elapsed()));
    // The same stack for the real app: `subcraft <file> --profile-edits out.txt` with
    // `SUBCRAFT_PROFILE_GENERATOR=dirt` (or edges, directional, ...).
    if let Some(path) = std::env::var_os("SUBCRAFT_BENCH_SAVE") {
        s.save(Some(std::path::Path::new(&path))).unwrap();
        println!("saved {}", std::path::Path::new(&path).display());
        if std::env::var_os("SUBCRAFT_BENCH_SAVE_ONLY").is_some() { return; }
    }
    subcraft_compositor::profile::take();

    println!("-- sync: one Level step, exact (ms; tiles evaluated)");
    for (name, layer) in &cases {
        let mut v = Vec::new();
        let mut tiles = 0;
        for i in 0..4 {
            let t = Instant::now();
            step(&mut s, *layer, i);
            tiles = s.update_composites().iter().map(|u| u.tiles.len()).sum();
            v.push(ms(t.elapsed()));
        }
        println!("  {name:<22} {}  ({tiles} tiles, field cache {} MB)", v.iter().map(|x| format!("{x:.0}")).collect::<Vec<_>>().join(" / "), subcraft_materials::sample::field::cached_bytes() >> 20);
        profile_dump();
    }
    if std::env::var_os("SUBCRAFT_BENCH_HASH").is_some() {
        println!("  composite hash after sync steps {:016x}", composite_hash(&s));
    }

    // What one quick preview costs: the 512² composite of the whole stack, and stretching it over
    // the 4K composite (what the worker publishes while a slider moves).
    if let Some(low_res) = subcraft_compositor::preview_resolution(res) {
        use subcraft_compositor::{Composite, EvalContext};
        let ts = s.document().texture_sets[0].clone();
        let low_geo = s.scene().geometry.get(ts.material_slot, low_res);
        let maps = s.mesh_maps(ts.id).cloned().unwrap_or_default();
        let resources = s.resources().clone();
        let ctx = EvalContext { geometry: &low_geo, resources: &resources, mesh_maps: &maps };
        let mut low = ts.clone();
        low.resolution = low_res;
        let never = || false;
        let mut full = s.composite(ts.id).unwrap().clone();
        let all: Vec<usize> = (0..full.tile_count()).collect();
        for (name, layer) in &cases {
            s.update_composites();
            let exact = s.composite(ts.id).unwrap().clone();
            let mut l = s.document().texture_sets[0].clone();
            l.resolution = low_res;
            let base = Composite::preview(&l, &ctx, res, &never).unwrap();
            let (mut v, mut o, mut d, mut n) = (Vec::new(), Vec::new(), Vec::new(), 0);
            for i in 0..4 {
                step(&mut s, *layer, 50 + i);
                let mut l = s.document().texture_sets[0].clone();
                l.resolution = low_res;
                let t = Instant::now();
                let quick = Composite::preview(&l, &ctx, res, &never).unwrap();
                v.push(ms(t.elapsed()));
                let t = Instant::now();
                full.overlay_preview(&quick, &all);
                o.push(ms(t.elapsed()));
                let mut shown = exact.clone();
                let t = Instant::now();
                let (changed, _) = shown.overlay_preview_against(&quick, &all, &exact, Some(&base));
                d.push(ms(t.elapsed()));
                n = changed.values().map(Vec::len).sum::<usize>();
            }
            println!(
                "  preview {name:<22} 512² composite {:.0} ms; stretched over every 4K tile {:.0} ms; only where it changed {:.0} ms ({n} of {} channel tiles) (medians of 4)",
                percentile(&v, 50), percentile(&o, 50), percentile(&d, 50), all.len() * exact.channels.len(),
            );
        }
        let _ = low;
        s.update_composites();
    }

    println!("-- drag: 30 Level steps at 60 Hz through the background worker (ms)");
    // Hand over to the worker and let it settle.
    s.update_composites_within(Some(Duration::from_millis(1)));
    while s.composites_pending() {
        std::thread::sleep(Duration::from_millis(5));
        s.update_composites_within(Some(Duration::from_millis(1)));
    }
    for (name, layer) in &cases {
        let (lat, max_call, settle, pubs, uploads) = drag(&mut s, *layer, 30);
        println!(
            "  {name:<22} visible update p50 {:.0} / p95 {:.0} / max {:.0}; settle after last step {:.0}; UI call max {:.1}; {pubs} publications, {uploads} channel tiles to upload",
            percentile(&lat, 50), percentile(&lat, 95), percentile(&lat, 100), settle, max_call,
        );
    }
}
