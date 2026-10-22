//! SubCraft application (desktop).
//!
//! Desktop usage: `subcraft [project.subcraft|mesh.obj|.gltf|.glb|.fbx] [--demo] [--view-2d] [--screenshot out.png]`

// Packaged Windows builds open without a console window (logs then go nowhere; run a normal
// build from a terminal to see them).
#![cfg_attr(all(windows, feature = "windows-gui"), windows_subsystem = "windows")]

/// mimalloc instead of the system heap: compositing allocates per-tile buffers on every core,
/// which the Windows heap serializes (ADR-0018).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

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
            "--shelf-panel" => options.shelf_panel = true,
            "--generator-panel" => options.generator_panel = true,
            "--effect-panel" => options.effect_panel = true,
            "--select-brush" => options.select_brush = Some(args.next().ok_or_else(|| anyhow::anyhow!("--select-brush expects a Shelf brush name"))?),
            "--capture-tool" => options.capture_tool = Some(args.next().ok_or_else(|| anyhow::anyhow!("--capture-tool expects clone or smudge"))?),
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
            "--quality" => options.quality = Some(args.next().ok_or_else(|| anyhow::anyhow!("--quality expects tone=..,bloom=..,ao=.."))?),
            "--export-window" => options.export_window = true,
            "--new-project" => options.new_project_model = args.next().map(Into::into),
            "--new-project-high" => options.new_project_high = args.next().map(Into::into),
            "--properties-panel" => options.properties_panel = true,
            "--export-template" => options.export_template = args.next().and_then(|s| s.parse().ok()),
            "--render-window" => options.render_window = true,
            "--symmetry" => {
                let axes = args.next().ok_or_else(|| anyhow::anyhow!("--symmetry expects x, y, z or a combination"))?.to_ascii_lowercase();
                if axes.is_empty() || !axes.chars().all(|c| "xyz".contains(c)) { anyhow::bail!("--symmetry expects x, y, z or a combination"); }
                options.symmetry_axes = Some(['x', 'y', 'z'].map(|axis| axes.contains(axis)));
            }
            "--bake-high" => options.bake_high_poly = Some(args.next().ok_or_else(|| anyhow::anyhow!("--bake-high expects a high-poly mesh path"))?.into()),
            "--bake-preview" => options.bake_preview_only = true,
            "--bake-preview-map" => {
                let name = args.next().ok_or_else(|| anyhow::anyhow!("--bake-preview-map expects a mesh-map key"))?;
                options.bake_preview_map = Some(subcraft_app::MeshMapKind::ALL.into_iter().find(|k| k.key() == name)
                    .ok_or_else(|| anyhow::anyhow!("unknown bake map {name}"))?);
            }
            "--bake-size" => {
                options.bake_size = Some(args.next().and_then(|s| s.parse::<u32>().ok()).filter(|s| [256, 512, 1024, 2048, 4096].contains(s))
                    .ok_or_else(|| anyhow::anyhow!("--bake-size expects 256, 512, 1024, 2048 or 4096"))?);
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
                println!("usage: subcraft [project.subcraft | mesh file] [--demo] [--view-2d] [--brush-panel] [--bake-high mesh] [--bake-preview] [--bake-preview-map map_key] [--bake-size 256|512|1024|2048|4096] [--screenshot out.png] [--welcome] [--quick-start] [--guide-step 1..6] [--recover] [--autosave-interval seconds]");
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
