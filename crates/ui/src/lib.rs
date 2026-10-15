//! SubCraft user interface: egui + egui_dock workspace over the `app` session.
//!
//! Widgets only read session state and dispatch commands/actions/stroke calls; all document
//! logic lives in `subcraft-app`.

pub mod app;
pub mod bug_report;
pub mod dialogs;
pub mod files;
pub mod icons;
pub mod orbit;
pub mod pen;
pub mod panels;
pub mod recovery;
pub mod state;
pub mod theme;
pub mod viewport;
pub mod widgets;
pub mod welcome;

pub mod updates;

pub use app::{LaunchOptions, SubcraftApp, Tab};

/// Open the main window.
#[cfg(not(target_arch = "wasm32"))]
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

/// Run in a web page, in the `<canvas>` with this id. Files the app writes (projects, exports)
/// are downloaded by the browser.
#[cfg(target_arch = "wasm32")]
pub async fn start_web(canvas_id: &str) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    subcraft_core::vfs::set_sink(|path, bytes| web::download(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into()), bytes));
    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(canvas_id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .ok_or_else(|| wasm_bindgen::JsValue::from_str("canvas not found"))?;
    let options = eframe::WebOptions { wgpu_options: wgpu_options(), ..Default::default() };
    eframe::WebRunner::new()
        .start(canvas, options, Box::new(|cc| Ok(Box::new(SubcraftApp::new(cc, LaunchOptions::default())))))
        .await
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::JsCast;

    /// Offer `bytes` as a file download named `name`.
    pub fn download(name: &str, bytes: &[u8]) {
        let result = (|| -> Option<()> {
            let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
            let opts = web_sys::BlobPropertyBag::new();
            opts.set_type("application/octet-stream");
            let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).ok()?;
            let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
            let doc = web_sys::window()?.document()?;
            let a: web_sys::HtmlAnchorElement = doc.create_element("a").ok()?.dyn_into().ok()?;
            a.set_href(&url);
            a.set_download(name);
            a.click();
            // Released a minute later: revoking at once can cancel the download in some browsers.
            let revoke = wasm_bindgen::closure::Closure::once_into_js(move || {
                let _ = web_sys::Url::revoke_object_url(&url);
            });
            web_sys::window()?.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 60_000).ok()?;
            Some(())
        })();
        if result.is_none() {
            log::error!("could not start the download of {name}");
        }
    }
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
pub mod asset_preview;
