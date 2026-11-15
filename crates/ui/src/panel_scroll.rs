//! Painter's tablet scroll: Ctrl+Alt + left drag scrolls whatever panel is under the pointer, so a
//! pen user can scroll the layer stack, Properties or the Assets panel without a wheel.
//!
//! Runs on the raw input before egui sees it ([`PanelScroll::filter`] from `raw_input_hook`). The
//! press is held back until the pointer moves past [`DRAG_START`]: a short click is then handed on
//! as it was (so Ctrl+Alt+click still reaches widgets such as the mouse-shortcut capture pad), a
//! drag becomes wheel events in points while egui's pointer stays where the press was, so the area
//! that was under it keeps scrolling. The 3D and 2D views keep Ctrl+Alt+LMB as their pan: they
//! register their rectangle and layer every frame ([`PanelScroll::add_view`]) and a press on them
//! is left alone.

use egui::{Event, LayerId, Modifiers, MouseWheelUnit, PointerButton, Pos2, Rect, TouchPhase};

/// Pointer travel (points) that turns a held Ctrl+Alt press into a scroll.
pub const DRAG_START: f32 = 4.0;

#[derive(Default)]
pub struct PanelScroll {
    phase: Phase,
    /// Views drawn in the previous frame: their rectangle and layer.
    views: Vec<(Rect, LayerId)>,
    /// Views drawn so far in this frame.
    next_views: Vec<(Rect, LayerId)>,
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
enum Phase {
    #[default]
    Idle,
    /// Ctrl+Alt press held back: where it was, its modifiers and where the pointer is now.
    Held { press: Pos2, modifiers: Modifiers, now: Pos2 },
    /// Scrolling: the last pointer position.
    Scrolling { last: Pos2 },
}

impl PanelScroll {
    /// A view (3D or 2D) drawn this frame keeps Ctrl+Alt+LMB for its own navigation.
    pub fn add_view(&mut self, rect: Rect, layer: LayerId) {
        self.next_views.push((rect, layer));
    }

    /// Start of a frame: the views registered during the last one become the ones that block.
    pub fn begin_frame(&mut self) {
        self.views = std::mem::take(&mut self.next_views);
    }

    pub fn is_scrolling(&self) -> bool {
        matches!(self.phase, Phase::Scrolling { .. })
    }

    /// Whether a press at `pos` lands on a view. `layer_at` is egui's topmost layer at a position
    /// (a floating window over a view takes the press, not the view).
    fn on_view(&self, pos: Pos2, layer_at: &dyn Fn(Pos2) -> Option<LayerId>) -> bool {
        self.views.iter().any(|(rect, layer)| rect.contains(pos) && layer_at(pos) == Some(*layer))
    }

    /// Rewrite this frame's input events (see the module documentation).
    pub fn filter(&mut self, events: &mut Vec<Event>, layer_at: &dyn Fn(Pos2) -> Option<LayerId>) {
        let mut out = Vec::with_capacity(events.len());
        for event in events.drain(..) {
            match (self.phase, &event) {
                (Phase::Idle, Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers })
                    if modifiers.command && modifiers.alt && !self.on_view(*pos, layer_at) =>
                {
                    self.phase = Phase::Held { press: *pos, modifiers: *modifiers, now: *pos };
                }
                (Phase::Held { press, modifiers, .. }, Event::PointerMoved(pos)) => {
                    if press.distance(*pos) > DRAG_START {
                        out.push(wheel(*pos - press));
                        self.phase = Phase::Scrolling { last: *pos };
                    } else {
                        self.phase = Phase::Held { press, modifiers, now: *pos };
                    }
                }
                (Phase::Held { press, modifiers, now }, Event::PointerButton { button: PointerButton::Primary, pressed: false, .. }) => {
                    // A click: hand the held press on, then the release.
                    out.push(Event::PointerButton { pos: press, button: PointerButton::Primary, pressed: true, modifiers });
                    if now != press {
                        out.push(Event::PointerMoved(now));
                    }
                    out.push(event);
                    self.phase = Phase::Idle;
                }
                (Phase::Scrolling { last }, Event::PointerMoved(pos)) => {
                    out.push(wheel(*pos - last));
                    self.phase = Phase::Scrolling { last: *pos };
                }
                (Phase::Scrolling { .. }, Event::PointerButton { button: PointerButton::Primary, pressed: false, .. }) => {
                    self.phase = Phase::Idle;
                }
                (Phase::Held { .. } | Phase::Scrolling { .. }, Event::PointerGone | Event::WindowFocused(false)) => {
                    self.phase = Phase::Idle;
                    out.push(event);
                }
                _ => out.push(event),
            }
        }
        *events = out;
    }
}

/// Content follows the pen, like a touch drag. No modifiers: with Ctrl held egui would zoom.
fn wheel(delta: egui::Vec2) -> Event {
    Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase: TouchPhase::Move, modifiers: Modifiers::NONE }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Id, Order, pos2, vec2};

    const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };

    fn press(pos: Pos2, modifiers: Modifiers) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers }
    }

    fn release(pos: Pos2) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: CTRL_ALT }
    }

    fn view_layer() -> LayerId {
        LayerId::new(Order::Background, Id::new("view"))
    }

    fn window_layer() -> LayerId {
        LayerId::new(Order::Middle, Id::new("window"))
    }

    fn run_on(s: &mut PanelScroll, top: LayerId, mut events: Vec<Event>) -> Vec<Event> {
        s.filter(&mut events, &|_| Some(top));
        events
    }

    fn run(s: &mut PanelScroll, events: Vec<Event>) -> Vec<Event> {
        run_on(s, view_layer(), events)
    }

    fn wheels(events: &[Event]) -> Vec<egui::Vec2> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::MouseWheel { delta, modifiers, unit, .. } => {
                    assert_eq!((*modifiers, *unit), (Modifiers::NONE, MouseWheelUnit::Point));
                    Some(*delta)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_ctrl_alt_drag_over_a_panel_becomes_wheel_scrolling_and_the_press_never_reaches_widgets() {
        let mut s = PanelScroll::default();
        let out = run(&mut s, vec![press(pos2(10.0, 100.0), CTRL_ALT), Event::PointerMoved(pos2(10.0, 90.0))]);
        assert!(s.is_scrolling());
        assert_eq!(wheels(&out), vec![vec2(0.0, -10.0)], "dragging up scrolls the content up");
        assert!(!out.iter().any(|e| matches!(e, Event::PointerButton { .. } | Event::PointerMoved(_))), "{out:?}");
        // The next frame keeps scrolling from the last position; the release ends it unseen.
        let out = run(&mut s, vec![Event::PointerMoved(pos2(12.0, 60.0)), release(pos2(12.0, 60.0)), Event::PointerMoved(pos2(12.0, 50.0))]);
        assert_eq!(wheels(&out), vec![vec2(2.0, -30.0)]);
        assert!(!s.is_scrolling());
        assert!(matches!(out.as_slice(), [Event::MouseWheel { .. }, Event::PointerMoved(_)]), "{out:?}");
    }

    #[test]
    fn a_ctrl_alt_click_without_moving_is_handed_on_unchanged() {
        let mut s = PanelScroll::default();
        let out = run(&mut s, vec![press(pos2(10.0, 10.0), CTRL_ALT), Event::PointerMoved(pos2(11.0, 11.0))]);
        assert!(out.is_empty(), "held back until it is a click or a drag: {out:?}");
        let out = run(&mut s, vec![release(pos2(11.0, 11.0))]);
        assert_eq!(out, vec![press(pos2(10.0, 10.0), CTRL_ALT), Event::PointerMoved(pos2(11.0, 11.0)), release(pos2(11.0, 11.0))]);
        assert!(!s.is_scrolling());
    }

    #[test]
    fn plain_and_single_modifier_drags_are_untouched() {
        let mut s = PanelScroll::default();
        for m in [Modifiers::NONE, Modifiers::ALT, Modifiers::COMMAND, Modifiers::SHIFT] {
            let events = vec![press(pos2(0.0, 0.0), m), Event::PointerMoved(pos2(0.0, 50.0)), release(pos2(0.0, 50.0))];
            assert_eq!(run(&mut s, events.clone()), events, "{m:?}");
        }
    }

    #[test]
    fn the_3d_and_2d_views_keep_ctrl_alt_drag_as_their_pan_unless_a_window_covers_them() {
        let view = Rect::from_min_max(pos2(100.0, 0.0), pos2(500.0, 400.0));
        let on_view = || vec![press(pos2(200.0, 200.0), CTRL_ALT), Event::PointerMoved(pos2(200.0, 150.0))];
        let mut s = PanelScroll::default();
        s.add_view(view, view_layer());
        s.begin_frame();
        assert_eq!(run(&mut s, on_view()), on_view(), "a press on the view pans it");
        assert!(!s.is_scrolling());
        // A floating window over the view scrolls.
        assert_eq!(wheels(&run_on(&mut s, window_layer(), on_view())), vec![vec2(0.0, -50.0)]);
        run(&mut s, vec![release(pos2(200.0, 150.0))]);
        // Outside the view rectangle: a panel.
        let out = run(&mut s, vec![press(pos2(50.0, 200.0), CTRL_ALT), Event::PointerMoved(pos2(50.0, 220.0))]);
        assert_eq!(wheels(&out), vec![vec2(0.0, 20.0)]);
    }

    #[test]
    fn a_view_blocks_from_the_frame_after_it_was_drawn_until_a_frame_without_it() {
        let view = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        let drag = || vec![press(pos2(50.0, 50.0), CTRL_ALT), Event::PointerMoved(pos2(50.0, 80.0)), release(pos2(50.0, 80.0))];
        let mut s = PanelScroll::default();
        s.add_view(view, view_layer());
        assert_eq!(wheels(&run(&mut s, drag())).len(), 1, "registered this frame: not blocking yet");
        s.begin_frame();
        assert_eq!(run(&mut s, drag()), drag());
        s.begin_frame(); // a frame in which the view was hidden
        assert_eq!(wheels(&run(&mut s, drag())).len(), 1);
    }

    #[test]
    fn losing_the_pointer_or_focus_ends_the_scroll() {
        let mut s = PanelScroll::default();
        run(&mut s, vec![press(pos2(0.0, 0.0), CTRL_ALT), Event::PointerMoved(pos2(0.0, 30.0))]);
        assert!(s.is_scrolling());
        assert_eq!(run(&mut s, vec![Event::WindowFocused(false)]), vec![Event::WindowFocused(false)]);
        assert!(!s.is_scrolling());
        assert_eq!(run(&mut s, vec![Event::PointerMoved(pos2(0.0, 60.0))]), vec![Event::PointerMoved(pos2(0.0, 60.0))]);
    }
}
