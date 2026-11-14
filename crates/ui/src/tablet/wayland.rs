//! Wayland backend: `zwp_tablet_manager_v2` on winit's `wl_display`, dispatched on its own event
//! queue from a background thread (libwayland lets several threads read one connection).

use std::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use wayland_client::backend::Backend;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_callback, wl_compositor, wl_registry, wl_seat, wl_shm, wl_surface};
use wayland_client::{delegate_noop, event_created_child, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_dial_v2::ZwpTabletPadDialV2,
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
    zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
    zwp_tablet_v2::ZwpTabletV2,
};

use super::{stylus_button, Queue, ToolEvent, ToolMapper};

struct State {
    /// `wl_surface*` of the main window; pen events over other surfaces are ignored.
    window: usize,
    queue: Queue,
    ctx: egui::Context,
    mapper: ToolMapper,
    out: Vec<egui::Event>,
    cursor: Option<Cursor>,
    log: bool,
}

/// Arrow image shown while the pen hovers the window (with a tablet seat bound, the compositor
/// no longer draws the pointer cursor for the pen).
struct Cursor {
    _theme: wayland_cursor::CursorTheme,
    surface: wl_surface::WlSurface,
    hotspot: (i32, i32),
}

pub(super) fn start(display: usize, window: usize, queue: Queue, ctx: egui::Context) -> Result<Box<dyn FnOnce() + Send>, String> {
    // SAFETY: `display` is winit's live `wl_display*`; the backend does not take ownership, and
    // the returned stop function (called from `on_exit`, before winit closes the display) ends
    // the thread that uses it.
    #[allow(unsafe_code)]
    let backend = unsafe { Backend::from_foreign_display(display as *mut _) };
    let conn = Connection::from_backend(backend);
    let (globals, mut event_queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
    let qh = event_queue.handle();
    let manager: ZwpTabletManagerV2 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "the compositor has no tablet protocol (zwp_tablet_manager_v2)".to_string())?;
    let seats: Vec<wl_seat::WlSeat> = globals.contents().with_list(|list| {
        list.iter()
            .filter(|g| g.interface == wl_seat::WlSeat::interface().name)
            .map(|g| globals.registry().bind(g.name, g.version.min(5), &qh, ()))
            .collect()
    });
    if seats.is_empty() {
        return Err("no wl_seat".into());
    }
    for seat in &seats {
        manager.get_tablet_seat(seat, &qh, ());
    }
    let cursor = (|| {
        let compositor: wl_compositor::WlCompositor = globals.bind(&qh, 1..=4, ()).ok()?;
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).ok()?;
        let size = std::env::var("XCURSOR_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(24);
        let mut theme = wayland_cursor::CursorTheme::load(&conn, shm, size).ok()?;
        let name = if theme.get_cursor("default").is_some() { "default" } else { "left_ptr" };
        let surface = compositor.create_surface(&qh, ());
        let hotspot = {
            let image = &theme.get_cursor(name)?[0];
            surface.attach(Some(&**image), 0, 0);
            surface.commit();
            let (hx, hy) = image.hotspot();
            (hx as i32, hy as i32)
        };
        Some(Cursor { _theme: theme, surface, hotspot })
    })();
    let log = crate::pen_input::input_log_enabled();
    let mut state = State { window, queue, ctx, mapper: ToolMapper::default(), out: Vec::new(), cursor, log };
    log::info!(target: "subcraft::input", "Wayland tablet input active (zwp_tablet_manager_v2, {} seat(s))", seats.len());

    let stop = Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let thread_stop = stop.clone();
    std::thread::Builder::new()
        .name("subcraft-tablet".into())
        .spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                if let Err(e) = event_queue.blocking_dispatch(&mut state) {
                    log::warn!(target: "subcraft::input", "tablet input stopped: {e}");
                    break;
                }
            }
            drop(seats);
            drop(manager);
            let _ = done_tx.send(());
        })
        .map_err(|e| e.to_string())?;
    Ok(Box::new(move || {
        stop.store(true, Ordering::Release);
        // Wake the blocked dispatch with a round trip on our queue, then wait for the thread.
        conn.display().sync(&qh, ());
        let _ = conn.flush();
        let _ = done_rx.recv_timeout(Duration::from_millis(500));
    }))
}

impl State {
    fn tool(&mut self, tool: &ZwpTabletToolV2, event: zwp_tablet_tool_v2::Event) {
        use zwp_tablet_tool_v2::Event as E;
        let mapped = match event {
            E::ProximityIn { serial, surface, .. } => {
                let on_window = surface.id().as_ptr() as usize == self.window;
                if on_window && let Some(c) = &self.cursor {
                    tool.set_cursor(serial, Some(&c.surface), c.hotspot.0, c.hotspot.1);
                }
                ToolEvent::ProximityIn { on_window }
            }
            E::ProximityOut => ToolEvent::ProximityOut,
            E::Down { .. } => ToolEvent::Down,
            E::Up => ToolEvent::Up,
            E::Motion { x, y } => ToolEvent::Motion { x: x as f32, y: y as f32 },
            E::Pressure { pressure } => ToolEvent::Pressure(pressure as f32 / 65535.0),
            E::Tilt { tilt_x, tilt_y } => ToolEvent::Tilt { x: tilt_x as f32, y: tilt_y as f32 },
            E::Button { button, state, .. } => match stylus_button(button) {
                Some(b) => ToolEvent::Button { button: b, pressed: state == WEnum::Value(zwp_tablet_tool_v2::ButtonState::Pressed) },
                None => return,
            },
            E::Frame { .. } => ToolEvent::Frame,
            _ => return,
        };
        self.mapper.handle(mapped, &mut self.out);
        if mapped == ToolEvent::Frame && !self.out.is_empty() {
            if self.log {
                let t = self.mapper.tilt();
                log::info!(target: "subcraft::input", "tablet frame: {} event(s), tilt ({:.0},{:.0})", self.out.len(), t[0], t[1]);
            }
            self.queue.lock().unwrap_or_else(|e| e.into_inner()).append(&mut self.out);
            self.ctx.request_repaint();
        }
    }
}

impl Dispatch<ZwpTabletToolV2, ()> for State {
    fn event(state: &mut Self, tool: &ZwpTabletToolV2, event: zwp_tablet_tool_v2::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let zwp_tablet_tool_v2::Event::Removed = event {
            tool.destroy();
            return;
        }
        state.tool(tool, event);
    }
}

impl Dispatch<ZwpTabletSeatV2, ()> for State {
    fn event(_: &mut Self, _: &ZwpTabletSeatV2, _: zwp_tablet_seat_v2::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}

    event_created_child!(State, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadV2, ()> for State {
    fn event(_: &mut Self, _: &ZwpTabletPadV2, _: zwp_tablet_pad_v2::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}

    event_created_child!(State, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for State {
    fn event(_: &mut Self, _: &ZwpTabletPadGroupV2, _: zwp_tablet_pad_group_v2::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}

    event_created_child!(State, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
        zwp_tablet_pad_group_v2::EVT_DIAL_OPCODE => (ZwpTabletPadDialV2, ()),
    ]);
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_noop!(State: ZwpTabletManagerV2);
delegate_noop!(State: ignore ZwpTabletV2);
delegate_noop!(State: ignore ZwpTabletPadRingV2);
delegate_noop!(State: ignore ZwpTabletPadStripV2);
delegate_noop!(State: ignore ZwpTabletPadDialV2);
delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_callback::WlCallback);
