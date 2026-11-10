//! Crash-safe GPU startup (issue #83): a breadcrumb log written before any graphics call, and a
//! probe that tries each backend / adapter in a short-lived child process so that a driver
//! crash kills only the child. No wgpu here; the child's wgpu work lives in `subcraft-ui`.
//!
//! Child protocol (stdout, one record per line, tab separated):
//! * `--gpu-probe <backend>`: `ADAPTER\t<index>\t<vendor>\t<device>\t<type>\t<driver>\t<name>` lines, then `DONE`.
//! * `--gpu-probe <backend> <index>`: `OK` or `FAIL\t<reason>`.
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const SKIP_ENV: &str = "SUBCRAFT_SKIP_GPU_PROBE";
pub const REPROBE_ENV: &str = "SUBCRAFT_GPU_REPROBE";
/// Run the probe on non-Windows systems too (used to test the mechanism).
pub const FORCE_ENV: &str = "SUBCRAFT_FORCE_GPU_PROBE";
pub const COMPLETE_MARKER: &str = "startup complete";
const START_MARKER: &str = "=== SubCraft start";

// ---------------------------------------------------------------- breadcrumb log

static PREVIOUS: OnceLock<String> = OnceLock::new();
static LOG: OnceLock<Mutex<Option<(std::fs::File, PathBuf)>>> = OnceLock::new();

/// `<data dir>/startup.log`.
pub fn startup_log_path() -> Option<PathBuf> { crate::recovery::data_dir().map(|d| d.join("startup.log")) }

/// Open the breadcrumb log and install a panic hook that writes to it. Returns the text of the
/// previous run's log (also kept for [`previous_log`]). The log keeps the last runs
/// (trimmed to about 64 KiB) and every step is flushed and synced to disk.
pub fn begin(path: Option<&Path>) {
    let previous = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let keep = trim_log(&previous, 64 * 1024);
    let open = |p: &Path| {
        std::fs::create_dir_all(p.parent()?).ok()?;
        std::fs::write(p, &keep).ok()?;
        std::fs::OpenOptions::new().append(true).open(p).ok().map(|f| (f, p.to_path_buf()))
    };
    // A read-only or unavailable data folder (locked-down PC, broken profile redirect) must not
    // mean "no log": fall back to the temp folder so there is always a startup.log somewhere.
    let fallback = std::env::temp_dir().join("SubCraft-startup.log");
    let file = path.and_then(open).or_else(|| open(&fallback));
    let _ = LOG.set(Mutex::new(file));
    let _ = PREVIOUS.set(previous);
    step(&format!("{START_MARKER} {} pid {} ===", env!("CARGO_PKG_VERSION"), std::process::id()));
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |p| {
        let location = p.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let message = p.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| p.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        step(&format!("PANIC at {location}: {message}"));
        previous_hook(p);
    }));
}

/// The log text from before this run (empty on a first start).
pub fn previous_log() -> &'static str { PREVIOUS.get().map(String::as_str).unwrap_or("") }

/// Append one line (with a time stamp), flush and sync. Safe to call from any thread, before or
/// after logging is set up; does nothing if the log could not be opened.
pub fn step(message: &str) {
    let Some(cell) = LOG.get() else { return };
    let Ok(mut guard) = cell.lock() else { return };
    if let Some((file, _)) = guard.as_mut() {
        let _ = writeln!(file, "{} {message}", crate::diagnostics::timestamp());
        let _ = file.flush();
        let _ = file.sync_data();
    }
}

/// Keep at most `max` bytes, cut at a line start.
pub fn trim_log(text: &str, max: usize) -> String {
    if text.len() <= max { return text.to_string(); }
    let mut cut = text.len() - max;
    while !text.is_char_boundary(cut) { cut += 1; }
    match text[cut..].find('\n') { Some(n) => text[cut + n + 1..].to_string(), None => String::new() }
}

/// True when the last run in `log` never reached [`COMPLETE_MARKER`] (it died during startup,
/// or was killed before the window appeared). An empty log is not a crash.
pub fn previous_run_incomplete(log: &str) -> bool {
    let Some(start) = log.rfind(START_MARKER) else { return false };
    !log[start..].contains(COMPLETE_MARKER)
}

/// Marker written (through [`step`]) when the GPU device is lost: driver reset, TDR, GPU removed.
pub const GPU_LOST_MARKER: &str = "GPU device lost";

/// Whether the GPU device was lost during the last recorded start (issue #119: an integrated GPU
/// resetting a few seconds after start-up, every start).
pub fn previous_run_gpu_lost(log: &str) -> bool {
    let Some(start) = log.rfind(START_MARKER) else { return false };
    log[start..].contains(GPU_LOST_MARKER)
}

// ---------------------------------------------------------------- pure model

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeBackend { Dx12, Vulkan, Gl }
impl ProbeBackend {
    /// Probe order: DirectX 12, Vulkan, OpenGL.
    pub const ALL: [ProbeBackend; 3] = [Self::Dx12, Self::Vulkan, Self::Gl];
    pub fn key(self) -> &'static str { match self { Self::Dx12 => "dx12", Self::Vulkan => "vulkan", Self::Gl => "gl" } }
    pub fn parse(s: &str) -> Option<Self> { Self::ALL.into_iter().find(|b| b.key() == s) }
    pub fn label(self) -> &'static str { match self { Self::Dx12 => "DirectX 12", Self::Vulkan => "Vulkan", Self::Gl => "OpenGL" } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind { Discrete, Integrated, Virtual, Other, Cpu }
impl AdapterKind {
    pub fn key(self) -> &'static str { match self { Self::Discrete => "discrete", Self::Integrated => "integrated", Self::Virtual => "virtual", Self::Other => "other", Self::Cpu => "cpu" } }
    pub fn parse(s: &str) -> Option<Self> { [Self::Discrete, Self::Integrated, Self::Virtual, Self::Other, Self::Cpu].into_iter().find(|k| k.key() == s) }
}

/// One adapter as reported by the enumeration child.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProbeAdapter {
    pub backend: ProbeBackend,
    /// Position in the child's enumeration (only meaningful together with the identity below).
    pub index: u32,
    pub vendor: u32,
    pub device: u32,
    pub kind: AdapterKind,
    pub driver: String,
    pub name: String,
}
impl ProbeAdapter {
    /// Identity used for the cache. Includes the driver text, so a driver update (or a different
    /// GPU) is a different adapter and never reuses an old verdict.
    pub fn same_gpu(&self, other: &Self) -> bool {
        self.backend == other.backend && self.vendor == other.vendor && self.device == other.device && self.name == other.name && self.driver == other.driver
    }
}

const VENDOR_NVIDIA: u32 = 0x10DE;
const VENDOR_AMD: u32 = 0x1002;

fn rank(a: &ProbeAdapter) -> (u8, u8, u8) {
    let kind = match a.kind { AdapterKind::Discrete => 0, AdapterKind::Integrated => 1, AdapterKind::Virtual => 2, AdapterKind::Other => 3, AdapterKind::Cpu => 4 };
    let vendor = match a.vendor { VENDOR_NVIDIA => 0, VENDOR_AMD => 1, _ => 2 };
    let backend = ProbeBackend::ALL.iter().position(|b| *b == a.backend).unwrap_or(9) as u8;
    (kind, vendor, backend)
}

/// Probe order across all backends: discrete before integrated before software, NVIDIA then AMD
/// then others, then DX12 -> Vulkan -> GL. `avoid` (adapters that were in use when the last
/// startup died) go last but are never dropped, so a single-GPU machine still has an option.
pub fn order_candidates(adapters: &[ProbeAdapter], avoid: &[ProbeAdapter]) -> Vec<ProbeAdapter> {
    let mut v = adapters.to_vec();
    v.sort_by_key(|a| (avoid.iter().any(|b| b.same_gpu(a)), rank(a), a.index));
    v
}

pub fn parse_enumeration(backend: ProbeBackend, stdout: &str) -> Result<Vec<ProbeAdapter>, String> {
    let mut out = Vec::new();
    let mut done = false;
    for line in stdout.lines().map(str::trim_end) {
        let f: Vec<&str> = line.splitn(7, '\t').collect();
        match f.as_slice() {
            ["ADAPTER", i, v, d, k, driver, name] => out.push(ProbeAdapter {
                backend,
                index: i.parse().map_err(|_| format!("bad index in {line:?}"))?,
                vendor: v.parse().map_err(|_| format!("bad vendor in {line:?}"))?,
                device: d.parse().map_err(|_| format!("bad device in {line:?}"))?,
                kind: AdapterKind::parse(k).ok_or_else(|| format!("bad type in {line:?}"))?,
                driver: driver.to_string(),
                name: name.to_string(),
            }),
            ["DONE"] => done = true,
            _ => {}
        }
    }
    if done { Ok(out) } else { Err("enumeration did not finish".into()) }
}

pub fn parse_device_result(stdout: &str) -> Result<(), String> {
    for line in stdout.lines().map(str::trim_end) {
        if line == "OK" { return Ok(()); }
        if let Some(reason) = line.strip_prefix("FAIL\t") { return Err(reason.to_string()); }
    }
    Err("no result was printed".into())
}

/// One protocol line for the child.
pub fn adapter_line(a: &ProbeAdapter) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    format!("ADAPTER\t{}\t{}\t{}\t{}\t{}\t{}", a.index, a.vendor, a.device, a.kind.key(), clean(&a.driver), clean(&a.name))
}

// ---------------------------------------------------------------- cache

/// Verified combinations, best first. Stored next to `graphics.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeCache { pub schema: u32, pub app_version: String, pub verified: Vec<ProbeAdapter> }

impl ProbeCache {
    pub fn path() -> Option<PathBuf> { crate::recovery::data_dir().map(|d| d.join("gpu-probe.json")) }
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("no parent folder")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let tmp = parent.join("gpu-probe.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
    /// The cache is trusted only for the same app version, when the last startup finished, and
    /// when the user did not ask for a fresh probe. (A driver update changes the adapter's
    /// driver text; that only shows up after enumeration, so it is compared in
    /// [`Self::confirm_active`].)
    pub fn is_valid(&self, app_version: &str, previous_incomplete: bool, reprobe: bool) -> bool {
        self.schema == 1 && self.app_version == app_version && !previous_incomplete && !reprobe && !self.verified.is_empty()
    }
    /// First verified adapter whose backend is allowed.
    pub fn choice(&self, allowed: &[ProbeBackend]) -> Option<&ProbeAdapter> {
        self.verified.iter().find(|a| allowed.contains(&a.backend))
    }
    /// Add a freshly verified adapter (after any already verified, so earlier = preferred).
    pub fn add(&mut self, app_version: &str, adapter: ProbeAdapter) {
        if self.app_version != app_version || self.schema != 1 { self.verified.clear(); }
        self.schema = 1;
        self.app_version = app_version.to_string();
        if !self.verified.iter().any(|a| a.same_gpu(&adapter)) { self.verified.push(adapter); }
    }
    /// True when the adapter in use is not the cached one (driver update, GPU swap): the cache
    /// should then be dropped.
    pub fn confirm_active(&self, active: &ProbeAdapter) -> bool { self.verified.iter().any(|a| a.same_gpu(active)) }
}

// ---------------------------------------------------------------- child processes

#[derive(Debug, PartialEq, Eq)]
pub enum ChildOutcome {
    Finished { stdout: String },
    /// Non-zero exit or killed by a signal / exception (Windows: e.g. 0xC0000005).
    Failed { code: Option<i32>, stdout: String, stderr: String },
    TimedOut,
    SpawnFailed(String),
}

fn drain(mut r: impl Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut r).take(1 << 20).read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// Run `command` with piped output, killing it after `timeout`.
pub fn run_child(mut command: Command, timeout: Duration) -> ChildOutcome {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match command.spawn() { Ok(c) => c, Err(e) => return ChildOutcome::SpawnFailed(e.to_string()) };
    let out = drain(child.stdout.take().expect("piped"));
    let err = drain(child.stderr.take().expect("piped"));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return ChildOutcome::TimedOut;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return ChildOutcome::SpawnFailed(e.to_string()),
        }
    };
    let (stdout, stderr) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    if status.success() { ChildOutcome::Finished { stdout } } else { ChildOutcome::Failed { code: status.code(), stdout, stderr } }
}

fn describe(o: &ChildOutcome) -> String {
    match o {
        ChildOutcome::Finished { .. } => "finished".into(),
        ChildOutcome::Failed { code, stderr, .. } => format!(
            "crashed or exited with {}{}",
            code.map(|c| format!("code {c} (0x{:X})", c as u32)).unwrap_or_else(|| "a signal".into()),
            stderr.lines().rev().find(|l| !l.trim().is_empty()).map(|l| format!("; last message: {}", l.trim())).unwrap_or_default()),
        ChildOutcome::TimedOut => "did not answer in time and was stopped".into(),
        ChildOutcome::SpawnFailed(e) => format!("could not be started: {e}"),
    }
}

/// How the probe runs; injectable so tests do not need the real executable.
pub struct ProbeRunner<'a> {
    /// Build the child command for `--gpu-probe <backend> [<index>]`.
    pub command: &'a dyn Fn(ProbeBackend, Option<u32>) -> Command,
    pub enumerate_timeout: Duration,
    pub device_timeout: Duration,
}

/// The real runner: this executable with `--gpu-probe`.
pub fn exe_command(backend: ProbeBackend, index: Option<u32>) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap_or_else(|_| "subcraft".into()));
    c.arg("--gpu-probe").arg(backend.key());
    if let Some(i) = index { c.arg(i.to_string()); }
    c
}

#[derive(Debug, PartialEq)]
pub enum ProbeResult {
    /// A verified adapter. `fresh` is true when a probe ran (the cache was rewritten).
    Verified { adapter: ProbeAdapter, fresh: bool },
    /// Probe not used (skipped by the user, or this platform does not need it).
    NotUsed,
    /// Every backend / adapter tried failed.
    AllFailed { report: Vec<String> },
}

/// Probe `allowed` backends in order, each adapter best-first, and return the first that
/// enumerates and creates a device in a child process. Every step is written to the breadcrumb.
pub fn probe(runner: &ProbeRunner, allowed: &[ProbeBackend], avoid: &[ProbeAdapter]) -> Result<ProbeAdapter, Vec<String>> {
    let mut report = Vec::new();
    let mut found = Vec::new();
    for backend in ProbeBackend::ALL.into_iter().filter(|b| allowed.contains(b)) {
        step(&format!("probe: enumerating {} adapters in a child process", backend.label()));
        let outcome = run_child((runner.command)(backend, None), runner.enumerate_timeout);
        let listed = match &outcome {
            ChildOutcome::Finished { stdout } => parse_enumeration(backend, stdout),
            other => Err(describe(other)),
        };
        match listed {
            Ok(list) => { step(&format!("probe: {} lists {} adapter(s)", backend.label(), list.len())); found.extend(list); }
            Err(e) => { step(&format!("probe: {} unusable: {e}", backend.label())); report.push(format!("{}: {e}", backend.label())); }
        }
    }
    // Adapters are tried best-first across backends, so an NVIDIA GPU on DX12 beats an Intel one.
    for adapter in order_candidates(&found, avoid) {
        step(&format!("probe: creating a device on {} / {} ({})", adapter.backend.label(), adapter.name, adapter.kind.key()));
        let outcome = run_child((runner.command)(adapter.backend, Some(adapter.index)), runner.device_timeout);
        let verdict = match &outcome {
            ChildOutcome::Finished { stdout } => parse_device_result(stdout),
            other => Err(describe(other)),
        };
        match verdict {
            Ok(()) => { step(&format!("probe: {} / {} works", adapter.backend.label(), adapter.name)); return Ok(adapter); }
            Err(e) => {
                step(&format!("probe: {} / {} unusable: {e}", adapter.backend.label(), adapter.name));
                report.push(format!("{} / {}: {e}", adapter.backend.label(), adapter.name));
            }
        }
    }
    Err(report)
}

/// Decide the adapter for this run: cached choice, else a probe. `previous_log` is the text
/// returned by [`begin`].
pub fn select(runner: &ProbeRunner, allowed: &[ProbeBackend], previous_log: &str, cache_path: Option<&Path>) -> ProbeResult {
    let version = env!("CARGO_PKG_VERSION");
    let incomplete = previous_run_incomplete(previous_log);
    let reprobe = std::env::var_os(REPROBE_ENV).is_some_and(|v| !v.is_empty() && v != "0");
    let mut cache = cache_path.map(ProbeCache::load).unwrap_or_default();
    let mut avoid = Vec::new();
    if incomplete {
        step("previous start did not finish; re-checking graphics");
        avoid.extend(cache.choice(allowed).cloned());
    }
    if cache.is_valid(version, incomplete, reprobe)
        && let Some(a) = cache.choice(allowed)
    {
        step(&format!("graphics: using verified {} / {} from cache", a.backend.label(), a.name));
        return ProbeResult::Verified { adapter: a.clone(), fresh: false };
    }
    if incomplete || reprobe { cache.verified.retain(|a| !avoid.iter().any(|b| b.same_gpu(a))); }
    match probe(runner, allowed, &avoid) {
        Ok(adapter) => {
            cache.add(version, adapter.clone());
            if let Some(p) = cache_path && let Err(e) = cache.save(p) { step(&format!("could not save gpu-probe.json: {e}")); }
            ProbeResult::Verified { adapter, fresh: true }
        }
        Err(report) => ProbeResult::AllFailed { report },
    }
}

/// Should the probe run in this process at all?
pub fn enabled(environment_backend_override: bool) -> bool {
    if environment_backend_override { return false; }
    if std::env::var_os(SKIP_ENV).is_some_and(|v| !v.is_empty() && v != "0") { return false; }
    cfg!(windows) || std::env::var_os(FORCE_ENV).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Text for the message box when nothing works.
pub fn failure_text(report: &[String], log: Option<&Path>) -> String {
    let mut t = String::from("SubCraft could not find a working graphics adapter.\n\n");
    if report.is_empty() { t.push_str("Windows reported no graphics adapters that SubCraft can use.\n"); }
    else {
        t.push_str("Every graphics option was tried in a safe test process and each one crashed, hung or failed:\n");
        for line in report.iter().take(8) { t.push_str(&format!("  - {line}\n")); }
    }
    match log {
        Some(p) => t.push_str(&format!("\nThe startup log is here:\n{}\n", p.display())),
        None => t.push_str("\nThe startup log could not be written.\n"),
    }
    t.push_str("\nPlease update your graphics drivers. If it keeps happening, report it at https://github.com/Aalzard/subcraft/issues/83 and attach startup.log.");
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ad(backend: ProbeBackend, index: u32, vendor: u32, kind: AdapterKind, name: &str) -> ProbeAdapter {
        ProbeAdapter { backend, index, vendor, device: 1, kind, driver: "1.0".into(), name: name.into() }
    }

    #[test]
    fn discrete_nvidia_first_then_backend_order() {
        use {AdapterKind::*, ProbeBackend::*};
        let list = [
            ad(Dx12, 0, 0x8086, Integrated, "Intel UHD"), ad(Dx12, 1, VENDOR_NVIDIA, Discrete, "RTX 2080 Super"),
            ad(Vulkan, 0, VENDOR_NVIDIA, Discrete, "RTX 2080 Super"), ad(Gl, 0, 0, Cpu, "llvmpipe"), ad(Vulkan, 1, 0x8086, Integrated, "Intel UHD"),
            ad(Dx12, 2, VENDOR_AMD, Discrete, "Radeon"),
        ];
        let names: Vec<_> = order_candidates(&list, &[]).iter().map(|a| (a.backend, a.name.clone())).collect();
        assert_eq!(names, vec![(Dx12, "RTX 2080 Super".into()), (Vulkan, "RTX 2080 Super".into()), (Dx12, "Radeon".into()),
            (Dx12, "Intel UHD".into()), (Vulkan, "Intel UHD".into()), (Gl, "llvmpipe".into())]);
    }

    #[test]
    fn avoided_adapters_go_last_but_stay() {
        use {AdapterKind::*, ProbeBackend::*};
        let nv = ad(Dx12, 1, VENDOR_NVIDIA, Discrete, "RTX");
        let intel = ad(Dx12, 0, 0x8086, Integrated, "Intel");
        let order = order_candidates(&[intel.clone(), nv.clone()], std::slice::from_ref(&nv));
        assert_eq!(order, vec![intel, nv.clone()]);
        assert_eq!(order_candidates(std::slice::from_ref(&nv), std::slice::from_ref(&nv)), vec![nv]);
    }

    #[test]
    fn enumeration_round_trip_and_garbage() {
        let a = ad(ProbeBackend::Vulkan, 3, VENDOR_NVIDIA, AdapterKind::Discrete, "NVIDIA GeForce\tRTX");
        let text = format!("noise line\n{}\nDONE\n", adapter_line(&a));
        let parsed = parse_enumeration(ProbeBackend::Vulkan, &text).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "NVIDIA GeForce RTX");
        assert_eq!((parsed[0].index, parsed[0].vendor, parsed[0].kind), (3, VENDOR_NVIDIA, AdapterKind::Discrete));
        // A child that died mid-list never prints DONE.
        assert!(parse_enumeration(ProbeBackend::Gl, &adapter_line(&a)).is_err());
        assert!(parse_enumeration(ProbeBackend::Gl, "ADAPTER\tx\t1\t1\tdiscrete\td\tn\nDONE").is_err());
        assert_eq!(parse_enumeration(ProbeBackend::Gl, "DONE").unwrap(), vec![]);
    }

    #[test]
    fn device_result_parsing() {
        assert!(parse_device_result("OK\n").is_ok());
        assert_eq!(parse_device_result("x\nFAIL\tno device\n"), Err("no device".into()));
        assert!(parse_device_result("").is_err());
    }

    #[test]
    fn cache_invalidation() {
        let a = ad(ProbeBackend::Dx12, 0, VENDOR_NVIDIA, AdapterKind::Discrete, "RTX");
        let mut c = ProbeCache::default();
        assert!(!c.is_valid("1", false, false), "empty cache");
        c.add("1", a.clone());
        assert!(c.is_valid("1", false, false));
        assert!(!c.is_valid("2", false, false), "new app version");
        assert!(!c.is_valid("1", true, false), "last start did not finish");
        assert!(!c.is_valid("1", false, true), "re-probe requested");
        assert!(c.choice(&[ProbeBackend::Gl]).is_none());
        assert_eq!(c.choice(&[ProbeBackend::Dx12]), Some(&a));
        let newer = ProbeAdapter { driver: "2.0".into(), ..a.clone() };
        assert!(!c.confirm_active(&newer), "driver update is a different adapter");
        c.add("2", newer.clone());
        assert_eq!(c.verified, vec![newer], "a version change drops old verdicts");
    }

    #[test]
    fn cache_file_round_trip_and_corruption() {
        let dir = std::env::temp_dir().join(format!("subcraft-probe-{}", uuid::Uuid::new_v4().simple()));
        let path = dir.join("gpu-probe.json");
        let mut c = ProbeCache::default();
        c.add("1", ad(ProbeBackend::Gl, 0, 0, AdapterKind::Cpu, "llvmpipe"));
        c.save(&path).unwrap();
        assert_eq!(ProbeCache::load(&path), c);
        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(ProbeCache::load(&path), ProbeCache::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn breadcrumb_detects_unfinished_start() {
        assert!(!previous_run_incomplete(""));
        let ok = format!("{START_MARKER} 1 ===\nstep\n{COMPLETE_MARKER}\n");
        assert!(!previous_run_incomplete(&ok));
        let crashed = format!("{ok}{START_MARKER} 1 ===\nenumerating adapters\n");
        assert!(previous_run_incomplete(&crashed));
        assert_eq!(trim_log("aaa\nbbb\nccc\n", 6), "ccc\n");
        assert_eq!(trim_log("short\n", 100), "short\n");
    }

    #[test]
    fn failure_text_names_the_log() {
        let t = failure_text(&["DirectX 12: crashed".into()], Some(Path::new("C:/x/startup.log")));
        assert!(t.contains("startup.log") && t.contains("DirectX 12: crashed"));
    }

    // Real child processes (a shell stands in for the SubCraft executable).
    #[cfg(unix)]
    mod children {
        use super::*;
        fn sh(script: &str) -> Command { let mut c = Command::new("sh"); c.arg("-c").arg(script.to_string()); c }
        const T: Duration = Duration::from_secs(5);

        #[test]
        fn exit_crash_and_hang_are_all_contained() {
            assert!(matches!(run_child(sh("echo hi"), T), ChildOutcome::Finished { stdout } if stdout == "hi\n"));
            assert!(matches!(run_child(sh("echo oops >&2; exit 3"), T), ChildOutcome::Failed { code: Some(3), .. }));
            // A segfault-style death by signal (what a driver access violation looks like).
            assert!(matches!(run_child(sh("kill -SEGV $$"), T), ChildOutcome::Failed { code: None, .. }));
            let start = Instant::now();
            assert_eq!(run_child(sh("sleep 30"), Duration::from_millis(300)), ChildOutcome::TimedOut);
            assert!(start.elapsed() < Duration::from_secs(5));
            assert!(matches!(run_child(Command::new("/definitely/not/here"), T), ChildOutcome::SpawnFailed(_)));
        }

        /// Fake "executable": DX12 enumeration segfaults, Vulkan lists an Intel and an NVIDIA GPU,
        /// the NVIDIA device hangs, the Intel one works.
        fn fake(backend: ProbeBackend, index: Option<u32>) -> Command {
            match (backend, index) {
                (ProbeBackend::Dx12, None) => sh("kill -SEGV $$"),
                (ProbeBackend::Vulkan, None) => sh("printf 'ADAPTER\\t0\\t32902\\t1\\tintegrated\\t1.0\\tIntel\\nADAPTER\\t1\\t4318\\t1\\tdiscrete\\t1.0\\tRTX\\nDONE\\n'"),
                (ProbeBackend::Vulkan, Some(1)) => sh("sleep 30"),
                (ProbeBackend::Vulkan, Some(_)) => sh("echo OK"),
                _ => sh("exit 1"),
            }
        }

        #[test]
        fn probe_moves_past_crashes_and_hangs_to_the_next_adapter() {
            let runner = ProbeRunner { command: &fake, enumerate_timeout: T, device_timeout: Duration::from_millis(400) };
            let got = probe(&runner, &ProbeBackend::ALL, &[]).unwrap();
            assert_eq!((got.backend, got.name.as_str()), (ProbeBackend::Vulkan, "Intel"));
            let report = probe(&runner, &[ProbeBackend::Dx12], &[]).unwrap_err();
            assert_eq!(report.len(), 1);
            assert!(report[0].contains("DirectX 12"), "{report:?}");
        }

        #[test]
        fn select_caches_then_reuses_without_probing() {
            let dir = std::env::temp_dir().join(format!("subcraft-probe-{}", uuid::Uuid::new_v4().simple()));
            let path = dir.join("gpu-probe.json");
            let runner = ProbeRunner { command: &fake, enumerate_timeout: T, device_timeout: Duration::from_millis(400) };
            let first = select(&runner, &ProbeBackend::ALL, "", Some(&path));
            assert!(matches!(&first, ProbeResult::Verified { fresh: true, .. }), "{first:?}");
            // Second launch: a runner that fails every spawn proves the cache is used.
            let dead = |_: ProbeBackend, _: Option<u32>| Command::new("/definitely/not/here");
            let dead = ProbeRunner { command: &dead, enumerate_timeout: T, device_timeout: T };
            let second = select(&dead, &ProbeBackend::ALL, "", Some(&path));
            assert!(matches!(&second, ProbeResult::Verified { fresh: false, .. }), "{second:?}");
            // After an unfinished start the cache is not trusted; with everything dead -> AllFailed.
            let crashed = format!("{START_MARKER} 1 ===\nstep\n");
            assert!(matches!(select(&dead, &ProbeBackend::ALL, &crashed, Some(&path)), ProbeResult::AllFailed { .. }));
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
