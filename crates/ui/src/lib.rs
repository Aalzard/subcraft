//! SubCraft user interface: egui + egui_dock workspace over the `app` session.
//!
//! Widgets only read session state and dispatch commands/actions/stroke calls; all document
//! logic lives in `subcraft-app`.

pub mod app;
pub mod bug_report;
mod brush_bar;
pub mod dialogs;
pub mod spp_dialogs;
pub mod files;
pub mod graphics;
pub mod icons;
pub mod new_project;
pub mod orbit;
pub mod render_export;
pub mod pen;
pub mod panels;
pub mod recovery;
pub mod state;
pub mod layer_previews;
pub mod edit_profile;
pub mod theme;
pub mod viewport;
pub mod viewport_bar;
pub mod widgets;
pub mod welcome;

pub mod updates;

pub use app::{LaunchOptions, SubcraftApp, Tab};

/// Open the main window.
pub fn run(options: LaunchOptions) -> eframe::Result {
    let graphics = graphics::GraphicsUi::load();
    let api = graphics.selected;
    let environment_override = graphics.environment_override;
    let result = run_with_graphics(options.clone(), graphics, api);
    if let Err(error) = result {
        if api == subcraft_app::graphics::GraphicsApi::Automatic || environment_override { return Err(error); }
        log::warn!("{} startup failed: {error}; retrying Automatic", api.label());
        let mut graphics = graphics::GraphicsUi::load();
        graphics.warning = Some(format!("{} could not start. Automatic is being used for this run. Choose another API here if needed.", api.label()));
        let mut options = options; options.preferences = true;
        return run_with_graphics(options, graphics, subcraft_app::graphics::GraphicsApi::Automatic);
    }
    Ok(())
}

fn run_with_graphics(mut options: LaunchOptions, graphics: graphics::GraphicsUi, api: subcraft_app::graphics::GraphicsApi) -> eframe::Result {
    let wgpu_options = graphics::wgpu_options(api, graphics.environment_override);
    options.graphics_startup = Some(graphics);
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SubCraft")
            .with_icon(std::sync::Arc::new(theme::app_icon()))
            .with_inner_size([1600.0, 940.0])
            .with_min_inner_size([900.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        // Scripted screenshots start from a clean UI state (no remembered scroll positions or
        // window size) and never touch the user's saved settings.
        persistence_path: options
            .screenshot
            .as_ref()
            .map(|_| std::env::temp_dir().join(format!("subcraft-screenshot-state-{}", std::process::id()))),
        ..Default::default()
    };
    eframe::run_native("SubCraft", native, Box::new(move |cc| Ok(Box::new(SubcraftApp::new(cc, options)))))
}

pub mod asset_preview;
