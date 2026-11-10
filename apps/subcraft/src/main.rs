//! SubCraft application (desktop).
//!
//! Desktop usage: `subcraft [project.subcraft|mesh.obj|.gltf|.glb|.fbx|.abc|.usd|.usda|.usdc|.usdz|.ply|.stl|.spp] [--demo] [--view-2d] [--screenshot out.png]`

// Packaged Windows builds open without a console window (logs then go nowhere; run a normal
// build from a terminal to see them).
#![cfg_attr(all(windows, feature = "windows-gui"), windows_subsystem = "windows")]

/// mimalloc instead of the system heap: compositing allocates per-tile buffers on every core,
/// which the Windows heap serializes (ADR-0018).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    // Every failure ends in something visible: a packaged Windows build has no console.
    let outcome = std::panic::catch_unwind(run_app);
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let text = format!("{e:#}

Log folder: {}", subcraft_app::recovery::data_dir().map(|d| d.display().to_string()).unwrap_or_else(|| "unavailable".into()));
            eprintln!("SubCraft: {text}");
            subcraft_ui::show_fatal("SubCraft cannot start", &text);
            std::process::exit(1);
        }
        Err(panic) => {
            let msg = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
            subcraft_ui::show_fatal("SubCraft crashed", &format!("SubCraft hit an internal error and has to close:
{msg}

A crash report was saved in the SubCraft data folder (crashes). Please attach it, and startup.log, to a bug report. Starting again opens in safe mode if this happened during start-up."));
            std::process::exit(101);
        }
    }
}

fn run_app() -> anyhow::Result<()> {
    let cli: Vec<_> = std::env::args_os().skip(1).collect();
    if cli.first().is_some_and(|a| a == "--apply-update") {
        let path = cli.get(1).ok_or_else(|| anyhow::anyhow!("Missing update job"))?;
        return subcraft_app::updater::apply_job(std::path::Path::new(path)).map_err(|e| anyhow::anyhow!("{e}"));
    }
    // Crash-safe startup (issue #83): the GPU probe child, then the breadcrumb log, before any
    // graphics call. A driver crash in the probe kills only the child.
    if cli.first().is_some_and(|a| a == "--gpu-probe") {
        let rest: Vec<String> = cli.iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
        std::process::exit(subcraft_ui::gpu_probe::child_main(&rest));
    }
    subcraft_app::startup_probe::begin(subcraft_app::startup_probe::startup_log_path().as_deref());
    // Logs go to the data folder (and to the console of non-packaged builds); panics leave a
    // crash report there that the next start shows.
    let data = subcraft_app::recovery::data_dir();
    subcraft_app::diagnostics::init_logging(data.as_ref().map(|d| d.join("logs")).as_deref(), !cfg!(feature = "windows-gui"));
    if let Some(d) = &data {
        subcraft_app::diagnostics::install_panic_hook(d.join("crashes"));
    }
    let flag = |name: &str| cli.iter().any(|a| a == name);
    subcraft_app::safe_mode::init(subcraft_app::startup_probe::previous_log(), flag("--safe-mode"), flag("--normal-mode"));
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
            "--safe-mode" | "--normal-mode" => {}
            "--demo" => options.demo = true,
            "--view-2d" => options.view_2d = true,
            "--brush-panel" => options.brush_panel = true,
            "--shelf-panel" => options.shelf_panel = true,
            "--texture-set-settings" => options.texture_set_settings = true,
            "--generator-panel" => options.generator_panel = true,
            "--effect-panel" => options.effect_panel = true,
            "--select-brush" => options.select_brush = Some(args.next().ok_or_else(|| anyhow::anyhow!("--select-brush expects a Shelf brush name"))?),
            "--capture-tool" => options.capture_tool = Some(args.next().ok_or_else(|| anyhow::anyhow!("--capture-tool expects clone or smudge"))?),
            "--import-smart" => options.import_smart.push(args.next().ok_or_else(|| anyhow::anyhow!("--import-smart expects a .scsmart file or a folder"))?.into()),
            "--shelf-tab" => options.shelf_tab = Some(args.next().ok_or_else(|| anyhow::anyhow!("--shelf-tab expects a Shelf tab name"))?),
            "--render-image" => {
                let path: std::path::PathBuf = args.next().ok_or_else(|| anyhow::anyhow!("--render-image expects an output .png"))?.into();
                options.render_job = Some(subcraft_ui::render_export::Job::Image(path));
                options.screenshot.get_or_insert_with(|| std::env::temp_dir().join("subcraft-render-window.png"));
            }
            "--turntable" => {
                let dir: std::path::PathBuf = args.next().ok_or_else(|| anyhow::anyhow!("--turntable expects an output folder"))?.into();
                options.render_job = Some(subcraft_ui::render_export::Job::Turntable(dir));
                options.screenshot.get_or_insert_with(|| std::env::temp_dir().join("subcraft-render-window.png"));
            }
            "--path-trace" => {
                let samples = args.next_if(|s| s.parse::<u32>().is_ok()).and_then(|s| s.parse::<u32>().ok()).unwrap_or(256);
                options.path_trace = Some(samples);
            }
            "--quality" => options.quality = Some(args.next().ok_or_else(|| anyhow::anyhow!("--quality expects tone=..,bloom=..,ao=.."))?),
            "--export-window" => options.export_window = true,
            "--new-project" => options.new_project_model = args.next().map(Into::into),
            "--new-project-high" => options.new_project_high = args.next().map(Into::into),
            "--properties-panel" => options.properties_panel = true,
            "--export-template" => options.export_template = args.next().and_then(|s| s.parse().ok()),
            "--render-window" => options.render_window = true,
            "--camera" => options.camera = Some(args.next().and_then(|s| subcraft_ui::orbit::CameraView::parse(&s)).ok_or_else(|| anyhow::anyhow!("--camera expects yaw,pitch,zoom[,x,y,z] (degrees, zoom factor, target offset)"))?),
            "--symmetry" => {
                let axes = args.next().ok_or_else(|| anyhow::anyhow!("--symmetry expects x, y, z or a combination"))?.to_ascii_lowercase();
                if axes.is_empty() || !axes.chars().all(|c| "xyz".contains(c)) { anyhow::bail!("--symmetry expects x, y, z or a combination"); }
                options.symmetry_axes = Some(['x', 'y', 'z'].map(|axis| axes.contains(axis)));
            }
            "--radial" => {
                let spec = args.next().ok_or_else(|| anyhow::anyhow!("--radial expects an axis and a count, e.g. y8"))?.to_ascii_lowercase();
                let axis = spec.chars().next().and_then(|c| "xyz".find(c)).ok_or_else(|| anyhow::anyhow!("--radial expects an axis and a count, e.g. y8"))?;
                let count: u32 = spec[1..].parse().map_err(|_| anyhow::anyhow!("--radial expects an axis and a count, e.g. y8"))?;
                if !(2..=64).contains(&count) { anyhow::bail!("--radial count must be 2..=64"); }
                options.radial_symmetry = Some((axis as u8, count));
            }
            "--bake-high" => options.bake_high_poly = Some(args.next().ok_or_else(|| anyhow::anyhow!("--bake-high expects a high-poly mesh path"))?.into()),
            "--bake-place" => options.bake_place = Some(args.next().ok_or_else(|| anyhow::anyhow!("--bake-place expects e.g. auto,offset=0.1:0:0,scale=0.8"))?),
            "--bake-preview" => options.bake_preview_only = true,
            "--bake-low" => options.bake_low = true,
            "--select-layer" => options.select_layer = args.next(),
            "--bake-match" => {
                options.bake_match = Some(match args.next().as_deref() {
                    Some("always") => subcraft_app::BakeMatchMode::Always,
                    Some("name") => subcraft_app::BakeMatchMode::ByName,
                    _ => anyhow::bail!("--bake-match expects always or name"),
                });
            }
            "--bake-preview-map" => {
                let name = args.next().ok_or_else(|| anyhow::anyhow!("--bake-preview-map expects a mesh-map key"))?;
                options.bake_preview_map = Some(subcraft_app::MeshMapKind::ALL.into_iter().find(|k| k.key() == name)
                    .ok_or_else(|| anyhow::anyhow!("unknown bake map {name}"))?);
            }
            "--mesh-map" => {
                let name = args.next().ok_or_else(|| anyhow::anyhow!("--mesh-map expects a mesh-map key"))?;
                options.mesh_map = Some(subcraft_app::MeshMapKind::ALL.into_iter().find(|k| k.key() == name)
                    .ok_or_else(|| anyhow::anyhow!("unknown mesh map {name}"))?);
            }
            "--bake-size" => {
                options.bake_size = Some(args.next().and_then(|s| s.parse::<u32>().ok()).filter(|s| [256, 512, 1024, 2048, 4096].contains(s))
                    .ok_or_else(|| anyhow::anyhow!("--bake-size expects 256, 512, 1024, 2048 or 4096"))?);
            }
            "--environment" => options.environment = args.next(),
            "--screenshot" => options.screenshot = args.next().map(Into::into),
            "--mesh-maps" => options.mesh_map_files.extend(args.next().ok_or_else(|| anyhow::anyhow!("--mesh-maps expects a comma-separated list of images"))?.split(',').map(Into::into)),
            "--bake-misses" => options.bake_misses = true,
            "--bake-cage" => {
                let spec = args.next().ok_or_else(|| anyhow::anyhow!("--bake-cage expects front,rear"))?;
                let v: Vec<f32> = spec.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                if v.len() != 2 { anyhow::bail!("--bake-cage expects front,rear (fractions of the diagonal)"); }
                options.bake_cage = Some([v[0], v[1]]);
            }
            "--curvature-from-normal" => options.curvature_from_normal = true,
            "--id-colors" => options.id_colors = true,
            "--reset-layout" => options.reset_layout = true,
            "--window-size" => {
                options.window_size = Some(args.next().and_then(|s| subcraft_ui::app::parse_size(&s))
                    .ok_or_else(|| anyhow::anyhow!("--window-size expects WIDTHxHEIGHT, e.g. 1280x720"))?);
            }
            "--scale" => {
                options.scale = Some(args.next().and_then(|s| s.parse::<f32>().ok()).filter(|s| (0.5..=4.0).contains(s))
                    .ok_or_else(|| anyhow::anyhow!("--scale expects a number from 0.5 to 4"))?);
            }
            "--recover" => options.recover = true,
            "--reload-mesh" => options.reload_mesh = Some(args.next().ok_or_else(|| anyhow::anyhow!("--reload-mesh expects a model path"))?.into()),
            "--reload-mode" => {
                options.reload_mode = Some(match args.next().as_deref() {
                    Some("keep") => subcraft_app::ReloadMode::KeepUvs,
                    Some("reproject") => subcraft_app::ReloadMode::Reproject,
                    _ => anyhow::bail!("--reload-mode expects keep or reproject"),
                })
            }
            "--auto-uv" => options.auto_uv = true,
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
                println!("usage: subcraft [project.subcraft | mesh file] [--demo] [--view-2d] [--brush-panel] [--bake-high mesh] [--bake-preview] [--bake-preview-map map_key] [--bake-match always|name] [--bake-place auto,center,origin,offset=x:y:z,scale=s,rot=x:y:z] [--environment name] [--bake-size 256|512|1024|2048|4096] [--symmetry xyz] [--radial y8] [--screenshot out.png] [--path-trace [samples]] [--welcome] [--quick-start] [--guide-step 1..6] [--reset-layout] [--window-size WxH] [--camera yaw,pitch,zoom[,x,y,z]] [--scale 1.5] [--recover] [--autosave-interval seconds]");
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
