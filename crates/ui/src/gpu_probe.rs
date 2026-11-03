//! GPU startup probe, wgpu side (issue #83). The policy, protocol and child handling live in
//! `subcraft_app::startup_probe`; this file only talks to wgpu. `child_main` runs in the
//! short-lived `subcraft --gpu-probe ...` process, so a driver crash kills only that process.
use egui_wgpu::wgpu;
use std::sync::OnceLock;
use subcraft_app::startup_probe::{self as sp, AdapterKind, ProbeAdapter, ProbeBackend};

/// The adapter the probe verified for this run (set before wgpu is initialised in-process).
static VERIFIED: OnceLock<std::sync::Mutex<Option<ProbeAdapter>>> = OnceLock::new();

pub fn set_verified(adapter: Option<ProbeAdapter>) {
    *VERIFIED.get_or_init(Default::default).lock().unwrap() = adapter;
}
pub fn verified() -> Option<ProbeAdapter> { VERIFIED.get_or_init(Default::default).lock().unwrap().clone() }

fn backends(b: ProbeBackend) -> wgpu::Backends {
    match b { ProbeBackend::Dx12 => wgpu::Backends::DX12, ProbeBackend::Vulkan => wgpu::Backends::VULKAN, ProbeBackend::Gl => wgpu::Backends::GL }
}

pub fn backend_of(b: wgpu::Backend) -> Option<ProbeBackend> {
    match b { wgpu::Backend::Dx12 => Some(ProbeBackend::Dx12), wgpu::Backend::Vulkan => Some(ProbeBackend::Vulkan), wgpu::Backend::Gl => Some(ProbeBackend::Gl), _ => None }
}

fn describe(index: u32, backend: ProbeBackend, info: &wgpu::AdapterInfo) -> ProbeAdapter {
    ProbeAdapter {
        backend, index, vendor: info.vendor, device: info.device,
        kind: match info.device_type {
            wgpu::DeviceType::DiscreteGpu => AdapterKind::Discrete, wgpu::DeviceType::IntegratedGpu => AdapterKind::Integrated,
            wgpu::DeviceType::VirtualGpu => AdapterKind::Virtual, wgpu::DeviceType::Cpu => AdapterKind::Cpu, _ => AdapterKind::Other,
        },
        driver: format!("{} {}", info.driver, info.driver_info).trim().to_string(),
        name: info.name.clone(),
    }
}

/// Does a wgpu adapter match a verified one (same backend, vendor, device, name)?
pub fn matches(adapter: &wgpu::Adapter, want: &ProbeAdapter) -> bool {
    let info = adapter.get_info();
    backend_of(info.backend) == Some(want.backend) && info.vendor == want.vendor && info.device == want.device && info.name == want.name
}

/// Point a wgpu setup at the verified adapter (only that backend, adapter chosen by identity).
/// False when the probe did not run, leaving the setup untouched.
pub fn apply(setup: &mut egui_wgpu::WgpuSetupCreateNew) -> bool {
    let Some(want) = verified() else { return false };
    setup.instance_descriptor.backends = backends(want.backend);
    setup.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, _surface| {
        adapters.iter().find(|a| matches(a, &want)).cloned()
            .ok_or_else(|| format!("verified adapter {} is no longer present", want.name))
    }));
    true
}

/// Windows: no "has stopped working" dialog when this child crashes.
#[cfg(windows)]
#[allow(unsafe_code)]
fn silence_crash_dialogs() {
    unsafe extern "system" { fn SetErrorMode(mode: u32) -> u32; }
    // SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX
    unsafe { SetErrorMode(0x0001 | 0x0002 | 0x8000); }
}
#[cfg(not(windows))]
fn silence_crash_dialogs() {}

/// Entry for `subcraft --gpu-probe <backend> [<index>]`. Prints the protocol on stdout and
/// returns the process exit code.
pub fn child_main(args: &[String]) -> i32 {
    silence_crash_dialogs();
    let Some(backend) = args.first().and_then(|s| ProbeBackend::parse(s)) else { println!("FAIL\tunknown backend"); return 2 };
    let index: Option<u32> = args.get(1).and_then(|s| s.parse().ok());
    let desc = wgpu::InstanceDescriptor { backends: backends(backend), ..wgpu::InstanceDescriptor::new_without_display_handle() };
    let instance = wgpu::Instance::new(desc);
    let adapters = pollster::block_on(instance.enumerate_adapters(backends(backend)));
    let Some(index) = index else {
        for (i, a) in adapters.iter().enumerate() { println!("{}", sp::adapter_line(&describe(i as u32, backend, &a.get_info()))); }
        println!("DONE");
        return 0;
    };
    let Some(adapter) = adapters.get(index as usize) else { println!("FAIL\tadapter {index} not found"); return 0 };
    match create_device(adapter) {
        Ok(()) => println!("OK"),
        Err(e) => println!("FAIL\t{}", e.replace(['\t', '\n', '\r'], " ")),
    }
    0
}

/// What the app needs from a device: the adapter's full texture limit, one buffer, one submit.
fn create_device(adapter: &wgpu::Adapter) -> Result<(), String> {
    let limits = wgpu::Limits { max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d, ..wgpu::Limits::default() };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("subcraft gpu probe"), required_limits: limits, ..Default::default()
    })).map_err(|e| format!("request_device: {e}"))?;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 256, usage: wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    queue.write_buffer(&buffer, 0, &[0u8; 16]);
    queue.submit([]);
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("poll: {e:?}"))?;
    Ok(())
}

/// Backends the probe may use for a user-chosen API (Automatic = everything this OS has).
pub fn allowed_backends(api: subcraft_app::graphics::GraphicsApi) -> Vec<ProbeBackend> {
    use subcraft_app::graphics::GraphicsApi::*;
    match api {
        DirectX12 => vec![ProbeBackend::Dx12],
        OpenGl => vec![ProbeBackend::Gl],
        Automatic if cfg!(windows) => ProbeBackend::ALL.to_vec(),
        Automatic => vec![ProbeBackend::Vulkan, ProbeBackend::Gl],
    }
}

/// Run the probe for `api` and remember the result for `graphics::wgpu_options`.
pub fn prepare(api: subcraft_app::graphics::GraphicsApi, environment_override: bool, previous_log: &str) -> sp::ProbeResult {
    set_verified(None);
    if !sp::enabled(environment_override) {
        sp::step("gpu probe skipped (not needed on this system, WGPU_BACKEND set, or SUBCRAFT_SKIP_GPU_PROBE)");
        return sp::ProbeResult::NotUsed;
    }
    let runner = sp::ProbeRunner { command: &sp::exe_command, enumerate_timeout: std::time::Duration::from_secs(20), device_timeout: std::time::Duration::from_secs(30) };
    let result = sp::select(&runner, &allowed_backends(api), previous_log, sp::ProbeCache::path().as_deref());
    if let sp::ProbeResult::Verified { adapter, .. } = &result { set_verified(Some(adapter.clone())); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The real child code path against whatever adapter exists (llvmpipe here); skips if none.
    #[test]
    fn child_lists_and_creates_a_device_when_an_adapter_exists() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        let Some(a) = adapters.first() else { eprintln!("no adapter; skipped"); return };
        let info = a.get_info();
        let Some(backend) = backend_of(info.backend) else { return };
        assert_eq!(create_device(a), Ok(()));
        let line = sp::adapter_line(&describe(0, backend, &info));
        let parsed = sp::parse_enumeration(backend, &format!("{line}\nDONE\n")).unwrap();
        assert_eq!(parsed[0].name, info.name);
    }
}
