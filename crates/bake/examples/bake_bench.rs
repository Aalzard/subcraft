//! CPU baseline timing for every mesh map: `cargo run --release -p subcraft-bake --example bake_bench -- <mesh> <size> [map]`
//! with `<mesh>` = `crate`, `torus100k` or `torus500k`. Run one configuration at a time; each map
//! is baked alone, so its time includes UV rasterisation and projection.

use std::time::Instant;

use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::MeshMapKind;
use subcraft_mesh::{Bvh, primitives};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = args.first().map_or("crate", String::as_str);
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2048);
    let only = args.get(2);
    let mesh = match name {
        "crate" => primitives::sample_crate(),
        "torus100k" => primitives::torus(1.0, 0.35, 250, 200),
        "torus500k" => primitives::torus(1.0, 0.35, 560, 446),
        other => panic!("unknown mesh {other}"),
    };
    let t = Instant::now();
    let bvh = Bvh::build(&mesh);
    println!("mesh {name}: {} triangles, BVH {:.2}s, {size}x{size}", mesh.triangle_count(), t.elapsed().as_secs_f32());
    for kind in MeshMapKind::ALL {
        if only.is_some_and(|o| !kind.display_name().eq_ignore_ascii_case(o)) {
            continue;
        }
        let settings = BakeSettings { size, maps: vec![kind], ..BakeSettings::default() };
        let input = BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: 0 };
        let t = Instant::now();
        bake(&input, &settings, &Progress::new()).expect("bake");
        println!("{:<22} {:8.2} s", kind.display_name(), t.elapsed().as_secs_f32());
    }
}
