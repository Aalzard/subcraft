//! Same as `subcraft-bake`'s `bake_bench`, with the GPU ray baker registered:
//! `cargo run --release -p subcraft-ui --example gpu_bake_bench -- <crate|torus100k|torus500k> <size> [map] [cpu]`
//! Prints the path that made each map. Pass `cpu` as the last argument to force the CPU.

use std::time::Instant;

use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::MeshMapKind;
use subcraft_mesh::{Bvh, primitives};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = args.first().map_or("crate", String::as_str);
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2048);
    let only = args.get(2).filter(|s| s.as_str() != "all");
    let use_gpu = args.last().is_none_or(|a| a != "cpu");
    subcraft_ui::register_gpu_baking();
    let mesh = match name {
        "crate" => primitives::sample_crate(),
        "torus100k" => primitives::torus(1.0, 0.35, 250, 200),
        "torus500k" => primitives::torus(1.0, 0.35, 560, 446),
        other => panic!("unknown mesh {other}"),
    };
    let bvh = Bvh::build(&mesh);
    println!("mesh {name}: {} triangles, {size}x{size}, use_gpu={use_gpu}", mesh.triangle_count());
    for kind in MeshMapKind::ALL {
        if only.is_some_and(|o| !kind.display_name().eq_ignore_ascii_case(o)) {
            continue;
        }
        let settings = BakeSettings { size, maps: vec![kind], use_gpu, ..BakeSettings::default() };
        let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
        let progress = Progress::new();
        let t = Instant::now();
        bake(&input, &settings, &progress).expect("bake");
        let path = progress.reports().first().map(|r| r.backend.clone()).unwrap_or_default();
        println!("{:<22} {:8.2} s   {path}", kind.display_name(), t.elapsed().as_secs_f32());
    }
}
