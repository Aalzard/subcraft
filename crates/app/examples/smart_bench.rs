//! Everyday-workflow benchmark: the default model (`SubCraft_base`) with real built-in smart
//! materials, timed through the same `Session` calls the app makes.
//!
//! `cargo run --release -p subcraft-app --example smart_bench -- [resolution] [materials]`
//!
//! Prints: startup (default model + default material), bake of all mesh maps, each smart material
//! added + composited, layer edits (visibility / opacity of the bottom and top material), brush
//! strokes on a paint layer on top (stamp time and stamp + recomposite), save and open. Set
//! `SUBCRAFT_BENCH_HASH=1` for a hash of every composite texel bit (outputs must not change).
#![allow(unsafe_code)]
use std::time::Instant;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_paint::Camera;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn run(s: &mut Session, cmd: Command) {
    s.execute(cmd).unwrap_or_else(|e| panic!("{e:?}"));
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn composite_hash(s: &Session) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for ts in &s.document().texture_sets {
        let Some(c) = s.composite(ts.id) else { continue };
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
    }
    h
}

fn hash_line(s: &Session, label: &str) {
    if std::env::var_os("SUBCRAFT_BENCH_HASH").is_some() {
        println!("    hash {label}: {:016x}", composite_hash(s));
    }
}

/// Recomposite until nothing is stale; returns tiles evaluated.
fn settle(s: &mut Session) -> usize {
    s.update_composites().iter().map(|u| u.tiles.len()).sum()
}

/// Node evaluations by kind (needs `SUBCRAFT_COMPOSITE_PROFILE=1`); time is summed over threads.
fn profile(label: &str) {
    let rows = subcraft_compositor::profile::take();
    if rows.is_empty() {
        return;
    }
    println!("    node profile ({label}):");
    for (k, n, t) in rows.iter().take(8) {
        println!("      {k:<16} {n:>8} evals {t:>9.0} ms (thread-sum)");
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let res: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2048);
    let count: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(4);
    println!("== smart_bench: {res}², {count} smart materials, {} threads", rayon::current_num_threads());

    let t = Instant::now();
    let mut s = Session::default_model();
    let t_model = ms(t);
    run(&mut s, Command::SetResolution { texture_set: None, resolution: res });
    let t2 = Instant::now();
    let tiles = settle(&mut s);
    println!("startup: default model {t_model:.0} ms, first composite {:.0} ms ({tiles} tiles)", ms(t2));

    // Bake every mesh map of every texture set at the set resolution.
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    let t = Instant::now();
    let mut job = s.start_bake(subcraft_app::BakeRequest { settings: subcraft_bake::BakeSettings { size: res, ..Default::default() }, high_poly: None, texture_sets: sets.clone() });
    while job.poll(&mut s).is_none() {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    println!("bake (all maps, {} set(s)): {:.0} ms", sets.len(), ms(t));
    let t = Instant::now();
    let tiles = settle(&mut s);
    println!("recomposite after bake: {:.0} ms ({tiles} tiles)", ms(t));

    let lib = subcraft_assets::Library::builtin();
    // `SUBCRAFT_BENCH_MATERIALS="Bricks,Brown Leather"` picks other materials (name prefixes).
    let custom = std::env::var("SUBCRAFT_BENCH_MATERIALS").ok();
    let wanted: Vec<&str> = match &custom {
        Some(list) => list.split(',').map(str::trim).collect(),
        None => vec!["Rusty Iron", "Dusty Plastic", "Worn Painted Metal", "Chipped Paint", "Brushed Steel", "Oxidized Copper"],
    };
    let mut picked: Vec<_> = wanted.iter().filter_map(|w| lib.smart_materials.iter().find(|m| m.name.starts_with(w))).collect();
    for m in &lib.smart_materials {
        if picked.len() >= count { break; }
        if !picked.iter().any(|p| p.name == m.name) { picked.push(m); }
    }
    picked.truncate(count);
    let mut added: Vec<LayerId> = Vec::new();
    let mut total = 0.0;
    for m in &picked {
        run(&mut s, Command::SelectLayer(None));
        let t = Instant::now();
        s.add_smart_material(m).unwrap();
        let t_add = ms(t);
        let tiles = settle(&mut s);
        let all = ms(t);
        total += all;
        added.push(s.selection().layer.unwrap());
        println!("add smart material {:<22} {all:7.0} ms (command {t_add:.0} ms, {tiles} tiles)", m.name);
    }
    println!("all smart materials: {total:.0} ms");
    profile("materials");
    hash_line(&s, "after materials");

    // Layer edits.
    let reps = 3;
    for (label, id) in [("bottom", added[0]), ("top", *added.last().unwrap())] {
        let mut v = Vec::new();
        for i in 0..reps * 2 {
            let t = Instant::now();
            run(&mut s, Command::SetVisible { layer: id, visible: i % 2 == 1 });
            settle(&mut s);
            v.push(ms(t));
        }
        let mut o = Vec::new();
        for i in 0..reps * 2 {
            let t = Instant::now();
            run(&mut s, Command::SetBlendOpacity { layer: id, channel: ChannelKind::BaseColor, opacity: if i % 2 == 0 { 0.6 } else { 1.0 } });
            settle(&mut s);
            o.push(ms(t));
        }
        profile(label);
        println!("{label:<6} material: visibility {:6.0} ms, opacity {:6.0} ms (median)", median(v), median(o));
    }
    hash_line(&s, "after edits");

    // Brush strokes on a paint layer above everything.
    run(&mut s, Command::SelectLayer(None));
    run(&mut s, Command::AddLayer(NewLayer::Paint));
    let paint = s.selection().layer.unwrap();
    run(&mut s, Command::SelectLayerTarget { layer: paint, target: LayerTarget::Content });
    s.brush.target = subcraft_paint::PaintTarget::Content;
    let mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).unwrap().mesh;
    let (lo, hi) = mesh.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let center = (lo + hi) * 0.5;
    let radius = (hi - lo).length() * 0.5;
    let cam = Camera::orthographic(center + Vec3::new(0.0, 0.0, radius * 3.0), center, Vec3::Y, radius * 1.05, 0.01, radius * 10.0, [1280, 800]);
    for (size, label) in [(30.0f32, "small"), (220.0, "large")] {
        s.brush.settings.size = size;
        s.brush.settings.hardness = 0.7;
        let mut stamp = Vec::new();
        let mut frame = Vec::new();
        let mut hits = 0;
        s.begin_stroke().unwrap();
        for i in 0..120 {
            let a = i as f32 / 120.0;
            let p = Vec2::new(640.0 + 360.0 * (a * 6.283).cos() * (1.0 - a * 0.5), 400.0 + 220.0 * (a * 6.283).sin());
            let t = Instant::now();
            let touched = s.stroke_to(&cam, p, false).unwrap();
            stamp.push(ms(t));
            hits += usize::from(!touched.is_empty());
            settle(&mut s);
            frame.push(ms(t));
        }
        s.end_stroke();
        let t = Instant::now();
        settle(&mut s);
        println!("stroke {label:<5} ({size} px): stamp {:.2} ms, stamp + composite {:.2} ms median per frame (sum {:.0} ms), finish {:.0} ms, {hits}/120 frames painted", median(stamp), median(frame.clone()), frame.iter().sum::<f64>(), ms(t));
    }
    hash_line(&s, "after strokes");

    let dir = std::env::temp_dir().join(format!("smart_bench_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bench.subcraft");
    let t = Instant::now();
    s.save(Some(&path)).unwrap();
    println!("save: {:.0} ms ({} MB)", ms(t), std::fs::metadata(&path).map(|m| m.len() >> 20).unwrap_or(0));
    let t = Instant::now();
    let mut o = Session::open_project(&path).unwrap();
    let t_open = ms(t);
    let t = Instant::now();
    settle(&mut o);
    println!("open: {t_open:.0} ms, first composite {:.0} ms", ms(t));
    let _ = std::fs::remove_dir_all(&dir);
}
