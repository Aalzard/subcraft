//! Contributed by MrHallM in issue #628 (memory profiling of large multi-set projects).
//! Memory profile: where does the RAM go when a project (.spp or .subcraft) is opened and composited?
//! `cargo run --release -p subcraft-app --example mem_profile -- file.spp|file.subcraft`
//! Prints the process working set per stage, document raster bytes per texture set, composite
//! bytes, the per-layer cache stats and the procedural field cache.

use std::sync::atomic::{AtomicUsize, Ordering};

use subcraft_app::spp_import::import_spp;
use subcraft_app::spp_job::SppEvent;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn rss_mb() -> u64 {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut system = System::new();
    let pid = sysinfo::get_current_pid().expect("pid");
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_memory());
    system.process(pid).map_or(0, |p| p.memory() >> 20)
}

fn report_doc(session: &subcraft_app::Session, label: &str) {
    println!("== {label}: working set {} MB", rss_mb());
    let mut doc_total = 0usize;
    let mut comp_total = 0usize;
    for ts in &session.document().texture_sets {
        let bytes = AtomicUsize::new(0);
        let rasters = AtomicUsize::new(0);
        for layer in &ts.layers {
            let mut l = (**layer).clone();
            l.map_rasters(&|img| {
                bytes.fetch_add(img.memory_bytes(), Ordering::Relaxed);
                rasters.fetch_add(1, Ordering::Relaxed);
                img.clone()
            });
        }
        let comp: usize = session.composite(ts.id).map_or(0, |c| c.channels.values().map(|i| i.memory_bytes()).sum());
        let b = bytes.into_inner();
        doc_total += b;
        comp_total += comp;
        println!(
            "  set {:<28} {:>5}² {:>3} top layers {:>4} rasters  doc {:>6} MB  composite {:>5} MB  channels {}",
            ts.name,
            ts.resolution,
            ts.layers.len(),
            rasters.into_inner(),
            b >> 20,
            comp >> 20,
            ts.channels.len()
        );
    }
    let lc = session.layer_cache_stats();
    println!(
        "  TOTAL doc rasters {} MB, composites {} MB, layer cache {} MB ({} entries), field cache {} MB",
        doc_total >> 20,
        comp_total >> 20,
        lc.bytes >> 20,
        lc.entries,
        subcraft_materials::sample::field::cached_bytes() >> 20
    );
    // REAL bytes: count each tile once (shared tiles by pointer), and how many unique tiles are
    // uniform (one value repeated) — what sharing uniform tiles could still save.
    let scan = |images: &mut dyn FnMut(&mut dyn FnMut(&subcraft_image::TiledImage))| {
        let mut seen = std::collections::HashSet::new();
        let (mut bytes, mut uniform, mut ubytes) = (0usize, 0usize, 0usize);
        images(&mut |img| {
            let c = img.comps();
            for i in 0..img.tile_count() {
                if let Some(t) = img.tile(i) {
                    if !seen.insert(std::sync::Arc::as_ptr(t) as usize) {
                        continue;
                    }
                    let b = t.data.len() * 2;
                    bytes += b;
                    let first = &t.data[..c];
                    if t.data.chunks(c).all(|px| px.iter().zip(first).all(|(a, b)| a.to_bits() == b.to_bits())) {
                        uniform += 1;
                        ubytes += b;
                    }
                }
            }
        });
        (bytes, uniform, ubytes)
    };
    let (db, du, dub) = scan(&mut |f| {
        for ts in &session.document().texture_sets {
            for layer in &ts.layers {
                let mut l = (**layer).clone();
                let cell = std::cell::RefCell::new(&mut *f);
                l.map_rasters(&|img| {
                    (cell.borrow_mut())(img);
                    img.clone()
                });
            }
        }
    });
    let (cb, cu, cub) = scan(&mut |f| {
        for ts in &session.document().texture_sets {
            if let Some(c) = session.composite(ts.id) {
                c.channels.values().for_each(|i| f(i));
            }
        }
    });
    println!("  UNIQUE doc tiles {} MB ({} uniform = {} MB) | UNIQUE composite tiles {} MB ({} uniform = {} MB)", db >> 20, du, dub >> 20, cb >> 20, cu, cub >> 20);
    let (content, geometry, maps) = session.memory_diagnostics();
    println!("  content cache {} MB, tile geometry {} MB, mesh maps {} MB", content >> 20, geometry >> 20, maps >> 20);
}

fn main() {
    let path = std::env::args().nth(1).expect("path to an .spp or .subcraft file");
    let control = subcraft_app::ImportControl::new();
    {
        let control = control.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let mut peak = 0;
            loop {
                let mb = rss_mb();
                peak = peak.max(mb);
                println!("[mem {:>6.1}s] {:>6} MB (peak {:>6})  {}", started.elapsed().as_secs_f32(), mb, peak, control.stage());
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
    }
    println!("start: {} MB", rss_mb());
    if path.to_ascii_lowercase().ends_with(".subcraft") {
        let mut s = subcraft_app::Session::open_project(std::path::Path::new(&path)).expect("open project");
        report_doc(&s, "project opened");
        let t = std::time::Instant::now();
        s.update_composites();
        println!("first full composite (inline, all sets): {:.1} s", t.elapsed().as_secs_f64());
        report_doc(&s, "after composite");
        return;
    }
    let mut opened: Option<subcraft_app::Session> = None;
    let result = import_spp(std::path::Path::new(&path), &control, &mut |event| match event {
        SppEvent::Opened(session, _) => {
            report_doc(&session, "flattened view opened");
            opened = Some(*session);
        }
        SppEvent::Stack(stack) => {
            println!("== stack rebuilt (before apply): working set {} MB", rss_mb());
            if let Some(session) = &mut opened {
                let _ = session.apply_spp_stack(stack);
            }
        }
    });
    if let Err(e) = result {
        println!("error: {e}");
        return;
    }
    let mut s = opened.expect("session");
    report_doc(&s, "stack applied");
    let t = std::time::Instant::now();
    s.update_composites();
    println!("first full composite (inline, all sets): {:.1} s", t.elapsed().as_secs_f64());
    report_doc(&s, "after composite");
}
