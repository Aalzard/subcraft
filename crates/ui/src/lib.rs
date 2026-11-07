//! SubCraft user interface: egui + egui_dock workspace over the `app` session.
//!
//! Widgets only read session state and dispatch commands/actions/stroke calls; all document
//! logic lives in `subcraft-app`.

pub mod app;
pub mod bug_report;
pub mod clipboard;
mod brush_bar;
pub mod keys;
pub mod shortcuts_window;
pub mod dialogs;
pub mod files;
pub mod graphics;
pub mod gpu_probe;
pub mod i18n;
pub mod icons;
pub mod auto_uv;
pub mod mesh_reload;
pub mod new_project;
pub mod nav;
pub mod orbit;
pub mod render_export;
pub mod pen;
pub mod pen_input;
pub mod text_panel;
pub mod panels;
pub mod recovery;
pub mod spp_import;
pub mod state;
pub mod layer_previews;
pub mod edit_profile;
pub mod theme;
pub mod viewport;
pub mod viewport_hints;
pub mod viewport_bar;
pub mod view_extras;
pub mod widgets;
pub mod welcome;

pub mod updates;

pub use app::{LaunchOptions, SubcraftApp, Tab};

/// Graphics APIs to try, in order, after `preferred` failed to start. A `WGPU_BACKEND`
/// override is never second-guessed. OpenGL is the last resort (it also covers drivers where
/// DirectX 12 and Vulkan are missing).
pub fn startup_attempts(preferred: subcraft_app::graphics::GraphicsApi, environment_override: bool) -> Vec<subcraft_app::graphics::GraphicsApi> {
    use subcraft_app::graphics::GraphicsApi::*;
    if environment_override { return vec![preferred]; }
    let mut order = vec![preferred];
    for api in [Automatic, OpenGl] {
        if !order.contains(&api) { order.push(api); }
    }
    order
}

/// Text of the window shown when no graphics API could start.
pub fn startup_failure_message(errors: &[(String, String)], log: Option<&std::path::Path>) -> String {
    let mut text = format!("SubCraft could not start its graphics.

Nothing is wrong with your files. Please update your graphics driver, then try again. If it keeps happening, report it at {} and attach the log file.

", subcraft_app::diagnostics::ISSUES_URL);
    for (api, error) in errors { text.push_str(&format!("{api}: {error}
")); }
    match log {
        Some(path) => text.push_str(&format!("
Log file: {}", path.display())),
        None => text.push_str("
No log file could be written."),
    }
    text
}

/// Let the baker cast AO / thickness rays on the GPU (its own headless device, created on the
/// first bake). Called once at start-up.
pub fn register_gpu_baking() {
    subcraft_mesh::ray_batch::register_ray_backend(subcraft_render::ray_bake::create_backend);
    subcraft_compositor::gpu::register_gpu_compositor(subcraft_render::gpu_composite::create_backend);
}

/// Native error dialog for failures before or outside the main window (a packaged Windows
/// build has no console, so otherwise the user sees nothing).
static DIALOG_SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn show_fatal(title: &str, text: &str) {
    if DIALOG_SHOWN.swap(true, std::sync::atomic::Ordering::Relaxed) { return; } // one dialog is enough
    subcraft_app::startup_probe::step(&format!("fatal: {title}: {}", text.replace('\n', " | ")));
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title(title).set_description(text).set_buttons(rfd::MessageButtons::Ok).show();
}

/// Open the main window. If the chosen graphics API cannot start, the others are tried in turn;
/// if none works, a message window explains what to do (a packaged build has no console, so
/// without it the app would simply vanish).
pub fn run(options: LaunchOptions) -> eframe::Result {
    let safe = subcraft_app::safe_mode::is_active();
    subcraft_render::caps::set_safe_mode(safe);
    if !safe { register_gpu_baking(); }
    let graphics = graphics::GraphicsUi::load();
    let preferred = graphics.selected;
    let environment_override = graphics.environment_override;
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut last = None;
    for (n, api) in startup_attempts(preferred, environment_override).into_iter().enumerate() {
        let mut graphics = graphics.clone();
        let mut options = options.clone();
        if n > 0 {
            graphics.warning = Some(format!("{} could not start. {} is being used for this run. Choose another API here if needed.", preferred.label(), api.label()));
            options.preferences = true;
        }
        match gpu_probe::prepare(api, environment_override, subcraft_app::startup_probe::previous_log()) {
            subcraft_app::startup_probe::ProbeResult::AllFailed { report } => {
                for line in report { errors.push((api.label().to_string(), line)); }
                if api == subcraft_app::graphics::GraphicsApi::Automatic { break; } // already covered every backend
                continue;
            }
            subcraft_app::startup_probe::ProbeResult::Verified { adapter, .. } => subcraft_app::startup_probe::step(&format!("opening window with {} / {}", adapter.backend.label(), adapter.name)),
            subcraft_app::startup_probe::ProbeResult::NotUsed => subcraft_app::startup_probe::step(&format!("opening window with {} (no probe)", api.label())),
        }
        match run_with_graphics(options, graphics, api) {
            Ok(()) => return Ok(()),
            Err(error) => {
                log::warn!("{} startup failed: {error}", api.label());
                errors.push((api.label().to_string(), error.to_string()));
                last = Some(error);
            }
        }
    }
    let log = subcraft_app::diagnostics::log_file();
    let probe_failed = last.is_none();
    let text = if probe_failed {
        let lines: Vec<String> = errors.iter().map(|(api, e)| format!("{api}: {e}")).collect();
        subcraft_app::startup_probe::failure_text(&lines, subcraft_app::startup_probe::startup_log_path().as_deref())
    } else { startup_failure_message(&errors, log.as_deref()) };
    subcraft_app::startup_probe::step(&format!("startup failed: {}", text.replace('\n', " | ")));
    log::error!("{text}");
    show_fatal("SubCraft cannot start", &text);
    Err(last.unwrap_or_else(|| eframe::Error::AppCreation("no graphics adapter passed the startup probe".into())))
}

fn run_with_graphics(mut options: LaunchOptions, graphics: graphics::GraphicsUi, api: subcraft_app::graphics::GraphicsApi) -> eframe::Result {
    let wgpu_options = graphics::wgpu_options(api, graphics.environment_override, graphics.gpu.clone());
    options.graphics_startup = Some(graphics);
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SubCraft")
            .with_icon(std::sync::Arc::new(theme::app_icon()))
            .with_inner_size(options.window_size.unwrap_or([1600.0, 940.0]))
            .with_min_inner_size([640.0, 420.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        // Scripted screenshots start from a clean UI state (no remembered scroll positions or
        // window size) and never touch the user's saved settings.
        // --reset-layout also ignores the saved window position and size.
        persist_window: !options.reset_layout && !subcraft_app::safe_mode::is_active(),
        persistence_path: options
            .screenshot
            .as_ref()
            .map(|_| std::env::temp_dir().join(format!("subcraft-screenshot-state-{}", std::process::id()))),
        ..Default::default()
    };
    eframe::run_native("SubCraft", native, Box::new(move |cc| Ok(Box::new(SubcraftApp::new(cc, options)))))
}

pub mod asset_preview;

#[cfg(test)]
mod startup_tests {
    use super::*;
    use subcraft_app::graphics::GraphicsApi::*;
    #[test]
    fn every_api_is_tried_once_and_environment_override_is_final() {
        assert_eq!(startup_attempts(Automatic, false), vec![Automatic, OpenGl]);
        assert_eq!(startup_attempts(DirectX12, false), vec![DirectX12, Automatic, OpenGl]);
        assert_eq!(startup_attempts(OpenGl, false), vec![OpenGl, Automatic]);
        assert_eq!(startup_attempts(DirectX12, true), vec![DirectX12]);
    }
    #[test]
    fn failure_message_names_errors_and_log() {
        let text = startup_failure_message(&[("Automatic".into(), "no adapter".into())], Some(std::path::Path::new("C:/x/subcraft.log")));
        assert!(text.contains("no adapter") && text.contains("subcraft.log") && text.contains("issues"));
        assert!(startup_failure_message(&[], None).contains("No log file"));
    }
}
