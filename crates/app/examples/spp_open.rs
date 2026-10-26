//! Developer tool: open a Substance Painter project like the app does and print the import notes.
//! `cargo run --release -p subcraft-app --example spp_open -- file.spp`

fn main() {
    let path = std::env::args().nth(1).expect("path to an .spp file");
    let t = std::time::Instant::now();
    match subcraft_app::Session::open_mesh(std::path::Path::new(&path), 1024) {
        Ok((session, notes)) => {
            for n in &notes {
                println!("{n}");
            }
            let ts = &session.document().texture_sets[0];
            println!("opened in {:?}: {} texture set(s), first has {} top-level layers", t.elapsed(), session.document().texture_sets.len(), ts.layers.len());
        }
        Err(e) => println!("error: {e}"),
    }
}
