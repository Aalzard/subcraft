//! Linux display backend choice: Wayland (winit's default when `WAYLAND_DISPLAY` is set) or X11
//! through XWayland (issue #82).
//!
//! winit 0.30 has no `zwp_tablet_v2` support, so on a Wayland compositor that does not emulate
//! a mouse for tablet tools (Niri, some wlroots setups) a pen moves the compositor's cursor but
//! SubCraft never receives a single event: no clicks, no painting. Under XWayland the same pen is
//! an XInput pointer and works. `SUBCRAFT_X11=1` or `--x11` asks winit for the X11 backend; it
//! is only honoured when an X server is reachable (`DISPLAY` set), so the request can never
//! turn a working Wayland start into a failed one.
//!
//! winit 0.30 removed the `WINIT_UNIX_BACKEND` environment variable; this replaces it.

/// Environment variable that requests the X11 backend (`1`, `true`).
pub const ENV: &str = "SUBCRAFT_X11";

/// Outcome of the backend decision, logged to `startup.log`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Nothing requested: winit picks (Wayland when `WAYLAND_DISPLAY` is set, else X11).
    Default,
    /// X11 requested and an X server is reachable: force the X11 backend.
    ForceX11,
    /// X11 requested but `DISPLAY` is empty: ignored with a warning (XWayland is not running).
    NoXServer,
}

/// Whether `value` of [`ENV`] asks for X11.
pub fn env_requests_x11(value: Option<&str>) -> bool {
    value.is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Decide from the request (`--x11` or the environment variable) and the `DISPLAY` variable.
pub fn decide(requested: bool, display: Option<&str>) -> Decision {
    if !requested { return Decision::Default; }
    if display.is_some_and(|d| !d.is_empty()) { Decision::ForceX11 } else { Decision::NoXServer }
}

/// Decision for this process: `flag` is `--x11`.
pub fn current(flag: bool) -> Decision {
    let env = std::env::var(ENV).ok();
    decide(flag || env_requests_x11(env.as_deref()), std::env::var("DISPLAY").ok().as_deref())
}

/// Whether this process runs on a Wayland session (where pen tablets need the X11 workaround).
pub fn on_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

/// One line for the log explaining the decision and, on Wayland, the tablet workaround.
pub fn describe(decision: Decision) -> String {
    match decision {
        Decision::ForceX11 => "display backend: X11 requested (SUBCRAFT_X11 / --x11); running under XWayland".into(),
        Decision::NoXServer => "display backend: X11 requested but DISPLAY is not set (no X server / XWayland); using the default backend".into(),
        Decision::Default if on_wayland() => "display backend: Wayland. Pen tablets send no clicks on some Wayland compositors (winit has no tablet protocol support); start with SUBCRAFT_X11=1 or --x11 to run under XWayland instead".into(),
        Decision::Default => "display backend: default".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_value_parsing() {
        assert!(env_requests_x11(Some("1")));
        assert!(env_requests_x11(Some("true")));
        assert!(env_requests_x11(Some("TRUE")));
        assert!(!env_requests_x11(Some("0")));
        assert!(!env_requests_x11(Some("")));
        assert!(!env_requests_x11(None));
    }

    #[test]
    fn x11_is_only_forced_when_an_x_server_is_reachable() {
        assert_eq!(decide(false, Some(":0")), Decision::Default);
        assert_eq!(decide(false, None), Decision::Default);
        assert_eq!(decide(true, Some(":0")), Decision::ForceX11);
        assert_eq!(decide(true, Some(":1")), Decision::ForceX11);
        assert_eq!(decide(true, Some("")), Decision::NoXServer, "an empty DISPLAY is no X server");
        assert_eq!(decide(true, None), Decision::NoXServer);
    }

    #[test]
    fn descriptions_name_the_switch() {
        assert!(describe(Decision::ForceX11).contains("XWayland"));
        assert!(describe(Decision::NoXServer).contains("DISPLAY"));
    }
}
