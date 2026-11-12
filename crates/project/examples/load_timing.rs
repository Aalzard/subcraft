//! Where project load time goes: `cargo run --release -p subcraft-project --example load_timing -- file.subcraft`
//! Times inflating every entry, decoding the rasters, and the whole `load_project`.
use std::io::Read;
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("project path");
    let t = Instant::now();
    let data = subcraft_project::load_project(std::path::Path::new(&path)).expect("load");
    println!("load_project: {:.0} ms ({} texture sets)", t.elapsed().as_secs_f64() * 1e3, data.document.texture_sets.len());

    let file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let (mut inflate, mut decode, mut raw, mut packed, mut rasters) = (0.0, 0.0, 0u64, 0u64, 0);
    for i in 0..zip.len() {
        let t = Instant::now();
        let mut f = zip.by_index(i).unwrap();
        let name = f.name().map(|n| n.to_string()).unwrap_or_default();
        packed += f.compressed_size();
        let mut v = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut v).unwrap();
        drop(f);
        let dt = t.elapsed().as_secs_f64() * 1e3;
        inflate += dt;
        raw += v.len() as u64;
        {
            let t = Instant::now();
            if subcraft_project::raster::decode(&v).is_ok() {
                rasters += 1;
                decode += t.elapsed().as_secs_f64() * 1e3;
            }
        }
        if dt > 300.0 {
            println!("  {name}: {dt:.0} ms inflate, {} MB", v.len() >> 20);
        }
    }
    println!("entries: {}, {} MB packed, {} MB raw", zip.len(), packed >> 20, raw >> 20);
    println!("inflate all entries: {inflate:.0} ms; decode {rasters} rasters: {decode:.0} ms");
}
