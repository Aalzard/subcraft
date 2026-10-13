//! SubCraft desktop application.
//!
//! Usage: `subcraft [mesh.obj|.gltf|.glb|.fbx] [--demo] [--view-2d] [--screenshot out.png]`

fn main() -> anyhow::Result<()> {
    env_logger_lite();
    let mut options = subcraft_ui::LaunchOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--demo" => options.demo = true,
            "--view-2d" => options.view_2d = true,
            "--screenshot" => options.screenshot = args.next().map(Into::into),
            "-h" | "--help" => {
                println!("usage: subcraft [mesh file] [--demo] [--view-2d] [--screenshot out.png]");
                return Ok(());
            }
            path => options.mesh = Some(path.into()),
        }
    }
    subcraft_ui::run(options).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Minimal stderr logger for warnings and errors (no extra dependency).
fn env_logger_lite() {
    struct L;
    impl log::Log for L {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.level() <= log::Level::Info && m.target().starts_with("subcraft")
        }
        fn log(&self, r: &log::Record) {
            if self.enabled(r.metadata()) {
                eprintln!("[{}] {}", r.level(), r.args());
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: L = L;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
}
