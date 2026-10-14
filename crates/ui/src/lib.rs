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
        wgpu_options: wgpu_options(),
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

/// Graphics setup. On Windows SubCraft runs on Direct3D 12: some GPU drivers (older Intel ones in
/// particular) crash inside their Vulkan driver at startup, and with DX12 the Vulkan driver is
/// never loaded. Vulkan is only used when the PC has no DX12 adapter. Setting `WGPU_BACKEND`
/// (`dx12`, `vulkan`, `gl`) overrides the choice.
fn wgpu_options() -> egui_wgpu::WgpuConfiguration {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    if let Some(backends) = preferred_backends() {
        setup.instance_descriptor.backends = backends;
    }
    egui_wgpu::WgpuConfiguration { wgpu_setup: setup.into(), ..Default::default() }
}

fn preferred_backends() -> Option<egui_wgpu::wgpu::Backends> {
    use egui_wgpu::wgpu;
    if !cfg!(windows) || std::env::var_os("WGPU_BACKEND").is_some() {
        return None;
    }
    let desc = wgpu::InstanceDescriptor { backends: wgpu::Backends::DX12, ..wgpu::InstanceDescriptor::new_without_display_handle() };
    let dx12 = pollster::block_on(wgpu::Instance::new(desc).enumerate_adapters(wgpu::Backends::DX12));
    if dx12.is_empty() {
        log::warn!("no Direct3D 12 adapter found; using Vulkan");
        Some(wgpu::Backends::VULKAN | wgpu::Backends::GL)
    } else {
        log::info!("graphics: Direct3D 12 ({})", dx12[0].get_info().name);
        Some(wgpu::Backends::DX12)
    }
}
