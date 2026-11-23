//! Sends the default model (with its starting material) to a game engine project once, as the
//! live link does after each edit, with the model. With a second argument, also saves a
//! .subcraft there that remembers the engine project (File > Live link opens on it).
//! cargo run --release -p subcraft-app --example live_link_send -- <engine project folder> [out.subcraft]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use subcraft_app::Session;
use subcraft_app::live_link::LiveLinkTarget;

fn main() -> Result<(), String> {
    let dir = std::env::args().nth(1).ok_or("expected an engine project folder")?;
    let target = LiveLinkTarget::for_folder(std::path::Path::new(&dir)).ok_or("not a Unity, Unreal or Godot project")?;
    let mut s = Session::default_model();
    s.update_composites();
    let report = s.live_link_snapshot(&target, true).send()?;
    println!("{} -> {}", report.summary(target.engine), report.dir.display());
    if let Some(out) = std::env::args().nth(2) {
        s.set_live_link_target(Some(target));
        let path = s.save(Some(std::path::Path::new(&out)))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
