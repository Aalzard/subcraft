//! Synthetic pen-stroke benchmark: replays a 500 Hz pen stream (varying pressure) through the same
//! calls the viewport makes (`set_stroke_pressure` + `stroke_to`, then the interactive composite
//! update every frame) and reports per-frame UI-thread cost, event-to-pixel latency and the worst
//! frames. No window, no GPU: the GPU upload is not measured, only the tiles it would upload.
//!
//! `cargo run --release -p subcraft-app --example pen_stroke_bench -- [res] [layers] [brush_px] [textured:0|1] [mode] [place]`
//!
//! `mode`: `frame` (default; the viewport's behaviour: one point per 60 Hz frame, the last event of
//! the frame), `event` (one `stroke_to` per 500 Hz event), `batch` (all events of a frame in one `stroke_to_many`: what the viewport does now).
//! `place`: `mid` (paint layer in the middle of the stack, default) or `top`.
//! Env: `PEN_HZ` (500), `PEN_FPS` (60), `PEN_MS` (stroke duration, 800).
#![allow(unsafe_code)]
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{BlendMode, FillChannel, FillSource, ProceduralKind, ProceduralParams};
use subcraft_paint::Camera;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn ms(d: Duration) -> f64 { d.as_secs_f64() * 1e3 }
fn run(s: &mut Session, cmd: Command) { s.execute(cmd).unwrap_or_else(|e| panic!("{e:?}")); }
fn pct(v: &[f64], p: usize) -> f64 {
    if v.is_empty() { return f64::NAN; }
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v[(v.len() * p / 100).min(v.len() - 1)]
}
fn env<T: std::str::FromStr>(k: &str, d: T) -> T { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }

const VIEW: u32 = 1024;
fn camera() -> Camera {
    Camera::orthographic(Vec3::new(0.0, 0.0, 4.0), Vec3::ZERO, Vec3::Y, 1.3, 0.01, 20.0, [VIEW, VIEW])
}

fn build(s: &mut Session, layers: usize, top: bool) {
    const KINDS: [ProceduralKind; 4] = [ProceduralKind::Clouds, ProceduralKind::Cells, ProceduralKind::Grunge, ProceduralKind::Dirt];
    let paint_at = if top { layers - 1 } else { layers / 2 };
    let mut paint_id = None;
    for i in 0..layers {
        run(s, Command::SelectLayer(None));
        if i == paint_at {
            run(s, Command::AddLayer(NewLayer::Paint));
            paint_id = s.selection().layer;
            continue;
        }
        run(s, Command::AddLayer(NewLayer::Fill));
        let id = s.selection().layer.unwrap();
        let t = (i % 11) as f32 / 11.0;
        let src = FillSource::Procedural {
            params: ProceduralParams { scale: 3.0 + (i % 5) as f32 * 2.0, ..ProceduralParams::new(KINDS[i % 4]) },
            low: ChannelValue::rgb(0.05 + t * 0.1, 0.04, 0.03),
            high: ChannelValue::rgb(0.3 + t * 0.4, 0.5 - t * 0.3, 0.2 + t * 0.2),
        };
        run(s, Command::SetFillChannel { layer: id, channel: ChannelKind::BaseColor, fill: FillChannel::new(src) });
        if i % 3 == 1 {
            run(s, Command::SetBlendMode { layer: id, channel: ChannelKind::BaseColor, mode: BlendMode::Multiply });
        }
    }
    // Select the paint layer's content.
    run(s, Command::SelectLayer(paint_id));
}

#[derive(Clone, Copy)]
struct Event { t: f64, pos: Vec2, pressure: f32 }

/// A looping S-curve across the viewport with a pressure ramp up and down.
fn pen_stream(hz: f64, dur_ms: f64) -> Vec<Event> {
    let n = (hz * dur_ms / 1e3) as usize;
    (0..n).map(|i| {
        let u = i as f32 / (n - 1) as f32;
        Event {
            t: i as f64 / hz,
            pos: Vec2::new(200.0 + 620.0 * u, 512.0 + 190.0 * (u * std::f32::consts::TAU * 1.5).sin()),
            pressure: 0.15 + 0.85 * (u * std::f32::consts::PI).sin().powf(0.8),
        }
    }).collect()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let res: u32 = a.first().and_then(|v| v.parse().ok()).unwrap_or(2048);
    let layers: usize = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(10);
    let brush_px: f32 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(300.0);
    let textured = a.get(3).is_some_and(|v| v == "1");
    let mode = a.get(4).map_or("frame", |s| s.as_str()).to_string();
    let top = a.get(5).is_some_and(|v| v == "top");
    let hz: f64 = env("PEN_HZ", 500.0);
    let fps: f64 = env("PEN_FPS", 60.0);
    let dur: f64 = env("PEN_MS", 800.0);

    // `PEN_PROJECT=file.subcraft`: paint on a new paint layer at the top of that project instead.
    let (mut s, diag) = match std::env::var_os("PEN_PROJECT") {
        Some(path) => {
            let mut s = Session::open_project(std::path::Path::new(&path)).expect("open project");
            run(&mut s, Command::SelectLayer(None));
            run(&mut s, Command::AddLayer(NewLayer::Paint));
            let diag = s.scene().mesh.bounds.diagonal();
            (s, diag)
        }
        None => {
            let mesh = subcraft_mesh::primitives::uv_sphere(1.0, 64, 64);
            let diag = mesh.bounds.diagonal();
            let mut s = Session::new(mesh, res);
            build(&mut s, layers, top);
            (s, diag)
        }
    };
    // Prime: composite the stack once so the stroke only pays for its own edits.
    s.update_composites();
    s.update_composites_within(Some(Duration::from_millis(1)));
    while s.composites_pending() { std::thread::sleep(Duration::from_millis(5)); s.update_composites_within(Some(Duration::from_millis(1))); }

    let cam = camera();
    let ppu = cam.pixels_per_unit(4.0);
    s.brush.settings.size = brush_px / ppu / diag * 100.0;
    s.brush.settings.hardness = if textured { 0.5 } else { 0.1 };
    s.brush.settings.pressure_size = 0.8;
    s.brush.settings.pressure_flow = 0.4;
    if textured {
        let n = 256usize;
        let data: Vec<f32> = (0..n * n).map(|i| {
            let (x, y) = ((i % n) as f32 / n as f32 - 0.5, (i / n) as f32 / n as f32 - 0.5);
            let r = (x * x + y * y).sqrt() * 2.0;
            let g = (((x * 40.0).sin() * (y * 37.0).cos()) * 0.5 + 0.5) * (1.0 - r).clamp(0.0, 1.0);
            g.clamp(0.0, 1.0)
        }).collect();
        s.brush.alpha = Some(Arc::new(subcraft_image::Bitmap::new_f32(n as u32, n as u32, 1, data)));
    }
    println!("== pen_stroke_bench: {res}², {layers} layers (paint layer {}), brush {brush_px:.0}px ({}), mode {mode}, {hz} Hz events, {fps} fps, {dur} ms",
        if top { "top" } else { "mid" }, if textured { "textured" } else { "soft round" });

    let events = pen_stream(hz, dur);
    let frame_dt = 1.0 / fps;
    let n_frames = (events.last().unwrap().t / frame_dt).ceil() as usize + 1;

    let mut frame_cost = Vec::new(); // UI-thread work per frame: stroke + sync
    let mut stroke_cost = Vec::new();
    let mut sync_cost = Vec::new();
    let mut edit_time: HashMap<usize, Instant> = HashMap::new();
    let mut latencies = Vec::new();
    let mut tiles_total = 0usize;
    let mut tiles_per_frame = Vec::new();
    let mut gen_calls = 0usize;
    // Per frame: stage times (bin, raster, accumulate, write) and tiles the stroke rewrote.
    let mut frame_stages: Vec<([f64; 4], usize)> = Vec::new();
    let mut frame_counts: Vec<[u64; 4]> = Vec::new();
    let _ = subcraft_paint::stroke::profile::take_counts();
    let mut stage_total = [0.0f64; 4];

    let _ = subcraft_paint::stroke::profile::take();
    s.begin_stroke().unwrap();
    let start = Instant::now();
    let mut next = 0usize;
    for f in 0..n_frames {
        // Pace to the display: wait until this frame's start.
        let frame_start_t = f as f64 * frame_dt;
        let wait = frame_start_t - start.elapsed().as_secs_f64();
        if wait > 0.0 { std::thread::sleep(Duration::from_secs_f64(wait)); }
        // Events delivered since the last frame.
        let from = next;
        while next < events.len() && events[next].t <= frame_start_t + frame_dt { next += 1; }
        let batch = &events[from..next];
        let t0 = Instant::now();
        let mut touched: Vec<usize> = Vec::new();
        if !batch.is_empty() {
            match mode.as_str() {
                "event" => for e in batch {
                    s.set_stroke_pressure(e.pressure);
                    touched.extend(s.stroke_to(&cam, e.pos, false).unwrap());
                    gen_calls += 1;
                },
                "batch" => {
                    let e = batch.last().unwrap();
                    let path: Vec<Vec2> = batch.iter().map(|e| e.pos).collect();
                    s.set_stroke_pressure(e.pressure);
                    touched.extend(s.stroke_to_many(&cam, &path).unwrap());
                    gen_calls += 1;
                }
                _ => {
                    let e = batch.last().unwrap();
                    s.set_stroke_pressure(e.pressure);
                    touched.extend(s.stroke_to(&cam, e.pos, false).unwrap());
                    gen_calls += 1;
                }
            }
        }
        let t_stroke = t0.elapsed();
        let stages = subcraft_paint::stroke::profile::take();
        for (t, v) in stage_total.iter_mut().zip(stages) { *t += v; }
        frame_stages.push((stages, touched.len()));
        frame_counts.push(subcraft_paint::stroke::profile::take_counts());
        let now = Instant::now();
        for t in &touched { edit_time.insert(*t, now); }
        let t1 = Instant::now();
        let ups = s.update_composites_within(Some(Duration::from_millis(10)));
        let t_sync = t1.elapsed();
        let arrived = Instant::now();
        let mut shown = 0;
        for u in &ups {
            shown += u.tiles.len();
            for t in &u.tiles {
                if let Some(at) = edit_time.remove(t) { latencies.push(ms(arrived - at)); }
            }
        }
        tiles_total += shown;
        tiles_per_frame.push(shown as f64);
        stroke_cost.push(ms(t_stroke));
        sync_cost.push(ms(t_sync));
        frame_cost.push(ms(t0.elapsed()));
    }
    let t_end = Instant::now();
    s.end_stroke();
    let end_ms = ms(t_end.elapsed());
    // Settle: until exact.
    let t_settle = Instant::now();
    while s.composites_pending() {
        let ups = s.update_composites_within(Some(Duration::from_millis(10)));
        let arrived = Instant::now();
        for u in &ups { for t in &u.tiles { if let Some(at) = edit_time.remove(t) { latencies.push(ms(arrived - at)); } } }
        std::thread::sleep(Duration::from_millis(2));
    }
    let settle = ms(t_settle.elapsed());

    let end_stages = subcraft_paint::stroke::profile::take();
    let prof: [f64; 4] = std::array::from_fn(|i| stage_total[i] + end_stages[i]);
    println!("stage wall ms: {}", subcraft_paint::stroke::profile::NAMES.iter().zip(prof).map(|(n, v)| format!("{n} {v:.0}")).collect::<Vec<_>>().join(", "));
    let total: f64 = stroke_cost.iter().sum();
    println!("stroke_to calls {gen_calls}, stroke total {total:.0} ms ({:.1} ms/call avg); end_stroke {end_ms:.1} ms; settle after release {settle:.0} ms",
        total / gen_calls as f64);
    println!("frame cost (UI thread, ms): p50 {:.2} p95 {:.2} max {:.2}   [stroke p50 {:.2} p95 {:.2} max {:.2}] [sync p50 {:.2} p95 {:.2} max {:.2}]",
        pct(&frame_cost, 50), pct(&frame_cost, 95), pct(&frame_cost, 100) , pct(&stroke_cost, 50), pct(&stroke_cost, 95), pct(&stroke_cost, 100),
        pct(&sync_cost, 50), pct(&sync_cost, 95), pct(&sync_cost, 100));
    let over = frame_cost.iter().filter(|&&c| c > 1e3 / fps).count();
    println!("frames over {:.1} ms budget: {over}/{}; tiles shown {tiles_total} ({:.0}/frame avg, max {:.0})", 1e3 / fps, frame_cost.len(),
        tiles_total as f64 / frame_cost.len() as f64, pct(&tiles_per_frame, 100));
    println!("edit-to-shown latency per tile (ms): p50 {:.0} p95 {:.0} max {:.0} ({} samples, {} never shown)",
        pct(&latencies, 50), pct(&latencies, 95), pct(&latencies, 100), latencies.len(), edit_time.len());
    let mut worst: Vec<(usize, f64)> = frame_cost.iter().copied().enumerate().collect();
    worst.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("worst frames: {}", worst.iter().take(5).map(|(i, c)| format!("#{i} {c:.1}ms")).collect::<Vec<_>>().join(", "));
    for (i, c) in worst.iter().take(5) {
        let (st, tiles) = frame_stages[*i];
        println!("  #{i}: {c:.1} ms = bin {:.1} + raster {:.1} + accumulate {:.1} + write {:.1}; {tiles} tiles rewritten; {}", st[0], st[1], st[2], st[3],
            subcraft_paint::stroke::profile::COUNT_NAMES.iter().zip(frame_counts[*i]).map(|(n, v)| format!("{n} {v}")).collect::<Vec<_>>().join(", "));
    }
    let sum: [u64; 4] = std::array::from_fn(|k| frame_counts.iter().map(|c| c[k]).sum());
    println!("whole stroke: {}", subcraft_paint::stroke::profile::COUNT_NAMES.iter().zip(sum).map(|(n, v)| format!("{n} {v}")).collect::<Vec<_>>().join(", "));
    let busy = frame_stages.iter().filter(|(_, t)| *t > 0).count();
    println!("frames that rewrote tiles: {busy}/{}; end_stroke stages: raster {:.0}, accumulate {:.0}, write {:.0}", frame_stages.len(), end_stages[1], end_stages[2], end_stages[3]);
}
