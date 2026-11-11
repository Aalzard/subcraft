//! Optional GPU capabilities, probed once from the adapter at start-up so features can degrade
//! with a reason instead of failing a pipeline / texture creation (which would be a validation
//! error on the device). Headless tools and tests never set it, which means "assume everything".
use egui_wgpu::wgpu;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub struct GpuCaps {
    pub name: String,
    pub backend: String,
    pub driver: String,
    pub device_type: String,
    pub max_texture_2d: u32,
    pub max_buffer_size: u64,
    /// 4x MSAA works for the viewport colour (Rgba8UnormSrgb) and depth (Depth32Float) formats.
    pub msaa4: bool,
    pub compute: bool,
    pub storage_textures_per_stage: u32,
    pub max_storage_buffer_binding: u64,
    pub rgba16f_filterable: bool,
    pub rgba16f_renderable: bool,
}

impl Default for GpuCaps {
    /// Everything available: used when no adapter was probed (tests, headless tools).
    fn default() -> Self {
        Self { name: "unprobed".into(), backend: String::new(), driver: String::new(), device_type: String::new(), max_texture_2d: 16384, max_buffer_size: u64::MAX,
            msaa4: true, compute: true, storage_textures_per_stage: 8, max_storage_buffer_binding: u64::MAX, rgba16f_filterable: true, rgba16f_renderable: true }
    }
}

impl GpuCaps {
    pub fn from_adapter(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        let limits = adapter.limits();
        let sample4 = |f: wgpu::TextureFormat| adapter.get_texture_format_features(f).flags.sample_count_supported(4);
        let f16 = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float);
        Self {
            name: info.name.clone(), backend: format!("{:?}", info.backend), driver: format!("{} {}", info.driver, info.driver_info).trim().to_string(),
            device_type: format!("{:?}", info.device_type),
            max_texture_2d: limits.max_texture_dimension_2d, max_buffer_size: limits.max_buffer_size,
            msaa4: sample4(crate::target::OUTPUT_FORMAT) && sample4(crate::target::DEPTH_FORMAT),
            compute: adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS),
            storage_textures_per_stage: limits.max_storage_textures_per_shader_stage,
            max_storage_buffer_binding: limits.max_storage_buffer_binding_size,
            rgba16f_filterable: f16.flags.contains(wgpu::TextureFormatFeatureFlags::FILTERABLE),
            rgba16f_renderable: f16.allowed_usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT),
        }
    }

    /// Why 4x anti-aliasing is off, if it is.
    pub fn msaa_problem(&self) -> Option<String> {
        (!self.msaa4).then(|| format!("{} does not support 4x multisampling on the viewport formats; anti-aliasing is disabled.", self.name))
    }

    /// Why GPU path tracing / GPU baking cannot run, if they cannot.
    pub fn compute_problem(&self) -> Option<String> {
        if !self.compute { return Some(format!("{} ({}) has no compute shader support; GPU path tracing, baking and compositing fall back to the CPU or are unavailable.", self.name, self.backend)); }
        if self.storage_textures_per_stage < 3 { return Some(format!("{} allows only {} storage textures per shader stage (3 are needed); GPU features that need them are unavailable.", self.name, self.storage_textures_per_stage)); }
        None
    }

    /// Largest texture edge the viewport can hold, with a hint when it is small.
    pub fn texture_problem(&self, wanted: u32) -> Option<String> {
        (self.max_texture_2d < wanted).then(|| format!("{} supports textures up to {0}px; {1}px previews are scaled down.", self.max_texture_2d, wanted))
    }

    /// One block of text for diagnostics.
    pub fn report(&self) -> String {
        format!("Adapter: {} ({}, {})\nDriver: {}\nMax texture: {}px, max buffer: {} MiB, storage textures/stage: {}\n4x MSAA: {}, compute: {}, RGBA16F filterable/renderable: {}/{}",
            self.name, self.backend, self.device_type, self.driver, self.max_texture_2d, self.max_buffer_size / (1 << 20), self.storage_textures_per_stage,
            self.msaa4, self.compute, self.rgba16f_filterable, self.rgba16f_renderable)
    }
}

static CAPS: OnceLock<GpuCaps> = OnceLock::new();

/// Record the capabilities of the adapter in use (first call wins).
pub fn set(caps: GpuCaps) { let _ = CAPS.set(caps); }
/// The recorded capabilities, or "everything available" when none were recorded.
pub fn get() -> GpuCaps {
    let mut c = CAPS.get().cloned().unwrap_or_default();
    if SAFE.load(std::sync::atomic::Ordering::Relaxed) { c.msaa4 = false; }
    c
}
static SAFE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Safe mode: no MSAA, no path tracing.
pub fn set_safe_mode(on: bool) { SAFE.store(on, std::sync::atomic::Ordering::Relaxed); }
pub fn safe_mode() -> bool { SAFE.load(std::sync::atomic::Ordering::Relaxed) }
pub fn is_probed() -> bool { CAPS.get().is_some() }

// ---- device errors -------------------------------------------------------------------------

use std::sync::Mutex;
static LOST: Mutex<Option<String>> = Mutex::new(None);
static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Replace wgpu's default "panic on any uncaptured error" with logging, and remember a lost
/// device so the UI can tell the user (and save) instead of the process dying mid-frame.
pub fn install_error_handlers(device: &wgpu::Device) {
    device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
        let text = e.to_string();
        log::error!("GPU error: {text}");
        if let Ok(mut v) = ERRORS.lock() { if v.len() < 50 && !v.contains(&text) { v.push(text); } }
    }));
    device.set_device_lost_callback(|reason, message| {
        let text = format!("{reason:?}: {message}");
        log::error!("GPU device lost: {text}");
        let first = LOST.lock().map(|mut l| l.replace(text.clone()).is_none()).unwrap_or(true);
        if first && let Some(hook) = LOST_HOOK.get() { hook(&text); }
    });
}
/// For the extra devices (GPU compositing, GPU baking): wgpu's default reaction to an uncaptured
/// error such as running out of video memory is a panic, which killed the compositor or bake
/// worker. Errors are logged instead; the failed work then errors at readback and falls back to
/// the processor. A lost worker device doesn't trigger the window's device-lost dialog.
pub fn install_worker_error_handlers(device: &wgpu::Device, name: &'static str) {
    device.on_uncaptured_error(std::sync::Arc::new(move |e: wgpu::Error| log::error!("GPU error ({name}): {e}")));
    device.set_device_lost_callback(move |reason, message| log::warn!("GPU device lost ({name}): {reason:?}: {message}"));
}
static LOST_HOOK: OnceLock<fn(&str)> = OnceLock::new();
/// Called once (on a wgpu thread) when the device is lost; the UI shows a native dialog there,
/// because a lost device also stops egui from drawing.
pub fn set_device_lost_hook(hook: fn(&str)) { let _ = LOST_HOOK.set(hook); }
/// Set when the GPU device was lost (driver reset, GPU removed, TDR). Not recoverable in-process.
pub fn device_lost() -> Option<String> { LOST.lock().ok().and_then(|l| l.clone()) }
/// Distinct uncaptured GPU errors seen so far (validation / out of memory), oldest first.
pub fn recent_errors() -> Vec<String> { ERRORS.lock().map(|v| v.clone()).unwrap_or_default() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limited_adapter_reports_reasons() {
        let c = GpuCaps { msaa4: false, compute: false, max_texture_2d: 2048, name: "Old GPU".into(), ..Default::default() };
        assert!(c.msaa_problem().unwrap().contains("Old GPU"));
        assert!(c.compute_problem().unwrap().contains("compute"));
        assert!(c.texture_problem(4096).is_some() && c.texture_problem(1024).is_none());
        assert!(GpuCaps::default().msaa_problem().is_none() && GpuCaps::default().compute_problem().is_none());
        assert!(c.report().contains("Old GPU"));
    }
}
