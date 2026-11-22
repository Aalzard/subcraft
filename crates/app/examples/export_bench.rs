//! Texture export benchmark: the default model with smart materials, exported through the same
//! `Session::export_textures` call the Export window makes.
//!
//! `cargo run --release -p subcraft-app --example export_bench -- [resolution] [materials] [template]`
//!
//! `template`: `pbr` (default, PBR Metal/Rough PNG) or `unreal`. Prints the export time, the number of
//! files and bytes written, and a hash of every written file (set `SUBCRAFT_BENCH_HASH=1`): the
//! hash must not change between builds when only speed changes.
use std::time::Instant;

use subcraft_app::{Command, Session};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let mut args = std::env::args().skip(1);
    let res: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2048);
    let count: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(2);
    let template = args.next().unwrap_or_else(|| "pbr".into());
    println!("== export_bench: {res}², {count} smart materials, template {template}, {} threads", rayon::current_num_threads());

    let mut s = Session::default_model();
    s.execute(Command::SetResolution { texture_set: None, resolution: res }).unwrap();
    let lib = subcraft_assets::Library::builtin();
    for m in lib.smart_materials.iter().take(count) {
        s.execute(Command::SelectLayer(None)).unwrap();
        s.add_smart_material(m).unwrap();
    }
    let t = Instant::now();
    s.update_composites();
    println!("composite: {:.0} ms", ms(t));

    let mut settings = subcraft_export::ExportSettings::default();
    if template == "unreal" {
        settings.template = subcraft_export::ExportTemplate::unreal();
    }
    let dir = std::env::temp_dir().join(format!("export_bench_{}", std::process::id()));
    let mut times = Vec::new();
    let mut files = Vec::new();
    for _ in 0..3 {
        let _ = std::fs::remove_dir_all(&dir);
        let t = Instant::now();
        files = s.export_textures(&dir, &settings).unwrap();
        times.push(ms(t));
    }
    let bytes: u64 = files.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    times.sort_by(f64::total_cmp);
    println!("export: best {:.0} ms, median {:.0} ms ({} files, {} MB)", times[0], times[1], files.len(), bytes >> 20);
    if std::env::var_os("SUBCRAFT_BENCH_HASH").is_some() {
        files.sort();
        let mut h = 0xcbf29ce484222325u64;
        for f in &files {
            for b in std::fs::read(f).unwrap() {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
        }
        println!("    hash files: {h:016x}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
