//! Graphics startup policy and Preferences controls; no graphics types enter app.
use egui_wgpu::wgpu;
use subcraft_app::graphics::{self as policy, AdapterClass, AdapterDesc, GpuChoice, GraphicsApi, GraphicsSettings};
use egui::RichText;
use std::sync::Mutex;

/// Adapter names seen at startup (for the Preferences list) and the one in use.
static ADAPTERS: Mutex<Vec<String>> = Mutex::new(Vec::new());
pub fn known_adapters() -> Vec<String> { ADAPTERS.lock().map(|a| a.clone()).unwrap_or_default() }

fn describe(info: &wgpu::AdapterInfo) -> AdapterDesc {
    let class = match info.device_type {
        wgpu::DeviceType::DiscreteGpu => AdapterClass::Discrete, wgpu::DeviceType::IntegratedGpu => AdapterClass::Integrated,
        wgpu::DeviceType::VirtualGpu => AdapterClass::Virtual, wgpu::DeviceType::Cpu => AdapterClass::Software, _ => AdapterClass::Other,
    };
    AdapterDesc { name: info.name.clone(), class }
}

#[derive(Clone, Debug, Default)]
pub struct GraphicsUi {
    pub selected: GraphicsApi,
    pub initial: GraphicsApi,
    pub gpu: GpuChoice,
    pub initial_gpu: GpuChoice,
    /// `SUBCRAFT_GPU` text, when set (overrides the saved GPU for this run).
    pub gpu_override: Option<String>,
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
        let (selected, gpu, warning) = match result { Ok(p) => (p.api, p.gpu, None), Err(e) => (GraphicsApi::Automatic, GpuChoice::Automatic, Some(e)) };
        Self { selected, initial: selected, gpu: gpu.clone(), initial_gpu: gpu, gpu_override: policy::gpu_env_override(), path, warning, environment_override: std::env::var_os("WGPU_BACKEND").is_some(), ..Default::default() }
    }
    pub fn select(&mut self, api: GraphicsApi) { let gpu = self.gpu.clone(); self.store(api, gpu); }
    pub fn select_gpu(&mut self, gpu: GpuChoice) { self.store(self.selected, gpu); }
    fn store(&mut self, api: GraphicsApi, gpu: GpuChoice) {
        let result = self.path.as_ref().ok_or_else(|| "No settings folder is available".to_string()).and_then(|p| GraphicsSettings::save(api, &gpu, p));
        match result { Ok(()) => { self.selected = api; self.gpu = gpu; self.error = None; }, Err(e) => self.error = Some(e) }
    }
    pub fn restart_required(&self) -> bool { self.selected != self.initial || self.gpu != self.initial_gpu }
    pub fn record_active(&mut self, backend: wgpu::Backend) {
        self.active = match backend { wgpu::Backend::Dx12 => "DirectX 12", wgpu::Backend::Gl => "OpenGL", wgpu::Backend::Vulkan => "Vulkan", wgpu::Backend::Metal => "Metal", wgpu::Backend::BrowserWebGpu => "WebGPU", _ => "Unknown" }.into();
    }
}

pub fn controls(graphics: &mut GraphicsUi, ui: &mut egui::Ui) {
    ui.label(RichText::new("Graphics").font(crate::theme::font_strong(12.5)));
    if !graphics.active.is_empty() { ui.label(format!("Active API: {}", graphics.active)); }
    gpu_control(graphics, ui);
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
    for message in [graphics.warning.as_ref(), graphics.error.as_ref()].into_iter().flatten() { ui.add(egui::Label::new(RichText::new(message).color(crate::theme::ACCENT)).wrap()); }
    ui.add_space(12.0);
}

/// Combo wording. "Automatic" alone would read the same as the API combo's entry.
fn gpu_label(choice: &GpuChoice) -> String {
    match choice { GpuChoice::Automatic => "Automatic (fastest GPU)".into(), other => other.label() }
}

fn gpu_control(graphics: &mut GraphicsUi, ui: &mut egui::Ui) {
    let mut choice = graphics.gpu.clone();
    ui.horizontal(|ui| {
        ui.label("GPU");
        egui::ComboBox::from_id_salt("graphics-gpu").selected_text(gpu_label(&choice)).show_ui(ui, |ui| {
            ui.selectable_value(&mut choice, GpuChoice::Automatic, gpu_label(&GpuChoice::Automatic));
            ui.selectable_value(&mut choice, GpuChoice::HighPerformance, gpu_label(&GpuChoice::HighPerformance));
            let mut names = known_adapters();
            if let GpuChoice::Named(n) = &graphics.gpu && !names.contains(n) { names.push(n.clone()); }
            for name in names { ui.selectable_value(&mut choice, GpuChoice::Named(name.clone()), name); }
        });
    });
    if choice != graphics.gpu { graphics.select_gpu(choice); }
    ui.label(RichText::new("Automatic and High performance both try the fastest graphics card first and fall back to the next one if it fails.").color(crate::theme::TEXT_DIM));
    if let Some(text) = &graphics.gpu_override { ui.label(format!("SUBCRAFT_GPU=\"{text}\" overrides this preference for this run.")); }
}

fn requested_backends(api: GraphicsApi, environment_override: bool) -> Option<wgpu::Backends> {
    if environment_override { return None; }
    match api { GraphicsApi::Automatic => None, GraphicsApi::DirectX12 => Some(wgpu::Backends::DX12), GraphicsApi::OpenGl => Some(wgpu::Backends::GL) }
}

pub fn wgpu_options(api: GraphicsApi, environment_override: bool, gpu: GpuChoice) -> egui_wgpu::WgpuConfiguration {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    if crate::gpu_probe::apply(&mut setup) {} // verified in a child process (issue #83)
    else if let Some(backends) = requested_backends(api, environment_override) { setup.instance_descriptor.backends = backends; }
    else if cfg!(windows) && !environment_override {
        // Avoid loading the Vulkan driver when DX12 is available (older Intel driver crashes).
        let desc = wgpu::InstanceDescriptor { backends: wgpu::Backends::DX12, ..wgpu::InstanceDescriptor::new_without_display_handle() };
        let dx12 = pollster::block_on(wgpu::Instance::new(desc).enumerate_adapters(wgpu::Backends::DX12));
        setup.instance_descriptor.backends = if dx12.is_empty() { wgpu::Backends::VULKAN | wgpu::Backends::GL } else { wgpu::Backends::DX12 };
    }
    setup.power_preference = wgpu::PowerPreference::HighPerformance;
    let env = policy::gpu_env_override();
    setup.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, surface| select_adapter(adapters, surface, &gpu, env.as_deref())));
    let egui_default = setup.device_descriptor.clone();
    setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut desc = egui_default(adapter);
        desc.required_limits.max_texture_dimension_2d = adapter.limits().max_texture_dimension_2d;
        desc
    });
    egui_wgpu::WgpuConfiguration { wgpu_setup: setup.into(), ..Default::default() }
}

/// Order the adapters, log them, and take the first that supports the surface and can create a
/// device. A card whose driver is broken (the Optimus laptop case) is skipped, not fatal.
fn select_adapter(adapters: &[wgpu::Adapter], surface: Option<&wgpu::Surface<'_>>, gpu: &GpuChoice, env: Option<&str>) -> Result<wgpu::Adapter, String> {
    let infos: Vec<wgpu::AdapterInfo> = adapters.iter().map(|a| a.get_info()).collect();
    let descs: Vec<AdapterDesc> = infos.iter().map(describe).collect();
    if let Ok(mut known) = ADAPTERS.lock() {
        known.clear();
        for d in &descs { if !known.contains(&d.name) { known.push(d.name.clone()); } }
    }
    for (i, info) in infos.iter().enumerate() {
        log::info!("GPU adapter {i}: {} ({:?}, {:?}, driver {} {})", info.name, info.device_type, info.backend, info.driver, info.driver_info);
    }
    let order = policy::order_adapters(&descs, gpu, env);
    log::info!("GPU preference: {} (SUBCRAFT_GPU: {}); trying adapters {:?}", gpu.label(), env.unwrap_or("unset"), order);
    let result = policy::first_working(&order, |i| {
        let adapter = &adapters[i];
        if let Some(surface) = surface && !adapter.is_surface_supported(surface) { return Err("cannot present to the window".to_string()); }
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..Default::default() }))
            .map(drop).map_err(|e| format!("device creation failed: {e}"))
    });
    match result {
        Ok((i, skipped)) => {
            for (s, why) in skipped { log::warn!("GPU adapter skipped: {} ({why})", infos[s].name); }
            log::info!("GPU adapter chosen: {} ({:?})", infos[i].name, infos[i].backend);
            Ok(adapters[i].clone())
        }
        Err(all) => {
            let text = all.iter().map(|(i, why)| format!("{}: {why}", infos[*i].name)).collect::<Vec<_>>().join("; ");
            log::error!("No usable GPU adapter: {text}");
            Err(if text.is_empty() { "no graphics adapter found".into() } else { format!("no usable graphics adapter ({text})") })
        }
    }
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
