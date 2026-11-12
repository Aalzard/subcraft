//! Self-healing safe mode: when the previous start never got as far as drawing a few frames
//! (see `startup_probe::previous_run_incomplete`), or the user asks for it, this run uses the
//! most conservative settings. The next normal start is automatic once a run completes, because
//! completion is what the log records; `--normal-mode` forces it right away.
use std::sync::OnceLock;

pub const ENV: &str = "SUBCRAFT_SAFE_MODE";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason { PreviousStartFailed, GpuLostLastRun, Requested }

impl Reason {
    pub fn text(&self) -> &'static str {
        match self {
            Self::PreviousStartFailed => "SubCraft did not finish starting last time, so this run uses the most conservative graphics settings.",
            Self::GpuLostLastRun => "The graphics card stopped responding last time (a driver reset), so this run uses the most conservative graphics settings, and GPU compositing and GPU baking stay off until you turn them back on in Preferences.",
            Self::Requested => "Safe mode was requested (--safe-mode or SUBCRAFT_SAFE_MODE), so this run uses the most conservative graphics settings.",
        }
    }
}

/// Pure decision. `--normal-mode` beats everything; an explicit request beats the automatic check.
pub fn decide(previous_log: &str, safe_flag: bool, normal_flag: bool, env: Option<&str>) -> Option<Reason> {
    if normal_flag { return None; }
    let env_on = env.is_some_and(|v| !v.is_empty() && v != "0");
    // A lost GPU last run is named as the reason even when safe mode was also asked for: that is
    // how SubCraft restarts itself after a driver reset (`gpu_loss::RESTART_ARG`).
    let gpu_lost = crate::startup_probe::previous_run_gpu_lost(previous_log);
    if safe_flag || env_on { return Some(if gpu_lost { Reason::GpuLostLastRun } else { Reason::Requested }); }
    if crate::startup_probe::previous_run_incomplete(previous_log) { return Some(Reason::PreviousStartFailed); }
    gpu_lost.then_some(Reason::GpuLostLastRun)
}

static ACTIVE: OnceLock<Reason> = OnceLock::new();

/// Decide once at start-up and remember. Safe mode turns off: GPU compositing, GPU baking,
/// 4x MSAA, path tracing, remembered window position and size.
pub fn init(previous_log: &str, safe_flag: bool, normal_flag: bool) -> Option<Reason> {
    let reason = decide(previous_log, safe_flag, normal_flag, std::env::var(ENV).ok().as_deref());
    if let Some(r) = &reason {
        let _ = ACTIVE.set(r.clone());
        crate::startup_probe::step(&format!("SAFE MODE: {}", r.text()));
        log::warn!("safe mode: {}", r.text());
    }
    reason
}
pub fn active() -> Option<&'static Reason> { ACTIVE.get() }
pub fn is_active() -> bool { ACTIVE.get().is_some() }

#[cfg(test)]
mod tests {
    use super::*;
    const OK: &str = "=== SubCraft start 1 pid 1 ===\nx\nstartup complete\n";
    const BAD: &str = "=== SubCraft start 1 pid 1 ===\nx\n";
    #[test]
    fn crashed_start_triggers_safe_mode_and_normal_flag_overrides() {
        assert_eq!(decide(BAD, false, false, None), Some(Reason::PreviousStartFailed));
        assert_eq!(decide(OK, false, false, None), None);
        assert_eq!(decide("", false, false, None), None);
        assert_eq!(decide(BAD, false, true, None), None);
        assert_eq!(decide(OK, true, false, None), Some(Reason::Requested));
        assert_eq!(decide(OK, false, false, Some("1")), Some(Reason::Requested));
        assert_eq!(decide(OK, false, false, Some("0")), None);
        assert_eq!(decide(OK, true, true, None), None);
    }

    #[test]
    fn a_lost_gpu_last_run_triggers_safe_mode_once_marked() {
        let lost = "=== SubCraft start 1 pid 1 ===
x
startup complete
12:00 GPU device lost: Unknown
";
        assert_eq!(decide(lost, false, false, None), Some(Reason::GpuLostLastRun));
        // Only the latest start counts: a later clean run clears it.
        let later = format!("{lost}=== SubCraft start 2 pid 2 ===
startup complete
");
        assert_eq!(decide(&later, false, false, None), None);
        assert_eq!(decide(lost, false, true, None), None);
        // The automatic restart after a driver reset passes --safe-mode: the reason shown (and
        // the GPU options turned off) are still those of the lost GPU.
        assert_eq!(decide(lost, true, false, None), Some(Reason::GpuLostLastRun));
        assert_eq!(decide(&later, true, false, None), Some(Reason::Requested));
    }
}
