//! Graphics startup policy and Preferences controls; no graphics types enter app.
use egui_wgpu::wgpu;
use subcraft_app::graphics::{GraphicsApi, GraphicsSettings};

#[derive(Clone, Debug, Default)]
pub struct GraphicsUi {
    pub selected: GraphicsApi,
    pub initial: GraphicsApi,
    pub active: String,
    pub warning: Option<String>,
    pub error: Option<String>,
    pub path: Option<std::path::PathBuf>,
    pub environment_override: bool,
}
impl GraphicsUi {
    pub fn load() -> Self {
        let path = GraphicsSettings::path();
        let result = path.as_ref().map(|p| GraphicsSettings::load(p)).unwrap_or_else(|| Ok(Default::default()));
        let (selected, warning) = match result { Ok(p) => (p.api, None), Err(e) => (GraphicsApi::Automatic, Some(e)) };
        Self { selected, initial: selected, path, warning, environment_override: std::env::var_os("WGPU_BACKEND").is_some(), ..Default::default() }
    }
    pub fn select(&mut self, api: GraphicsApi) {
        let result = self.path.as_ref().ok_or_else(|| "No settings folder is available".to_string()).and_then(|p| GraphicsSettings::save(api, p));
        match result { Ok(()) => { self.selected = api; self.error = None; }, Err(e) => self.error = Some(e) }
    }
    pub fn restart_required(&self) -> bool { self.selected != self.initial }
    pub fn record_active(&mut self, backend: wgpu::Backend) {
        self.active = match backend { wgpu::Backend::Dx12 => "DirectX 12", wgpu::Backend::Gl => "OpenGL", wgpu::Backend::Vulkan => "Vulkan", wgpu::Backend::Metal => "Metal", wgpu::Backend::BrowserWebGpu => "WebGPU", _ => "Unknown" }.into();
    }
}

pub fn controls(graphics: &mut GraphicsUi, ui: &mut egui::Ui) {
    use egui::RichText;
    ui.label(RichText::new("Graphics").font(crate::theme::font_strong(12.5)));
    if !graphics.active.is_empty() { ui.label(format!("Active API: {}", graphics.active)); }
    {
        let mut choice = graphics.selected;
        egui::ComboBox::from_id_salt("graphics-api").selected_text(choice.label()).show_ui(ui, |ui| {
            for api in [GraphicsApi::Automatic, GraphicsApi::DirectX12, GraphicsApi::OpenGl] {
                let available = api == GraphicsApi::Automatic || (api == GraphicsApi::DirectX12 && cfg!(windows))
                    || (api == GraphicsApi::OpenGl && cfg!(any(windows, target_os = "linux")));
                if available { ui.selectable_value(&mut choice, api, api.label()); }
            }
        });
        if choice != graphics.selected { graphics.select(choice); }
        ui.label(RichText::new(if graphics.restart_required() { "Restart required. Save your project, then restart SubCraft to apply." }
            else { "Changes apply the next time SubCraft starts." }).color(crate::theme::TEXT_DIM));
        if graphics.environment_override { ui.label("WGPU_BACKEND overrides this preference for this run."); }
    }
    for message in [graphics.warning.as_ref(), graphics.error.as_ref()].into_iter().flatten() { ui.label(RichText::new(message).color(crate::theme::ACCENT)); }
    ui.add_space(12.0);
}

fn requested_backends(api: GraphicsApi, environment_override: bool) -> Option<wgpu::Backends> {
    if environment_override { return None; }
    match api { GraphicsApi::Automatic => None, GraphicsApi::DirectX12 => Some(wgpu::Backends::DX12), GraphicsApi::OpenGl => Some(wgpu::Backends::GL) }
}

pub fn wgpu_options(api: GraphicsApi, environment_override: bool) -> egui_wgpu::WgpuConfiguration {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    if let Some(backends) = requested_backends(api, environment_override) { setup.instance_descriptor.backends = backends; }
    else if cfg!(windows) && !environment_override {
        // Avoid loading the Vulkan driver when DX12 is available (older Intel driver crashes).
        let desc = wgpu::InstanceDescriptor { backends: wgpu::Backends::DX12, ..wgpu::InstanceDescriptor::new_without_display_handle() };
        let dx12 = pollster::block_on(wgpu::Instance::new(desc).enumerate_adapters(wgpu::Backends::DX12));
        setup.instance_descriptor.backends = if dx12.is_empty() { wgpu::Backends::VULKAN | wgpu::Backends::GL } else { wgpu::Backends::DX12 };
    }
    egui_wgpu::WgpuConfiguration { wgpu_setup: setup.into(), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_api_is_exclusive_and_environment_override_is_respected() {
        assert_eq!(requested_backends(GraphicsApi::DirectX12, false), Some(wgpu::Backends::DX12));
        assert_eq!(requested_backends(GraphicsApi::OpenGl, false), Some(wgpu::Backends::GL));
        for api in [GraphicsApi::Automatic, GraphicsApi::DirectX12, GraphicsApi::OpenGl] { assert_eq!(requested_backends(api, true), None); }
    }
}
