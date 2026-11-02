//! Developer tool: open a Substance Painter project like the app does and print the import notes
//! with the time until the model and flattened textures are usable and until the rebuilt stack is in.
//! `cargo run --release -p subcraft-app --example spp_open -- file.spp`

use subcraft_app::spp_import::import_spp;
use subcraft_app::spp_job::SppEvent;

fn main() {
    let path = std::env::args().nth(1).expect("path to an .spp file");
    let t = std::time::Instant::now();
    let control = subcraft_app::ImportControl::new();
    // SUBCRAFT_SPP_MEMLOG=1 prints the import stage and the working set every 2 seconds.
    if std::env::var_os("SUBCRAFT_SPP_MEMLOG").is_some() {
        let control = control.clone();
        std::thread::spawn(move || {
            use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
            let mut system = System::new();
            let pid = sysinfo::get_current_pid().expect("pid");
            let started = std::time::Instant::now();
            loop {
                system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_memory());
                if let Some(p) = system.process(pid) {
                    println!("[mem {:>6.1}s] {:>5} MB  {}", started.elapsed().as_secs_f32(), p.memory() >> 20, control.stage());
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        });
    }
    let mut opened: Option<subcraft_app::Session> = None;
    let mut notes: Vec<String> = Vec::new();
    let result = import_spp(std::path::Path::new(&path), &control, &mut |event| match event {
        SppEvent::Opened(session, n) => {
            println!("first textured view after {:?}", t.elapsed());
            notes.extend(n);
            opened = Some(*session);
        }
        SppEvent::Stack(stack) => {
            println!("rebuilt stack ready after {:?}", t.elapsed());
            if let Some(session) = &mut opened {
                notes.extend(session.apply_spp_stack(stack));
            }
        }
    });
    for n in &notes {
        println!("{n}");
    }
    match (result, opened) {
        (Ok(()), Some(session)) => {
            let ts = &session.document().texture_sets[0];
            println!("opened in {:?}: {} texture set(s), first has {} top-level layers", t.elapsed(), session.document().texture_sets.len(), ts.layers.len());
        }
        (Err(e), _) => println!("error: {e}"),
        _ => println!("error: no project"),
    }
}
