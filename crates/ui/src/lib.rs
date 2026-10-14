//! SubCraft user interface: egui + egui_dock workspace over the `app` session.
//!
//! Widgets only read session state and dispatch commands/actions/stroke calls; all document
//! logic lives in `subcraft-app`.

pub mod app;
pub mod dialogs;
pub mod icons;
pub mod orbit;
pub mod panels;
pub mod state;
pub mod theme;
pub mod viewport;
pub mod widgets;

pub use app::{LaunchOptions, SubcraftApp, Tab};

/// Open the main window.
pub fn run(options: LaunchOptions) -> eframe::Result {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SubCraft")
            .with_icon(std::sync::Arc::new(theme::app_icon()))
            .with_inner_size([1600.0, 940.0])
            .with_min_inner_size([900.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
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
