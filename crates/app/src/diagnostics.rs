//! Log files, crash reports and bug-report links.
//!
//! Nothing here sends data anywhere: logs and crash reports stay in SubCraft's data folder
//! (`logs/`, `crashes/`), and a bug report is a GitHub link the user opens and submits themself.

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Where bug reports go.
pub const ISSUES_URL: &str = "https://github.com/Aalzard/subcraft/issues/new";
/// Log files kept (`subcraft.log` + `subcraft.1.log` … older).
pub const LOG_FILES_KEPT: usize = 5;
/// A log file is rotated during the run once it grows past this.
pub const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
/// Recent log lines kept in memory for crash reports.
pub const RECENT_LINES: usize = 300;
/// Crash reports kept.
pub const CRASH_REPORTS_KEPT: usize = 10;

const LOG_NAME: &str = "subcraft";
const CRASH_PREFIX: &str = "crash-";
const SEEN_SUFFIX: &str = ".seen";

// ----- system info ---------------------------------------------------------------------------

/// What a bug report needs to know about this PC.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemInfo {
    pub version: String,
    pub os: String,
    /// GPU name and graphics backend, e.g. "NVIDIA GeForce RTX 4060 (Dx12, driver 32.0.15.6094)".
    pub gpu: String,
}

static GPU: OnceLock<Mutex<String>> = OnceLock::new();

/// Record the GPU once the renderer is up (shown in logs, crash reports and bug reports).
pub fn set_gpu(gpu: impl Into<String>) {
    let gpu = gpu.into();
    log::info!("GPU: {gpu}");
    *GPU.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()) = gpu;
}

fn gpu() -> String {
    GPU.get().and_then(|m| m.try_lock().ok().map(|g| g.clone())).filter(|g| !g.is_empty()).unwrap_or_else(|| "unknown".into())
}

impl SystemInfo {
    pub fn current() -> Self {
        Self { version: env!("CARGO_PKG_VERSION").into(), os: os_description(), gpu: gpu() }
    }
}

/// "Windows 10.0.26200 (x86_64)", "Ubuntu 24.04 LTS (x86_64)"…
pub fn os_description() -> String {
    static OS: OnceLock<String> = OnceLock::new();
    OS.get_or_init(|| {
        let arch = std::env::consts::ARCH;
        let name = os_name().unwrap_or_else(|| std::env::consts::OS.to_string());
        format!("{name} ({arch})")
    })
    .clone()
}

#[cfg(windows)]
fn os_name() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // "Microsoft Windows [Version 10.0.26200.6584]"
    let out = std::process::Command::new("cmd").args(["/c", "ver"]).creation_flags(CREATE_NO_WINDOW).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let version = text.split("Version").nth(1)?.trim().trim_end_matches(']').trim();
    Some(format!("Windows {version}"))
}

#[cfg(target_os = "linux")]
fn os_name() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")).map(|v| v.trim_matches('"').to_string())
}

#[cfg(target_os = "macos")]
fn os_name() -> Option<String> {
    let out = std::process::Command::new("sw_vers").arg("-productVersion").output().ok()?;
    Some(format!("macOS {}", String::from_utf8_lossy(&out.stdout).trim()))
}


#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn os_name() -> Option<String> {
    None
}

// ----- bug-report link -----------------------------------------------------------------------

/// Percent-encode for a URL query value.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Link to a new GitHub issue (the `bug_report.yml` form) with version, OS and GPU filled in.
/// `crash` adds the crash message to the description; `log_file` tells which file to attach.
pub fn bug_report_url(info: &SystemInfo, crash: Option<&str>, log_file: Option<&Path>) -> String {
    let mut url = format!(
        "{ISSUES_URL}?template=bug_report.yml&labels=bug&version={}&os={}&gpu={}",
        encode(&info.version),
        encode(&info.os),
        encode(&info.gpu)
    );
    if let Some(c) = crash {
        // Keep the link short: the full report is attached as a file.
        let first: String = c.lines().take(3).collect::<Vec<_>>().join("\n").chars().take(400).collect();
        url.push_str(&format!("&title={}", encode(&format!("Crash: {}", first.lines().next().unwrap_or("").chars().take(80).collect::<String>()))));
        url.push_str(&format!("&what-happened={}", encode(&format!("SubCraft crashed:\n\n```\n{first}\n```\n\nWhat I was doing:\n"))));
    }
    if let Some(p) = log_file {
        url.push_str(&format!("&logs={}", encode(&format!("(please attach {})", p.display()))));
    }
    url
}

// ----- log files -----------------------------------------------------------------------------

/// `logs/subcraft.log`, `logs/subcraft.1.log`, …
pub fn log_path(dir: &Path, index: usize) -> PathBuf {
    if index == 0 { dir.join(format!("{LOG_NAME}.log")) } else { dir.join(format!("{LOG_NAME}.{index}.log")) }
}

/// Shift `subcraft.log` → `.1.log` → … and drop the oldest, keeping `keep` files in total.
pub fn rotate_logs(dir: &Path, keep: usize) {
    if keep == 0 {
        return;
    }
    let _ = std::fs::remove_file(log_path(dir, keep - 1));
    for i in (0..keep - 1).rev() {
        let from = log_path(dir, i);
        if from.exists() {
            let _ = std::fs::rename(&from, log_path(dir, i + 1));
        }
    }
}

struct Sink {
    file: Option<File>,
    path: Option<PathBuf>,
    written: u64,
    recent: VecDeque<String>,
}

struct Logger {
    sink: Mutex<Sink>,
    stderr: bool,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

fn wanted(m: &log::Metadata) -> bool {
    // Our own messages from Info up; other crates (wgpu, eframe…) only warnings and errors.
    if m.target().starts_with("subcraft") { m.level() <= log::Level::Info } else { m.level() <= log::Level::Warn }
}

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        wanted(m)
    }

    fn log(&self, r: &log::Record) {
        if !wanted(r.metadata()) {
            return;
        }
        let line = format!("{} [{}] {}", timestamp(), r.level(), r.args());
        if self.stderr {
            eprintln!("[{}] {}", r.level(), r.args());
        }
        let Ok(mut sink) = self.sink.lock() else { return };
        if sink.recent.len() >= RECENT_LINES {
            sink.recent.pop_front();
        }
        sink.recent.push_back(line.clone());
        if sink.written > MAX_LOG_BYTES
            && let Some(path) = sink.path.clone()
            && let Some(dir) = path.parent()
        {
            sink.file = None;
            rotate_logs(dir, LOG_FILES_KEPT);
            sink.file = File::create(&path).ok();
            sink.written = 0;
        }
        if let Some(f) = sink.file.as_mut() {
            let _ = writeln!(f, "{line}");
            sink.written += line.len() as u64 + 1;
        }
    }

    fn flush(&self) {
        if let Ok(mut s) = self.sink.lock()
            && let Some(f) = s.file.as_mut()
        {
            let _ = f.flush();
        }
    }
}

/// Install the logger: stderr (when `stderr`), the last lines in memory, and `dir/subcraft.log`
/// (older runs rotated to `subcraft.1.log` …). Returns the log file path. Call once at start.
pub fn init_logging(dir: Option<&Path>, stderr: bool) -> Option<PathBuf> {
    let (file, path) = match dir {
        Some(d) if std::fs::create_dir_all(d).is_ok() => {
            rotate_logs(d, LOG_FILES_KEPT);
            let path = log_path(d, 0);
            match File::create(&path) {
                Ok(f) => (Some(f), Some(path)),
                Err(_) => (None, None),
            }
        }
        _ => (None, None),
    };
    let logger = LOGGER.get_or_init(|| Logger { sink: Mutex::new(Sink { file, path: path.clone(), written: 0, recent: VecDeque::new() }), stderr });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    let info = SystemInfo::current();
    log::info!("SubCraft {} on {} (log times are UTC)", info.version, info.os);
    path
}

/// The current log file, if logging to a file.
pub fn log_file() -> Option<PathBuf> {
    LOGGER.get().and_then(|l| l.sink.lock().ok().and_then(|s| s.path.clone()))
}

/// The last log lines (newest last). Never blocks: empty if the logger is busy.
pub fn recent_lines() -> Vec<String> {
    LOGGER.get().and_then(|l| l.sink.try_lock().ok().map(|s| s.recent.iter().cloned().collect())).unwrap_or_default()
}

/// "2026-10-14 18:03:07" (UTC; no time-zone database needed).
pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    format_utc(secs)
}

fn format_utc(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

// ----- crash reports -------------------------------------------------------------------------

/// Text of a crash report.
pub fn crash_report_text(info: &SystemInfo, message: &str, location: &str, backtrace: &str, recent: &[String]) -> String {
    let mut s = String::new();
    s.push_str(&format!("SubCraft crash report\n\nTime: {} UTC\nVersion: {}\nOS: {}\nGPU: {}\n\n", timestamp(), info.version, info.os, info.gpu));
    s.push_str(&format!("Message: {message}\nLocation: {location}\n\nBacktrace:\n{backtrace}\n\nLast log lines:\n"));
    for l in recent {
        s.push_str(l);
        s.push('\n');
    }
    s
}

/// Write a crash report into `dir` (named after the time) and drop old ones.
pub fn write_crash_report(dir: &Path, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    // `crash-2026-10-14-18-03-07-01.txt`: names sort by time, also within one second.
    let stamp = timestamp().replace([' ', ':'], "-");
    let mut n = 1;
    let mut path = dir.join(format!("{CRASH_PREFIX}{stamp}-{n:02}.txt"));
    while path.exists() {
        n += 1;
        path = dir.join(format!("{CRASH_PREFIX}{stamp}-{n:02}.txt"));
    }
    std::fs::write(&path, text)?;
    prune_crash_reports(dir, CRASH_REPORTS_KEPT);
    Ok(path)
}

fn crash_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(CRASH_PREFIX) && n.ends_with(".txt")))
        .collect();
    v.sort(); // names start with the time
    v
}

fn seen_marker(report: &Path) -> PathBuf {
    let mut n = report.file_name().unwrap_or_default().to_os_string();
    n.push(SEEN_SUFFIX);
    report.with_file_name(n)
}

fn prune_crash_reports(dir: &Path, keep: usize) {
    let files = crash_files(dir);
    let excess = files.len().saturating_sub(keep);
    for f in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(seen_marker(&f));
        let _ = std::fs::remove_file(f);
    }
}

/// Crash reports the user has not been shown yet, oldest first.
pub fn unseen_crash_reports(dir: &Path) -> Vec<PathBuf> {
    crash_files(dir).into_iter().filter(|p| !seen_marker(p).exists()).collect()
}

/// Don't show this report again on start.
pub fn mark_seen(report: &Path) -> std::io::Result<()> {
    std::fs::write(seen_marker(report), b"")
}

/// "Message: …" line of a report (for the bug-report title).
pub fn crash_message(report_text: &str) -> &str {
    report_text.lines().find_map(|l| l.strip_prefix("Message: ")).unwrap_or("unknown")
}

/// On a panic, write a crash report to `dir` (message, location, backtrace, last log lines),
/// then run the previous hook (which prints the panic).
pub fn install_panic_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |p| {
        let message = p
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let location = p.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        // Not through `log`: the panic may have happened while the logger was busy.
        if let Some(l) = LOGGER.get()
            && let Ok(mut sink) = l.sink.try_lock()
            && let Some(f) = sink.file.as_mut()
        {
            let _ = writeln!(f, "{} [PANIC] thread '{thread}' at {location}: {message}", timestamp());
        }
        let text = crash_report_text(&SystemInfo::current(), &message, &format!("{location} (thread '{thread}')"), &backtrace, &recent_lines());
        match write_crash_report(&dir, &text) {
            Ok(path) => eprintln!("SubCraft crashed. Crash report: {}", path.display()),
            Err(e) => eprintln!("SubCraft crashed and the crash report could not be written: {e}"),
        }
        previous(p);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("subcraft-diag-{name}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn issue_link_carries_version_os_and_gpu() {
        let info = SystemInfo { version: "0.1.2".into(), os: "Windows 10.0.26200 (x86_64)".into(), gpu: "NVIDIA GeForce RTX 4060 (Dx12)".into() };
        let url = bug_report_url(&info, None, Some(Path::new("C:\\Users\\me\\AppData\\Local\\SubCraft\\logs\\subcraft.log")));
        assert!(url.starts_with("https://github.com/Aalzard/subcraft/issues/new?template=bug_report.yml"));
        assert!(url.contains("version=0.1.2"));
        assert!(url.contains("gpu=NVIDIA%20GeForce%20RTX%204060%20%28Dx12%29"), "{url}");
        assert!(url.contains("os=Windows%2010.0.26200%20%28x86_64%29"));
        assert!(url.contains("logs=%28please%20attach%20C%3A%5CUsers"));
        assert!(!url.contains("title="), "no crash, no title");
        // Every query value is encoded: no raw spaces, newlines or extra `&`.
        assert!(!url.contains(' ') && !url.contains('\n'));
        assert_eq!(url.matches('&').count(), 5);

        let crash = bug_report_url(&info, Some("index out of bounds: the len is 3 but the index is 7\nmore"), None);
        assert!(crash.contains("title=Crash%3A%20index%20out%20of%20bounds"));
        assert!(crash.contains("what-happened=SubCraft%20crashed"));
    }

    #[test]
    fn log_rotation_shifts_and_drops_the_oldest() {
        let d = temp_dir("logs");
        for i in 0..5 {
            std::fs::write(log_path(&d, i), format!("run {i}")).unwrap();
        }
        rotate_logs(&d, 5);
        assert!(!log_path(&d, 0).exists(), "the current log starts fresh");
        for i in 1..5 {
            assert_eq!(std::fs::read_to_string(log_path(&d, i)).unwrap(), format!("run {}", i - 1));
        }
        assert!(!log_path(&d, 5).exists(), "only 5 files are kept (run 4 dropped)");
        // With gaps (first runs): nothing breaks.
        let e = temp_dir("logs2");
        std::fs::write(log_path(&e, 0), "only").unwrap();
        rotate_logs(&e, 5);
        assert_eq!(std::fs::read_to_string(log_path(&e, 1)).unwrap(), "only");
        std::fs::remove_dir_all(d).unwrap();
        std::fs::remove_dir_all(e).unwrap();
    }

    #[test]
    fn crash_reports_are_shown_once_and_pruned() {
        let d = temp_dir("crash");
        let info = SystemInfo { version: "9.9.9".into(), os: "TestOS".into(), gpu: "TestGPU".into() };
        let text = crash_report_text(&info, "boom", "src/x.rs:1", "frame 0", &["line a".into(), "line b".into()]);
        assert!(text.contains("Version: 9.9.9") && text.contains("GPU: TestGPU") && text.contains("line b"));
        assert_eq!(crash_message(&text), "boom");
        let a = write_crash_report(&d, &text).unwrap();
        let b = write_crash_report(&d, &text).unwrap();
        assert_ne!(a, b, "same second, different files");
        assert_eq!(unseen_crash_reports(&d), vec![a.clone(), b.clone()]);
        mark_seen(&a).unwrap();
        assert_eq!(unseen_crash_reports(&d), vec![b]);
        for _ in 0..CRASH_REPORTS_KEPT + 3 {
            write_crash_report(&d, &text).unwrap();
        }
        assert_eq!(crash_files(&d).len(), CRASH_REPORTS_KEPT);
        assert!(!seen_marker(&a).exists(), "markers of pruned reports go too");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn utc_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(format_utc(1_791_994_216), "2026-10-14 16:10:16");
    }
}
