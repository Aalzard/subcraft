//! Pen pressure from egui input.
//!
//! On Windows, winit turns Windows Ink pointer messages (WM_POINTER*, pen and touch) into touch
//! events with a normalized force; egui-winit forwards them as `Event::Touch { force, .. }` and
//! also emulates the primary mouse button, so painting itself still follows the pointer. A plain
//! mouse sends no touch events and paints at full pressure. Tablets driven only through WinTab
//! (Windows Ink disabled in the driver) arrive as mouse input and report no pressure either.

use egui::{Event, TouchPhase};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stylus {
    /// Pressure of the pen currently in contact, if the device reports one.
    contact: Option<f32>,
    /// A pressure reading arrived at least once this session (shown in the Brush panel).
    pub seen: bool,
}

impl Stylus {
    /// Fold this frame's events in. The last reading of the frame wins.
    pub fn update(&mut self, events: &[Event]) {
        for e in events {
            let Event::Touch { phase, force, .. } = e else { continue };
            match phase {
                TouchPhase::Start => {
                    // Contact without a reading (touch screens, some pens) paints at full pressure.
                    self.contact = force.filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0));
                }
                TouchPhase::Move => {
                    // Hover updates and missing readings keep the last contact pressure.
                    if let Some(f) = force.filter(|f| f.is_finite()) {
                        self.contact = Some(f.clamp(0.0, 1.0));
                    }
                }
                TouchPhase::End | TouchPhase::Cancel => self.contact = None,
            }
            if force.is_some() {
                self.seen = true;
            }
        }
    }

    /// Pressure for the next stroke sample: the pen's reading, or 1.0 for mouse input.
    pub fn pressure(&self) -> f32 {
        self.contact.unwrap_or(1.0)
    }

    /// The live pen reading, if a pen is in contact.
    pub fn reading(&self) -> Option<f32> {
        self.contact
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(phase: TouchPhase, force: Option<f32>) -> Event {
        Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(7), phase, pos: egui::pos2(10.0, 10.0), force }
    }

    #[test]
    fn mouse_is_full_pressure_and_pen_contact_follows_force() {
        let mut s = Stylus::default();
        s.update(&[Event::PointerMoved(egui::pos2(1.0, 2.0))]);
        assert_eq!(s.pressure(), 1.0);
        assert!(!s.seen);
        s.update(&[touch(TouchPhase::Start, Some(0.2)), touch(TouchPhase::Move, Some(0.45))]);
        assert_eq!(s.pressure(), 0.45);
        assert!(s.seen);
        s.update(&[touch(TouchPhase::Move, None)]);
        assert_eq!(s.pressure(), 0.45, "a missing reading keeps the last pressure");
        s.update(&[touch(TouchPhase::Move, Some(3.0))]);
        assert_eq!(s.pressure(), 1.0);
        s.update(&[touch(TouchPhase::End, None)]);
        assert_eq!(s.pressure(), 1.0, "after lifting the pen, a mouse paints at full pressure");
        assert_eq!(s.reading(), None);
        s.update(&[touch(TouchPhase::Start, None)]);
        assert_eq!(s.pressure(), 1.0, "touch without force paints at full pressure");
    }
}
