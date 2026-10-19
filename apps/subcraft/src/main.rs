//! SubCraft application (desktop, and the web version built from the same code).
//!
//! Desktop usage: `subcraft [project.subcraft|mesh.obj|.gltf|.glb|.fbx] [--demo] [--view-2d] [--screenshot out.png]`

// Packaged Windows builds open without a console window (logs then go nowhere; run a normal
// build from a terminal to see them).
#![cfg_attr(all(windows, feature = "windows-gui"), windows_subsystem = "windows")]

/// mimalloc instead of the system heap: compositing allocates per-tile buffers on every core,
/// which the Windows heap serializes (ADR-0018).
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
    let cli: Vec<_> = std::env::args_os().skip(1).collect();
    if cli.first().is_some_and(|a| a == "--apply-update") {
        let path = cli.get(1).ok_or_else(|| anyhow::anyhow!("Missing update job"))?;
        return subcraft_app::updater::apply_job(std::path::Path::new(path)).map_err(|e| anyhow::anyhow!("{e}"));
    }
    // Logs go to the data folder (and to the console of non-packaged builds); panics leave a
    // crash report there that the next start shows.
    let data = subcraft_app::recovery::data_dir();
    subcraft_app::diagnostics::init_logging(data.as_ref().map(|d| d.join("logs")).as_deref(), !cfg!(feature = "windows-gui"));
    if let Some(d) = &data {
        subcraft_app::diagnostics::install_panic_hook(d.join("crashes"));
    }
    let mut options = subcraft_ui::LaunchOptions::default();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--preferences" => options.preferences = true,
            "--welcome" => options.welcome = true,
            "--quick-start" => options.guide_step = Some(0),
            "--guide-step" => {
                let step = args.next().and_then(|s| s.parse::<usize>().ok()).filter(|s| (1..=6).contains(s))
                    .ok_or_else(|| anyhow::anyhow!("--guide-step expects 1 through 6"))?;
                options.guide_step = Some(step - 1);
            }

            "--resume-update" => options.resume_update = args.next().map(Into::into),
            "--check-updates" => options.check_updates = true,
            #[cfg(feature = "update-fixture")]
            "--auto-update-fixture" => options.auto_update_fixture = true,
            "--demo" => options.demo = true,
            "--view-2d" => options.view_2d = true,
            "--brush-panel" => options.brush_panel = true,
            "--bake" => options.bake = true,
            "--bake-start" => options.bake_start = true,
            "--bake-high" => options.bake_high.extend(args.next().map(Into::into)),
            "--bake-by-name" => options.bake_by_name = true,
            "--screenshot-frames" => {
                options.screenshot_frames =
                    Some(args.next().and_then(|n| n.parse().ok()).ok_or_else(|| anyhow::anyhow!("--screenshot-frames expects a frame count"))?);
            }
            "--screenshot" => options.screenshot = args.next().map(Into::into),
            "--recover" => options.recover = true,
            "--test-crash" => options.test_crash = true,
            "--profile-edits" => {
                let out = args.next().ok_or_else(|| anyhow::anyhow!("--profile-edits expects an output file"))?;
                let resolution = args.peek().and_then(|s| s.parse::<u32>().ok());
                if resolution.is_some() { args.next(); }
                options.profile_edits = Some((out.into(), resolution));
            }
            "--autosave-interval" => {
                options.autosave_interval = args.next().and_then(|s| s.parse::<f64>().ok()).map(std::time::Duration::from_secs_f64);
            }
            "-h" | "--help" => {
                println!("usage: subcraft [project.subcraft | mesh file] [--demo] [--view-2d] [--brush-panel] [--bake] [--bake-start] [--bake-high mesh]... [--bake-by-name] [--screenshot out.png] [--screenshot-frames n] [--welcome] [--quick-start] [--guide-step 1..6] [--recover] [--autosave-interval seconds]");
                return Ok(());
            }
            path => options.mesh = Some(path.into()),
        }
    }
    #[cfg(feature = "update-fixture")]
    if options.resume_update.is_some() {
        options.screenshot = std::env::var_os("SUBCRAFT_UPDATE_FIXTURE_SCREENSHOT").map(Into::into);
    }
    subcraft_ui::run(options).map_err(|e| anyhow::anyhow!("{e}"))
}
