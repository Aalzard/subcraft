//! Window-less device creation for tests, offline rendering tools and the application's secondary
//! GPU devices (GPU compositing, GPU baking).
//!
//! Once the application has picked its window adapter it calls [`set_main_adapter`]. From then on
//! secondary devices are created on the same graphics API and, when present, the same adapter.
//! Without this, a secondary `Instance` with every backend enabled also starts the Vulkan loader
//! on Windows even when the window runs on DX12, which loads every implicit Vulkan layer
//! installed on the machine (overlays such as GOG Galaxy's). A faulty layer then crashes the whole
//! process a few seconds after start-up, which is what issue #98's log shows.

use egui_wgpu::wgpu;
use std::sync::Mutex;

/// The window adapter: graphics API and adapter name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainAdapter {
    pub backend: wgpu::Backend,
    pub name: String,
}

static MAIN_ADAPTER: Mutex<Option<MainAdapter>> = Mutex::new(None);

/// Record the adapter the window uses so secondary devices follow it.
pub fn set_main_adapter(info: &wgpu::AdapterInfo) {
    if let Ok(mut main) = MAIN_ADAPTER.lock() {
        *main = Some(MainAdapter { backend: info.backend, name: info.name.clone() });
    }
}

/// The adapter recorded by [`set_main_adapter`], if any.
pub fn main_adapter() -> Option<MainAdapter> {
    MAIN_ADAPTER.lock().ok().and_then(|m| m.clone())
}

/// Backends a secondary instance may use: only the window's API when it is known, otherwise
/// whatever the environment (`WGPU_BACKEND`) or wgpu's defaults allow.
pub fn secondary_backends(main: Option<&MainAdapter>) -> wgpu::Backends {
    match main {
        Some(m) if m.backend != wgpu::Backend::Noop => wgpu::Backends::from(m.backend),
        _ => wgpu::InstanceDescriptor::new_without_display_handle_from_env().backends,
    }
}

/// Instance and adapter for a secondary device: the window's adapter on the window's API when
/// known, else the default adapter.
pub fn secondary_adapter() -> Option<(wgpu::Instance, wgpu::Adapter)> {
    let main = main_adapter();
    let desc = wgpu::InstanceDescriptor { backends: secondary_backends(main.as_ref()), ..wgpu::InstanceDescriptor::new_without_display_handle_from_env() };
    let instance = wgpu::Instance::new(desc);
    if let Some(main) = &main {
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::from(main.backend)));
        if let Some(adapter) = adapters.into_iter().find(|a| a.get_info().name == main.name) {
            return Some((instance, adapter));
        }
    }
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    Some((instance, adapter))
}

/// Create a device on any available adapter (hardware or software, e.g. lavapipe/WARP).
/// `None` when the machine has no usable adapter; callers should skip rather than fail.
pub fn device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let (_instance, adapter) = secondary_adapter()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("subcraft headless"),
        ..Default::default()
    }))
    .ok()?;
    Some((adapter, device, queue))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_instances_follow_the_window_api_only() {
        let dx12 = MainAdapter { backend: wgpu::Backend::Dx12, name: "GPU".into() };
        assert_eq!(secondary_backends(Some(&dx12)), wgpu::Backends::DX12);
        let vk = MainAdapter { backend: wgpu::Backend::Vulkan, name: "GPU".into() };
        assert_eq!(secondary_backends(Some(&vk)), wgpu::Backends::VULKAN);
        let gl = MainAdapter { backend: wgpu::Backend::Gl, name: "GPU".into() };
        assert_eq!(secondary_backends(Some(&gl)), wgpu::Backends::GL);
        // No window adapter yet (tests, tools): unchanged behaviour.
        assert_eq!(secondary_backends(None), wgpu::InstanceDescriptor::new_without_display_handle_from_env().backends);
    }
}
