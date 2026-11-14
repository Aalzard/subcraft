//! X11 / XWayland backend: pen pressure from XInput2 raw events on a second X connection.
//! winit already delivers the pen's clicks and motion; this only adds the "Abs Pressure"
//! valuator of devices that have one. Raw events are selected on the root window, which never
//! conflicts with winit's own (exclusive) button selection on the window.

use std::collections::HashMap;

use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::xinput::{self, ConnectionExt as _, DeviceClassData, Fp3232, XIEventMask};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::Event;

use super::{PressureAxis, Queue, RawKind, X11Pen};

fn fp(v: Fp3232) -> f64 {
    v.integral as f64 + v.frac as f64 / 4_294_967_296.0
}

pub(super) fn start(queue: Queue, ctx: egui::Context) -> Result<(), String> {
    let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let err = |e: &dyn std::fmt::Display| e.to_string();
    conn.extension_information(xinput::X11_EXTENSION_NAME).map_err(|e| err(&e))?.ok_or("no XInput extension")?;
    let version = conn.xinput_xi_query_version(2, 2).map_err(|e| err(&e))?.reply().map_err(|e| err(&e))?;
    if (version.major_version, version.minor_version) < (2, 1) {
        return Err(format!("XInput {}.{} (raw events need 2.1)", version.major_version, version.minor_version));
    }
    let devices = conn.xinput_xi_query_device(xinput::Device::ALL).map_err(|e| err(&e))?.reply().map_err(|e| err(&e))?;
    let mut axes = HashMap::new();
    let mut names = Vec::new();
    for info in &devices.infos {
        for class in &info.classes {
            if let DeviceClassData::Valuator(v) = &class.data {
                let label = conn.get_atom_name(v.label).ok().and_then(|c| c.reply().ok()).map(|r| r.name).unwrap_or_default();
                if label == b"Abs Pressure" {
                    axes.insert(info.deviceid, PressureAxis { number: v.number, min: fp(v.min), max: fp(v.max) });
                    names.push(String::from_utf8_lossy(&info.name).into_owned());
                }
            }
        }
    }
    let mut pen = X11Pen::new(axes);
    if !pen.has_devices() {
        return Err("no X input device with a pressure axis".into());
    }
    let root = conn.setup().roots[screen].root;
    let mask = XIEventMask::RAW_MOTION | XIEventMask::RAW_BUTTON_PRESS | XIEventMask::RAW_BUTTON_RELEASE;
    conn.xinput_xi_select_events(root, &[xinput::EventMask { deviceid: xinput::Device::ALL.into(), mask: vec![mask] }])
        .map_err(|e| err(&e))?
        .check()
        .map_err(|e| err(&e))?;
    log::info!(target: "subcraft::input", "X11 tablet pressure active for: {}", names.join(", "));
    let log = crate::pen_input::input_log_enabled();
    std::thread::Builder::new()
        .name("subcraft-tablet".into())
        .spawn(move || {
            loop {
                let event = match conn.wait_for_event() {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!(target: "subcraft::input", "X11 tablet pressure stopped: {e}");
                        return;
                    }
                };
                let (kind, raw) = match event {
                    Event::XinputRawButtonPress(e) => (RawKind::Press, e),
                    Event::XinputRawButtonRelease(e) => (RawKind::Release, e),
                    Event::XinputRawMotion(e) => (RawKind::Motion, e),
                    _ => continue,
                };
                let values: Vec<f64> = raw.axisvalues.iter().copied().map(fp).collect();
                if let Some(touch) = pen.raw(kind, raw.deviceid, raw.detail, &raw.valuator_mask, &values) {
                    if log {
                        log::info!(target: "subcraft::input", "X11 pen device {} {kind:?}: {touch:?}", raw.deviceid);
                    }
                    queue.lock().unwrap_or_else(|e| e.into_inner()).push(touch);
                    ctx.request_repaint();
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
