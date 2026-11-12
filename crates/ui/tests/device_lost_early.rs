//! Issue #119: a device lost before the window finished starting (before the device-lost hook was
//! installed) must still stop drawing and end the run. Global state, so its own test binary.

use subcraft_app::gpu_loss;
use subcraft_ui::device_lost::{self, Gate};

struct Quiet;
impl gpu_loss::Shell for Quiet {
    fn ask_restart(&self, _: &str) -> bool { false }
    fn restart(&self) -> Result<(), String> { Ok(()) }
    fn tell(&self, _: &str) {}
}

#[test]
fn a_loss_reported_before_install_still_stops_the_frames() {
    let dir = std::env::temp_dir().join(format!("subcraft-device-lost-early-{}", std::process::id()));
    subcraft_app::startup_probe::begin(Some(&dir.join("startup.log")));
    // The device dies while the window is still being set up: no hook yet.
    subcraft_render::caps::report_device_lost("Unknown: Device is lost");
    assert!(gpu_loss::may_render(), "nothing has told the UI side yet");

    let ctx = egui::Context::default();
    device_lost::install(&ctx, false);
    assert!(!gpu_loss::may_render(), "install catches up with the loss");
    let mut ended = 0;
    assert_eq!(device_lost::gate(&Quiet, || false, false, |_| ended += 1), Gate::Stop);
    assert_eq!(device_lost::gate(&Quiet, || false, false, |_| ended += 1), Gate::Stop);
    assert_eq!(ended, 1, "the ending runs once");
    let _ = std::fs::remove_dir_all(dir);
}
