//! Project open cost in a fresh process:
//! `cargo run --release -p subcraft-app --example open_timing -- file.subcraft`
//! Times the file load, the session setup, the first full composite and the first quick preview.
use std::time::Instant;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("project path"));
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let data = subcraft_project::load_project(&path).expect("load");
    println!("load_project (file): {:.0} ms", ms(t));
    drop(data);
    let t = Instant::now();
    let mut s = subcraft_app::Session::open_project(&path).expect("open");
    println!("Session::open_project: {:.0} ms", ms(t));
    let t = Instant::now();
    let ups = s.update_composites();
    println!("first composite: {:.0} ms, {} tiles", ms(t), ups.iter().map(|u| u.tiles.len()).sum::<usize>());
    profile();
    // Bottom top-level layer: visibility off and on again, opacity twice (the edits users wait on).
    let ts = &s.document().texture_sets[0];
    let bottom = ts.layers[0].id;
    let name = ts.layers[0].name.clone();
    for (label, cmd) in [
        ("bottom visibility off", subcraft_app::Command::SetVisible { layer: bottom, visible: false }),
        ("bottom visibility on", subcraft_app::Command::SetVisible { layer: bottom, visible: true }),
        ("bottom opacity 0.6", subcraft_app::Command::SetBlendOpacity { layer: bottom, channel: subcraft_core::ChannelKind::BaseColor, opacity: 0.6 }),
        ("bottom opacity 0.9", subcraft_app::Command::SetBlendOpacity { layer: bottom, channel: subcraft_core::ChannelKind::BaseColor, opacity: 0.9 }),
    ] {
        let t = Instant::now();
        s.execute(cmd).expect("edit");
        let tiles: usize = s.update_composites().iter().map(|u| u.tiles.len()).sum();
        println!("{label} ({name}): {:.0} ms, {tiles} tiles", ms(t));
        profile();
    }
}

/// Node evaluations by kind (needs `SUBCRAFT_COMPOSITE_PROFILE=1`); time is summed over threads.
fn profile() {
    for (k, n, t) in subcraft_compositor::profile::take().iter().take(12) {
        println!("  {k:<20} {n:>8} evals {t:>9.0} ms (thread-sum)");
    }
}
