//! Painter's Pause engine computation (Shift+Esc): edits wait while paused and arrive on resume;
//! the full composite (export, save) still computes.

use std::time::{Duration, Instant};

use subcraft_app::{Action, Session};
use subcraft_mesh::primitives;

fn settle(s: &mut Session) -> usize {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut published = 0;
    loop {
        published += s.update_composites_within(Some(Duration::from_millis(5))).len();
        if !s.composites_pending() { return published; }
        assert!(Instant::now() < deadline, "composites never settled");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_paused_engine_holds_edits_until_resumed() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    settle(&mut s);
    assert!(!s.engine_paused());
    assert!(s.perform(Action::PauseEngine).is_ok());
    assert!(s.engine_paused(), "Shift+Esc pauses");
    // An edit while paused: nothing is computed or published, the work stays pending.
    s.perform(Action::NewFillLayer).unwrap();
    for _ in 0..20 {
        assert!(s.update_composites_within(Some(Duration::from_millis(5))).is_empty(), "nothing published while paused");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(s.composites_pending(), "the edit waits");
    // Resuming computes it.
    s.perform(Action::PauseEngine).unwrap();
    assert!(!s.engine_paused());
    settle(&mut s);
    assert!(!s.composites_pending(), "the waiting edit is computed after resuming");
    // The full composite (export and save use it) ignores the pause.
    s.set_engine_paused(true);
    s.perform(Action::NewFillLayer).unwrap();
    s.update_composites();
    let ts = s.selection().texture_set;
    assert!(s.composite(ts).is_some());
    s.set_engine_paused(false);
}
