//! Print what a smart material / smart mask file holds: its datasets and the root of its preset.
//! `cargo run -p subcraft-mesh --example spsm_probe -- file.spsm [...]`
use std::io::BufReader;

fn main() {
    for arg in std::env::args().skip(1) {
        println!("== {arg}");
        let file = match std::fs::File::open(&arg) {
            Ok(f) => f,
            Err(e) => {
                println!("  cannot open: {e}");
                continue;
            }
        };
        let mut h5 = match subcraft_mesh::spp::hdf5::Hdf5File::open(BufReader::new(file)) {
            Ok(h) => h,
            Err(e) => {
                println!("  not HDF5: {e}");
                continue;
            }
        };
        let names = h5.list("/").unwrap_or_default();
        println!("  root: {names:?}");
        for name in names.iter().filter(|n| n.contains("preset")) {
            let Ok(info) = h5.dataset(name) else { continue };
            let Ok(bytes) = h5.read(&info) else { continue };
            match subcraft_mesh::spp::document::parse(&bytes) {
                Ok(root) => {
                    println!("  {name}: {} bytes, root class {:?}", bytes.len(), root.class);
                    for (field, _) in root.fields.iter().take(12) {
                        print!(" {field}");
                    }
                    println!();
                }
                Err(e) => println!("  {name}: {} bytes, parse error {e:?}", bytes.len()),
            }
        }
    }
}
