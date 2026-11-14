//! SubCraft user interface: egui + egui_dock workspace over the `app` session.
//!
//! Widgets only read session state and dispatch commands/actions/stroke calls; all document
//! logic lives in `subcraft-app`.

pub mod app;
pub mod intro;
pub mod bake_view;
pub mod blender_bridge;
pub mod bug_report;
pub mod clipboard;
pub mod color_picker;
mod brush_bar;
pub mod brush_cursor;
pub mod keys;
pub mod linux_backend;
pub mod shortcuts_window;
pub mod shortcut_editor;
pub mod device_lost;
pub mod dialogs;
pub mod docks;
pub mod export_mesh;
pub mod export_presets;
pub mod layer_export;
pub mod files;
pub mod graphics;
pub mod gpu_probe;
pub mod i18n;
pub mod icons;
pub mod import_resources;
pub mod libraries;
pub mod auto_uv;
pub mod mesh_reload;
pub mod mesh_map_import;
pub mod new_project;
pub mod nav;
pub mod orbit;
pub mod render_export;
pub mod pen;
pub mod part_overlay;
pub mod paths_panel;
pub mod placement;
pub mod pen_input;
pub mod tablet;
pub mod text_panel;
pub mod panels;
pub mod recovery;
pub mod spp_import;
pub mod state;
pub mod symmetry_view;
pub mod layer_previews;
pub mod mask_view;
pub mod edit_profile;
pub mod view_profile;
pub mod theme;
pub mod viewport;
pub mod viewport_hints;
pub mod viewport_bar;
pub mod view_extras;
pub mod view_state;
pub mod color_lut;
pub mod materials_download;
pub mod view_props;
pub mod projection_gizmo;
pub mod warp_view;
pub mod widgets;
pub mod window_frame;
pub mod welcome;

pub mod updates;

pub use app::{LaunchOptions, SubcraftApp, Tab};

/// Graphics APIs to try, in order, after `preferred` failed to start. A `WGPU_BACKEND`
/// override is never second-guessed. OpenGL is the last resort (it also covers drivers where
/// DirectX 12 and Vulkan are missing). When the previous run died during startup with a chosen
/// API, Automatic goes first, so a choice that crashes the app cannot lock the user out (#91).
pub fn startup_attempts(preferred: subcraft_app::graphics::GraphicsApi, environment_override: bool, previous_start_crashed: bool) -> Vec<subcraft_app::graphics::GraphicsApi> {
    use subcraft_app::graphics::GraphicsApi::*;
    if environment_override { return vec![preferred]; }
    let mut order = if previous_start_crashed && preferred != Automatic { vec![Automatic, preferred] } else { vec![preferred] };
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

/// Text of the window shown when the window system could not be reached: no display server, or
/// (Linux) the X11 / Wayland client libraries winit loads at run time are not installed. Never
/// tells the user to update the graphics driver; that is not the problem.
pub fn window_failure_message(error: &str, log: Option<&std::path::Path>) -> String {
    let lower = error.to_ascii_lowercase();
    let mut text = String::from("SubCraft could not open a window. This is not a graphics driver problem.\n\n");
    if lower.contains("shared librar") || lower.contains("libx11") || lower.contains("libwayland") || lower.contains("libxkbcommon") {
        text.push_str("Some system libraries for windows are missing. Install them with your package manager, then start SubCraft again:\n");
        text.push_str("Debian / Ubuntu: sudo apt install libx11-6 libx11-xcb1 libxcursor1 libxrandr2 libxi6 libxkbcommon-x11-0 libwayland-client0\n");
        text.push_str("Fedora: sudo dnf install libX11 libX11-xcb libXcursor libXrandr libXi libxkbcommon-x11 libwayland-client\n");
        text.push_str("Arch: sudo pacman -S libx11 libxcursor libxrandr libxi libxkbcommon-x11 wayland\n");
    } else if lower.contains("display") {
        text.push_str("No display was found (neither DISPLAY nor WAYLAND_DISPLAY is set). Start SubCraft from a desktop session, not from a plain console or SSH without X forwarding.\n");
    } else {
        text.push_str(&format!("The window system reported an error. Please report it at {} and attach the log file.\n", subcraft_app::diagnostics::ISSUES_URL));
    }
    text.push_str(&format!("\nError: {error}\n"));
    match log {
        Some(path) => text.push_str(&format!("Log file: {}", path.display())),
        None => text.push_str("No log file could be written."),
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
    let previous_start_crashed = subcraft_app::startup_probe::previous_run_incomplete(subcraft_app::startup_probe::previous_log());
    if cfg!(target_os = "linux") {
        let decision = linux_backend::current(options.force_x11);
        let text = linux_backend::describe(decision);
        subcraft_app::startup_probe::step(&text);
        if decision == linux_backend::Decision::NoXServer { log::warn!("{text}"); } else { log::info!("{text}"); }
    }
    for api in startup_attempts(preferred, environment_override, previous_start_crashed) {
        let mut graphics = graphics.clone();
        let mut options = options.clone();
        if api != preferred {
            let why = if previous_start_crashed { "stopped SubCraft during the last start" } else { "could not start" };
            graphics.warning = Some(format!("{} {why}. {} is being used for this run. Choose another API here if needed.", preferred.label(), api.label()));
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
                // The window system itself failed (no display, missing X11 / Wayland libraries).
                // That is not a graphics-API problem, and winit allows one event loop per
                // process, so every further attempt would only fail with "EventLoop can't be
                // recreated" and hide the real cause.
                if matches!(error, eframe::Error::WinitEventLoop(_)) {
                    let text = window_failure_message(&error.to_string(), subcraft_app::diagnostics::log_file().as_deref());
                    subcraft_app::startup_probe::step(&format!("startup failed: {}", text.replace('\n', " | ")));
                    log::error!("{text}");
                    show_fatal("SubCraft cannot open a window", &text);
                    return Err(error);
                }
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
    let mut wgpu_options = graphics::wgpu_options(api, graphics.environment_override, graphics.gpu.clone());
    if options.profile_view.is_some() {
        // Frame intervals must be costs, not the display's refresh.
        wgpu_options.surface.present_mode = egui_wgpu::wgpu::PresentMode::AutoNoVsync;
    }
    options.graphics_startup = Some(graphics);
    let mut native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SubCraft")
            .with_icon(std::sync::Arc::new(theme::app_icon()))
            .with_inner_size(options.window_size.unwrap_or([1600.0, 940.0]))
            .with_min_inner_size([640.0, 420.0])
            // SubCraft draws its own title bar (window_frame.rs), as Painter does.
            .with_decorations(!window_frame::frameless())
            .with_resizable(true),
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
    // Pen tablets on Wayland (issue #82): run under XWayland when asked and an X server exists.
    #[cfg(target_os = "linux")]
    if linux_backend::current(options.force_x11) == linux_backend::Decision::ForceX11 {
        native.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }));
    }
    #[cfg(not(target_os = "linux"))]
    { let _ = &mut native; }
    eframe::run_native("SubCraft", native, Box::new(move |cc| Ok(Box::new(SubcraftApp::new(cc, options)))))
}

pub mod asset_preview;

#[cfg(test)]
mod startup_tests {
    use super::*;
    use subcraft_app::graphics::GraphicsApi::*;
    #[test]
    fn every_api_is_tried_once_and_environment_override_is_final() {
        assert_eq!(startup_attempts(Automatic, false, false), vec![Automatic, OpenGl]);
        assert_eq!(startup_attempts(DirectX12, false, false), vec![DirectX12, Automatic, OpenGl]);
        assert_eq!(startup_attempts(OpenGl, false, false), vec![OpenGl, Automatic]);
        assert_eq!(startup_attempts(DirectX12, true, false), vec![DirectX12]);
    }
    #[test]
    fn a_choice_that_crashed_the_last_start_goes_after_automatic() {
        assert_eq!(startup_attempts(OpenGl, false, true), vec![Automatic, OpenGl]);
        assert_eq!(startup_attempts(DirectX12, false, true), vec![Automatic, DirectX12, OpenGl]);
        assert_eq!(startup_attempts(Automatic, false, true), vec![Automatic, OpenGl], "nothing to fall back from");
        assert_eq!(startup_attempts(OpenGl, true, true), vec![OpenGl], "WGPU_BACKEND is still final");
    }
    #[test]
    fn missing_window_libraries_name_packages_not_drivers() {
        // The exact text winit 0.30 produced in a minimal Ubuntu 20.04 container (CI, PR #125).
        let error = "winit EventLoopError: os error at winit-0.30.13/src/platform_impl/linux/mod.rs:788: Failed to load one of xlib's shared libraries";
        let text = window_failure_message(error, Some(std::path::Path::new("/tmp/subcraft.log")));
        assert!(text.contains("libx11-6") && text.contains("libX11") && text.contains("libxkbcommon-x11"), "{text}");
        assert!(text.contains("not a graphics driver problem") && !text.contains("update your graphics driver"), "{text}");
        assert!(text.contains(error) && text.contains("/tmp/subcraft.log"));
    }
    #[test]
    fn no_display_and_unknown_window_errors() {
        let text = window_failure_message("neither WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set.", None);
        assert!(text.contains("No display was found") && !text.contains("apt install") && text.contains("No log file"));
        assert!(window_failure_message("something else", None).contains("issues"));
    }
    #[test]
    fn failure_message_names_errors_and_log() {
        let text = startup_failure_message(&[("Automatic".into(), "no adapter".into())], Some(std::path::Path::new("C:/x/subcraft.log")));
        assert!(text.contains("no adapter") && text.contains("subcraft.log") && text.contains("issues"));
        assert!(startup_failure_message(&[], None).contains("No log file"));
    }
}
