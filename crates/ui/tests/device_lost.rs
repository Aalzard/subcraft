//! Issue #119: after the graphics device is lost (driver reset), no frame is drawn again, the
//! safe-mode marker is in the startup log at once, a recovery copy is saved, and the ending
//! (ask, restart in safe mode) runs exactly once. Headless: the loss is simulated through the
//! same entry point wgpu's device-lost callback uses. Global state, so one test in its own binary.

use std::cell::{Cell, RefCell};

use subcraft_app::gpu_loss::{self, Outcome, Shell};
use subcraft_app::safe_mode::{self, Reason};
use subcraft_app::startup_probe;
use subcraft_ui::device_lost::{self, Gate};

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<String>>,
}

impl Shell for Recorder {
    fn ask_restart(&self, text: &str) -> bool {
        self.calls.borrow_mut().push(format!("ask:{text}"));
        true
    }
    fn restart(&self) -> Result<(), String> {
        self.calls.borrow_mut().push("restart".into());
        Ok(())
    }
    fn tell(&self, text: &str) {
        self.calls.borrow_mut().push(format!("tell:{text}"));
    }
}

#[test]
fn a_lost_device_stops_every_frame_marks_the_next_start_safe_and_ends_once() {
    let dir = std::env::temp_dir().join(format!("subcraft-device-lost-{}", subcraft_core::ObjectId::new()));
    let log = dir.join("startup.log");
    startup_probe::begin(Some(&log));
    startup_probe::step(startup_probe::COMPLETE_MARKER);
    let read = || std::fs::read_to_string(&log).expect("startup log");

    let ctx = egui::Context::default();
    device_lost::install(&ctx, true);

    let shell = Recorder::default();
    let (drawn, saves) = (Cell::new(0u32), Cell::new(0u32));
    let outcomes = RefCell::new(Vec::new());
    // One frame of the application: the gate first, then (only if allowed) the drawing.
    let frame = || {
        let gate = device_lost::gate(&shell, || { saves.set(saves.get() + 1); true }, true, |o| outcomes.borrow_mut().push(o));
        if gate == Gate::Render {
            drawn.set(drawn.get() + 1);
        }
        gate
    };

    // A healthy device: frames are drawn, nothing is written, the next start would be normal.
    for _ in 0..3 {
        assert_eq!(frame(), Gate::Render);
    }
    assert_eq!((drawn.get(), saves.get()), (3, 0));
    assert!(shell.calls.borrow().is_empty());
    assert!(!startup_probe::previous_run_gpu_lost(&read()));
    assert_eq!(safe_mode::decide(&read(), false, false, None), None);
    assert!(gpu_loss::may_render() && subcraft_render::caps::device_lost().is_none());

    // The driver resets: wgpu's device-lost callback ends up here.
    subcraft_render::caps::report_device_lost("Unknown: Device is lost");

    // Before any frame runs, the marker is already on disk and the next start is a safe one.
    let text = read();
    assert!(text.contains(startup_probe::GPU_LOST_MARKER) && text.contains("Unknown: Device is lost"), "{text}");
    assert!(startup_probe::previous_run_gpu_lost(&text));
    assert_eq!(safe_mode::decide(&text, false, false, None), Some(Reason::GpuLostLastRun));
    assert_eq!(safe_mode::decide(&text, true, false, None), Some(Reason::GpuLostLastRun), "the automatic restart passes --safe-mode");
    assert!(!gpu_loss::may_render() && subcraft_render::caps::device_lost().is_some());
    assert!(ctx.has_requested_repaint(), "the UI is woken up to handle the loss even when idle");

    // No frame is drawn again, however often the loop turns; the ending runs once.
    for _ in 0..5 {
        assert_eq!(frame(), Gate::Stop);
    }
    assert_eq!(drawn.get(), 3, "nothing may be drawn on a lost device");
    assert_eq!(saves.get(), 1, "one recovery copy");
    assert_eq!(*outcomes.borrow(), vec![Outcome::Restarted]);
    {
        let calls = shell.calls.borrow();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert!(calls[0].starts_with("ask:") && calls[0].contains("Device is lost") && calls[0].contains("recovery copy"), "{}", calls[0]);
        assert_eq!(calls[1], "restart");
    }
    assert!(read().contains("restarting in safe mode"));

    // A second report (every device on the adapter reports the reset) changes nothing.
    subcraft_render::caps::report_device_lost("Unknown: again");
    assert_eq!(gpu_loss::reason().as_deref(), Some("Unknown: Device is lost"));
    assert_eq!(read().matches(startup_probe::GPU_LOST_MARKER).count(), 1);

    // The renderer's panic from the issue, had it come now, is recognised as the lost device
    // (no crash report) and does not start a second ending.
    let location = r"C:\Users\x\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\egui-wgpu-0.36.2\src\renderer.rs:984";
    assert!(gpu_loss::handle_panic(location, "Failed to create staging buffer for index data. Index count: 24534."));
    assert!(!gpu_loss::handle_panic(r"crates\ui\src\app.rs:1", "index out of bounds"), "our own bugs still produce a crash report");
    assert_eq!(outcomes.borrow().len(), 1);
    assert_eq!(shell.calls.borrow().len(), 2);

    let _ = std::fs::remove_dir_all(dir);
}
