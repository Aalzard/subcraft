//! Reproducible big-project benchmark: a 4096 texture set with dozens of layers (fills, folders,
//! masks with Edges/Cavity/Directional generators, procedural patterns, blur/sharpen/gradient-map/
//! median filters, strokes on masks) on a 100k-triangle sphere.
//!
//! `cargo run --release -p subcraft-app --example big_project_bench -- [resolution] [layers] [stages]`
//!
//! `stages` is a list of letters (default `abcdg`): `a` full composite, `b` incremental edits,
//! `c` open project, `d` save + load, `f` bake of the 7 mesh maps (always first when present),
//! `g` memory. Set `SUBCRAFT_BENCH_BAKE=<size>` to bake the maps at another size than the set.
#![allow(unsafe_code)]
use std::time::Instant;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue, LayerId, MeshMapKind};
use subcraft_doc::{
    BlendMode, ContentEffectKind, FillChannel, FillSource, Filter, Generator, GeneratorKind, Gradient, Levels, MaskEffectKind, ProceduralKind,
    ProceduralParams,
};
use subcraft_paint::Camera;

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

/// (peak, current) working set in MB.
#[cfg(windows)]
fn memory() -> (usize, usize) {
    #[repr(C)]
    #[derive(Default)]
    struct Counters { cb: u32, faults: u32, peak: usize, ws: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(h: isize, c: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters::default();
    c.cb = std::mem::size_of::<Counters>() as u32;
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (c.peak >> 20, c.ws >> 20)
}
#[cfg(not(windows))]
fn memory() -> (usize, usize) { (0, 0) }

fn ms(t: Instant) -> f64 { t.elapsed().as_secs_f64() * 1e3 }

fn run(s: &mut Session, cmd: Command) {
    s.execute(cmd).unwrap_or_else(|e| panic!("{e:?}"));
}

fn camera() -> Camera {
    Camera::orthographic(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y, 1.3, 0.01, 20.0, [1024, 1024])
}

fn stroke(s: &mut Session, y: f32) {
    s.brush.settings.size = 40.0;
    s.brush.settings.hardness = 0.6;
    s.begin_stroke().unwrap();
    for i in 0..40 {
        s.stroke_to(&camera(), Vec2::new(300.0 + 10.0 * i as f32, y + 20.0 * (i as f32 * 0.3).sin()), false).unwrap();
    }
    s.end_stroke();
}

fn pattern(kind: ProceduralKind, scale: f32, low: ChannelValue, high: ChannelValue) -> FillSource {
    FillSource::Procedural { params: ProceduralParams { scale, ..ProceduralParams::new(kind) }, low, high }
}

const KINDS: [ProceduralKind; 7] = [
    ProceduralKind::Clouds, ProceduralKind::Cells, ProceduralKind::Grunge, ProceduralKind::Scratches,
    ProceduralKind::WoodRings, ProceduralKind::Brushed, ProceduralKind::Dirt,
];

/// Layers added bottom to top; returns every layer id in creation order.
fn build_stack(s: &mut Session, layers: usize) -> Vec<LayerId> {
    let mut ids = Vec::new();
    let mut folder: Option<(LayerId, usize)> = None; // (folder, children still to add)
    for i in 0..layers {
        run(s, Command::SelectLayer(None));
        if folder.is_none() && i % 9 == 0 && i > 0 {
            run(s, Command::AddLayer(NewLayer::Folder));
            let f = s.selection().layer.unwrap();
            ids.push(f);
            folder = Some((f, 4));
            continue;
        }
        let paint_layer = i % 7 == 3;
        run(s, Command::AddLayer(if paint_layer { NewLayer::Paint } else { NewLayer::Fill }));
        let id = s.selection().layer.unwrap();
        if let Some((f, left)) = folder {
            run(s, Command::MoveLayer { layer: id, parent: Some(f), index: 0 });
            folder = (left > 1).then_some((f, left - 1));
        }
        ids.push(id);
        let k = KINDS[i % KINDS.len()];
        if !paint_layer {
            let t = (i % 11) as f32 / 11.0;
            run(s, Command::SetFillChannel {
                layer: id,
                channel: ChannelKind::BaseColor,
                fill: FillChannel::new(pattern(k, 2.0 + (i % 5) as f32 * 3.0, ChannelValue::rgb(0.05 + t * 0.1, 0.04, 0.03), ChannelValue::rgb(0.3 + t * 0.4, 0.5 - t * 0.3, 0.2 + t * 0.2))),
            });
            run(s, Command::SetFillChannel {
                layer: id,
                channel: ChannelKind::Roughness,
                fill: FillChannel::new(pattern(KINDS[(i + 3) % KINDS.len()], 6.0, ChannelValue::scalar(0.3), ChannelValue::scalar(0.85))),
            });
        }
        if i % 3 == 1 {
            run(s, Command::SetBlendMode { layer: id, channel: ChannelKind::BaseColor, mode: [BlendMode::Multiply, BlendMode::Overlay, BlendMode::Normal][i / 3 % 3] });
        }
        // Masks with generators / filters / paint.
        if i % 2 == 0 || paint_layer {
            run(s, Command::AddMask { layer: id, white: paint_layer });
            let kind = match i % 4 {
                0 => MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)),
                1 => MaskEffectKind::Generator(Generator::new(GeneratorKind::Cavity)),
                2 => MaskEffectKind::Generator({
                    let mut g = Generator::new(GeneratorKind::Directional);
                    g.noise_amount = 0.35;
                    g
                }),
                _ => MaskEffectKind::Filter(Filter::Blur { radius: 6.0 }),
            };
            run(s, Command::AddMaskEffect { layer: id, kind });
            if i % 6 == 0 {
                run(s, Command::AddMaskEffect { layer: id, kind: MaskEffectKind::Levels(Levels { out_low: 0.1, ..Levels::default() }) });
            }
            if paint_layer {
                run(s, Command::SelectLayerTarget { layer: id, target: LayerTarget::Mask });
                s.brush.target = subcraft_paint::PaintTarget::MaskBase;
                stroke(s, 300.0 + 40.0 * (i % 8) as f32);
            }
        }
        if paint_layer {
            run(s, Command::SelectLayerTarget { layer: id, target: LayerTarget::Content });
            s.brush.target = subcraft_paint::PaintTarget::Content;
            stroke(s, 340.0 + 30.0 * (i % 6) as f32);
        }
        // Content filters.
        let filter = match i % 5 {
            0 => Some((ChannelKind::BaseColor, Filter::Blur { radius: 4.0 })),
            1 => Some((ChannelKind::BaseColor, Filter::Sharpen { radius: 3.0, amount: 1.0 })),
            2 => Some((ChannelKind::BaseColor, Filter::GradientMap { gradient: Gradient::preset(1).unwrap(), to_gray: false })),
            3 => Some((ChannelKind::Roughness, Filter::Median { radius: 2.0 })),
            _ => None,
        };
        if let Some((channel, filter)) = filter.filter(|_| !paint_layer) {
            run(s, Command::AddContentEffect { layer: id, kind: ContentEffectKind::Filter { channel, filter } });
        }
    }
    ids
}

/// Node evaluations by kind (needs `SUBCRAFT_COMPOSITE_PROFILE=1`); time is summed over threads.
fn profile_dump(label: &str) {
    let rows = subcraft_compositor::profile::take();
    if rows.is_empty() { return; }
    println!("    node profile ({label}):");
    for (k, n, t) in rows.iter().take(8) {
        println!("      {k:<16} {n:>8} evals {t:>9.0} ms (thread-sum)");
    }
}

/// FNV-1a over every texel bit of every channel of the first texture set's composite.
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

fn percentile(mut v: Vec<f64>, p: usize) -> f64 {
    v.sort_by(f64::total_cmp);
    v[(v.len() * p / 100).min(v.len() - 1)]
}

fn main() {
    no_throttle();
    let mut args = std::env::args().skip(1);
    let res: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(4096);
    let layers: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(40);
    let stages = args.next().unwrap_or_else(|| "abcdg".into());
    let has = |c: char| stages.contains(c);
    println!("== big_project_bench: {res}², {layers} layers, {} threads", rayon::current_num_threads());
    let mesh = subcraft_mesh::primitives::uv_sphere(1.0, 224, 224);
    println!("mesh: {} triangles", mesh.indices.len() / 3);
    let mut s = Session::new(mesh, res);

    // (f) bake of the 7 mesh maps.
    {
        let size: u32 = std::env::var("SUBCRAFT_BENCH_BAKE").ok().and_then(|v| v.parse().ok()).unwrap_or(res);
        let ts = s.document().texture_sets[0].clone();
        let bake_settings = subcraft_bake::BakeSettings { size, ..Default::default() };
        let t = Instant::now();
        let mut job = s.start_bake(subcraft_app::BakeRequest { settings: bake_settings, high_poly: None, texture_sets: vec![(ts.id, ts.material_slot)] });
        let mut last = Instant::now();
        let mut stage = String::new();
        loop {
            if job.poll(&mut s).is_some() { break; }
            let st = job.stage();
            if st != stage {
                println!("  bake [{:.1}s] {st}", t.elapsed().as_secs_f64());
                stage = st;
                last = Instant::now();
            }
            let _ = last;
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        println!("(f) bake of {} maps at {size}²: {:.2} s", MeshMapKind::ALL.len(), t.elapsed().as_secs_f64());
        let (peak, now) = memory();
        println!("    memory after bake: {now} MB (peak {peak} MB)");
    }

    let t = Instant::now();
    let ids = build_stack(&mut s, layers);
    println!("stack built: {} layers in {:.0} ms (strokes + commands)", ids.len(), ms(t));
    let (_, mem_stack) = memory();

    // (a) full composite from scratch.
    let t = Instant::now();
    let updates = s.update_composites();
    let tiles: usize = updates.iter().map(|u| u.tiles.len()).sum();
    println!("(a) full composite: {:.0} ms, {tiles} tiles", ms(t));
    let t = Instant::now();
    let ups = s.update_composites();
    profile_dump("full composite");
    if std::env::var_os("SUBCRAFT_BENCH_HASH").is_some() { println!("    composite hash {:016x}", composite_hash(&s)); }
    println!("    no-change recomposite: {:.1} ms, {} tiles", ms(t), ups.iter().map(|u| u.tiles.len()).sum::<usize>());

    if has('b') {
        let top = *ids.last().unwrap();
        let mid = ids[ids.len() / 2];
        // One stroke on a mid-stack paint layer's mask (a 40 px brush).
        let paint = ids.iter().copied().find(|id| s.document().texture_sets[0].find(*id).is_some_and(|l| matches!(l.content, subcraft_doc::LayerContent::Paint(_)) && l.mask.is_some())).unwrap();
        let mut times = Vec::new();
        for n in 0..6 {
            run(&mut s, Command::SelectLayerTarget { layer: paint, target: LayerTarget::Mask });
            s.brush.target = subcraft_paint::PaintTarget::MaskBase;
            let t = Instant::now();
            stroke(&mut s, 280.0 + 12.0 * n as f32);
            let ups = s.update_composites();
            times.push((ms(t), ups.iter().map(|u| u.tiles.len()).sum::<usize>()));
        }
        profile_dump("stroke + opacity");
        println!("(b) stroke on a mask: p50 {:.0} ms, {} tiles evaluated (last)", percentile(times.iter().map(|t| t.0).collect(), 50), times.last().unwrap().1);
        // Opacity of the top layer.
        let mut v = Vec::new();
        let mut tiles = 0;
        for i in 0..6 {
            let t = Instant::now();
            run(&mut s, Command::SetBlendOpacity { layer: top, channel: ChannelKind::BaseColor, opacity: 0.9 - 0.05 * i as f32 });
            tiles = s.update_composites().iter().map(|u| u.tiles.len()).sum();
            v.push(ms(t));
        }
        println!("    top-layer opacity change: p50 {:.0} ms, {tiles} tiles", percentile(v, 50));
        // A mid-stack generator parameter.
        let layer = s.document().texture_sets[0].find(mid).cloned();
        let target = ids.iter().copied().find(|id| s.document().texture_sets[0].find(*id).is_some_and(|l| l.mask.as_ref().is_some_and(|m| m.effects.iter().any(|e| matches!(e.kind, MaskEffectKind::Generator(_)))))).unwrap();
        let _ = layer;
        let eff = s.document().texture_sets[0].find(target).unwrap().mask.as_ref().unwrap().effects.iter().find(|e| matches!(e.kind, MaskEffectKind::Generator(_))).unwrap().clone();
        let mut v = Vec::new();
        for i in 0..6 {
            let mut e = (*eff).clone();
            if let MaskEffectKind::Generator(g) = &mut e.kind { g.level = 0.4 + 0.02 * i as f32; }
            let t = Instant::now();
            run(&mut s, Command::SetMaskEffect { layer: target, effect: e });
            tiles = s.update_composites().iter().map(|u| u.tiles.len()).sum();
            v.push(ms(t));
        }
        println!("    mask generator level change: p50 {:.0} ms, {tiles} tiles", percentile(v, 50));
        // A fill pattern scale (bottom layer).
        let bottom = ids[0];
        let mut v = Vec::new();
        for i in 0..4 {
            let t = Instant::now();
            run(&mut s, Command::SetFillChannel {
                layer: bottom,
                channel: ChannelKind::BaseColor,
                fill: FillChannel::new(pattern(ProceduralKind::Clouds, 3.0 + i as f32, ChannelValue::rgb(0.1, 0.1, 0.1), ChannelValue::rgb(0.7, 0.6, 0.5))),
            });
            tiles = s.update_composites().iter().map(|u| u.tiles.len()).sum();
            v.push(ms(t));
        }
        println!("    bottom-layer pattern change: p50 {:.0} ms, {tiles} tiles", percentile(v, 50));
        if std::env::var_os("SUBCRAFT_BENCH_HASH").is_some() { println!("    composite hash after edits {:016x}", composite_hash(&s)); }
    }

    // (d)/(c) save, load.
    if has('c') || has('d') {
        let path = std::env::temp_dir().join("subcraft_bench.subcraft");
        let t = Instant::now();
        s.save(Some(&path)).unwrap();
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        println!("(d) save: {:.0} ms, {} MB on disk", ms(t), size >> 20);
        let t = Instant::now();
        let loaded = Session::open_project(&path).unwrap();
        println!("    load (open_project): {:.0} ms", ms(t));
        let mut loaded = loaded;
        let t = Instant::now();
        let ups = loaded.update_composites();
        println!("(c) first composite after open: {:.0} ms, {} tiles; open total {:.0} ms", ms(t), ups.iter().map(|u| u.tiles.len()).sum::<usize>(), ms(t) + 0.0);
        std::fs::remove_file(&path).ok();
    }

    if has('g') {
        let (peak, now) = memory();
        println!("(g) memory: after stack {mem_stack} MB, now {now} MB, peak {peak} MB");
    }
}
