//! Window-less device creation for tests and offline rendering tools.

use egui_wgpu::wgpu;

/// Create a device on any available adapter (hardware or software, e.g. lavapipe/WARP).
/// `None` when the machine has no usable adapter; callers should skip rather than fail.
pub fn device() -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("subcraft headless"),
        ..Default::default()
    }))
    .ok()?;
    Some((adapter, device, queue))
}
