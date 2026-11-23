//! Windows pen tilt and rotation. winit turns `WM_POINTER*` pen messages into touch events with
//! pressure only; this wraps the window procedure, reads the pen's tilt and rotation with
//! `GetPointerPenInfo` for each pen pointer message and hands them to [`super::report_pen_pose`],
//! then lets winit's procedure handle the message as before (nothing is consumed).
#![allow(unsafe_code)] // Win32 window procedure and pointer calls (the only way to read tilt).

use std::sync::atomic::{AtomicIsize, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

type Hwnd = isize;
type WndProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;

const GWLP_WNDPROC: i32 = -4;
const WM_POINTERUPDATE: u32 = 0x0245;
const WM_POINTERDOWN: u32 = 0x0246;
const PT_PEN: u32 = 3;
const PEN_MASK_ROTATION: u32 = 0x2;
const PEN_MASK_TILT_X: u32 = 0x4;
const PEN_MASK_TILT_Y: u32 = 0x8;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Point {
    x: i32,
    y: i32,
}

/// `POINTER_INFO` (winuser.h).
#[repr(C)]
#[derive(Clone, Copy)]
struct PointerInfo {
    pointer_type: u32,
    pointer_id: u32,
    frame_id: u32,
    pointer_flags: u32,
    source_device: isize,
    hwnd_target: isize,
    pixel_location: Point,
    himetric_location: Point,
    pixel_location_raw: Point,
    himetric_location_raw: Point,
    time: u32,
    history_count: u32,
    input_data: i32,
    key_states: u32,
    performance_count: u64,
    button_change_type: i32,
}

/// `POINTER_PEN_INFO` (winuser.h).
#[repr(C)]
#[derive(Clone, Copy)]
struct PointerPenInfo {
    pointer_info: PointerInfo,
    pen_flags: u32,
    pen_mask: u32,
    pressure: u32,
    rotation: u32,
    tilt_x: i32,
    tilt_y: i32,
}

// The layouts must match winuser.h on 64-bit Windows (96 and 120 bytes).
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<PointerInfo>() == 96 && std::mem::size_of::<PointerPenInfo>() == 120);

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn CallWindowProcW(prev: isize, hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetPointerType(pointer_id: u32, pointer_type: *mut u32) -> i32;
    fn GetPointerPenInfo(pointer_id: u32, info: *mut PointerPenInfo) -> i32;
}

/// winit's window procedure, called for every message after reading the pen.
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

/// Pose of a pen from its `POINTER_PEN_INFO`; axes the pen does not report stay upright / absent.
fn pose(info: &PointerPenInfo) -> subcraft_paint::PenPose {
    let tilt_x = if info.pen_mask & PEN_MASK_TILT_X != 0 { info.tilt_x as f32 } else { 0.0 };
    let tilt_y = if info.pen_mask & PEN_MASK_TILT_Y != 0 { info.tilt_y as f32 } else { 0.0 };
    let rotation = (info.pen_mask & PEN_MASK_ROTATION != 0).then_some(info.rotation as f32);
    subcraft_paint::PenPose { tilt: [tilt_x, tilt_y], rotation }
}

unsafe extern "system" fn wndproc(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize {
    if msg == WM_POINTERUPDATE || msg == WM_POINTERDOWN {
        let id = (wparam & 0xffff) as u32;
        let mut kind = 0u32;
        // SAFETY: plain Win32 calls with valid out pointers; failures leave the pose unreported.
        unsafe {
            if GetPointerType(id, &mut kind) != 0 && kind == PT_PEN {
                let mut info: PointerPenInfo = std::mem::zeroed();
                if GetPointerPenInfo(id, &mut info) != 0 && info.pen_mask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y | PEN_MASK_ROTATION) != 0 {
                    super::report_pen_pose(pose(&info));
                }
            }
        }
    }
    // SAFETY: PREVIOUS holds the procedure that was installed before ours.
    unsafe { CallWindowProcW(PREVIOUS.load(Ordering::Acquire), hwnd, msg, wparam, lparam) }
}

/// Wrap the window procedure of the eframe window (once).
pub fn start(frame: &eframe::Frame) -> Result<(), String> {
    let hwnd = match frame.window_handle().map_err(|e| e.to_string())?.as_raw() {
        RawWindowHandle::Win32(h) => h.hwnd.get(),
        _ => return Err("not a Win32 window".into()),
    };
    if PREVIOUS.load(Ordering::Acquire) != 0 {
        return Ok(());
    }
    let ours: WndProc = wndproc;
    // SAFETY: `hwnd` is the live eframe window on this (UI) thread; the previous procedure is
    // stored before any message can reach ours, because messages are dispatched on this thread.
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours as usize as isize) };
    if previous == 0 {
        return Err("SetWindowLongPtrW failed".into());
    }
    PREVIOUS.store(previous, Ordering::Release);
    Ok(())
}
