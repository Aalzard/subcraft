//! Viewport navigation schemes and continuous mode takeover (issues #80, #81).
//!
//! Everything here is pure (no egui types) so the mapping and the takeover rules are unit-tested.
//! `viewport.rs` only feeds in the held buttons / modifiers and the pointer position.

use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Which mouse/modifier combinations navigate the 3D view. The wheel always zooms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavScheme {
    /// Alt+LMB or RMB orbit (Alt+Shift+LMB snaps every 90 degrees), MMB or Ctrl+Alt+LMB pan,
    /// Alt+RMB zoom; Shift+RMB rotates the environment (as in Painter), Shift+MMB turns only the
    /// key light. Ctrl+MMB (focus) and Ctrl+Alt+RMB (texture set) are clicks, not navigation.
    #[default]
    Painter,
    /// MMB orbit, Shift+MMB pan, Ctrl+MMB zoom; Shift+RMB rotates the environment.
    Blender,
}

impl NavScheme {
    pub const ALL: [NavScheme; 2] = [NavScheme::Painter, NavScheme::Blender];

    pub fn label(self) -> &'static str {
        match self {
            NavScheme::Painter => "Painter",
            NavScheme::Blender => "Blender",
        }
    }
    /// Text for the shortcuts window: (action, how).
    pub fn help(self) -> [(&'static str, &'static str); 4] {
        match self {
            NavScheme::Painter => [("Orbit", "Alt+LMB or RMB drag (add Shift to snap every 90°)"), ("Pan", "MMB or Ctrl+Alt+LMB drag"), ("Zoom", "Wheel or Alt+RMB drag"), ("Rotate environment", "Shift+RMB drag")],
            NavScheme::Blender => [("Orbit", "MMB drag (Alt+MMB click centers the view on the model)"), ("Pan", "Shift+MMB drag"), ("Zoom", "Wheel or Ctrl+MMB drag"), ("Rotate environment", "Shift+RMB drag")],
        }
    }
    /// Short orbit hint for the status bar.
    pub fn orbit_hint(self) -> &'static str {
        match self {
            NavScheme::Painter => "RMB orbit",
            NavScheme::Blender => "MMB orbit",
        }
    }
    /// Whether Alt is a navigation modifier (and so must not paint / open menus).
    pub fn alt_navigates(self) -> bool {
        self == NavScheme::Painter
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavMode {
    None,
    Orbit,
    /// Orbit that snaps the view every 90 degrees (Painter's Alt+Shift+LMB).
    OrbitSnap,
    Pan,
    Zoom,
    /// Turn the key light around the model.
    Light,
    /// Turn the environment (HDRI / sky) around the model.
    Environment,
}

/// Held mouse buttons and modifiers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavInput {
    pub primary: bool,
    pub middle: bool,
    pub secondary: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// The navigation mode an input state means under `scheme`.
pub fn mode_for(scheme: NavScheme, i: NavInput) -> NavMode {
    match scheme {
        NavScheme::Painter => {
            if i.primary && i.alt && i.shift && !i.ctrl {
                NavMode::OrbitSnap
            } else if i.shift && i.secondary {
                NavMode::Environment
            } else if i.shift && i.middle {
                NavMode::Light
            } else if i.secondary && i.alt && i.ctrl {
                // Ctrl+Alt+RMB selects the texture set under the cursor.
                NavMode::None
            } else if i.secondary && i.alt {
                NavMode::Zoom
            } else if i.middle && i.ctrl && !i.alt {
                // Ctrl+MMB focuses the depth of field on the point under the cursor.
                NavMode::None
            } else if i.middle || (i.primary && i.alt && i.ctrl) {
                NavMode::Pan
            } else if (i.secondary && !i.ctrl) || (i.primary && i.alt) {
                // Ctrl+RMB is the brush size / hardness drag, not navigation.
                NavMode::Orbit
            } else {
                NavMode::None
            }
        }
        NavScheme::Blender => {
            if i.shift && i.secondary && !i.middle {
                NavMode::Environment
            } else if !i.middle {
                NavMode::None
            } else if i.ctrl {
                NavMode::Zoom
            } else if i.shift {
                NavMode::Pan
            } else {
                NavMode::Orbit
            }
        }
    }
}

/// `angle` (radians) rounded to the nearest multiple of `step`.
pub fn snap_angle(angle: f32, step: f32) -> f32 {
    if step > 0.0 { (angle / step).round() * step } else { angle }
}

/// End point of a straight line from `from` towards `to` whose direction is snapped to
/// `step_deg` steps (Painter's Ctrl+Shift+click); the length is kept.
pub fn snap_line_end(from: Vec2, to: Vec2, step_deg: f32) -> Vec2 {
    let d = to - from;
    let len = d.length();
    if len <= f32::EPSILON {
        return to;
    }
    let a = snap_angle(d.y.atan2(d.x), step_deg.to_radians());
    from + Vec2::new(a.cos(), a.sin()) * len
}

/// A navigation drag in progress. The pointer position is the drag origin; when the mode changes
/// the origin is rebased to the current position, so the new mode starts from where the pointer
/// is now and the camera does not jump.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavDrag {
    mode: NavMode,
    origin: Vec2,
}

impl NavDrag {
    /// Start tracking at `pos`; `None` if the input is not a navigation combination.
    pub fn begin(scheme: NavScheme, input: NavInput, pos: Vec2) -> Option<Self> {
        let mode = mode_for(scheme, input);
        (mode != NavMode::None).then_some(Self { mode, origin: pos })
    }
    pub fn mode(&self) -> NavMode {
        self.mode
    }
    /// Advance one frame. Returns the mode to apply and the pointer movement since the last
    /// step in that mode (zero on the frame a takeover happens), or `None` when the drag ended
    /// (no navigation combination is held any more).
    pub fn step(&mut self, scheme: NavScheme, input: NavInput, pos: Vec2) -> Option<(NavMode, Vec2)> {
        let mode = mode_for(scheme, input);
        if mode == NavMode::None {
            return None;
        }
        let delta = if mode == self.mode { pos - self.origin } else { Vec2::ZERO };
        self.mode = mode;
        self.origin = pos;
        Some((mode, delta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inp(p: bool, m: bool, s: bool, shift: bool, ctrl: bool, alt: bool) -> NavInput {
        NavInput { primary: p, middle: m, secondary: s, shift, ctrl, alt }
    }
    const NONE: NavInput = NavInput { primary: false, middle: false, secondary: false, shift: false, ctrl: false, alt: false };

    #[test]
    fn painter_mapping() {
        let p = NavScheme::Painter;
        assert_eq!(mode_for(p, NONE), NavMode::None);
        assert_eq!(mode_for(p, inp(true, false, false, false, false, false)), NavMode::None, "plain LMB paints");
        assert_eq!(mode_for(p, inp(true, false, false, false, false, true)), NavMode::Orbit);
        assert_eq!(mode_for(p, inp(false, false, true, false, false, false)), NavMode::Orbit);
        assert_eq!(mode_for(p, inp(false, true, false, false, false, false)), NavMode::Pan);
        assert_eq!(mode_for(p, inp(false, true, false, false, false, true)), NavMode::Pan);
        assert_eq!(mode_for(p, inp(false, false, true, false, false, true)), NavMode::Zoom);
        assert_eq!(mode_for(p, inp(false, false, true, true, false, false)), NavMode::Environment, "Shift+RMB rotates the environment (Painter)");
        assert_eq!(mode_for(p, inp(false, true, false, true, false, false)), NavMode::Light, "Shift+MMB turns only the key light");
    }

    #[test]
    fn painter_ctrl_alt_lmb_pans_and_ctrl_rmb_is_left_for_the_brush() {
        let p = NavScheme::Painter;
        assert_eq!(mode_for(p, inp(true, false, false, false, true, true)), NavMode::Pan);
        assert_eq!(mode_for(p, inp(true, false, false, false, true, false)), NavMode::None, "Ctrl+LMB adjusts the brush");
        assert_eq!(mode_for(p, inp(false, false, true, false, true, false)), NavMode::None, "Ctrl+RMB adjusts the brush");
        assert_eq!(mode_for(p, inp(false, false, true, false, true, true)), NavMode::None, "Ctrl+Alt+RMB selects a texture set");
        assert_eq!(mode_for(p, inp(false, true, false, false, true, false)), NavMode::None, "Ctrl+MMB sets the focus");
        assert_eq!(mode_for(p, inp(false, true, false, false, true, true)), NavMode::Pan, "Alt+MMB pans whatever else is held");
    }

    #[test]
    fn painter_alt_shift_lmb_snaps_the_orbit() {
        let p = NavScheme::Painter;
        assert_eq!(mode_for(p, inp(true, false, false, true, false, true)), NavMode::OrbitSnap);
        assert_eq!(mode_for(p, inp(true, false, false, false, false, true)), NavMode::Orbit, "without Shift it orbits freely");
        assert_eq!(mode_for(p, inp(true, false, false, true, false, false)), NavMode::None, "Shift+LMB is the straight line");
        assert_eq!(mode_for(NavScheme::Blender, inp(true, false, false, true, false, true)), NavMode::None);
        // Releasing Shift mid-drag continues as a free orbit with no jump.
        let mut d = NavDrag::begin(p, inp(true, false, false, true, false, true), Vec2::ZERO).unwrap();
        assert_eq!(d.step(p, inp(true, false, false, true, false, true), Vec2::new(8.0, 0.0)), Some((NavMode::OrbitSnap, Vec2::new(8.0, 0.0))));
        assert_eq!(d.step(p, inp(true, false, false, false, false, true), Vec2::new(20.0, 0.0)), Some((NavMode::Orbit, Vec2::ZERO)));
    }

    #[test]
    fn straight_lines_snap_to_fifteen_degrees_and_keep_their_length() {
        let from = Vec2::new(100.0, 100.0);
        let end = snap_line_end(from, Vec2::new(200.0, 104.0), 15.0);
        assert!((end - Vec2::new(from.x + (end - from).length(), 100.0)).length() < 1e-3, "nearly horizontal becomes horizontal: {end}");
        assert!(((end - from).length() - Vec2::new(100.0, 4.0).length()).abs() < 1e-3);
        let diagonal = snap_line_end(from, Vec2::new(150.0, 148.0), 15.0) - from;
        assert!((diagonal.x - diagonal.y).abs() < 1e-3, "close to 45 degrees snaps to it: {diagonal}");
        let up = snap_line_end(from, Vec2::new(103.0, 20.0), 15.0) - from;
        assert!(up.x.abs() < 1e-3 && up.y < 0.0);
        assert_eq!(snap_line_end(from, from, 15.0), from, "a zero-length line stays put");
        assert!((snap_angle(0.30, 15f32.to_radians()) - 15f32.to_radians()).abs() < 1e-6);
        assert!((snap_angle(-1.60, std::f32::consts::FRAC_PI_2) + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn blender_mapping() {
        let b = NavScheme::Blender;
        assert_eq!(mode_for(b, NONE), NavMode::None);
        assert_eq!(mode_for(b, inp(false, true, false, false, false, false)), NavMode::Orbit);
        assert_eq!(mode_for(b, inp(false, true, false, true, false, false)), NavMode::Pan);
        assert_eq!(mode_for(b, inp(false, true, false, false, true, false)), NavMode::Zoom);
        assert_eq!(mode_for(b, inp(false, true, false, true, true, false)), NavMode::Zoom, "ctrl wins");
        // Painter's combinations do nothing here.
        assert_eq!(mode_for(b, inp(true, false, false, false, false, true)), NavMode::None);
        assert_eq!(mode_for(b, inp(false, false, true, false, false, false)), NavMode::None);
        assert_eq!(mode_for(b, inp(false, false, false, true, true, false)), NavMode::None, "modifiers alone");
        assert_eq!(mode_for(b, inp(false, false, true, true, false, false)), NavMode::Environment, "Shift+RMB rotates the environment");
    }

    #[test]
    fn same_mode_moves_by_pointer_delta() {
        let m = inp(false, true, false, false, false, false);
        let mut d = NavDrag::begin(NavScheme::Blender, m, Vec2::new(10.0, 10.0)).unwrap();
        assert_eq!(d.step(NavScheme::Blender, m, Vec2::new(14.0, 13.0)), Some((NavMode::Orbit, Vec2::new(4.0, 3.0))));
        assert_eq!(d.step(NavScheme::Blender, m, Vec2::new(14.0, 20.0)), Some((NavMode::Orbit, Vec2::new(0.0, 7.0))));
    }

    #[test]
    fn takeover_pan_to_zoom_has_no_jump() {
        let s = NavScheme::Blender;
        let pan = inp(false, true, false, true, false, false);
        let zoom = inp(false, true, false, true, true, false);
        let mut d = NavDrag::begin(s, pan, Vec2::ZERO).unwrap();
        assert_eq!(d.step(s, pan, Vec2::new(100.0, 0.0)), Some((NavMode::Pan, Vec2::new(100.0, 0.0))));
        // ctrl pressed while the pointer is far from where the drag began
        assert_eq!(d.step(s, zoom, Vec2::new(100.0, 5.0)), Some((NavMode::Zoom, Vec2::ZERO)));
        assert_eq!(d.mode(), NavMode::Zoom);
        // zoom continues from the takeover position, not from the original origin
        assert_eq!(d.step(s, zoom, Vec2::new(100.0, 12.0)), Some((NavMode::Zoom, Vec2::new(0.0, 7.0))));
        // release ctrl: back to pan, again with no jump
        assert_eq!(d.step(s, pan, Vec2::new(90.0, 12.0)), Some((NavMode::Pan, Vec2::ZERO)));
        assert_eq!(d.step(s, pan, Vec2::new(95.0, 12.0)), Some((NavMode::Pan, Vec2::new(5.0, 0.0))));
    }

    #[test]
    fn takeover_across_buttons_painter() {
        let s = NavScheme::Painter;
        let orbit = inp(false, false, true, false, false, false);
        let zoom = inp(false, false, true, false, false, true);
        let pan = inp(false, true, true, false, false, false);
        let mut d = NavDrag::begin(s, orbit, Vec2::ZERO).unwrap();
        d.step(s, orbit, Vec2::new(30.0, 30.0));
        assert_eq!(d.step(s, zoom, Vec2::new(50.0, 60.0)), Some((NavMode::Zoom, Vec2::ZERO)), "alt added mid orbit");
        assert_eq!(d.step(s, orbit, Vec2::new(55.0, 60.0)), Some((NavMode::Orbit, Vec2::ZERO)), "alt released");
        assert_eq!(d.step(s, pan, Vec2::new(70.0, 70.0)), Some((NavMode::Pan, Vec2::ZERO)), "MMB added");
        assert_eq!(d.step(s, pan, Vec2::new(71.0, 72.0)), Some((NavMode::Pan, Vec2::new(1.0, 2.0))));
    }

    #[test]
    fn drag_ends_when_nothing_navigates() {
        let s = NavScheme::Blender;
        let m = inp(false, true, false, false, false, false);
        let mut d = NavDrag::begin(s, m, Vec2::ZERO).unwrap();
        assert_eq!(d.step(s, NONE, Vec2::new(5.0, 5.0)), None);
        assert!(NavDrag::begin(s, NONE, Vec2::ZERO).is_none());
    }

    #[test]
    fn preferences_without_the_field_load_as_painter() {
        let p: crate::recovery::Preferences = serde_json::from_str(r#"{"ui_size_percent":100}"#).unwrap();
        assert_eq!(p.nav_scheme, NavScheme::Painter);
        let p: crate::recovery::Preferences = serde_json::from_str(r#"{"nav_scheme":"Blender"}"#).unwrap();
        assert_eq!(p.nav_scheme, NavScheme::Blender);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<crate::recovery::Preferences>(&json).unwrap(), p);
    }

    #[test]
    fn help_text_follows_the_scheme() {
        assert!(NavScheme::Blender.help().iter().any(|(_, h)| h.contains("Shift+MMB")));
        assert!(NavScheme::Painter.help().iter().any(|(_, h)| h.contains("Alt+LMB")));
        assert!(NavScheme::Painter.help().iter().any(|(_, h)| h.contains("snap every 90")));
        for scheme in NavScheme::ALL {
            assert!(scheme.help().contains(&("Rotate environment", "Shift+RMB drag")), "{scheme:?} lists the environment rotation");
        }
    }
}
