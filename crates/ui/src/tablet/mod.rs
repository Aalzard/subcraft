//! Native pen tablet input on Linux (issue #82).
//!
//! winit speaks neither the Wayland tablet protocol nor XInput2 valuators, so a pen there gives
//! no pressure, and on Wayland compositors that don't emulate a pointer for it, no clicks. Two
//! backends fill the gap and feed SubCraft's existing pen path (`Event::Touch` with a force for
//! pressure, pointer events for clicks and hover):
//!
//! * Wayland: binds `zwp_tablet_manager_v2` on winit's own `wl_display` (see `wayland.rs`). Once a
//!   client has a tablet seat the compositor sends the pen as tablet events instead of pointer
//!   events, so this backend produces hover, press/release, stylus buttons and pressure.
//! * X11 / XWayland: winit already delivers clicks; a second X connection listens to XInput2 raw
//!   events and adds the pen's "Abs Pressure" valuator (see `x11.rs`).
//!
//! Both run on a background thread that queues events here and asks egui for a repaint; the
//! queue is drained in `raw_input_hook`. Without the protocol / extension (or with
//! `SUBCRAFT_TABLET=0`) nothing starts and input works as before. The mapping from tablet events
//! to egui events is plain code below, unit-tested without hardware.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Event, Modifiers, PointerButton, Pos2, TouchDeviceId, TouchId, TouchPhase};

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;

/// Device id of the touch events this module produces. `pen_input::PenInputFixer` leaves them
/// alone: the Wayland backend emits its own pointer events, and on X11 winit does.
pub const TABLET_DEVICE: TouchDeviceId = TouchDeviceId(0x5375_6263_7261_6674);
const PEN_TOUCH: TouchId = TouchId(0x7065_6e);

/// Queue shared with a backend thread. Positions are window-logical pixels (Wayland surface
/// coordinates); an X11 pressure sample has no position (NaN) and takes the pointer's.
type Queue = Arc<Mutex<Vec<Event>>>;

/// Lives in `UiState`; started from the first frame (it needs the window handles).
#[derive(Default)]
pub struct Tablet {
    started: bool,
    queue: Option<Queue>,
    last_pointer: Option<Pos2>,
    #[cfg(target_os = "linux")]
    stop: Option<Box<dyn FnOnce() + Send>>,
}

impl std::fmt::Debug for Tablet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tablet").field("active", &self.queue.is_some()).finish()
    }
}

impl Tablet {
    /// Start the platform backend once (no-op elsewhere and on later calls).
    pub fn start_once(&mut self, frame: &eframe::Frame, ctx: &egui::Context) {
        if std::mem::replace(&mut self.started, true) {
            return;
        }
        if std::env::var_os("SUBCRAFT_TABLET").is_some_and(|v| v == "0") {
            log::info!(target: "subcraft::input", "native tablet input disabled (SUBCRAFT_TABLET=0)");
            return;
        }
        #[cfg(target_os = "linux")]
        {
            let queue = Queue::default();
            match start_linux(frame, ctx, queue.clone()) {
                Ok(stop) => {
                    self.queue = Some(queue);
                    self.stop = stop;
                }
                Err(e) => log::info!(target: "subcraft::input", "native tablet input not available: {e}"),
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (frame, ctx);
    }

    /// Move the queued tablet events into this frame's input.
    pub fn drain_into(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(p) = raw.events.iter().rev().find_map(|e| match e {
            Event::PointerMoved(p) => Some(*p),
            _ => None,
        }) {
            self.last_pointer = Some(p);
        }
        let Some(queue) = &self.queue else { return };
        let queued = std::mem::take(&mut *queue.lock().unwrap_or_else(|e| e.into_inner()));
        if queued.is_empty() {
            return;
        }
        let events = finish_events(queued, ctx.zoom_factor(), ctx.input(|i| i.modifiers), self.last_pointer);
        if let Some(p) = events.iter().rev().find_map(|e| match e {
            Event::PointerMoved(p) => Some(*p),
            _ => None,
        }) {
            self.last_pointer = Some(p);
        }
        raw.events.extend(events);
    }

    /// Stop the backend thread before the window (and its `wl_display`) goes away.
    pub fn shutdown(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(stop) = self.stop.take() {
            stop();
        }
        self.queue = None;
    }
}

#[cfg(target_os = "linux")]
type Stop = Option<Box<dyn FnOnce() + Send>>;

#[cfg(target_os = "linux")]
fn start_linux(frame: &eframe::Frame, ctx: &egui::Context, queue: Queue) -> Result<Stop, String> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    let display = frame.display_handle().map_err(|e| e.to_string())?.as_raw();
    match display {
        RawDisplayHandle::Wayland(d) => {
            let surface = match frame.window_handle().map_err(|e| e.to_string())?.as_raw() {
                RawWindowHandle::Wayland(w) => w.surface.as_ptr() as usize,
                _ => return Err("Wayland display without a Wayland surface".into()),
            };
            wayland::start(d.display.as_ptr() as usize, surface, queue, ctx.clone()).map(Some)
        }
        RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => x11::start(queue, ctx.clone()).map(|()| None),
        _ => Err("not a Wayland or X11 window".into()),
    }
}

/// Convert queued backend events to this frame's input: logical pixels to points, the keyboard
/// modifiers on button events, and the pointer position for X11 pressure samples.
pub fn finish_events(queued: Vec<Event>, zoom: f32, modifiers: Modifiers, pointer: Option<Pos2>) -> Vec<Event> {
    let zoom = if zoom > 0.0 { zoom } else { 1.0 };
    let mut pointer = pointer.unwrap_or(Pos2::ZERO);
    let scale = |p: Pos2| Pos2::new(p.x / zoom, p.y / zoom);
    queued
        .into_iter()
        .map(|e| match e {
            Event::PointerMoved(p) => {
                pointer = scale(p);
                Event::PointerMoved(pointer)
            }
            Event::PointerButton { pos, button, pressed, .. } => {
                Event::PointerButton { pos: scale(pos), button, pressed, modifiers }
            }
            Event::Touch { device_id, id, phase, pos, force } => {
                let pos = if pos.x.is_nan() { pointer } else { scale(pos) };
                Event::Touch { device_id, id, phase, pos, force }
            }
            other => other,
        })
        .collect()
}

fn pen_touch(phase: TouchPhase, pos: Pos2, pressure: f32) -> Event {
    Event::Touch { device_id: TABLET_DEVICE, id: PEN_TOUCH, phase, pos, force: Some(pressure.clamp(0.0, 1.0)) }
}

// ----- Wayland tablet tool ------------------------------------------------------------------------

/// One `zwp_tablet_tool_v2` event, already decoded (pressure normalized to 0..1, positions in
/// surface-local logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolEvent {
    ProximityIn { on_window: bool },
    ProximityOut,
    Down,
    Up,
    Motion { x: f32, y: f32 },
    Pressure(f32),
    Tilt { x: f32, y: f32 },
    Button { button: PointerButton, pressed: bool },
    Frame,
}

/// Stylus button codes (linux/input-event-codes.h) to egui buttons: the lower barrel button is a
/// right click, the upper one a middle click (both like Krita / Blender defaults).
pub fn stylus_button(code: u32) -> Option<PointerButton> {
    match code {
        0x14b => Some(PointerButton::Secondary), // BTN_STYLUS
        0x14c | 0x149 => Some(PointerButton::Middle), // BTN_STYLUS2, BTN_STYLUS3
        _ => None,
    }
}

#[derive(Debug, Default)]
struct Pending {
    moved: bool,
    down: bool,
    up: bool,
    pressure: bool,
    left: bool,
    buttons: Vec<(PointerButton, bool)>,
}

/// Accumulates tool events and emits egui events on each `frame`, the protocol's grouping.
#[derive(Debug)]
pub struct ToolMapper {
    on_window: bool,
    down: bool,
    pos: Pos2,
    pressure: f32,
    tilt: [f32; 2],
    pending: Pending,
}

impl Default for ToolMapper {
    fn default() -> Self {
        Self { on_window: false, down: false, pos: Pos2::ZERO, pressure: 1.0, tilt: [0.0; 2], pending: Pending::default() }
    }
}

impl ToolMapper {
    /// Last tilt in degrees (x, y), for diagnostics; brushes don't use tilt yet.
    pub fn tilt(&self) -> [f32; 2] {
        self.tilt
    }

    pub fn handle(&mut self, event: ToolEvent, out: &mut Vec<Event>) {
        let p = &mut self.pending;
        match event {
            ToolEvent::ProximityIn { on_window } => {
                self.on_window = on_window;
                p.left = false;
            }
            ToolEvent::ProximityOut => p.left = true,
            ToolEvent::Down => p.down = true,
            ToolEvent::Up => p.up = true,
            ToolEvent::Motion { x, y } => {
                self.pos = Pos2::new(x, y);
                p.moved = true;
            }
            ToolEvent::Pressure(v) => {
                self.pressure = v.clamp(0.0, 1.0);
                p.pressure = true;
            }
            ToolEvent::Tilt { x, y } => self.tilt = [x, y],
            ToolEvent::Button { button, pressed } => p.buttons.push((button, pressed)),
            ToolEvent::Frame => self.frame(out),
        }
    }

    fn frame(&mut self, out: &mut Vec<Event>) {
        let p = std::mem::take(&mut self.pending);
        if !self.on_window {
            return;
        }
        let pos = self.pos;
        let press = |out: &mut Vec<Event>, button, pressed| {
            out.push(Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
        };
        if p.down && !self.down {
            self.down = true;
            out.push(pen_touch(TouchPhase::Start, pos, self.pressure));
            out.push(Event::PointerMoved(pos));
            press(out, PointerButton::Primary, true);
        } else if self.down && (p.moved || p.pressure) {
            out.push(pen_touch(TouchPhase::Move, pos, self.pressure));
            if p.moved {
                out.push(Event::PointerMoved(pos));
            }
        } else if p.moved {
            out.push(Event::PointerMoved(pos));
        }
        for (button, pressed) in p.buttons {
            press(out, button, pressed);
        }
        if self.down && (p.up || p.left) {
            self.down = false;
            out.push(pen_touch(TouchPhase::End, pos, self.pressure));
            press(out, PointerButton::Primary, false);
        }
        if p.left {
            self.on_window = false;
            out.push(Event::PointerGone);
        }
    }
}

// ----- X11 pressure valuator ----------------------------------------------------------------------

/// Range of a device's "Abs Pressure" valuator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressureAxis {
    pub number: u16,
    pub min: f64,
    pub max: f64,
}

impl PressureAxis {
    fn normalize(&self, v: f64) -> f32 {
        let span = self.max - self.min;
        if span <= 0.0 { 1.0 } else { ((v - self.min) / span).clamp(0.0, 1.0) as f32 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawKind {
    Press,
    Release,
    Motion,
}

/// Value of valuator `number` in an XI2 raw event: `values` holds one entry per set bit of `mask`.
pub fn valuator(mask: &[u32], values: &[f64], number: u16) -> Option<f64> {
    let (word, bit) = (number as usize / 32, number as u32 % 32);
    if mask.get(word)? & (1 << bit) == 0 {
        return None;
    }
    let before: u32 = mask[..word].iter().map(|w| w.count_ones()).sum::<u32>() + (mask[word] & ((1u32 << bit) - 1)).count_ones();
    values.get(before as usize).copied()
}

/// Turns XI2 raw events of pressure-capable devices into pen touch events (pressure only; the
/// clicks come from winit). Positions are left NaN for `finish_events`.
#[derive(Debug, Default)]
pub struct X11Pen {
    axes: HashMap<u16, PressureAxis>,
    down: Option<u16>,
    pressure: f32,
}

impl X11Pen {
    pub fn new(axes: HashMap<u16, PressureAxis>) -> Self {
        Self { axes, down: None, pressure: 1.0 }
    }

    pub fn has_devices(&self) -> bool {
        !self.axes.is_empty()
    }

    pub fn raw(&mut self, kind: RawKind, device: u16, detail: u32, mask: &[u32], values: &[f64]) -> Option<Event> {
        let axis = *self.axes.get(&device)?;
        if let Some(v) = valuator(mask, values, axis.number) {
            self.pressure = axis.normalize(v);
        }
        let nan = Pos2::new(f32::NAN, f32::NAN);
        match kind {
            RawKind::Press if detail == 1 && self.down.is_none() => {
                self.down = Some(device);
                Some(pen_touch(TouchPhase::Start, nan, self.pressure))
            }
            RawKind::Motion if self.down == Some(device) && valuator(mask, values, axis.number).is_some() => {
                Some(pen_touch(TouchPhase::Move, nan, self.pressure))
            }
            RawKind::Release if detail == 1 && self.down == Some(device) => {
                self.down = None;
                Some(pen_touch(TouchPhase::End, nan, self.pressure))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn run(m: &mut ToolMapper, events: &[ToolEvent]) -> Vec<Event> {
        let mut out = Vec::new();
        for e in events {
            m.handle(*e, &mut out);
        }
        out
    }
    fn buttons(e: &[Event]) -> Vec<(PointerButton, bool)> {
        e.iter().filter_map(|e| match e {
            Event::PointerButton { button, pressed, .. } => Some((*button, *pressed)),
            _ => None,
        }).collect()
    }
    fn touches(e: &[Event]) -> Vec<(TouchPhase, f32)> {
        e.iter().filter_map(|e| match e {
            Event::Touch { phase, force, device_id, .. } => {
                assert_eq!(*device_id, TABLET_DEVICE);
                Some((*phase, force.unwrap()))
            }
            _ => None,
        }).collect()
    }

    #[test]
    fn hover_moves_the_pointer_without_pressing() {
        let mut m = ToolMapper::default();
        let out = run(&mut m, &[ToolEvent::ProximityIn { on_window: true }, ToolEvent::Motion { x: 10.0, y: 20.0 }, ToolEvent::Frame]);
        assert_eq!(out, vec![Event::PointerMoved(pos2(10.0, 20.0))]);
    }

    #[test]
    fn stroke_is_press_drag_release_with_pressure() {
        use ToolEvent::*;
        let mut m = ToolMapper::default();
        run(&mut m, &[ProximityIn { on_window: true }, Motion { x: 1.0, y: 1.0 }, Frame]);
        let down = run(&mut m, &[Down, Pressure(0.3), Frame]);
        assert_eq!(touches(&down), vec![(TouchPhase::Start, 0.3)]);
        assert_eq!(buttons(&down), vec![(PointerButton::Primary, true)]);
        // The touch comes first, then the pointer events (as egui-winit orders them).
        assert!(matches!(down[0], Event::Touch { .. }));
        let drag = run(&mut m, &[Motion { x: 5.0, y: 1.0 }, Pressure(0.7), Frame]);
        assert_eq!(touches(&drag), vec![(TouchPhase::Move, 0.7)]);
        assert!(drag.contains(&Event::PointerMoved(pos2(5.0, 1.0))));
        assert!(buttons(&drag).is_empty());
        let press_only = run(&mut m, &[Pressure(0.9), Frame]);
        assert_eq!(touches(&press_only), vec![(TouchPhase::Move, 0.9)], "pressure changes while still");
        let up = run(&mut m, &[Up, Frame]);
        assert_eq!(touches(&up).len(), 1);
        assert_eq!(touches(&up)[0].0, TouchPhase::End);
        assert_eq!(buttons(&up), vec![(PointerButton::Primary, false)]);
    }

    #[test]
    fn tap_within_one_frame_presses_and_releases() {
        use ToolEvent::*;
        let mut m = ToolMapper::default();
        let out = run(&mut m, &[ProximityIn { on_window: true }, Motion { x: 2.0, y: 3.0 }, Down, Pressure(0.5), Up, Frame]);
        assert_eq!(buttons(&out), vec![(PointerButton::Primary, true), (PointerButton::Primary, false)]);
        assert_eq!(touches(&out), vec![(TouchPhase::Start, 0.5), (TouchPhase::End, 0.5)]);
    }

    #[test]
    fn leaving_proximity_while_down_releases_and_hides_the_pointer() {
        use ToolEvent::*;
        let mut m = ToolMapper::default();
        run(&mut m, &[ProximityIn { on_window: true }, Motion { x: 2.0, y: 3.0 }, Down, Frame]);
        let out = run(&mut m, &[ProximityOut, Frame]);
        assert_eq!(buttons(&out), vec![(PointerButton::Primary, false)]);
        assert_eq!(out.last(), Some(&Event::PointerGone));
        assert!(run(&mut m, &[Motion { x: 9.0, y: 9.0 }, Frame]).is_empty(), "out of proximity: nothing");
    }

    #[test]
    fn other_surfaces_are_ignored() {
        use ToolEvent::*;
        let mut m = ToolMapper::default();
        assert!(run(&mut m, &[ProximityIn { on_window: false }, Motion { x: 2.0, y: 3.0 }, Down, Frame]).is_empty());
    }

    #[test]
    fn stylus_buttons_are_right_and_middle_clicks() {
        use ToolEvent::*;
        let mut m = ToolMapper::default();
        run(&mut m, &[ProximityIn { on_window: true }, Motion { x: 2.0, y: 3.0 }, Frame]);
        let b = stylus_button(0x14b).unwrap();
        let out = run(&mut m, &[Button { button: b, pressed: true }, Frame, Button { button: b, pressed: false }, Frame]);
        assert_eq!(buttons(&out), vec![(PointerButton::Secondary, true), (PointerButton::Secondary, false)]);
        assert_eq!(stylus_button(0x14c), Some(PointerButton::Middle));
        assert_eq!(stylus_button(0x110), None);
    }

    #[test]
    fn tilt_is_kept() {
        let mut m = ToolMapper::default();
        run(&mut m, &[ToolEvent::Tilt { x: 30.0, y: -10.0 }]);
        assert_eq!(m.tilt(), [30.0, -10.0]);
    }

    #[test]
    fn finish_scales_positions_and_sets_modifiers() {
        let queued = vec![
            Event::PointerMoved(pos2(20.0, 40.0)),
            Event::PointerButton { pos: pos2(20.0, 40.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            pen_touch(TouchPhase::Move, pos2(f32::NAN, f32::NAN), 0.4),
        ];
        let out = finish_events(queued, 2.0, Modifiers::SHIFT, None);
        assert_eq!(out[0], Event::PointerMoved(pos2(10.0, 20.0)));
        assert_eq!(out[1], Event::PointerButton { pos: pos2(10.0, 20.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::SHIFT });
        match out[2] {
            Event::Touch { pos, force, .. } => {
                assert_eq!(pos, pos2(10.0, 20.0), "X11 sample takes the pointer position");
                assert_eq!(force, Some(0.4));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn valuator_lookup_follows_the_mask() {
        // Valuators 0, 1 and 2 set: the third value is pressure.
        assert_eq!(valuator(&[0b111], &[100.0, 200.0, 512.0], 2), Some(512.0));
        // Only valuator 2 set: it is the first value.
        assert_eq!(valuator(&[0b100], &[512.0], 2), Some(512.0));
        assert_eq!(valuator(&[0b011], &[1.0, 2.0], 2), None);
        assert_eq!(valuator(&[0, 1], &[7.0], 32), Some(7.0));
        assert_eq!(valuator(&[], &[], 0), None);
    }

    #[test]
    fn x11_pen_reports_pressure_of_tip_contact() {
        let axis = PressureAxis { number: 2, min: 0.0, max: 2048.0 };
        let mut pen = X11Pen::new(HashMap::from([(12, axis)]));
        let start = pen.raw(RawKind::Press, 12, 1, &[0b111], &[1.0, 1.0, 512.0]).unwrap();
        assert!(matches!(start, Event::Touch { phase: TouchPhase::Start, force: Some(f), .. } if f == 0.25));
        let mv = pen.raw(RawKind::Motion, 12, 0, &[0b100], &[1024.0]).unwrap();
        assert!(matches!(mv, Event::Touch { phase: TouchPhase::Move, force: Some(f), .. } if f == 0.5));
        assert!(pen.raw(RawKind::Motion, 12, 0, &[0b011], &[3.0, 4.0]).is_none(), "no pressure in the event");
        assert!(pen.raw(RawKind::Press, 12, 3, &[], &[]).is_none(), "barrel button is not the tip");
        let end = pen.raw(RawKind::Release, 12, 1, &[0b100], &[0.0]).unwrap();
        assert!(matches!(end, Event::Touch { phase: TouchPhase::End, .. }));
        assert!(pen.raw(RawKind::Motion, 12, 0, &[0b100], &[900.0]).is_none(), "hover: no contact");
    }

    #[test]
    fn x11_mouse_devices_are_ignored() {
        let mut pen = X11Pen::new(HashMap::from([(12, PressureAxis { number: 2, min: 0.0, max: 1.0 })]));
        assert!(pen.raw(RawKind::Press, 4, 1, &[0b11], &[1.0, 2.0]).is_none());
    }
}
