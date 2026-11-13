//! Pen / touch contact to primary-button mapping, applied to the raw input before egui sees it.
//!
//! egui-winit already turns a touch into pointer events, but only for ONE touch id at a time
//! (`pointer_touch_id`): if a touch never reports `Ended` (a pen leaving proximity on some
//! Wayland / Xwayland setups) every later touch id is ignored and nothing can be clicked or
//! painted until restart. This layer watches the `Event::Touch` stream and, whenever a contact
//! starts, moves or ends without the matching pointer events, adds them: contact down is a
//! primary press, contact up a release, movement a pointer move. Real mouse input passes through
//! untouched, and nothing is added when egui-winit already did its job.

use egui::{Event, Modifiers, PointerButton, Pos2, TouchId, TouchPhase};

#[derive(Debug, Default)]
pub struct PenInputFixer {
    /// Contact currently driving the pointer.
    active: Option<TouchId>,
    /// Primary button state as seen in the (fixed) event stream.
    primary_down: bool,
    /// Number of events this layer synthesized (for the diagnostics log).
    pub synthesized: u64,
}

fn is_primary(e: &Event, pressed: bool) -> bool {
    matches!(e, Event::PointerButton { button: PointerButton::Primary, pressed: p, .. } if *p == pressed)
}

impl PenInputFixer {
    pub fn fix(&mut self, events: &mut Vec<Event>) {
        let input = std::mem::take(events);
        let mut out = Vec::with_capacity(input.len() + 2);
        for (i, event) in input.iter().enumerate() {
            // Events up to the next touch event belong to this one (egui-winit emits the pointer
            // events right after the touch event).
            let rest = || input[i + 1..].iter().take_while(|e| !matches!(e, Event::Touch { .. }));
            out.push(event.clone());
            match event {
                Event::PointerButton { button: PointerButton::Primary, pressed, .. } => self.primary_down = *pressed,
                // Native tablet touches carry pressure only; their pointer events are already there.
                Event::Touch { device_id, .. } if *device_id == crate::tablet::TABLET_DEVICE => {}
                Event::Touch { id, phase, pos, force, .. } => match phase {
                    TouchPhase::Start => {
                        let has_press = rest().any(|e| is_primary(e, true));
                        if !has_press && !self.primary_down {
                            self.press(&mut out, *pos);
                        }
                        // A new pen contact (it reports force) replaces one whose End was lost; a second
                        // finger does not take the pointer away.
                        if self.active.is_none() || (!has_press && force.is_some()) {
                            self.active = Some(*id);
                        }
                    }
                    TouchPhase::Move if self.active == Some(*id) => {
                        if !rest().any(|e| matches!(e, Event::PointerMoved(_))) {
                            out.push(Event::PointerMoved(*pos));
                            self.synthesized += 1;
                        }
                        if !self.primary_down && !rest().any(|e| is_primary(e, true)) {
                            self.press(&mut out, *pos);
                        }
                    }
                    TouchPhase::End | TouchPhase::Cancel if self.active == Some(*id) => {
                        self.active = None;
                        if self.primary_down && !rest().any(|e| is_primary(e, false)) {
                            out.push(Event::PointerButton { pos: *pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
                            self.primary_down = false;
                            self.synthesized += 1;
                            if !rest().any(|e| matches!(e, Event::PointerGone)) {
                                out.push(Event::PointerGone);
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        *events = out;
    }

    fn press(&mut self, out: &mut Vec<Event>, pos: Pos2) {
        out.push(Event::PointerMoved(pos));
        out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        self.primary_down = true;
        self.synthesized += 2;
    }
}

/// One-line description of a raw input event for the `SUBCRAFT_INPUT_LOG` diagnostics, or `None`
/// for events that are not pointer-related.
pub fn describe(event: &Event) -> Option<String> {
    Some(match event {
        Event::PointerMoved(p) => format!("PointerMoved ({:.1},{:.1})", p.x, p.y),
        Event::PointerButton { pos, button, pressed, .. } => format!("PointerButton {button:?} {} ({:.1},{:.1})", if *pressed { "down" } else { "up" }, pos.x, pos.y),
        Event::PointerGone => "PointerGone".to_owned(),
        Event::Touch { device_id, id, phase, pos, force } => format!("Touch dev={} id={} {phase:?} ({:.1},{:.1}) force={force:?}", device_id.0, id.0, pos.x, pos.y),
        Event::MouseWheel { delta, .. } => format!("MouseWheel {delta:?}"),
        _ => return None,
    })
}

pub fn input_log_enabled() -> bool {
    std::env::var_os("SUBCRAFT_INPUT_LOG").is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, TouchDeviceId};

    fn touch(id: u64, phase: TouchPhase, x: f32) -> Event {
        Event::Touch { device_id: TouchDeviceId(0), id: TouchId(id), phase, pos: pos2(x, 5.0), force: Some(0.5) }
    }
    fn press(x: f32) -> Event {
        Event::PointerButton { pos: pos2(x, 5.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }
    }
    fn release(x: f32) -> Event {
        Event::PointerButton { pos: pos2(x, 5.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }
    }
    fn presses(e: &[Event]) -> usize { e.iter().filter(|e| is_primary(e, true)).count() }
    fn releases(e: &[Event]) -> usize { e.iter().filter(|e| is_primary(e, false)).count() }

    #[test]
    fn bare_touch_sequence_becomes_press_move_release() {
        let mut f = PenInputFixer::default();
        let mut ev = vec![touch(1, TouchPhase::Start, 1.0)];
        f.fix(&mut ev);
        assert_eq!(presses(&ev), 1);
        assert!(ev.contains(&Event::PointerMoved(pos2(1.0, 5.0))));
        let mut ev = vec![touch(1, TouchPhase::Move, 4.0)];
        f.fix(&mut ev);
        assert!(ev.contains(&Event::PointerMoved(pos2(4.0, 5.0))));
        assert_eq!(presses(&ev) + releases(&ev), 0);
        let mut ev = vec![touch(1, TouchPhase::End, 4.0)];
        f.fix(&mut ev);
        assert_eq!(releases(&ev), 1);
        assert!(ev.contains(&Event::PointerGone));
    }

    #[test]
    fn nothing_added_when_egui_winit_already_emitted_pointer_events() {
        let mut f = PenInputFixer::default();
        let seq = vec![
            touch(1, TouchPhase::Start, 1.0), Event::PointerMoved(pos2(1.0, 5.0)), press(1.0),
            touch(1, TouchPhase::Move, 2.0), Event::PointerMoved(pos2(2.0, 5.0)),
            touch(1, TouchPhase::End, 2.0), release(2.0), Event::PointerGone,
        ];
        let mut ev = seq.clone();
        f.fix(&mut ev);
        assert_eq!(ev, seq);
        assert_eq!(f.synthesized, 0);
    }

    #[test]
    fn lost_end_does_not_block_the_next_contact() {
        // egui-winit ignores id 2 while it thinks id 1 is still down.
        let mut f = PenInputFixer::default();
        let mut ev = vec![touch(1, TouchPhase::Start, 1.0), Event::PointerMoved(pos2(1.0, 5.0)), press(1.0)];
        f.fix(&mut ev);
        let mut ev = vec![touch(2, TouchPhase::Start, 9.0)];
        f.fix(&mut ev);
        assert_eq!(presses(&ev), 0, "button is still down from contact 1, so id 2 just takes over the pointer");
        let mut ev = vec![touch(2, TouchPhase::Move, 10.0)];
        f.fix(&mut ev);
        assert!(ev.contains(&Event::PointerMoved(pos2(10.0, 5.0))));
        let mut ev = vec![touch(2, TouchPhase::End, 10.0)];
        f.fix(&mut ev);
        assert_eq!(releases(&ev), 1, "the stuck press is finally released");
    }

    #[test]
    fn cancel_releases_a_held_button() {
        let mut f = PenInputFixer::default();
        let mut ev = vec![touch(1, TouchPhase::Start, 1.0)];
        f.fix(&mut ev);
        let mut ev = vec![touch(1, TouchPhase::Cancel, 1.0)];
        f.fix(&mut ev);
        assert_eq!(releases(&ev), 1);
    }

    #[test]
    fn mouse_events_pass_through_unchanged() {
        let mut f = PenInputFixer::default();
        let seq = vec![Event::PointerMoved(pos2(3.0, 3.0)), press(3.0), Event::PointerMoved(pos2(8.0, 3.0)), release(8.0)];
        let mut ev = seq.clone();
        f.fix(&mut ev);
        assert_eq!(ev, seq);
    }

    #[test]
    fn second_finger_does_not_press_again() {
        let mut f = PenInputFixer::default();
        let mut ev = vec![touch(1, TouchPhase::Start, 1.0)];
        f.fix(&mut ev);
        let finger = |phase| Event::Touch { device_id: TouchDeviceId(0), id: TouchId(2), phase, pos: pos2(7.0, 5.0), force: None };
        let mut ev = vec![finger(TouchPhase::Start), finger(TouchPhase::End)];
        f.fix(&mut ev);
        assert_eq!(presses(&ev) + releases(&ev), 0, "only the first contact drives the pointer");
    }

    #[test]
    fn describe_formats_pointer_events() {
        assert!(describe(&touch(3, TouchPhase::Start, 1.0)).unwrap().contains("force=Some(0.5)"));
        assert!(describe(&Event::Copy).is_none());
    }
}
