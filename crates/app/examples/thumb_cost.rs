//! Cost of one shelf thumbnail composite (the CPU part; the GPU render is a 160 px draw).
//! cargo run --release -p subcraft-app --example thumb_cost -- [resolution] [boost: 0|1]
use std::time::Instant;
use subcraft_app::thumbnails::{PreviewScene, zoom_out_tiling};
use subcraft_assets::{Library, presets::decode_images};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let res: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(128);
    let boost = args.get(1).is_some_and(|s| s == "1");
    let lib = Library::builtin();
    let t = Instant::now();
    let baked = PreviewScene::baked_sphere_at(res);
    let plain = PreviewScene::sphere_at(res);
    println!("resolution {res}, tiling boost {boost}: scene setup (sphere bakes) {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let mut times = Vec::new();
    for m in &lib.materials {
        let (ids, images) = decode_images(&m.images);
        let t = Instant::now();
        let mut layer = m.build_with(plain.texture_set(), &ids);
        if boost { zoom_out_tiling(&mut layer); }
        let _ = plain.composite_with(layer, &images);
        times.push(("material", m.name.clone(), t.elapsed().as_secs_f64() * 1000.0));
    }
    for m in &lib.smart_materials {
        let (ids, decoded) = decode_images(m.images());
        let images: Vec<_> = m.resources().iter().cloned().chain(decoded).collect();
        let t = Instant::now();
        let mut layer = m.build_with(baked.texture_set(), &ids);
        if boost { zoom_out_tiling(&mut layer); }
        let _ = baked.composite_with(layer, &images);
        times.push(("smart", m.name.clone(), t.elapsed().as_secs_f64() * 1000.0));
    }
    for kind in ["material", "smart"] {
        let mut v: Vec<f64> = times.iter().filter(|t| t.0 == kind).map(|t| t.2).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        println!("{kind:9} n={:2} mean {mean:7.1} ms  median {:7.1}  max {:7.1}", v.len(), v[v.len() / 2], v[v.len() - 1]);
    }
    if let Some(w) = times.iter().max_by(|a, b| a.2.partial_cmp(&b.2).unwrap()) { println!("slowest: {} {} {:.1} ms", w.0, w.1, w.2); }
}
