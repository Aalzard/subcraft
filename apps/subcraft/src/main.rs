//! SubCraft application (desktop, and the web version built from the same code).
//!
//! Desktop usage: `subcraft [project.subcraft|mesh.obj|.gltf|.glb|.fbx] [--demo] [--view-2d] [--screenshot out.png]`

// Packaged Windows builds open without a console window (logs then go nowhere; run a normal
// build from a terminal to see them).
#![cfg_attr(all(windows, feature = "windows-gui"), windows_subsystem = "windows")]

/// Web version: start in the page's `subcraft_canvas` (see `web/index.html`).
#[cfg(target_arch = "wasm32")]
fn main() {
    let _ = eframe::WebLogger::init(log::LevelFilter::Info);
    wasm_bindgen_futures::spawn_local(async {
        if let Err(e) = subcraft_ui::start_web("subcraft_canvas").await {
            log::error!("SubCraft failed to start: {e:?}");
            if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("loading")) {
                el.set_inner_html("SubCraft could not start in this browser. It needs WebGPU: use a recent Chrome, Edge, Firefox or Safari.");
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
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
                println!("usage: subcraft [project.subcraft | mesh file] [--demo] [--view-2d] [--screenshot out.png]");
                return Ok(());
            }
            path => options.mesh = Some(path.into()),
        }
    }
    subcraft_ui::run(options).map_err(|e| anyhow::anyhow!("{e}"))
}

/// Minimal stderr logger for warnings and errors (no extra dependency).
#[cfg(not(target_arch = "wasm32"))]
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
